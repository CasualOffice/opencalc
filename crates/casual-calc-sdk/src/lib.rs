//! `casual-calc-sdk` — the host-facing engine facade.
//!
//! A single surface that composes the whole pipeline: open a `.xlsx` into a
//! [`WorkbookSession`], read the model, edit through the transaction op set
//! (with undo/redo), recalculate, lay out and render a viewport, and save back.
//! This is the crate a host (a Tauri desktop app, a headless service, the WASM
//! bridge) embeds. The public surface is deliberately narrower than the internal
//! crates. See `docs/02-ARCHITECTURE.md`.

use casual_calc_eval::{recalculate, recalculate_incremental};
use casual_calc_export::{ExportError, write_workbook};
use casual_calc_import::{CompatibilityReport, ImportError, import_package_with};
use casual_calc_layout::{DisplayList, GridGeometry, Viewport, layout_viewport};
use casual_calc_model::{Id, Workbook};
use casual_calc_render::{RenderError, render_png};
use casual_calc_transaction::{History, Operation, TxnError, apply};

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
}

impl core::fmt::Display for SdkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SdkError::Import(e) => write!(f, "{e}"),
            SdkError::Export(e) => write!(f, "{e}"),
            SdkError::Render(e) => write!(f, "{e}"),
            SdkError::Edit(e) => write!(f, "{e}"),
            SdkError::ReadOnly => f.write_str("this workbook is open for reading only"),
        }
    }
}

impl std::error::Error for SdkError {}

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
        let outcome = import_package_with(bytes, config.limits)?;
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
        self.history.apply(&mut self.workbook, op)?;
        // Manual mode still applies the edit — it is calculation that is
        // deferred, not editing — and records that something is outstanding so
        // the host can say so.
        if self.calculation == CalculationMode::Manual {
            self.stale |= !matches!(plan, RecalcPlan::Skip);
            return Ok(());
        }
        match plan {
            RecalcPlan::Skip => {}
            RecalcPlan::Cells(cells) => recalculate_incremental(&mut self.workbook, &cells),
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
    pub fn undo(&mut self) -> Result<(), SdkError> {
        self.history.undo(&mut self.workbook)?;
        self.recalculate_if_automatic();
        Ok(())
    }

    /// Redo the last undone edit, then recalculate.
    pub fn redo(&mut self) -> Result<(), SdkError> {
        self.history.redo(&mut self.workbook)?;
        self.recalculate_if_automatic();
        Ok(())
    }

    /// Recompute all formula cells — Excel's Calculate Now, and the only thing
    /// that brings a manual-mode workbook up to date.
    pub fn recalculate(&mut self) {
        recalculate(&mut self.workbook);
        self.stale = false;
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
        if self.config.read_only {
            return Err(SdkError::ReadOnly);
        }
        Ok(apply(&mut self.workbook, op)?)
    }

    /// A mutable view of the workbook, for programmatic construction/setup.
    pub fn workbook_mut(&mut self) -> &mut Workbook {
        &mut self.workbook
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
    pub fn render_png(
        &self,
        sheet_index: usize,
        viewport: &Viewport,
        dpi: u32,
    ) -> Result<Vec<u8>, SdkError> {
        let geometry = self.geometry(sheet_index);
        let list = layout_viewport(&self.workbook, sheet_index, &geometry, viewport);
        Ok(render_png(&list, &geometry, viewport, dpi)?)
    }

    /// Serialize the workbook to a `.xlsx` package (the semantic writer).
    pub fn save(&self) -> Result<Vec<u8>, SdkError> {
        Ok(write_workbook(&self.workbook)?)
    }
}

impl Default for WorkbookSession {
    fn default() -> Self {
        Self::blank()
    }
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
        // Swaps sheet metadata (merges / sizes / hidden / freeze) — no cell
        // value changes, so nothing to recompute.
        | Operation::SetSheetMetadata { .. }
        // Reordering tabs and recoloring one don't change any value or which
        // sheet name a reference resolves to.
        | Operation::MoveSheet { .. }
        | Operation::SetTabColor { .. } => RecalcPlan::Skip,
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
