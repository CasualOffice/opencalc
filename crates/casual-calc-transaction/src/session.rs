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

use std::collections::BTreeMap;

use casual_calc_model::{ModelError, Workbook};

use crate::{
    Operation, TxnError, apply,
    transform::{Side, TransformError, is_noop, transform_with_formulas},
    wire::WireOperation,
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
    /// This client is no longer a participant: an arrival could not be merged,
    /// so the session latched and refuses everything that crosses the network
    /// (`COL-47`).
    ///
    /// The first refusal is reported as itself — [`Self::Transform`] or
    /// [`Self::Apply`] — and every later
    /// [`receive`](ClientSession::receive) answers this instead. The
    /// distinction is worth having: the first names the pair that could not be
    /// merged, and this one says the session has already stopped, so a host
    /// does not report the same failure twice under two different causes.
    ///
    /// The only recovery is a **full rejoin from a server snapshot** — a new
    /// [`ClientSession`], not [`resume`](ClientSession::resume), which
    /// deliberately keeps the latch. See the type's own documentation for why.
    Desynced,
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
            Self::Desynced => write!(
                f,
                "this client is desynced: an arrival could not be merged, so it must rejoin from a snapshot"
            ),
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

/// Which participant a chunk came from.
///
/// Assigned by the host — the engine never invents identity, for the same
/// reason it never reads a clock. Must be unique among the live participants of
/// a document, since it is what deduplication keys on.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct ClientId(pub u64);

/// A chunk of a client's own edits, offered to the server.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Submission {
    /// Who sent it.
    pub client: ClientId,
    /// Which chunk this is from that client, counted from one.
    ///
    /// Together with [`Self::client`] this is an idempotency key, and it exists
    /// for one specific failure: a leader can commit a chunk and die **before
    /// acknowledging it**. The client, having heard nothing, resends — and
    /// without a key the new leader cannot tell a lost chunk from a duplicate,
    /// so it applies it twice. Typing a value twice is invisible; inserting a
    /// row twice is not.
    ///
    /// A resend carries the *same* sequence, which is what makes it a resend
    /// rather than a new edit.
    pub seq: u64,
    /// What the operations were written against.
    pub base: Base,
    /// The operations, in the order the client made them, **each packaged with
    /// what its handles mean**.
    ///
    /// Not bare [`Operation`]s: a cell refers to its formula and style by an
    /// index into the sending workbook's own tables, which names something
    /// different — or nothing — anywhere else. See [`WireOperation`].
    pub ops: Vec<WireOperation>,
}

/// What a chunk was written on top of.
///
/// Two answers rather than one revision number, and the second is what lets a
/// client keep sending without waiting to be acknowledged
/// ([ADR-016](../../../docs/62-COLLABORATION-PIPELINING.md)).
///
/// A second chunk is written on top of the first, locally, before the first has
/// been ordered. If it named the same revision the first did, the server would
/// rebase it against the first as well — and it already contains the first, so
/// it would be transformed twice and land wrong. Silently: there is no error to
/// raise, only two documents that no longer agree.
///
/// The client cannot name the right revision, because the right revision is
/// wherever the first chunk landed and it will not know that until the
/// acknowledgement arrives. That circle is what stop-and-wait avoided by never
/// having a second chunk in flight.
///
/// So it does not name one. A sender does not need to know the receiver's
/// position; it says "after my last one" and the server, which does know,
/// substitutes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Base {
    /// Written against this revision.
    ///
    /// The first chunk after a join or a resume, which is the only moment a
    /// client knows an absolute answer.
    Revision(u64),
    /// Written on top of this client's previous chunk, wherever it landed.
    Chained,
}

/// What committing a chunk did.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum Commit {
    /// Newly committed. Carries the operations **as they landed** — rebased
    /// onto whatever had been committed since the client's base — which is
    /// what every other participant is told about.
    Applied {
        /// The rebased operations, in order, packaged for the wire so every
        /// other participant can localise them into its own tables.
        ops: Vec<WireOperation>,
        /// The revision the document reached.
        revision: u64,
    },
    /// Already committed by an earlier delivery of this same chunk. Nothing was
    /// applied a second time.
    ///
    /// The revision is the one the chunk landed at originally, **not** the
    /// server's current one: acknowledging a client at a later revision would
    /// have it skip over everything committed in between, which it would then
    /// never receive.
    Duplicate {
        /// Where the chunk landed the first time.
        revision: u64,
    },
}

/// A chunk that has been sent and not yet acknowledged.
#[derive(Debug, Clone)]
struct Outstanding {
    seq: u64,
    ops: Vec<Operation>,
}

/// How many chunks a client will have in flight before it stops making more.
///
/// Unbounded pipelining is unbounded memory here and unbounded queued work at
/// the server, reachable by one participant on a bad link. Past this, `flush`
/// produces nothing and edits accumulate in `pending` exactly as they did
/// before — which is to say it degrades to stop-and-wait, which is a good thing
/// to degrade to, being what this replaced.
const MAX_OUTSTANDING: usize = 32;

/// The largest frame the transport will carry, in bytes (`COL-63`).
///
/// **Stated once, here, because both sides have to agree and only one of them
/// can be wrong quietly.** The collaboration server sets this as its WebSocket
/// `max_message_size`; a frame over it is not refused but *closes the
/// connection*, which is the failure this constant exists to prevent. The
/// server reads it from this crate rather than repeating the number, so the
/// two cannot drift.
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

/// What a chunk is allowed to reach before it is split.
///
/// Under the frame cap, not equal to it: the measurement below is of the
/// operations, and the envelope around them — client id, sequence, base, and
/// JSON's own punctuation — is not free. The margin is generous because the
/// cost of being wrong is asymmetric: a chunk slightly smaller than it could
/// have been costs one extra round trip, and a chunk slightly larger than the
/// cap costs the connection.
const CHUNK_BUDGET_BYTES: usize = MAX_FRAME_BYTES - (256 * 1024);

/// The name and id of every sheet, by index — what an operation's sheet number
/// means, which the transform needs and cannot look up itself (`FID-28`).
fn sheet_names(workbook: &Workbook) -> Vec<(String, casual_calc_model::SheetId)> {
    workbook
        .sheets
        .iter()
        .map(|s| (s.name.clone(), s.id))
        .collect()
}

/// One participant's view: its revision, what it has sent, and what it has not.
#[derive(Debug, Clone)]
pub struct ClientSession {
    client: ClientId,
    revision: u64,
    /// Sent and awaiting acknowledgement, oldest first.
    ///
    /// A queue rather than a single chunk since ADR-016. Ordered, and it must
    /// stay ordered: each chunk was written on top of the one before it, so
    /// they mean nothing rearranged.
    sent: Vec<Outstanding>,
    /// Made locally and not yet sent.
    pending: Vec<Operation>,
    /// Chunks taken from `pending` so far, which is what numbers them.
    chunks: u64,
    /// Latched the first time an arrival could not be merged (`COL-47`).
    ///
    /// **One way, and never cleared here.** Recovery is a new session built on
    /// a server snapshot; [`ClientSession::resume`] keeps this set on purpose,
    /// because resuming means "my document is continuous" and this is the
    /// statement that it is not.
    desynced: bool,
}

impl ClientSession {
    /// A client joined at `revision`, with nothing outstanding.
    #[must_use]
    pub fn new(client: ClientId, revision: u64) -> Self {
        Self {
            client,
            revision,
            sent: Vec::new(),
            pending: Vec::new(),
            chunks: 0,
            desynced: false,
        }
    }

    /// Continue after a reconnect, keeping everything outstanding.
    ///
    /// The counterpart to [`new`](Self::new), and the difference is the whole
    /// point: `new` starts a participant with nothing written and nothing sent,
    /// which on a reconnect would discard the edits made just before the socket
    /// dropped — the ones most likely to be unacknowledged, and the ones a user
    /// most recently watched themselves type.
    ///
    /// The chunk counter is kept too. It numbers submissions, the server
    /// suppresses duplicates by `(client, seq)`, and restarting the count would
    /// have a *new* chunk collide with an old one's number and be discarded as
    /// something already seen.
    ///
    /// `client` is normally the id this session already had — the server
    /// reissues it, which is what makes the suppression work across the gap. It
    /// is taken as an argument rather than assumed so the server stays the only
    /// authority on identity.
    ///
    /// **A desync is not resumable and this does not clear it** (`COL-47`).
    /// Resuming asserts that this client's document is continuous with the
    /// server's; a desynced one is exactly the case where that is false, and
    /// the outstanding edits it would carry over are the half-rebased ones. The
    /// route back is a new session over a `welcome` snapshot.
    ///
    /// See [ADR-015](../../../docs/61-COLLABORATION-RESUME.md).
    pub fn resume(&mut self, client: ClientId, revision: u64) {
        self.client = client;
        self.revision = revision;
    }

    /// Who this client is, as the server sees it.
    #[must_use]
    pub fn id(&self) -> ClientId {
        self.client
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

    /// Record an operation the **host already applied**, for sending on.
    ///
    /// The counterpart to [`edit`](Self::edit) for a host that owns the apply
    /// path itself — an editor with its own undo history, say, which cannot
    /// have the operation applied twice.
    ///
    /// The contract is that `op` is already **narrowed** against the state it
    /// was written against. It cannot be narrowed here: by the time this is
    /// called that state is gone, and an operation still claiming to change
    /// everything contends with every concurrent edit and loses one of them.
    pub fn record(&mut self, op: Operation) {
        self.pending.push(op);
    }

    /// Take the pending edits as a chunk to send.
    ///
    /// Returns `None` when there is nothing to send, or when
    /// a bounded number of chunks are already in flight — at which point edits
    /// keep accumulating and go out as one larger chunk later, which is what
    /// this did unconditionally before ADR-016.
    ///
    /// The base is [`Base::Chained`] whenever anything is already outstanding,
    /// because this chunk was written on top of it and only the server knows
    /// where that landed.
    /// A desynced session sends nothing (`COL-47`). The pending edits are kept
    /// rather than discarded, so
    /// [`has_unacknowledged`](Self::has_unacknowledged) can still tell a host
    /// there is work about to be lost — which is the whole difference between
    /// losing it loudly and losing it quietly.
    pub fn flush(&mut self, workbook: &Workbook) -> Option<Submission> {
        if self.desynced || self.pending.is_empty() || self.sent.len() >= MAX_OUTSTANDING {
            return None;
        }
        let base = if self.sent.is_empty() {
            Base::Revision(self.revision)
        } else {
            Base::Chained
        };
        // **Bounded by bytes, not only by count** (`COL-63`).
        //
        // This took every pending operation into one chunk with no size limit.
        // At the 84 B per changed cell `SAVE-08` measured, a restore or a paste
        // of more than ~50,000 changed cells exceeds the transport's frame cap
        // — and a frame over that cap does not come back refused, it *closes
        // the connection*. The work is then neither sent nor kept, and the user
        // is told only that collaboration dropped.
        //
        // So the chunk is filled to a budget and the rest stays pending. That
        // is not a new concept here: chunking, sequencing and `MAX_OUTSTANDING`
        // already exist, and the next flush carries the remainder with the same
        // machinery. A large paste becomes several round trips instead of one
        // closed socket.
        let taken = self.take_within_budget(workbook);
        self.chunks += 1;
        let chunk = Outstanding {
            seq: self.chunks,
            ops: taken,
        };
        let submission = self.package(&chunk, base, workbook);
        self.sent.push(chunk);
        Some(submission)
    }

    /// Everything outstanding, to send again, oldest first.
    ///
    /// After a reconnect a client cannot know which of its chunks were
    /// committed, so it sends them all again — each with its *original*
    /// sequence number, which is what lets the server recognise the ones it
    /// already has instead of applying them twice.
    ///
    /// Only the first names a revision. The rest were written on top of it and
    /// are chained, exactly as they were the first time; the server resolves
    /// them the same way whether this is a first delivery or a resend, which is
    /// what keeps the two paths from diverging.
    ///
    /// The absolute base is the client's *current* revision rather than the one
    /// it originally sent, because remote operations that arrived meanwhile
    /// have already rebased these chunks.
    ///
    /// **Empty when the session is desynced** (`COL-47`). These chunks were
    /// half-rebased against an arrival this client never applied, so submitting
    /// them would push a divergence into the shared document — the one place it
    /// would stop being this client's problem.
    #[must_use]
    pub fn resend(&self, workbook: &Workbook) -> Vec<Submission> {
        if self.desynced {
            return Vec::new();
        }
        self.sent
            .iter()
            .enumerate()
            .map(|(i, chunk)| {
                let base = if i == 0 {
                    Base::Revision(self.revision)
                } else {
                    Base::Chained
                };
                self.package(chunk, base, workbook)
            })
            .collect()
    }

    /// A chunk, packaged against `workbook`.
    ///
    /// The workbook is required because an operation's handles mean nothing
    /// apart from the tables they index, and this is the last point at which
    /// those are to hand.
    /// As many pending operations as fit in one frame, oldest first.
    ///
    /// Each operation is measured once, on its wire form, and the budget is
    /// spent in order. Measuring the *whole* chunk and halving would package
    /// the same operations several times; measuring each once is linear and
    /// gives the same answer, because a JSON array's size is its elements plus
    /// its separators.
    ///
    /// **At least one operation is always taken**, even when that one operation
    /// is over the budget by itself. Returning an empty chunk would leave
    /// `flush` looping on a submission it can never make, and a single
    /// operation that large is a different problem (`COL-64`) — it cannot be
    /// split by chunking, because chunking splits *between* operations. It goes
    /// out, and the transport reports what the server says about it, which is
    /// at least an answer.
    fn take_within_budget(&mut self, workbook: &Workbook) -> Vec<Operation> {
        let mut spent = 0usize;
        let mut take = 0usize;
        for op in &self.pending {
            let wire = WireOperation::of(op.clone(), workbook);
            let size = serde_json::to_vec(&wire).map(|v| v.len()).unwrap_or(0);
            // `+ 1` for the comma this element needs in the array.
            if take > 0 && spent + size + 1 > CHUNK_BUDGET_BYTES {
                break;
            }
            spent += size + 1;
            take += 1;
        }
        let rest = self.pending.split_off(take);
        core::mem::replace(&mut self.pending, rest)
    }

    fn package(&self, chunk: &Outstanding, base: Base, workbook: &Workbook) -> Submission {
        Submission {
            client: self.client,
            seq: chunk.seq,
            base,
            ops: chunk
                .ops
                .iter()
                .cloned()
                .map(|op| WireOperation::of(op, workbook))
                .collect(),
        }
    }

    /// The server has ordered everything up to and including `through`.
    ///
    /// **Cumulative**, as TCP's is, and for the same reason: the server orders
    /// one client's chunks in sequence, so acknowledging chunk *n* already
    /// implies every chunk before it. Letting the client rely on that makes a
    /// lost or skipped acknowledgement self-healing — the next one covers it —
    /// rather than leaving a chunk outstanding forever with nothing to say so.
    pub fn acknowledge(&mut self, through: u64, revision: u64) {
        self.sent.retain(|chunk| chunk.seq > through);
        self.revision = revision;
    }

    /// Whether this client has stopped being a participant (`COL-47`).
    ///
    /// True once an arrival could not be merged. From that moment
    /// [`receive`](Self::receive) refuses, [`flush`](Self::flush) and
    /// [`resend`](Self::resend) send nothing, and the only way out is a full
    /// rejoin from a server snapshot.
    ///
    /// A host that wants to *report* the state reads this; a host that wants
    /// to *recover* replaces the session. Nothing here can do the second: the
    /// engine has no transport, and the document it would have to fetch lives
    /// on the server.
    #[must_use]
    pub fn is_desynced(&self) -> bool {
        self.desynced
    }

    /// Someone else's operation arrived, already committed at `revision`.
    ///
    /// # What a client does with a chunk it cannot transform (`COL-47`)
    ///
    /// It **stops being a client**, loudly, and every route to the network is
    /// closed behind it. Three answers were available and two of them are
    /// wrong:
    ///
    /// - *Drop the chunk and carry on.* This is the one that looks cheapest and
    ///   is the most expensive: the document is then permanently missing a
    ///   committed revision, and every later arrival is rebased past
    ///   outstanding edits that assume it happened. Two replicas hold different
    ///   documents and nothing anywhere says so — the class `COL-46` was.
    /// - *Refuse this arrival and keep the session.* What this did. It is the
    ///   same outcome by accident: a refusal left the outstanding edits
    ///   **half-rebased** and then let the *next*, non-contending arrival
    ///   through, so the client applied revision *n+1* without revision *n*.
    ///   "Blocks every later arrival" was the kinder reading and it was not
    ///   what happened.
    /// - *Rejoin from a snapshot.* Correct, and expensive, and not available
    ///   here — the engine has no transport. So this half latches and reports,
    ///   and the host does the fetch. That division is the same one the rest of
    ///   this file keeps: the engine computes, the host owns policy.
    ///
    /// **What it costs the user.** Everything unacknowledged, `sent` and
    /// `pending` alike, because those edits were written against a state the
    /// client is about to abandon and re-applying them untransformed is the
    /// divergence being prevented. The loss is *nameable* rather than silent:
    /// [`has_unacknowledged`](Self::has_unacknowledged) still answers true, and
    /// that is the question a host already asks before replacing a document
    /// with a snapshot.
    ///
    /// **What the server has to know: nothing new.** Recovery is an ordinary
    /// join — reconnect without a resume key and take the `welcome` snapshot,
    /// a path the server already serves. No message, no field, and no enum
    /// variant crosses the wire for this, so `PROTOCOL_VERSION` does not move.
    ///
    /// Local editing is deliberately *not* sealed. [`edit`](Self::edit) and
    /// [`record`](Self::record) still work, because the harm is in what leaves
    /// this machine and in what is applied to it, not in a user continuing to
    /// type. The line is drawn exactly at the network.
    ///
    /// # Atomic
    ///
    /// Either every pair transforms and the arrival is applied, or **nothing**
    /// outstanding is touched. The rebases are computed into a buffer and
    /// written back only on success. Before, they were written in place as the
    /// fold went, so a refusal in the fourth operation left the first three
    /// rebased past an arrival the workbook never received — a client whose own
    /// unacknowledged edits had silently moved. (Formulas interned by a
    /// half-completed fold stay in the arena. That is garbage, not corruption:
    /// a handle nothing references, on a session that is about to be replaced.)
    ///
    /// # Errors
    ///
    /// [`SessionError::Transform`] when the arrival cannot be rebased past this
    /// client's outstanding edits, [`SessionError::Apply`] when it cannot be
    /// applied, and [`SessionError::Desynced`] for every call after either.
    pub fn receive(
        &mut self,
        workbook: &mut Workbook,
        incoming: &WireOperation,
        revision: u64,
    ) -> Result<(), SessionError> {
        if self.desynced {
            return Err(SessionError::Desynced);
        }
        match self.merge(workbook, incoming, revision) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.desynced = true;
                Err(error)
            }
        }
    }

    /// [`receive`](Self::receive) without the latch, so the latch is set in
    /// exactly one place and cannot be forgotten on a new error path.
    fn merge(
        &mut self,
        workbook: &mut Workbook,
        incoming: &WireOperation,
        revision: u64,
    ) -> Result<(), SessionError> {
        // Localised first, into this workbook's own tables, before it is
        // compared with anything local.
        let mut arriving = incoming.clone().localise(workbook);

        // Fold through everything outstanding, oldest first, rebasing both
        // sides as we go. Both halves are needed: the arrival has to be
        // expressed in coordinates that include our edits, and our edits have
        // to be expressed in coordinates that include the arrival.
        // Through every outstanding chunk in order, then everything not yet
        // sent. The order is not incidental: each chunk was written on top of
        // the one before it, so rebasing them in any other order rebases an
        // operation against coordinates it was never expressed in.
        // What the sheet indices in these operations actually name. The
        // transform cannot look it up, so it is handed over (`FID-28`).
        // The workbook is passed as the formula table too: an outstanding
        // `SetCell` carrying a formula has to have that formula rewritten by
        // the arrival's band, and rewriting produces a tree that needs
        // interning before a handle for it exists (`COL-46`).
        let sheets = sheet_names(workbook);
        let outstanding = self.sent.iter().flat_map(|chunk| chunk.ops.iter());
        // Buffered rather than written in place: see "Atomic" above.
        let mut rebased: Vec<Operation> = Vec::new();
        for local in outstanding.chain(self.pending.iter()) {
            let rebased_arrival =
                transform_with_formulas(&arriving, local, Side::Earlier, &sheets, workbook)?;
            rebased.push(transform_with_formulas(
                local,
                &arriving,
                Side::Later,
                &sheets,
                workbook,
            )?);
            arriving = rebased_arrival;
        }

        if !is_noop(&arriving) {
            apply(workbook, arriving)?;
        }

        // Past every fallible step, so this cannot leave a partial rewrite.
        let outstanding = self.sent.iter_mut().flat_map(|chunk| chunk.ops.iter_mut());
        for (slot, op) in outstanding.chain(self.pending.iter_mut()).zip(rebased) {
            *slot = op;
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
    /// The last chunk accepted from each client, and where it landed.
    ///
    /// Part of the session's durable state, not a cache: a leader that dies
    /// takes this with it, and a successor without it cannot tell a resend
    /// from a new edit — which is the whole failure this exists to prevent.
    accepted: BTreeMap<ClientId, (u64, u64)>,
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
            accepted: BTreeMap::new(),
        }
    }

    /// The per-client acceptance record, to persist alongside the log.
    #[must_use]
    pub fn accepted(&self) -> &BTreeMap<ClientId, (u64, u64)> {
        &self.accepted
    }

    /// Reinstate the acceptance record on a successor leader.
    ///
    /// A promotion that skips this is a promotion that will double-apply the
    /// first resend it sees.
    pub fn restore_accepted(&mut self, accepted: BTreeMap<ClientId, (u64, u64)>) {
        self.accepted = accepted;
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
    ) -> Result<Commit, SessionError> {
        // Recognise a chunk we have already taken. Checked before the base is
        // validated, because a resend that arrives after compaction has a base
        // the server no longer holds — and refusing it would tell a client its
        // committed work was lost.
        if let Some(&(seq, revision)) = self.accepted.get(&submission.client)
            && submission.seq <= seq
        {
            return Ok(Commit::Duplicate { revision });
        }
        // Resolved here, and only here. A chained chunk was written on top of
        // this client's previous one and names no revision, because the client
        // could not have known which — this table does, having recorded where
        // that chunk landed, and it is correct to consult because one client's
        // chunks arrive in order on one connection, so chunk n-1 is ordered
        // before chunk n is read.
        let base = match submission.base {
            Base::Revision(revision) => revision,
            Base::Chained => {
                let Some(&(_, landed)) = self.accepted.get(&submission.client) else {
                    // A client with nothing accepted has nothing to chain to.
                    // Impossible from a correct client, and inventing a base
                    // here is how two documents quietly stop agreeing.
                    return Err(SessionError::UnknownRevision {
                        claimed: 0,
                        oldest: self.first,
                        current: self.revision,
                    });
                };
                landed
            }
        };
        if base < self.first || base > self.revision {
            return Err(SessionError::UnknownRevision {
                claimed: base,
                oldest: self.first,
                current: self.revision,
            });
        }

        // Rebase the whole chunk before applying any of it, so a failure part
        // way through leaves the document untouched rather than half-committed.
        let skip = usize::try_from(base - self.first).unwrap_or(usize::MAX);
        let mut rebased = Vec::with_capacity(submission.ops.len());
        let mut history: Vec<Operation> = self.log[skip.min(self.log.len())..].to_vec();

        // Localise before transforming: the transform compares positions and
        // fields, and an operation still carrying the sender's handles would
        // transform correctly and then write a cell nothing here can resolve.
        let incoming: Vec<Operation> = submission
            .ops
            .iter()
            .cloned()
            .map(|wire| wire.localise(workbook))
            .collect();

        let sheets = sheet_names(workbook);
        for op in &incoming {
            let mut current = op.clone();
            for committed in &mut history {
                let next =
                    transform_with_formulas(&current, committed, Side::Later, &sheets, workbook)?;
                // The concurrent operation has to move past this one too, so
                // the *next* operation in the chunk is rebased onto a history
                // that has advanced. Missing this is the batch-threading bug
                // one layer up.
                *committed =
                    transform_with_formulas(committed, &current, Side::Earlier, &sheets, workbook)?;
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
        self.accepted
            .insert(submission.client, (submission.seq, self.revision));
        let packaged = rebased
            .into_iter()
            .map(|op| WireOperation::of(op, workbook))
            .collect();
        Ok(Commit::Applied {
            ops: packaged,
            revision: self.revision,
        })
    }

    /// Take operations that have **already been ordered** by somebody else.
    ///
    /// What a node that is not the leader does with a committed batch. The
    /// operations arrive rebased — the leader transformed them against
    /// everything committed before them — so they are applied as they are and
    /// **not** transformed again. Transforming them here would rebase them past
    /// the very operations they were already rebased past, which does not fail;
    /// it produces a document that quietly disagrees with the leader's.
    ///
    /// `revision` is where the batch leaves the document, and it must follow
    /// directly from where this session is. A caller that has missed something
    /// has to read the log rather than skip forward, because the operations in
    /// between are what these were transformed against
    /// ([ADR-017](../../../docs/63-COLLABORATION-RELAY.md)).
    ///
    /// # Errors
    ///
    /// [`SessionError::UnknownRevision`] when the batch does not follow
    /// directly, and [`SessionError::Apply`] if an operation cannot be applied.
    /// On either, nothing is applied: the batch is checked before any of it
    /// lands, so a refusal leaves the document where it was rather than half
    /// advanced.
    pub fn adopt(
        &mut self,
        workbook: &mut Workbook,
        ops: &[WireOperation],
        revision: u64,
    ) -> Result<(), SessionError> {
        let expected = self.revision.saturating_add(ops.len() as u64);
        if revision != expected {
            return Err(SessionError::UnknownRevision {
                claimed: revision,
                oldest: self.first,
                current: self.revision,
            });
        }
        // Localised first — into this workbook's own formula, style and string
        // tables — because the handles in them index the *leader's* tables and
        // mean something different, or nothing, here.
        let incoming: Vec<Operation> = ops
            .iter()
            .cloned()
            .map(|wire| wire.localise(workbook))
            .collect();
        for op in incoming {
            if !is_noop(&op) {
                apply(workbook, op.clone())?;
            }
            self.log.push(op);
            self.revision += 1;
        }
        Ok(())
    }

    /// Record that `client`'s chunk `seq` was ordered at `revision`.
    ///
    /// A relay learns this from a committed batch rather than from having
    /// ordered it. Without it, the client's next reconnect would resend a chunk
    /// this node has no record of accepting, and the duplicate suppression that
    /// makes reconnection safe would not fire.
    pub fn note_accepted(&mut self, client: ClientId, seq: u64, revision: u64) {
        self.accepted.insert(client, (seq, revision));
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

#[cfg(test)]
mod receive_is_atomic {
    //! Here rather than in `session_tests` on purpose: once
    //! [`ClientSession::receive`] latches, the outstanding chunks stop being
    //! reachable through [`ClientSession::resend`] — the seal is doing its job
    //! — so the only place this invariant is observable at all is inside the
    //! module that owns the field.

    use super::{ClientId, ClientSession};
    use crate::{Operation, wire::WireOperation};
    use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

    fn book() -> Workbook {
        let mut workbook = Workbook::new(Id::from_parts(1, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        for row in 0..8u32 {
            for col in 0..8u32 {
                sheet.cells.set(
                    CellRef::new(row, col),
                    Cell::value(CellValue::Number(f64::from(row * 10 + col))),
                );
            }
        }
        workbook.sheets.push(sheet);
        workbook
    }

    /// A refused arrival must leave **nothing** outstanding rewritten.
    ///
    /// The fold rebased each local operation in place as it went, so a refusal
    /// on the second one had already rewritten the first: a `SetCell` the user
    /// typed into column 2 became a `SetCell` on column 5, rebased past an
    /// arrival this workbook never received. The session survived holding an
    /// edit that had silently moved.
    #[test]
    fn a_refusal_leaves_every_outstanding_operation_exactly_as_it_was() {
        let mut wb = book();
        let mut client = ClientSession::new(ClientId(1), 0);
        client.edit(&mut wb, keystroke()).expect("a keystroke");
        client.edit(&mut wb, narrow_drag()).expect("a drag");
        client.flush(&wb).expect("the chunk goes out");

        let before = outstanding_ops(&client);
        let arrival = WireOperation::of(wide_drag(), &wb);
        assert!(
            client.receive(&mut wb, &arrival, 1).is_err(),
            "the pair has no transform"
        );
        assert_eq!(
            outstanding_ops(&client),
            before,
            "a refused merge must not have rewritten half the chunk on its way out"
        );
    }

    /// Everything the session is holding, chunk by chunk.
    fn outstanding_ops(client: &ClientSession) -> Vec<Vec<Operation>> {
        client
            .sent
            .iter()
            .map(|chunk| chunk.ops.clone())
            .chain(core::iter::once(client.pending.clone()))
            .collect()
    }

    fn keystroke() -> Operation {
        Operation::SetCell {
            sheet: 0,
            at: CellRef::new(0, 2),
            cell: Some(Cell::value(CellValue::Number(42.0))),
        }
    }

    /// Transformable against the arrival — it is the operation *after* this one
    /// that refuses, which is what makes the half-rewrite reachable.
    fn narrow_drag() -> Operation {
        Operation::MoveColumns {
            sheet: 0,
            at: 2,
            count: 1,
            before: 0,
        }
    }

    fn wide_drag() -> Operation {
        Operation::MoveColumns {
            sheet: 0,
            at: 1,
            count: 3,
            before: 7,
        }
    }
}
