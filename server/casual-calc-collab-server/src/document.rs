//! One document's session: the order, the document, and when to hand it back.
//!
//! Composes the three things that already exist —
//! [`ServerSession`] for
//! ordering, a [`Workbook`] for the document, and
//! [`SessionLifecycle`] for the save cadence — and
//! adds the two rules that only make sense once they are together.

use casual_calc_eval::recalculate;
use casual_calc_export::write_workbook;
use casual_calc_import::import_package;
use casual_calc_model::Workbook;
use casual_calc_transaction::Operation;
use casual_calc_transaction::session::{
    Commit, ServerSession, SessionError, Snapshot, SnapshotPolicy, Submission,
};
use casual_calc_transaction::wire::WireOperation;

use crate::lifecycle::{Action, CallbackOutcome, SavePolicy, SessionLifecycle};

/// Why a document session could not do what was asked.
#[derive(Debug)]
#[non_exhaustive]
pub enum ServerError {
    /// The document could not be read.
    Open(String),
    /// The document could not be written.
    Write(String),
    /// The protocol refused it — see [`SessionError`].
    Session(SessionError),
    /// The session has stopped accepting edits.
    ///
    /// Set when the host's callback failed often enough that persistence can no
    /// longer be relied on. Continuing to take work that provably cannot be
    /// saved is silent loss dressed up as availability, so it is refused with a
    /// reason instead.
    ReadOnly,
}

impl From<SessionError> for ServerError {
    fn from(error: SessionError) -> Self {
        Self::Session(error)
    }
}

impl core::fmt::Display for ServerError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Open(why) => write!(f, "cannot open the document: {why}"),
            Self::Write(why) => write!(f, "cannot write the document: {why}"),
            Self::Session(error) => write!(f, "{error}"),
            Self::ReadOnly => {
                write!(f, "the session is not accepting edits: it cannot be saved")
            }
        }
    }
}

impl core::error::Error for ServerError {}

/// What a joining participant is given.
///
/// The **model snapshot and the revision it is at** — never the file. Everyone
/// in a session must start from the same revision, and a client that fetched
/// the document itself would arrive at revision zero while the session was at
/// five hundred.
#[derive(Debug, Clone)]
pub struct Joined {
    /// The document as normalized JSON, which the browser engine loads
    /// directly — no re-import, and exactly the state everyone else is on.
    pub snapshot: Vec<u8>,
    /// The revision that snapshot is at.
    pub revision: u64,
}

/// What a reconnecting participant is told: where the document is, and the
/// operations it slept through.
///
/// The counterpart to [`Joined`], and the contrast is the point — a joining
/// participant is given the whole document because it has none, and a resuming
/// one is given only the difference because replacing what it has would throw
/// away its unsent edits.
#[derive(Debug, Clone)]
pub struct Resumption {
    /// The revision the document has reached, once `missed` is applied.
    pub revision: u64,
    /// Everything committed while this participant was away, oldest first.
    pub missed: Vec<WireOperation>,
}

/// A document being edited by one or more participants.
#[derive(Debug)]
pub struct DocumentSession {
    server: ServerSession,
    pub(crate) workbook: Workbook,
    life: SessionLifecycle,
    snapshots: SnapshotPolicy,
    latest: Option<Snapshot>,
    /// Whether anything has changed since the workbook was last recalculated.
    stale: bool,
}

impl DocumentSession {
    /// Open a session from the document's bytes.
    ///
    /// # Errors
    ///
    /// [`ServerError::Open`] if the package cannot be admitted.
    pub fn open(
        bytes: Vec<u8>,
        save: SavePolicy,
        snapshots: SnapshotPolicy,
        now_ms: u64,
    ) -> Result<Self, ServerError> {
        let import = import_package(bytes).map_err(|e| ServerError::Open(e.to_string()))?;
        Ok(Self::from_workbook(
            import.workbook,
            0,
            save,
            snapshots,
            now_ms,
        ))
    }

    /// Resume from a snapshot — a promotion, or a cold start after the compute
    /// hibernated.
    ///
    /// Faithful, including the parts nothing models: the retention table is
    /// serialized with everything else, which was verified rather than assumed
    /// after an earlier revision of the design asserted the opposite.
    ///
    /// # Errors
    ///
    /// [`ServerError::Open`] if the snapshot cannot be read.
    pub fn resume(
        snapshot: &Snapshot,
        save: SavePolicy,
        snapshots: SnapshotPolicy,
        now_ms: u64,
    ) -> Result<Self, ServerError> {
        let workbook = snapshot.restore()?;
        let mut session = Self::from_workbook(workbook, snapshot.revision, save, snapshots, now_ms);
        session.latest = Some(snapshot.clone());
        Ok(session)
    }

    fn from_workbook(
        workbook: Workbook,
        revision: u64,
        save: SavePolicy,
        snapshots: SnapshotPolicy,
        now_ms: u64,
    ) -> Self {
        Self {
            server: ServerSession::resumed_at(revision),
            workbook,
            life: SessionLifecycle::new(save, revision, now_ms),
            snapshots,
            latest: None,
            stale: false,
        }
    }

    /// The revision the document is at.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.server.revision()
    }

    /// Whether edits are being refused.
    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.life.is_read_only()
    }

    /// A participant joins, and is told where to start.
    ///
    /// # Errors
    ///
    /// [`ServerError::Open`] if the document cannot be serialized.
    pub fn join(&mut self) -> Result<Joined, ServerError> {
        self.life.joined();
        // Values are brought up to date first: a participant restoring from a
        // snapshot with stale cached values would show them until it
        // recalculated for itself.
        self.settle();
        let snapshot = Snapshot::capture(&self.workbook, self.revision())?;
        Ok(Joined {
            snapshot: snapshot.bytes,
            revision: snapshot.revision,
        })
    }

    /// A participant reconnects, and is told only what it missed.
    ///
    /// Named for the participant, not the document: [`resume`](Self::resume) is
    /// this *document* coming back from a snapshot after a promotion or a cold
    /// start, which is a different thing that happens to a different noun.
    ///
    /// Deliberately **not** a snapshot. A reconnecting client's document is
    /// continuous and may hold edits the server never received; replacing it
    /// would discard exactly the unacknowledged work that resuming exists to
    /// preserve ([ADR-015](../../../docs/61-COLLABORATION-RESUME.md)).
    ///
    /// Returns `None` when `from` is older than the history still retained, in
    /// which case the caller must fall back to a fresh join and say so — the
    /// bounded-offline limit of ADR-011, which this makes audible rather than
    /// silent.
    pub fn rejoin(&mut self, from: u64) -> Option<Resumption> {
        self.life.joined();
        let missed: Vec<Operation> = self.server.history_since(from)?.to_vec();
        // Packaged against this workbook, which is the last place the handles
        // in them mean anything: an operation's formula and style ids index
        // tables the receiver does not share.
        let missed = missed
            .into_iter()
            .map(|op| WireOperation::of(op, &self.workbook))
            .collect();
        Some(Resumption {
            revision: self.revision(),
            missed,
        })
    }

    /// The oldest revision a client may still resume from.
    #[must_use]
    pub fn oldest_rebasable(&self) -> u64 {
        self.server.oldest_rebasable()
    }

    /// Take a batch another node has already ordered.
    ///
    /// The relay half of [`commit`](Self::commit): the operations arrive
    /// rebased, so they are applied rather than transformed again
    /// ([ADR-017](../../../docs/63-COLLABORATION-RELAY.md)).
    ///
    /// # Errors
    ///
    /// [`ServerError::Session`] when the batch does not follow directly from
    /// where this session is, in which case nothing is applied and the caller
    /// must read the log.
    pub fn adopt(
        &mut self,
        ops: &[WireOperation],
        revision: u64,
        now_ms: u64,
    ) -> Result<(), ServerError> {
        self.server.adopt(&mut self.workbook, ops, revision)?;
        // Values are left stale deliberately, as they are after a commit:
        // recalculation happens where the document is read, not on every
        // arriving edit, because edits arrive at typing speed.
        self.stale = true;
        // The real clock, not `0`. A literal zero here said "the last edit
        // happened at the epoch", so the lifecycle's quiesce test —
        // `now - last_edit >= quiesce_ms` — was true on the very next tick, and
        // a replica that adopted a remote batch believed the document had been
        // sitting idle for fifty-six years. Leadership now decides whether a
        // save happens at all (DEP-02), but a wrong timestamp is the kind of
        // thing that produces the *next* defect: a replica promoted to leader
        // would inherit it and save immediately, on a cadence nobody chose.
        self.life.committed(revision, now_ms);
        Ok(())
    }

    /// Record that a client's chunk was ordered, having seen it rather than
    /// ordered it.
    pub fn note_accepted(
        &mut self,
        client: casual_calc_transaction::session::ClientId,
        seq: u64,
        revision: u64,
    ) {
        self.server.note_accepted(client, seq, revision);
    }

    /// A participant leaves. The last one leaving is a save point.
    pub fn left(&mut self) {
        self.life.left();
    }

    /// Order and apply a participant's chunk.
    ///
    /// # Errors
    ///
    /// [`ServerError::ReadOnly`] when the session has stopped accepting edits,
    /// and [`ServerError::Session`] when the protocol refuses the chunk.
    pub fn commit(&mut self, submission: &Submission, now_ms: u64) -> Result<Commit, ServerError> {
        if self.life.is_read_only() {
            return Err(ServerError::ReadOnly);
        }
        let outcome = self.server.commit(&mut self.workbook, submission)?;
        if let Commit::Applied { revision, .. } = &outcome {
            // Deliberately *not* recalculated here. Commits happen at typing
            // speed and saves do not, so values are brought up to date at the
            // two moments they are read — assembling the file, and capturing a
            // snapshot — rather than on every keystroke.
            self.stale = true;
            self.life.committed(*revision, now_ms);
            self.snapshot_if_due();
        }
        Ok(outcome)
    }

    /// What the caller should do now — save, warn, or stop. See [`Action`].
    pub fn tick(&mut self, now_ms: u64) -> Option<Action> {
        self.life.tick(now_ms)
    }

    /// Report what the host's callback did.
    pub fn callback(&mut self, outcome: CallbackOutcome, now_ms: u64) -> Option<Action> {
        self.life.callback(outcome, now_ms)
    }

    /// Assemble the document as `.xlsx`, for the host's callback.
    ///
    /// The server does this and a participant does not: this is the one place
    /// holding both the ordered document and its retained parts.
    ///
    /// # Errors
    ///
    /// [`ServerError::Write`] if the package cannot be written.
    pub fn assemble(&mut self) -> Result<Vec<u8>, ServerError> {
        self.settle();
        write_workbook(&self.workbook).map_err(|e| ServerError::Write(e.to_string()))
    }

    /// Whether the host has not yet confirmed the newest revision.
    ///
    /// The question an eviction has to ask: letting go of a document with work
    /// outstanding loses exactly the thing the lifecycle exists to deliver.
    #[must_use]
    pub fn has_unsaved(&self) -> bool {
        self.life.has_unsaved()
    }

    /// The most recent snapshot, for the storage layer to persist.
    #[must_use]
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.latest.as_ref()
    }

    /// Bring cached values up to date, if anything has changed.
    fn settle(&mut self) {
        if self.stale {
            recalculate(&mut self.workbook);
            self.stale = false;
        }
    }

    fn snapshot_if_due(&mut self) {
        if !self
            .server
            .snapshot_due(self.latest.as_ref(), self.snapshots)
        {
            return;
        }
        self.settle();
        if let Ok(snapshot) = Snapshot::capture(&self.workbook, self.revision()) {
            self.server.compact_behind(&snapshot, self.snapshots);
            self.latest = Some(snapshot);
        }
    }
}

#[cfg(test)]
mod tests {
    use casual_calc_model::{Cell, CellRef, CellValue};
    use casual_calc_transaction::{
        Operation,
        session::{ClientId, Commit, SnapshotPolicy},
        wire::WireOperation,
    };

    use super::*;
    use crate::lifecycle::SaveReason;
    use casual_calc_transaction::session::Base;

    fn document() -> Vec<u8> {
        include_bytes!("../../../fixtures/generated/minimal.xlsx").to_vec()
    }

    fn policies() -> (SavePolicy, SnapshotPolicy) {
        (
            SavePolicy {
                quiesce_ms: 5_000,
                ceiling_ms: 60_000,
                every_revisions: 200,
                max_callback_attempts: 2,
                retry_base_ms: 500,
            },
            SnapshotPolicy {
                every: 3,
                retain_intervals: 2,
            },
        )
    }

    fn open() -> DocumentSession {
        let (save, snapshots) = policies();
        DocumentSession::open(document(), save, snapshots, 0).expect("the fixture opens")
    }

    fn chunk(seq: u64, base: u64, row: u32, value: f64) -> Submission {
        // A plain value carries no handles, so any workbook will package it.
        let scratch = casual_calc_model::Workbook::new(casual_calc_model::Id::from_parts(9, 9));
        Submission {
            client: ClientId(1),
            seq,
            base: Base::Revision(base),
            ops: vec![WireOperation::of(
                Operation::SetCell {
                    sheet: 0,
                    at: CellRef::new(row, 0),
                    cell: Some(Cell::value(CellValue::Number(value))),
                },
                &scratch,
            )],
        }
    }

    #[test]
    fn a_joining_participant_is_given_the_model_and_the_revision() {
        let mut session = open();
        session.commit(&chunk(1, 0, 4, 11.0), 100).unwrap();

        let joined = session.join().expect("join");
        assert_eq!(joined.revision, session.revision());
        assert!(
            !joined.snapshot.is_empty(),
            "the model, which the browser engine loads directly"
        );
        // It is the model, not the file: a package would start with `PK`.
        assert_ne!(&joined.snapshot[..2], b"PK", "not the .xlsx");
    }

    #[test]
    fn the_assembled_document_is_a_package_with_the_edits_in_it() {
        let mut session = open();
        session.commit(&chunk(1, 0, 6, 4242.0), 100).unwrap();

        let bytes = session.assemble().expect("assembles");
        assert_eq!(&bytes[..2], b"PK", "a real package");

        // Round-trip it to prove the edit is in there rather than trusting the
        // byte count.
        let back = casual_calc_import::import_package(bytes).unwrap().workbook;
        let cell = back.sheets[0].cells.get(CellRef::new(6, 0)).cloned();
        assert_eq!(
            cell.map(|c| c.value),
            Some(CellValue::Number(4242.0)),
            "the committed edit survived the round trip"
        );
    }

    #[test]
    fn a_session_resumed_from_a_snapshot_keeps_editing_where_it_left_off() {
        let (save, snapshots) = policies();
        let mut session = open();
        for seq in 1..=3u64 {
            session
                .commit(&chunk(seq, seq - 1, seq as u32, f64::from(seq as u32)), 100)
                .unwrap();
        }
        let snapshot = session.snapshot().expect("the cadence fired").clone();
        assert_eq!(snapshot.revision, 3);

        let mut revived = DocumentSession::resume(&snapshot, save, snapshots, 0).unwrap();
        assert_eq!(revived.revision(), 3, "resumes at the revision it left");

        revived.commit(&chunk(4, 3, 9, 99.0), 200).unwrap();
        let bytes = revived.assemble().unwrap();
        let back = casual_calc_import::import_package(bytes).unwrap().workbook;
        assert_eq!(
            back.sheets[0]
                .cells
                .get(CellRef::new(9, 0))
                .map(|c| c.value.clone()),
            Some(CellValue::Number(99.0))
        );
    }

    #[test]
    fn editing_stops_once_the_document_cannot_be_saved() {
        // The rule ADR-012 states, enforced here rather than described: a
        // session that provably cannot persist work refuses to take more.
        let mut session = open();
        session.commit(&chunk(1, 0, 1, 1.0), 0).unwrap();
        session.join().unwrap();

        let mut now = 6_000;
        assert!(matches!(
            session.tick(now),
            Some(Action::Save {
                reason: SaveReason::Quiesced,
                ..
            })
        ));
        assert_eq!(
            session.callback(CallbackOutcome::Failed, now),
            Some(Action::WarnNotSaving { attempt: 1 }),
            "told on the first failure, while there is still time"
        );
        now += 600;
        session.tick(now);
        assert_eq!(
            session.callback(CallbackOutcome::Failed, now),
            Some(Action::GoReadOnly)
        );

        assert!(session.is_read_only());
        assert!(matches!(
            session.commit(&chunk(2, 1, 2, 2.0), now),
            Err(ServerError::ReadOnly)
        ));
    }

    #[test]
    fn a_resent_chunk_is_not_applied_twice() {
        let mut session = open();
        let sub = chunk(1, 0, 3, 7.0);
        let Commit::Applied { revision, .. } = session.commit(&sub, 100).unwrap() else {
            panic!("applied")
        };
        assert_eq!(
            session.commit(&sub, 200).unwrap(),
            Commit::Duplicate { revision },
            "the acknowledgement was lost, not the chunk"
        );
        assert_eq!(session.revision(), revision);
    }

    /// A handle that never went through the wire form still means nothing here.
    ///
    /// COL-12 is fixed by packaging operations with what their handles refer
    /// to; this pins what happens when something skips that, because the
    /// failure is silent — the chunk commits, and the writer then drops the
    /// whole cell rather than only its formula.
    #[test]
    fn an_unpackaged_handle_is_detectable_before_it_is_committed() {
        let mut client = casual_calc_import::import_package(document())
            .unwrap()
            .workbook;
        let handle = client.store_formula(casual_calc_formula::parse("1+2").unwrap());
        let mut cell = Cell::value(CellValue::Number(3.0));
        cell.formula = Some(handle);
        let op = Operation::SetCell {
            sheet: 0,
            at: CellRef::new(30, 0),
            cell: Some(cell),
        };

        let session = open();
        assert!(
            casual_calc_transaction::wire::carries_handles(&op),
            "it refers to a formula, so it must be packaged before it is sent"
        );
        assert_eq!(
            casual_calc_transaction::wire::dangling_handle(&op, &session.workbook),
            Some(handle),
            "and against this workbook the handle resolves to nothing, which is \
             what a server checks rather than committing and losing the cell"
        );
    }

    #[test]
    fn values_are_current_in_the_assembled_file() {
        // Recalculation is deferred off the commit path, so this is the check
        // that it still happens before anything reads the values. It needs a
        // *formula* to mean anything: an earlier version of this test set two
        // literals, which need no recalculation at all, and consequently
        // passed with the recalculation removed entirely.
        let mut session = open();
        let mut scratch = casual_calc_import::import_package(document())
            .unwrap()
            .workbook;
        let handle = scratch.store_formula(casual_calc_formula::parse("2*21").unwrap());
        let mut formula_cell = Cell::value(CellValue::Number(0.0));
        formula_cell.formula = Some(handle);

        session
            .commit(
                &Submission {
                    client: ClientId(1),
                    seq: 1,
                    base: Base::Revision(0),
                    ops: vec![WireOperation::of(
                        Operation::SetCell {
                            sheet: 0,
                            at: CellRef::new(25, 0),
                            cell: Some(formula_cell),
                        },
                        &scratch,
                    )],
                },
                100,
            )
            .unwrap();

        let bytes = session.assemble().unwrap();
        let back = casual_calc_import::import_package(bytes).unwrap().workbook;
        assert_eq!(
            back.sheets[0]
                .cells
                .get(CellRef::new(25, 0))
                .map(|c| c.value.clone()),
            Some(CellValue::Number(42.0)),
            "the cached value in the file is the computed one, not the stale zero"
        );
    }
}
