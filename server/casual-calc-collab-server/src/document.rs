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
use casual_calc_transaction::session::{
    Commit, ServerSession, SessionError, Snapshot, SnapshotPolicy, Submission,
};

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
    };

    use super::*;
    use crate::lifecycle::SaveReason;

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
        Submission {
            client: ClientId(1),
            seq,
            base,
            ops: vec![Operation::SetCell {
                sheet: 0,
                at: CellRef::new(row, 0),
                cell: Some(Cell::value(CellValue::Number(value))),
            }],
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

    /// A formula handle is an index into the *sending* workbook's arena, so an
    /// operation carrying one does not mean the same thing on another replica.
    ///
    /// Demonstrated rather than asserted, because this is a gap in the wire
    /// format that ADR-011 assumed away when it called the op set ready to
    /// transmit. Tracked as COL-12.
    #[test]
    fn a_formula_handle_from_another_replica_does_not_travel() {
        // A client interns a formula in *its* workbook and gets handle 0.
        let mut client = casual_calc_import::import_package(document())
            .unwrap()
            .workbook;
        let handle = client.store_formula(casual_calc_formula::parse("1+2").unwrap());
        let mut cell = Cell::value(CellValue::Number(3.0));
        cell.formula = Some(handle);

        // The server's arena does not have it.
        let mut session = open();
        session
            .commit(
                &Submission {
                    client: ClientId(1),
                    seq: 1,
                    base: 0,
                    ops: vec![Operation::SetCell {
                        sheet: 0,
                        at: CellRef::new(30, 0),
                        cell: Some(cell),
                    }],
                },
                100,
            )
            .unwrap();

        let bytes = session.assemble().unwrap();
        let back = casual_calc_import::import_package(bytes).unwrap().workbook;
        assert!(
            back.sheets[0].cells.get(CellRef::new(30, 0)).is_none(),
            "the cell vanished from the written file: its handle indexed the \
             sender's formula arena, which does not exist here, and the writer \
             dropped the whole cell rather than part of it. An operation has to \
             carry the expression itself (COL-12)"
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
        let handle = session
            .workbook
            .store_formula(casual_calc_formula::parse("2*21").unwrap());
        let mut formula_cell = Cell::value(CellValue::Number(0.0));
        formula_cell.formula = Some(handle);

        session
            .commit(
                &Submission {
                    client: ClientId(1),
                    seq: 1,
                    base: 0,
                    ops: vec![Operation::SetCell {
                        sheet: 0,
                        at: CellRef::new(25, 0),
                        cell: Some(formula_cell),
                    }],
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
