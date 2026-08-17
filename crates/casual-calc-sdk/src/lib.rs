//! `casual-calc-sdk` — the host-facing engine facade.
//!
//! A single surface that composes the whole pipeline: open a `.xlsx` into a
//! [`WorkbookSession`], read the model, edit through the transaction op set
//! (with undo/redo), recalculate, lay out and render a viewport, and save back.
//! This is the crate a host (a Tauri desktop app, a headless service, the WASM
//! bridge) embeds. The public surface is deliberately narrower than the internal
//! crates. See `docs/02-ARCHITECTURE.md`.

use casual_calc_eval::{Recalculated, Recalculator, recalculate, recalculate_cancellable};
use casual_calc_export::{ExportError, write_workbook};
use casual_calc_import::{CompatibilityReport, ImportError, import_package_cancellable};
use casual_calc_layout::{DisplayList, Freeze, GridGeometry, Viewport, layout_viewport, panes};
use casual_calc_model::{Id, Workbook};
use casual_calc_render::{PanePaint, RenderError, render_panes_png};
use casual_calc_transaction::{
    Axis, History, Operation, SheetFields, TxnError, WouldDiscard, apply, undo_would_discard,
};

// Re-export the vocabulary a host needs, so embedders depend on one crate.
pub use casual_calc_layout::Viewport as GridViewport;
pub use casual_calc_model::{Cell, CellRef, CellValue, Sheet, SheetId, Style};
pub use casual_calc_transaction::{Operation as EditOperation, SheetMetadata};

pub use casual_calc_ooxml::OoxmlLimits;

const SESSION_NAMESPACE: u64 = 0x5345_5300_0000_0000; // "SES"

/// When the engine recalculates.
///
/// Read from the file's `<calcPr calcMode>` on open, because a workbook saved
/// in manual mode was saved that way for a reason — usually that a full recalc
/// is slow enough to be disruptive. Recalculating it anyway on the first edit
/// is the opposite of what its author asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalculationMode {
    /// After every edit that can change a value.
    #[default]
    Automatic,
    /// Only when the host asks. Edits still mark the workbook stale, so a host
    /// can show "Calculate" the way Excel shows it in the status bar.
    Manual,
}

impl CalculationMode {
    /// The mode an OOXML `<calcPr calcMode>` token names.
    ///
    /// `autoNoTable` is automatic for everything except data-table cells, which
    /// this engine does not have — so it is automatic, not a third state that
    /// would silently behave like manual.
    #[must_use]
    pub fn from_token(token: &str) -> Self {
        match token {
            "manual" => Self::Manual,
            _ => Self::Automatic,
        }
    }

    /// The token to write back.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Automatic => "auto",
            Self::Manual => "manual",
        }
    }
}

/// The environment a calculation reads instead of the machine it runs on.
///
/// `TODAY()` and `NOW()` come from `now`, and the random functions from `seed`.
/// Supplied rather than sampled because an engine that reaches for the wall
/// clock cannot be tested, replayed, or agreed on by two hosts computing the
/// same workbook — which is the whole determinism contract.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Environment {
    /// The moment `TODAY()`/`NOW()` report, as a date serial.
    pub now: f64,
    /// The seed the random functions draw from. Changing it is what makes
    /// `RAND` reroll; leaving it alone reproduces the previous values exactly.
    pub seed: u64,
}

/// Everything a host can decide about a session.
///
/// Every field here was previously a constant somewhere inside the pipeline,
/// which meant a desktop app opening a file its user chose and a service
/// admitting anonymous uploads got identical behaviour whether or not that
/// suited either of them.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct SessionConfig {
    /// Admission bounds for opening a package. A **security** bound: it caps
    /// what an untrusted file can make the reader allocate before it is
    /// refused.
    pub limits: OoxmlLimits,
    /// When to recalculate. `None` takes the file's own `<calcPr calcMode>`,
    /// which is the right default for an editor; a host that always wants one
    /// or the other says so.
    pub calculation: Option<CalculationMode>,
    /// The clock and seed calculations read.
    pub environment: Environment,
    /// How many edits undo can reverse. `None` is unbounded, which is what a
    /// short-lived session wants and a long-lived one cannot afford: each entry
    /// holds a whole inverse operation, and a metadata edit's inverse is a
    /// snapshot of the sheet's metadata.
    pub undo_depth: Option<usize>,
    /// Refuse every edit — a viewer rather than an editor.
    ///
    /// Enforced **here**, not by hiding buttons. A read-only mode that only
    /// removes the toolbar is read-only until someone calls the API, which
    /// makes it a suggestion rather than a mode. Reading, selecting, scrolling
    /// and copying all still work: a viewer you cannot copy out of is hostile,
    /// and copying changes nothing.
    pub read_only: bool,
}

impl SessionConfig {
    /// The defaults: stock limits, the file's own calculation mode, a zero
    /// clock, and unbounded undo.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bound how far back undo can reach.
    #[must_use]
    pub fn with_undo_depth(mut self, depth: usize) -> Self {
        self.undo_depth = Some(depth);
        self
    }

    /// Force a calculation mode regardless of what the file asks for.
    #[must_use]
    pub fn with_calculation(mut self, mode: CalculationMode) -> Self {
        self.calculation = Some(mode);
        self
    }

    /// Set the clock and seed calculations read.
    #[must_use]
    pub fn with_environment(mut self, environment: Environment) -> Self {
        self.environment = environment;
        self
    }

    /// Set the package admission bounds.
    #[must_use]
    pub fn with_limits(mut self, limits: OoxmlLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Open the workbook for reading only.
    #[must_use]
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }
}

/// An error from an SDK operation.
#[derive(Debug)]
#[non_exhaustive]
pub enum SdkError {
    /// Import failed.
    Import(ImportError),
    /// Export failed.
    Export(ExportError),
    /// A render failed.
    Render(RenderError),
    /// An edit operation failed.
    Edit(TxnError),
    /// The session is read-only and refused an edit.
    ReadOnly,
    /// The workbook is not in a state this engine will write out.
    ///
    /// Reached through [`workbook_mut`](WorkbookSession::workbook_mut), which
    /// hands a host the right to change anything (`SDK-008`). Returned rather
    /// than written, because a corrupt workbook in memory is a bug and one that
    /// became a file is data loss.
    Model(casual_calc_model::ModelError),
    /// Undo would have deleted a band somebody else has since filled (docs/69,
    /// `COL-28`). Refused, and refused loudly: the alternative is a structural
    /// undo that destroys work no undo stack anywhere can bring back.
    UndoWouldDiscard(WouldDiscard),
}

impl core::fmt::Display for SdkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SdkError::Import(e) => write!(f, "{e}"),
            SdkError::Export(e) => write!(f, "{e}"),
            SdkError::Render(e) => write!(f, "{e}"),
            SdkError::Edit(e) => write!(f, "{e}"),
            SdkError::Model(e) => write!(f, "{e}"),
            SdkError::ReadOnly => f.write_str("this workbook is open for reading only"),
            SdkError::UndoWouldDiscard(what) => {
                let (line, kind) = match what.axis {
                    Axis::Row => (what.at + 1, if what.count == 1 { "row" } else { "rows" }),
                    Axis::Col => (
                        what.at + 1,
                        if what.count == 1 { "column" } else { "columns" },
                    ),
                };
                write!(
                    f,
                    "undo would remove {} {kind} starting at {line}, and somebody has since \
                     put data there ({} cell{}). Undo it from their end, or clear the {kind} first.",
                    what.count,
                    what.cells,
                    if what.cells == 1 { "" } else { "s" },
                )
            }
        }
    }
}

impl std::error::Error for SdkError {}

impl From<casual_calc_model::ModelError> for SdkError {
    fn from(e: casual_calc_model::ModelError) -> Self {
        SdkError::Model(e)
    }
}

impl From<ImportError> for SdkError {
    fn from(e: ImportError) -> Self {
        SdkError::Import(e)
    }
}
impl From<ExportError> for SdkError {
    fn from(e: ExportError) -> Self {
        SdkError::Export(e)
    }
}
impl From<RenderError> for SdkError {
    fn from(e: RenderError) -> Self {
        SdkError::Render(e)
    }
}
impl From<TxnError> for SdkError {
    fn from(e: TxnError) -> Self {
        SdkError::Edit(e)
    }
}

/// An open workbook and its editing state.
#[derive(Debug)]
pub struct WorkbookSession {
    workbook: Workbook,
    history: History,
    report: CompatibilityReport,
    config: SessionConfig,
    /// The mode in force, after resolving the config against the file.
    calculation: CalculationMode,
    /// Whether an edit has changed a value since the last recalculation. Only
    /// meaningful in manual mode; automatic never leaves it set.
    stale: bool,
    /// Recalculation, and from step three of docs/66 the precedent graph it
    /// keeps between edits.
    recalc: Recalculator,
    /// Operations applied since the host last collected them, **narrowed**.
    ///
    /// Off unless a host asks for it. A collaborative host has to send what it
    /// applied, and the alternative is threading a return value through every
    /// one of the forty-odd entry points that edit — which is the same thing
    /// done forty times, and wrong the first time somebody adds the forty-first.
    ///
    /// Narrowed here rather than by the collector, because narrowing needs the
    /// state the operation was written against and this is the last moment that
    /// state exists.
    applied: Option<Vec<Operation>>,
    /// The package this session was opened from, kept while nothing has been
    /// edited so that saving an untouched file returns it unchanged (P1B-002).
    ///
    /// Dropped the moment anything is edited, which both frees the memory and
    /// makes the invariant impossible to get wrong: there is no way to hand
    /// back stale bytes, because after an edit there are no bytes to hand back.
    ///
    /// The cost is the package's own size, held until the first edit. For a
    /// host that opens from a `Vec<u8>` and drops it — which is the normal
    /// shape — this is the only copy rather than a second one.
    source: Option<Vec<u8>>,
}

impl WorkbookSession {
    /// A new, empty session with the default configuration.
    pub fn blank() -> Self {
        Self::blank_with(SessionConfig::new())
    }

    /// A new, empty session.
    pub fn blank_with(config: SessionConfig) -> Self {
        let mut workbook = Workbook::new(Id::from_parts(SESSION_NAMESPACE, 1));
        apply_environment(&mut workbook, &config);
        Self {
            calculation: config.calculation.unwrap_or_default(),
            history: History::with_depth(config.undo_depth),
            workbook,
            report: CompatibilityReport::default(),
            config,
            stale: false,
            applied: None,
            recalc: Recalculator::new(),
            // Not opened from a package, so there is nothing to give back
            // unchanged.
            source: None,
        }
    }

    /// A session over an already-built workbook (e.g. from a CSV import).
    pub fn from_workbook(workbook: Workbook) -> Self {
        Self::from_workbook_with(workbook, SessionConfig::new())
    }

    /// A session over an already-built workbook, configured.
    pub fn from_workbook_with(mut workbook: Workbook, config: SessionConfig) -> Self {
        apply_environment(&mut workbook, &config);
        recalculate(&mut workbook);
        Self {
            calculation: config.calculation.unwrap_or_default(),
            history: History::with_depth(config.undo_depth),
            workbook,
            report: CompatibilityReport::default(),
            config,
            stale: false,
            applied: None,
            recalc: Recalculator::new(),
            // Not opened from a package, so there is nothing to give back
            // unchanged.
            source: None,
        }
    }

    /// Open a `.xlsx` package, importing and recalculating it.
    pub fn open(bytes: Vec<u8>) -> Result<Self, SdkError> {
        Self::open_with(bytes, SessionConfig::new())
    }

    /// Open a `.xlsx` package under a given configuration.
    ///
    /// A workbook saved in manual calculation mode is opened in manual mode
    /// unless the config says otherwise — and is **not** recalculated on the
    /// way in. Its author turned automatic calculation off, usually because a
    /// full recalc is slow enough to be disruptive; doing one anyway before
    /// they have seen the file is the opposite of what they asked for.
    pub fn open_with(bytes: Vec<u8>, config: SessionConfig) -> Result<Self, SdkError> {
        Self::open_cancellable(bytes, config, &casual_calc_model::Never)
    }

    /// The same, with a way to stop it.
    ///
    /// Opening is the longest thing a session does, and until `SEC-012` the
    /// only way out of it was for it to finish — on a browser's single thread,
    /// which docs/07 and docs/21 both said would not be the case. A cancelled
    /// open returns [`SdkError::Import`] carrying `ImportError::Cancelled` and
    /// leaves no session behind.
    ///
    /// # Errors
    ///
    /// As [`open_with`](Self::open_with), plus a cancellation.
    pub fn open_cancellable(
        bytes: Vec<u8>,
        config: SessionConfig,
        cancel: &dyn casual_calc_model::Cancel,
    ) -> Result<Self, SdkError> {
        // Kept before the import consumes it: an untouched file saves as
        // itself, and that is only possible if the original survives the read.
        let source = bytes.clone();
        let outcome = import_package_cancellable(bytes, config.limits, cancel)?;
        let mut workbook = outcome.workbook;
        apply_environment(&mut workbook, &config);
        let calculation = config
            .calculation
            .unwrap_or_else(|| file_calculation_mode(&workbook));
        if calculation == CalculationMode::Automatic {
            recalculate(&mut workbook);
        }
        Ok(Self {
            workbook,
            history: History::with_depth(config.undo_depth),
            report: outcome.report,
            config,
            calculation,
            // A manual-mode file arrives with its own cached values, which are
            // what its author last saw; nothing is stale until something is
            // edited.
            stale: false,
            applied: None,
            recalc: Recalculator::new(),
            source: Some(source),
        })
    }

    /// The configuration in force.
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// The calculation mode in force, after resolving the config against the
    /// file's own `<calcPr calcMode>`.
    pub fn calculation_mode(&self) -> CalculationMode {
        self.calculation
    }

    /// Switch calculation mode. Switching to automatic recalculates at once if
    /// anything is outstanding — the point of the mode is that values are
    /// current.
    pub fn set_calculation_mode(&mut self, mode: CalculationMode) {
        self.calculation = mode;
        self.config.calculation = Some(mode);
        write_calculation_mode(&mut self.workbook, mode);
        if mode == CalculationMode::Automatic && self.stale {
            self.recalculate();
        }
    }

    /// Whether an edit has changed a value that has not been recalculated.
    /// Always false in automatic mode.
    pub fn needs_recalculation(&self) -> bool {
        self.stale
    }

    /// Replace the clock and seed calculations read.
    ///
    /// Recalculates in automatic mode: the volatile functions have new answers
    /// the moment this changes, and leaving the old ones on screen while
    /// claiming the clock moved is worse than the cost of the pass.
    pub fn set_environment(&mut self, environment: Environment) {
        self.config.environment = environment;
        apply_environment(&mut self.workbook, &self.config);
        if self.calculation == CalculationMode::Automatic {
            recalculate(&mut self.workbook);
        } else {
            self.stale = true;
        }
    }

    /// The normalized workbook model.
    pub fn workbook(&self) -> &Workbook {
        &self.workbook
    }

    /// The compatibility report from import (empty for a blank session).
    pub fn compatibility_report(&self) -> &CompatibilityReport {
        &self.report
    }

    /// Apply an edit operation, then recalculate. Records undo history.
    ///
    /// The recalc is scoped to what the edit can affect: value edits recompute
    /// only the changed cells' transitive dependents (incremental); pure style
    /// or geometry edits skip recalc entirely; structural edits (insert/delete
    /// rows or columns), which shift references workbook-wide, do a full recalc.
    pub fn edit(&mut self, op: Operation) -> Result<(), SdkError> {
        // Before anything else, and before the history records a step: a
        // refused edit must leave no trace, or undo has an entry that undoes
        // nothing.
        if self.config.read_only {
            return Err(SdkError::ReadOnly);
        }
        let plan = recalc_plan(&op);
        // Narrowed **before** the edit is applied, because afterwards the state
        // it was written against is gone and an operation still claiming to
        // change everything contends with every concurrent edit.
        //
        // Recorded **after** it succeeds, which is a different question and used
        // to have the same answer. A refused edit was already in the outgoing
        // log by the time `apply` said no, so this client would send the server
        // — and through it every peer — an operation it had itself rejected.
        // Nothing downstream can detect that: the operation is well formed, it
        // is simply not what happened here.
        let candidate = self
            .applied
            .is_some()
            .then(|| op.clone().narrowed(&self.workbook));
        self.history.apply(&mut self.workbook, op)?;
        if let Some(narrowed) = candidate
            && let Some(log) = self.applied.as_mut()
        {
            log.push(narrowed);
        }
        self.source = None;
        // Dropped **before** the manual-mode return, not inside the match
        // below. Whether a recalculation is wanted right now and whether the
        // graph still describes this document are different questions, and
        // answering only the first left manual mode holding a graph about a
        // document that had moved under it — the deferred recalculation then
        // ran against it and was wrong in exactly the way deferring was
        // supposed to be safe.
        if matches!(plan, RecalcPlan::Full) {
            self.recalc.invalidate();
        }
        // Manual mode still applies the edit — it is calculation that is
        // deferred, not editing — and records that something is outstanding so
        // the host can say so.
        if self.calculation == CalculationMode::Manual {
            self.stale |= !matches!(plan, RecalcPlan::Skip);
            return Ok(());
        }
        match plan {
            RecalcPlan::Skip => {}
            RecalcPlan::Cells(cells) => self.recalc.recalculate(&mut self.workbook, &cells),
            // The graph was already dropped above; a structural edit shifts
            // every reference past its insertion point, so whatever it said
            // was about a document that no longer exists.
            RecalcPlan::Full => recalculate(&mut self.workbook),
        }
        Ok(())
    }

    /// Whether an edit can be undone.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// What undo would reverse, for a menu label.
    pub fn undo_label(&self) -> Option<&'static str> {
        self.history.undo_label()
    }

    /// What redo would reapply.
    pub fn redo_label(&self) -> Option<&'static str> {
        self.history.redo_label()
    }

    /// Whether an undone edit can be redone.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Undo the last edit, then recalculate.
    ///
    /// **An undo is an edit**, and travels like one. It changes the document, so
    /// a collaborating host has to send it; until it did, one participant
    /// reverted while the server and every peer kept the change — a divergence
    /// nothing later disagrees loudly enough to reveal, and one that no amount
    /// of subsequent editing repairs.
    ///
    /// It rides the same outgoing log as [`edit`](Self::edit) rather than a
    /// second channel: the server transforms whatever arrives against everything
    /// committed since the sender's revision, which is exactly the treatment an
    /// inverse needs and already exists.
    pub fn undo(&mut self) -> Result<(), SdkError> {
        // Before anything is applied, because the state this would destroy is
        // the evidence that it must not be (docs/69). Cell edits are allowed to
        // clobber — that is last-writer-wins, and the value is one cell somebody
        // else's stack still holds. A structural undo is different in kind: it
        // takes work that was never in the band when the undo was recorded, and
        // no undo stack anywhere contains it.
        if let Some(next) = self.history.peek_undo()
            && let Some(blocked) = undo_would_discard(&self.workbook, next)
        {
            return Err(SdkError::UndoWouldDiscard(blocked));
        }
        let applied = self.history.undo(&mut self.workbook)?;
        self.record_for_peers(applied);
        self.source = None;
        // Undo replays whatever it reverses, which may have been structural.
        // The history does not say which, and guessing to keep a graph is
        // exactly the trade that makes staleness possible for a saving that
        // does not matter here: undo is not a keystroke.
        self.recalc.invalidate();
        self.recalculate_if_automatic();
        Ok(())
    }

    /// Redo the last undone edit, then recalculate.
    ///
    /// A redo is a fresh intention rather than the cancellation of one, and is
    /// transmitted for the same reason as [`undo`](Self::undo).
    pub fn redo(&mut self) -> Result<(), SdkError> {
        let applied = self.history.redo(&mut self.workbook)?;
        self.record_for_peers(applied);
        self.source = None;
        self.recalc.invalidate();
        self.recalculate_if_automatic();
        Ok(())
    }

    /// Queue an already-applied operation for collaborators.
    ///
    /// Narrowed against the workbook **after** the operation ran, unlike
    /// [`edit`](Self::edit), which must narrow before. The difference is which
    /// state the operation describes: an edit is written against the document
    /// the user was looking at, while an undo's inverse has just been executed
    /// and is described by the document it produced.
    ///
    /// Nothing to do when the stack was empty — pressing undo with nothing to
    /// undo is not an event anybody needs to hear about.
    fn record_for_peers(&mut self, applied: Option<Operation>) {
        let (Some(op), Some(_)) = (applied, self.applied.as_ref()) else {
            return;
        };
        let narrowed = op.narrowed(&self.workbook);
        if let Some(log) = self.applied.as_mut() {
            log.push(narrowed);
        }
    }

    /// Start recording what this session applies, for a host that has to send
    /// it on.
    pub fn record_applied(&mut self) {
        self.applied.get_or_insert_with(Vec::new);
    }

    /// Stop recording, discarding anything uncollected.
    pub fn stop_recording(&mut self) {
        self.applied = None;
    }

    /// Whether anything applied is waiting to be collected.
    ///
    /// A read, where [`take_applied`](Self::take_applied) is a take. A host
    /// asking "is there unsaved work" must not empty the queue to find out, and
    /// must not miss what is sitting here: between an edit and the next flush,
    /// this buffer is the *only* place that work exists.
    #[must_use]
    pub fn has_applied(&self) -> bool {
        self.applied.as_ref().is_some_and(|log| !log.is_empty())
    }

    /// Take everything applied since the last call.
    #[must_use]
    pub fn take_applied(&mut self) -> Vec<Operation> {
        self.applied
            .as_mut()
            .map(core::mem::take)
            .unwrap_or_default()
    }

    /// Discard the undo history, making the current state the document's
    /// starting point.
    ///
    /// Call it after populating a fresh session, the way a host seeds a
    /// template or restores a document from its own store. Those writes go
    /// through [`edit`](Self::edit) so they recalculate and stay consistent,
    /// which also means they land on the undo stack — and a user who presses
    /// Ctrl+Z enough times then walks backwards out of the document they were
    /// given, one cell at a time, into an empty sheet. No spreadsheet does
    /// that, because opening a file is not an edit.
    ///
    /// [`open`](Self::open) has no need of it: it builds the workbook by import
    /// rather than by editing, so its history is empty already.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Recompute all formula cells — Excel's Calculate Now, and the only thing
    /// that brings a manual-mode workbook up to date.
    pub fn recalculate(&mut self) {
        recalculate(&mut self.workbook);
        self.stale = false;
    }

    /// The same, with a way to stop it.
    ///
    /// A cancelled recalculation **keeps what it computed** and leaves the
    /// session marked stale, so the next automatic pass finishes the job. That
    /// is the difference from a cancelled open: there is no half-built document
    /// to throw away, only a document whose cached values were already stale
    /// when this started.
    pub fn recalculate_cancellable(
        &mut self,
        cancel: &dyn casual_calc_model::Cancel,
    ) -> Recalculated {
        let outcome = recalculate_cancellable(&mut self.workbook, cancel);
        self.stale = outcome != Recalculated::Fully;
        outcome
    }

    fn recalculate_if_automatic(&mut self) {
        if self.calculation == CalculationMode::Automatic {
            self.recalculate();
        } else {
            self.stale = true;
        }
    }

    /// Whether this session refuses edits.
    pub fn is_read_only(&self) -> bool {
        self.config.read_only
    }

    /// Change the configuration of a live session.
    ///
    /// Mutable rather than replace-only because a host toggles read-only far
    /// more often than it opens a workbook — a preview that becomes editable
    /// once permissions load should not have to reopen the file to say so.
    pub fn config_mut(&mut self) -> &mut SessionConfig {
        &mut self.config
    }

    /// Apply an edit without recording history (e.g. programmatic setup),
    /// returning the inverse operation.
    ///
    /// Refused in a read-only session too. This is the path a host reaches for
    /// when it wants to bypass undo, and a read-only mode with a documented
    /// bypass is not one.
    pub fn apply_raw(&mut self, op: Operation) -> Result<Operation, SdkError> {
        // Refusal first. Clearing `source` before deciding whether to refuse
        // threw away the untouched-original guarantee on behalf of an edit that
        // never happened — the same "a refused edit must leave no trace" rule
        // the outgoing log above now keeps.
        if self.config.read_only {
            return Err(SdkError::ReadOnly);
        }
        self.source = None;
        // The op is applied without classification, so it may have been a
        // structural one. Same reasoning as `workbook_mut` below.
        self.recalc.invalidate();
        Ok(apply(&mut self.workbook, op)?)
    }

    /// A mutable view of the workbook, for programmatic construction/setup.
    pub fn workbook_mut(&mut self) -> &mut Workbook {
        // Handing out a `&mut Workbook` is handing out the right to change
        // anything, and this cannot see what happens next — so the untouched
        // guarantee ends here whether or not the caller writes a single byte.
        // Conservative on purpose: giving back a stale package is silent data
        // loss, and re-serializing an unchanged workbook costs only time.
        self.source = None;
        // And for exactly that reason the kept precedent graph ends here too: a
        // caller may rewrite every formula in the book through this reference
        // and nothing would observe it.
        self.recalc.invalidate();
        &mut self.workbook
    }

    /// Which scripts this document contains that the renderer cannot draw.
    ///
    /// Ask before shipping a PNG. Fonts are supplied by the host rather than
    /// bundled ([ADR-018](../../../docs/64-TEXT-SHAPING.md)), which is the right
    /// trade and leaves one sharp edge: a sheet in a script nobody registered
    /// renders as a row of boxes, and a box looks like a bug in the renderer
    /// rather than a font that was never installed. This is the difference
    /// between a support ticket and a sentence naming what to install.
    ///
    /// Empty is the answer for the overwhelming majority of documents, and an
    /// empty vector costs one pass over the string table to obtain.
    ///
    /// Only the **editor** is exempt from caring: it draws through the browser,
    /// which supplies its own faces. This is about the headless renderer —
    /// thumbnails, previews, server-side exports.
    #[must_use]
    pub fn missing_font_coverage(&self) -> Vec<casual_calc_render::MissingScript> {
        // The string table holds every piece of text in the document exactly
        // once, which is both cheaper than walking cells and more complete:
        // a string is there whether or not any cell currently shows it.
        let mut text = String::new();
        for s in self.workbook.strings.iter() {
            text.push_str(s);
        }
        // Sheet names are drawn by the editor's tab strip rather than by this
        // renderer, so they are deliberately not included: reporting a script
        // that no PNG will ever contain is a false alarm, and a false alarm in
        // a diagnostic is worse than no diagnostic.
        casual_calc_render::missing_scripts(&text)
    }

    /// The grid geometry (column widths / row heights) of a sheet.
    fn geometry(&self, sheet_index: usize) -> GridGeometry {
        self.workbook
            .sheets
            .get(sheet_index)
            .map(GridGeometry::for_sheet)
            .unwrap_or_default()
    }

    /// Lay out a viewport of a sheet into a display list.
    pub fn layout(&self, sheet_index: usize, viewport: &Viewport) -> DisplayList {
        layout_viewport(
            &self.workbook,
            sheet_index,
            &self.geometry(sheet_index),
            viewport,
        )
    }

    /// Render a viewport of a sheet to PNG bytes.
    ///
    /// Honours the sheet's frozen rows and columns: the pinned bands hold still
    /// and the body scrolls under them, as they do in the editor canvas. An
    /// unfrozen sheet renders exactly as before.
    pub fn render_png(
        &self,
        sheet_index: usize,
        viewport: &Viewport,
        dpi: u32,
    ) -> Result<Vec<u8>, SdkError> {
        let geometry = self.geometry(sheet_index);
        render_sheet_png(&self.workbook, sheet_index, &geometry, viewport, dpi)
    }

    /// Serialize the workbook to a `.xlsx` package.
    ///
    /// A file that was **opened and not edited saves as itself, byte for byte**
    /// (P1B-002). That is not an optimisation: the semantic writer reconstructs
    /// canonical OOXML, so anything this engine does not model is rebuilt from
    /// what it *does* model, and a part it merely retained comes back in a
    /// package that is no longer the one the author had. Opening a workbook to
    /// look at it and saving it should not rewrite it.
    ///
    /// After any edit the semantic writer runs, and the guarantee narrows to the
    /// documented one: retained parts survive, and the model round-trips to an
    /// equal model.
    ///
    /// The limit worth knowing: an unedited save returns the file's **own**
    /// cached values, not the ones this engine computed on open. If the two
    /// disagree the file was already inconsistent, and reproducing it exactly is
    /// the more conservative answer — this engine does not model every construct
    /// that might explain the difference, so overwriting the author's cached
    /// values on the strength of its own recalculation is the riskier of the two.
    pub fn save(&self) -> Result<Vec<u8>, SdkError> {
        if let Some(source) = &self.source {
            // The original bytes, untouched. Nothing has edited them, so there
            // is nothing to validate — and validating here would refuse to hand
            // back a file this engine merely does not fully model.
            return Ok(source.clone());
        }
        // **The last place an invalid workbook can be stopped** (`SDK-008`).
        //
        // `workbook_mut` hands a host the right to change anything, including
        // into a state the model calls invalid: a duplicate sheet id, a cell
        // pointing at a string that was never interned. The session cannot see
        // what happens through that reference, and checking on every call is
        // not affordable — `validate` walks every cell, and hosts reach for
        // that reference per keystroke.
        //
        // Here it is affordable, because writing the package already walks
        // every cell, and here is where being wrong stops being recoverable: a
        // corrupt workbook that stayed in memory is a bug, and one that became
        // a file the author will open tomorrow is data loss.
        self.workbook.validate()?;
        Ok(write_workbook(&self.workbook)?)
    }

    /// Whether saving now would return the opened file unchanged.
    ///
    /// A host uses it to leave "Save" disabled, or to skip a write it does not
    /// need. `false` for a session that was not opened from a package, and for
    /// one where anything has been edited.
    #[must_use]
    pub fn is_unmodified(&self) -> bool {
        self.source.is_some()
    }
}

impl Default for WorkbookSession {
    fn default() -> Self {
        Self::blank()
    }
}

/// Render a sheet's viewport to PNG bytes, splitting its frozen panes.
///
/// The composition step of the headless renderer: it reads the sheet's frozen
/// row and column counts, splits the viewport into the panes they imply, lays
/// out each one, and hands the set to the render backend. A sheet with nothing
/// frozen yields a single pane and the same bytes the unsplit path produced.
///
/// Free-standing because more than one host needs it — the session below and
/// the WASM `render_xlsx` entry point — and a freeze honoured in one renderer
/// and not the other is the gap this closes rather than moves.
pub fn render_sheet_png(
    workbook: &Workbook,
    sheet_index: usize,
    geometry: &GridGeometry,
    viewport: &Viewport,
    dpi: u32,
) -> Result<Vec<u8>, SdkError> {
    let freeze = workbook
        .sheets
        .get(sheet_index)
        .map(|sheet| Freeze {
            rows: sheet.view.frozen_rows,
            cols: sheet.view.frozen_cols,
        })
        .unwrap_or_default();

    let regions = panes(geometry, viewport, freeze);
    let lists: Vec<DisplayList> = regions
        .iter()
        .map(|pane| layout_viewport(workbook, sheet_index, geometry, &pane.viewport))
        .collect();
    let paints: Vec<PanePaint<'_>> = regions
        .iter()
        .zip(&lists)
        .map(|(pane, display_list)| PanePaint {
            pane: *pane,
            display_list,
        })
        .collect();

    Ok(render_panes_png(&paints, geometry, viewport, dpi)?)
}

/// How an operation should be recalculated.
enum RecalcPlan {
    /// No value can have changed (pure style / geometry edit) — skip recalc.
    Skip,
    /// Value edits: recompute the transitive dependents of these cells.
    Cells(Vec<(usize, CellRef)>),
    /// Reference-shifting edits (insert/delete rows or columns) — full recalc.
    Full,
}

/// Classify an operation's recalc scope. A `Batch` is `Full` if any member is,
/// otherwise the union of its members' changed cells (or `Skip` if none touch
/// values).
fn recalc_plan(op: &Operation) -> RecalcPlan {
    match op {
        Operation::SetCell { sheet, at, .. }
        | Operation::SetValue { sheet, at, .. }
        | Operation::ClearCell { sheet, at } => RecalcPlan::Cells(vec![(*sheet, *at)]),
        Operation::SetStyle { .. }
        | Operation::SetColumnWidth { .. }
        | Operation::SetRowHeight { .. }
        | Operation::SetTabColor { .. } => RecalcPlan::Skip,
        // Most of this bundle is presentation, but two of its fields are read
        // by the evaluator: `SUBTOTAL`'s 101–111 codes and `AGGREGATE` skip
        // hidden rows, and `Sheet::is_row_hidden` is the union of the
        // hand-hidden set and the set the autofilter hides. So applying,
        // changing or clearing a filter changes what a subtotal *is* — while
        // changing nothing the dependency graph can see, since no cell was
        // written. Nothing localizes it either: the graph records which cells a
        // formula reads, and "the visibility of the rows under it" is not a
        // cell. Hence full, and only for the two fields that can do it.
        Operation::SetSheetMetadata { changed, .. } => {
            if changed.intersects(SheetFields::HIDDEN_ROWS.union(SheetFields::FILTER_HIDDEN)) {
                RecalcPlan::Full
            } else {
                RecalcPlan::Skip
            }
        }
        // Reordering tabs changes values, which is not the obvious part. It
        // was classified `Skip` on the grounds that no reference resolves
        // differently — true, and not the whole question. `SHEET()` returns a
        // sheet's *position*, so a tab drag changes its result; and the kept
        // precedent graph is keyed by sheet **index**, which `MoveSheet`
        // renumbers wholesale by removing and re-inserting. Left as `Skip`,
        // the graph went on describing the old numbering and every later edit
        // to the moved sheet stopped propagating — silently, permanently, and
        // into the saved file.
        Operation::MoveSheet { .. } => RecalcPlan::Full,
        Operation::InsertRows { .. }
        | Operation::DeleteRows { .. }
        | Operation::InsertColumns { .. }
        | Operation::DeleteColumns { .. }
        // Adding, removing, or renaming a sheet changes which name a cross-sheet
        // reference resolves to (or turns it into #REF!), so recompute fully.
        | Operation::InsertSheet { .. }
        | Operation::RemoveSheet { .. }
        | Operation::RenameSheet { .. }
        // Defining, renaming, or deleting a name changes what any formula
        // referencing it resolves to (or turns it into #NAME?).
        | Operation::SetDefinedNames(_) => RecalcPlan::Full,
        Operation::Batch(ops) => {
            let mut cells = Vec::new();
            for o in ops {
                match recalc_plan(o) {
                    RecalcPlan::Full => return RecalcPlan::Full,
                    RecalcPlan::Cells(mut c) => cells.append(&mut c),
                    RecalcPlan::Skip => {}
                }
            }
            if cells.is_empty() {
                RecalcPlan::Skip
            } else {
                RecalcPlan::Cells(cells)
            }
        }
    }
}

#[cfg(test)]
mod tests;

/// Install the configured clock and seed on the workbook.
///
/// They live on the model rather than in the evaluator because a formula's
/// answer has to be reproducible from the workbook alone — hand the same model
/// and the same environment to two hosts and they agree.
fn apply_environment(workbook: &mut Workbook, config: &SessionConfig) {
    workbook.volatile_now = config.environment.now;
    workbook.volatile_seed = config.environment.seed;
}

/// The calculation mode the file itself asks for.
fn file_calculation_mode(workbook: &Workbook) -> CalculationMode {
    workbook
        .settings
        .calc
        .get("calcMode")
        .map_or(CalculationMode::Automatic, |m| {
            CalculationMode::from_token(m)
        })
}

/// Record the mode so a save carries it, as Excel does.
///
/// Without this, turning calculation off and saving produced a file that turns
/// itself back on when reopened — and the reason it was turned off does not go
/// away just because the file was closed.
fn write_calculation_mode(workbook: &mut Workbook, mode: CalculationMode) {
    match mode {
        // `auto` is the schema default, so it is written by omission — leaving
        // the attribute behind would be a difference in a file nobody changed.
        CalculationMode::Automatic => {
            workbook.settings.calc.remove("calcMode");
        }
        CalculationMode::Manual => {
            workbook
                .settings
                .calc
                .insert("calcMode".to_owned(), mode.token().to_owned());
        }
    }
}
