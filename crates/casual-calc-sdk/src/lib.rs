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
use casual_calc_import::{CompatibilityReport, ImportError, import_package};
use casual_calc_layout::{DisplayList, GridGeometry, Viewport, layout_viewport};
use casual_calc_model::{Id, Workbook};
use casual_calc_render::{RenderError, render_png};
use casual_calc_transaction::{History, Operation, TxnError, apply};

// Re-export the vocabulary a host needs, so embedders depend on one crate.
pub use casual_calc_layout::Viewport as GridViewport;
pub use casual_calc_model::{Cell, CellRef, CellValue, Sheet, SheetId, Style};
pub use casual_calc_transaction::{Operation as EditOperation, SheetMetadata};

const SESSION_NAMESPACE: u64 = 0x5345_5300_0000_0000; // "SES"

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
}

impl core::fmt::Display for SdkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SdkError::Import(e) => write!(f, "{e}"),
            SdkError::Export(e) => write!(f, "{e}"),
            SdkError::Render(e) => write!(f, "{e}"),
            SdkError::Edit(e) => write!(f, "{e}"),
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
}

impl WorkbookSession {
    /// A new, empty session.
    pub fn blank() -> Self {
        Self {
            workbook: Workbook::new(Id::from_parts(SESSION_NAMESPACE, 1)),
            history: History::new(),
            report: CompatibilityReport::default(),
        }
    }

    /// A session over an already-built workbook (e.g. from a CSV import).
    pub fn from_workbook(mut workbook: Workbook) -> Self {
        recalculate(&mut workbook);
        Self {
            workbook,
            history: History::new(),
            report: CompatibilityReport::default(),
        }
    }

    /// Open a `.xlsx` package, importing and recalculating it.
    pub fn open(bytes: Vec<u8>) -> Result<Self, SdkError> {
        let outcome = import_package(bytes)?;
        let mut workbook = outcome.workbook;
        recalculate(&mut workbook);
        Ok(Self {
            workbook,
            history: History::new(),
            report: outcome.report,
        })
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
        let plan = recalc_plan(&op);
        self.history.apply(&mut self.workbook, op)?;
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
        recalculate(&mut self.workbook);
        Ok(())
    }

    /// Redo the last undone edit, then recalculate.
    pub fn redo(&mut self) -> Result<(), SdkError> {
        self.history.redo(&mut self.workbook)?;
        recalculate(&mut self.workbook);
        Ok(())
    }

    /// Recompute all formula cells.
    pub fn recalculate(&mut self) {
        recalculate(&mut self.workbook);
    }

    /// Apply an edit without recording history (e.g. programmatic setup),
    /// returning the inverse operation.
    pub fn apply_raw(&mut self, op: Operation) -> Result<Operation, SdkError> {
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
