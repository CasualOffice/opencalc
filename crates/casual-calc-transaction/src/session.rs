//! The client and server halves of the concurrency protocol from
//! [ADR-011](../../../docs/56-COLLABORATION-CONCURRENCY-DESIGN.md).
//!
//! Two state machines over [`transform`](crate::transform), and nothing else:
//! no transport, no storage, no clock. What arrives and what is sent are plain
//! values, so the whole protocol runs in a test with several clients and no
//! network — which is the only way the interleavings that break it get
//! exercised at all.
//!
//! # The shape
//!
//! The server owns the order. A client applies its own edits immediately, so
//! typing never waits for a round trip, and holds them until they are
//! acknowledged. **One chunk is in flight at a time** — the client sends, waits
//! for the acknowledgement, then sends the next. That rule is Wave's, and it is
//! what makes a single global order enough and removes the TP2 obligation that
//! peer-to-peer OT carries.
//!
//! # Which way the transform points
//!
//! A client's unacknowledged edits are, by definition, not yet in the order. So
//! an operation arriving from the server is ordered **before** them:
//!
//! - the arriving operation is rebased past each outstanding local one, as
//!   [`Side::Earlier`];
//! - each outstanding local one is rebased past the arrival, as
//!   [`Side::Later`].
//!
//! Those are the two halves of the same diamond, which is why TP1 is the
//! property that makes this work rather than a nicety.

use casual_calc_model::{ModelError, Workbook};

use crate::{
    Operation, TxnError, apply,
    transform::{Side, TransformError, is_noop, transform},
};

/// Why a session could not accept something.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionError {
    /// The operation could not be rebased. The pair has no transform — see
    /// [`TransformError`] — so the two edits cannot be merged and the caller
    /// must resolve it another way: refuse the edit, or reload the client.
    Transform(TransformError),
    /// Applying an operation failed.
    Apply(TxnError),
    /// A snapshot could not be written or read back.
    Snapshot(String),
    /// Replaying the log from an earlier snapshot did not reproduce the later
    /// one, byte for byte.
    ///
    /// The document on disk and the document the log describes have diverged,
    /// which is corruption however it happened. Surfaced rather than repaired:
    /// the two disagree and nothing here knows which is right.
    SnapshotMismatch {
        /// The snapshot replayed from.
        from: u64,
        /// The snapshot that should have been reproduced.
        to: u64,
    },
    /// A client submitted against a revision the server no longer has history
    /// for, or one it has never issued.
    ///
    /// The first is the bounded-offline edge: the log has been compacted past
    /// the point the client left, so there is nothing left to rebase against
    /// and it must reload from a snapshot. The client is *told*, rather than
    /// having its work quietly dropped.
    UnknownRevision {
        /// What the client claimed to be based on.
        claimed: u64,
        /// The oldest revision the server can still rebase from.
        oldest: u64,
        /// The revision the server is currently at.
        current: u64,
    },
}

impl From<TransformError> for SessionError {
    fn from(error: TransformError) -> Self {
        Self::Transform(error)
    }
}

impl From<TxnError> for SessionError {
    fn from(error: TxnError) -> Self {
        Self::Apply(error)
    }
}

impl core::fmt::Display for SessionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Transform(error) => write!(f, "{error}"),
            Self::Apply(error) => write!(f, "{error}"),
            Self::Snapshot(why) => write!(f, "snapshot: {why}"),
            Self::SnapshotMismatch { from, to } => write!(
                f,
                "replaying revisions {from}..={to} did not reproduce the stored snapshot"
            ),
            Self::UnknownRevision {
                claimed,
                oldest,
                current,
            } => write!(
                f,
                "revision {claimed} is outside the retained history {oldest}..={current}"
            ),
        }
    }
}

impl core::error::Error for SessionError {}

/// A chunk of a client's own edits, offered to the server.
#[derive(Debug, Clone, PartialEq)]
pub struct Submission {
    /// The revision the operations were written against.
    pub base: u64,
    /// The operations, in the order the client made them.
    pub ops: Vec<Operation>,
}

/// One participant's view: its revision, what it has sent, and what it has not.
#[derive(Debug, Clone, Default)]
pub struct ClientSession {
    revision: u64,
    /// Sent and awaiting acknowledgement. At most one chunk, by design.
    sent: Vec<Operation>,
    /// Made locally and not yet sent.
    pending: Vec<Operation>,
}

impl ClientSession {
    /// A client joined at `revision`, with nothing outstanding.
    #[must_use]
    pub fn new(revision: u64) -> Self {
        Self {
            revision,
            sent: Vec::new(),
            pending: Vec::new(),
        }
    }

    /// The last revision this client has seen.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether anything is written but not yet acknowledged. What a host shows
    /// as "saving", and what must not be discarded on disconnect.
    #[must_use]
    pub fn has_unacknowledged(&self) -> bool {
        !self.sent.is_empty() || !self.pending.is_empty()
    }

    /// Make a local edit: apply it now, and hold it for sending.
    ///
    /// Applied immediately and on purpose. Waiting for the server would put a
    /// round trip between a keystroke and the character appearing, which is the
    /// thing collaborative editors are judged on.
    ///
    /// # Errors
    ///
    /// [`SessionError::Apply`] if the operation is invalid for this workbook,
    /// in which case nothing is recorded.
    pub fn edit(&mut self, workbook: &mut Workbook, op: Operation) -> Result<(), SessionError> {
        // Narrowed before it is recorded, never after: the transform reads the
        // mask and never sees a workbook, and this is the last moment the state
        // the op was written against still exists.
        let op = op.narrowed(workbook);
        if is_noop(&op) {
            return Ok(());
        }
        apply(workbook, op.clone())?;
        self.pending.push(op);
        Ok(())
    }

    /// Take the pending edits as a chunk to send, if any and if the previous
    /// chunk has been acknowledged.
    ///
    /// Returns `None` when there is nothing to send or a chunk is already in
    /// flight — the one-at-a-time rule, which is what keeps a single server
    /// order sufficient.
    pub fn flush(&mut self) -> Option<Submission> {
        if !self.sent.is_empty() || self.pending.is_empty() {
            return None;
        }
        self.sent = core::mem::take(&mut self.pending);
        Some(Submission {
            base: self.revision,
            ops: self.sent.clone(),
        })
    }

    /// The server committed our chunk. It is now part of the order.
    pub fn acknowledge(&mut self, revision: u64) {
        self.sent.clear();
        self.revision = revision;
    }

    /// Someone else's operation arrived, already committed at `revision`.
    ///
    /// # Errors
    ///
    /// [`SessionError::Transform`] when it cannot be rebased past this client's
    /// outstanding edits.
    pub fn receive(
        &mut self,
        workbook: &mut Workbook,
        incoming: &Operation,
        revision: u64,
    ) -> Result<(), SessionError> {
        let mut arriving = incoming.clone();

        // Fold through everything outstanding, oldest first, rebasing both
        // sides as we go. Both halves are needed: the arrival has to be
        // expressed in coordinates that include our edits, and our edits have
        // to be expressed in coordinates that include the arrival.
        for local in self.sent.iter_mut().chain(self.pending.iter_mut()) {
            let rebased_arrival = transform(&arriving, local, Side::Earlier)?;
            *local = transform(local, &arriving, Side::Later)?;
            arriving = rebased_arrival;
        }

        if !is_noop(&arriving) {
            apply(workbook, arriving)?;
        }
        self.revision = revision;
        Ok(())
    }
}

/// The authority on order.
///
/// Holds the log of committed operations. The workbook itself is passed in
/// rather than owned, so a deployment can keep it wherever it likes — in the
/// object, in a cache, rebuilt from a snapshot on a cold start.
#[derive(Debug, Clone, Default)]
pub struct ServerSession {
    revision: u64,
    /// Committed operations. `log[i]` is what took the document from revision
    /// `first + i` to `first + i + 1`.
    log: Vec<Operation>,
    /// The revision `log[0]` starts from. Non-zero once the log has been
    /// compacted behind a snapshot.
    first: u64,
}

impl ServerSession {
    /// A server at revision zero with an empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A server resuming from a snapshot taken at `revision`, with no history
    /// before it.
    #[must_use]
    pub fn resumed_at(revision: u64) -> Self {
        Self {
            revision,
            log: Vec::new(),
            first: revision,
        }
    }

    /// The current revision.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The oldest revision a client may still submit against.
    #[must_use]
    pub fn oldest_rebasable(&self) -> u64 {
        self.first
    }

    /// Drop history up to `revision`, which a snapshot now covers.
    ///
    /// This is the edge of bounded offline: a client based on anything older
    /// can no longer be rebased, and [`Self::commit`] tells it so rather than
    /// dropping its work silently.
    pub fn compact_to(&mut self, revision: u64) {
        let keep_from = revision.min(self.revision);
        let drop = usize::try_from(keep_from.saturating_sub(self.first)).unwrap_or(usize::MAX);
        let drop = drop.min(self.log.len());
        self.log.drain(..drop);
        self.first = keep_from;
    }

    /// Commit a client's chunk, rebasing it onto everything since its base.
    ///
    /// Returns the operations as committed — which is what every client is then
    /// told about, and what the submitter's own acknowledgement covers. They
    /// may differ from what was sent, and a no-op result is normal: an edit to
    /// a cell someone else deleted has genuinely become nothing.
    ///
    /// # Errors
    ///
    /// [`SessionError::UnknownRevision`] when the base is outside the retained
    /// history, and [`SessionError::Transform`] when a pair cannot be merged.
    /// On either, nothing is committed.
    pub fn commit(
        &mut self,
        workbook: &mut Workbook,
        submission: &Submission,
    ) -> Result<Vec<Operation>, SessionError> {
        if submission.base < self.first || submission.base > self.revision {
            return Err(SessionError::UnknownRevision {
                claimed: submission.base,
                oldest: self.first,
                current: self.revision,
            });
        }

        // Rebase the whole chunk before applying any of it, so a failure part
        // way through leaves the document untouched rather than half-committed.
        let skip = usize::try_from(submission.base - self.first).unwrap_or(usize::MAX);
        let mut rebased = Vec::with_capacity(submission.ops.len());
        let mut history: Vec<Operation> = self.log[skip.min(self.log.len())..].to_vec();

        for op in &submission.ops {
            let mut current = op.clone();
            for committed in &mut history {
                let next = transform(&current, committed, Side::Later)?;
                // The concurrent operation has to move past this one too, so
                // the *next* operation in the chunk is rebased onto a history
                // that has advanced. Missing this is the batch-threading bug
                // one layer up.
                *committed = transform(committed, &current, Side::Earlier)?;
                current = next;
            }
            // Deliberately not pushed onto `history`: the next operation in the
            // chunk was written by the client *after* this one, so it is
            // already expressed in coordinates that include it. Rebasing it
            // here too would shift it a second time — which is exactly what a
            // two-operation chunk against concurrent history caught.
            rebased.push(current);
        }

        for op in &rebased {
            if !is_noop(op) {
                apply(workbook, op.clone())?;
            }
            self.log.push(op.clone());
            self.revision += 1;
        }
        Ok(rebased)
    }
}

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

/// A document's state at one revision.
///
/// The bytes are the model's own deterministic snapshot (ADR-010), so this is
/// that plus a revision number — which is the whole reason the snapshot model
/// in ADR-011 was nearly free to adopt rather than a format to design.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The revision these bytes are the state at.
    pub revision: u64,
    /// The deterministic normalized-JSON encoding of the workbook.
    pub bytes: Vec<u8>,
}

impl Snapshot {
    /// Capture `workbook` as it stands at `revision`.
    ///
    /// # Errors
    ///
    /// [`SessionError::Snapshot`] if the model cannot be serialized.
    pub fn capture(workbook: &Workbook, revision: u64) -> Result<Self, SessionError> {
        Ok(Self {
            revision,
            bytes: workbook.to_snapshot().map_err(snapshot_error)?,
        })
    }

    /// Rebuild the workbook this snapshot holds.
    ///
    /// # Errors
    ///
    /// [`SessionError::Snapshot`] if the bytes are not a valid snapshot.
    pub fn restore(&self) -> Result<Workbook, SessionError> {
        Workbook::from_snapshot(&self.bytes).map_err(snapshot_error)
    }
}

fn snapshot_error(error: ModelError) -> SessionError {
    SessionError::Snapshot(error.to_string())
}

/// When to write a snapshot, and how much history to keep behind it.
///
/// The defaults are starting points to benchmark, not values derived from
/// anything — the number that matters is time-to-first-paint on a cold start,
/// and only a measurement settles it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotPolicy {
    /// Write a snapshot once this many revisions have accumulated since the
    /// last one.
    ///
    /// An order of magnitude under ShareDB's thousand-version default, for two
    /// reasons: our operations are far coarser — a `Batch` paste is one
    /// operation carrying a hundred thousand cells — and our snapshots are
    /// cheap, being a serialization the model already produces.
    pub every: u64,
    /// How many snapshot intervals of operations to keep.
    ///
    /// Two, so a client that was away across a single snapshot boundary can
    /// still be rebased rather than forced to reload. **This number is the
    /// definition of "bounded offline"** in ADR-011.
    pub retain_intervals: u64,
}

impl Default for SnapshotPolicy {
    fn default() -> Self {
        Self {
            every: 200,
            retain_intervals: 2,
        }
    }
}

impl ServerSession {
    /// Whether enough has happened since `latest` to be worth a snapshot.
    ///
    /// The other trigger is quiesce, which this cannot know about: on
    /// serverless compute the object hibernates after seconds of inactivity and
    /// discards its memory, so the moment before it sleeps is the highest-value
    /// snapshot there is — the next event is guaranteed to be a cold start.
    /// That call belongs to whatever owns the lifecycle.
    #[must_use]
    pub fn snapshot_due(&self, latest: Option<&Snapshot>, policy: SnapshotPolicy) -> bool {
        let since = self.revision - latest.map_or(0, |s| s.revision.min(self.revision));
        policy.every > 0 && since >= policy.every
    }

    /// Drop the history a snapshot now covers, keeping the policy's margin.
    ///
    /// Deliberately keeps more than the snapshot strictly needs: trimming to
    /// the snapshot exactly would refuse every client that had been away even
    /// briefly across it.
    pub fn compact_behind(&mut self, latest: &Snapshot, policy: SnapshotPolicy) {
        let margin = policy.every.saturating_mul(policy.retain_intervals);
        self.compact_to(latest.revision.saturating_sub(margin));
    }

    /// The operations from `revision` onward, if they are still retained.
    #[must_use]
    pub fn history_since(&self, revision: u64) -> Option<&[Operation]> {
        if revision < self.oldest_rebasable() || revision > self.revision() {
            return None;
        }
        let skip = usize::try_from(revision - self.oldest_rebasable()).ok()?;
        self.log.get(skip..)
    }

    /// Check that replaying the log from `from` reproduces `to`, byte for byte.
    ///
    /// Free to have, and worth having: because snapshots are deterministic and
    /// byte-stable, a stored one can be **verified rather than trusted**. If
    /// the two disagree, the document on disk and the document the log
    /// describes have diverged — which is corruption, whatever caused it.
    ///
    /// # Errors
    ///
    /// [`SessionError::SnapshotMismatch`] when they differ, and
    /// [`SessionError::UnknownRevision`] when the history between them is no
    /// longer retained.
    pub fn verify_snapshot(&self, from: &Snapshot, to: &Snapshot) -> Result<(), SessionError> {
        let Some(ops) = self.history_since(from.revision) else {
            return Err(SessionError::UnknownRevision {
                claimed: from.revision,
                oldest: self.oldest_rebasable(),
                current: self.revision(),
            });
        };
        let span = usize::try_from(to.revision.saturating_sub(from.revision)).unwrap_or(0);
        let mut replayed = from.restore()?;
        for op in ops.iter().take(span) {
            if !is_noop(op) {
                apply(&mut replayed, op.clone())?;
            }
        }
        if Snapshot::capture(&replayed, to.revision)?.bytes == to.bytes {
            Ok(())
        } else {
            Err(SessionError::SnapshotMismatch {
                from: from.revision,
                to: to.revision,
            })
        }
    }
}
