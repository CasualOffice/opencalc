//! `casual-calc-transaction` — atomic, invertible edit operations.
//!
//! Increment 1 (the Edit dimension): the closed cell-level operation set. Every
//! [`apply`] returns the **inverse** operation, so undo/redo is inverse replay
//! and never a separate implementation that can drift. All mutation of the model
//! flows through here — the transaction contract in
//! `docs/24-TRANSACTION-AND-EDIT-SEMANTICS.md`.
//!
//! Covered now: set a cell's value / style, set or clear a whole cell, and an
//! atomic [`Operation::Batch`]. Structural ops (insert/delete rows & columns with
//! formula-reference rewriting) are the next increment.

use std::collections::BTreeSet;

use casual_calc_formula::{Expr, rename_sheet_references};
use casual_calc_model::{
    AxisSizing, Cell, CellRange, CellRef, CellValue, DefinedName, Sheet, SheetView, StyleId,
    Workbook,
};

mod structural;

use structural::Axis;

/// An error applying an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TxnError {
    /// The target sheet does not exist.
    SheetNotFound {
        /// The sheet index.
        index: usize,
    },
}

impl TxnError {
    /// The stable diagnostic code (`docs/20`).
    pub fn code(&self) -> &'static str {
        "OC-TXN-0001"
    }
}

impl core::fmt::Display for TxnError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TxnError::SheetNotFound { index } => {
                write!(f, "[{}] sheet {index} not found", self.code())
            }
        }
    }
}

impl std::error::Error for TxnError {}

/// A closed set of atomic edit operations. Every operation is invertible; the
/// inverse of any operation is expressible as a `SetCell` (or a `Batch` of them).
#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    /// Replace a whole cell (or clear it with `None`). This is the primitive and
    /// the universal inverse form.
    SetCell {
        /// Sheet index.
        sheet: usize,
        /// Cell address.
        at: CellRef,
        /// The new cell, or `None` to clear.
        cell: Option<Cell>,
    },
    /// Set a cell's value, preserving its style and clearing any formula.
    SetValue {
        /// Sheet index.
        sheet: usize,
        /// Cell address.
        at: CellRef,
        /// The new value.
        value: CellValue,
    },
    /// Set (or clear) a cell's style, preserving its value and formula.
    SetStyle {
        /// Sheet index.
        sheet: usize,
        /// Cell address.
        at: CellRef,
        /// The new style, or `None` for the default.
        style: Option<StyleId>,
    },
    /// Clear a cell entirely.
    ClearCell {
        /// Sheet index.
        sheet: usize,
        /// Cell address.
        at: CellRef,
    },
    /// Set (or clear, with `None`) a column's explicit width in twips.
    SetColumnWidth {
        /// Sheet index.
        sheet: usize,
        /// Zero-based column.
        col: u32,
        /// The new width (twips), or `None` to revert to the sheet default.
        width: Option<i64>,
    },
    /// Set (or clear, with `None`) a row's explicit height in twips.
    SetRowHeight {
        /// Sheet index.
        sheet: usize,
        /// Zero-based row.
        row: u32,
        /// The new height (twips), or `None` to revert to the sheet default.
        height: Option<i64>,
    },
    /// Insert `count` blank rows at row `at`, shifting rows on/after `at` down
    /// and rewriting formula references that target this sheet.
    InsertRows {
        /// Sheet index.
        sheet: usize,
        /// Zero-based row the inserted band begins at.
        at: u32,
        /// Number of rows to insert.
        count: u32,
    },
    /// Delete `count` rows starting at row `at`, shifting rows on/after
    /// `at + count` up and rewriting formula references that target this sheet
    /// (references onto a deleted row become `#REF!`).
    DeleteRows {
        /// Sheet index.
        sheet: usize,
        /// Zero-based row the deleted band begins at.
        at: u32,
        /// Number of rows to delete.
        count: u32,
    },
    /// Insert `count` blank columns at column `at`, shifting columns on/after
    /// `at` right and rewriting formula references that target this sheet.
    InsertColumns {
        /// Sheet index.
        sheet: usize,
        /// Zero-based column the inserted band begins at.
        at: u32,
        /// Number of columns to insert.
        count: u32,
    },
    /// Delete `count` columns starting at column `at`, shifting columns on/after
    /// `at + count` left and rewriting formula references that target this sheet
    /// (references onto a deleted column become `#REF!`).
    DeleteColumns {
        /// Sheet index.
        sheet: usize,
        /// Zero-based column the deleted band begins at.
        at: u32,
        /// Number of columns to delete.
        count: u32,
    },
    /// Replace a sheet's position-indexed metadata wholesale: merged ranges,
    /// column widths, row heights, hidden row/column sets, and frozen-pane
    /// counts. This is the universal inverse form for the metadata half of a
    /// structural insert/delete — a delete that drops merges, sizing, hidden
    /// lines, or freeze bands cannot recover them by re-inserting an empty band,
    /// so its inverse carries a pre-mutation snapshot and this op restores it.
    SetSheetMetadata {
        /// Sheet index.
        sheet: usize,
        /// Merged ranges to install.
        merges: Vec<CellRange>,
        /// Column widths to install.
        columns: AxisSizing,
        /// Row heights to install.
        rows: AxisSizing,
        /// Hidden rows to install.
        hidden_rows: BTreeSet<u32>,
        /// Hidden columns to install.
        hidden_cols: BTreeSet<u32>,
        /// View state (frozen panes) to install.
        view: SheetView,
    },
    /// Insert a fully-formed sheet at position `index`, shifting later sheets
    /// right. The caller assigns the sheet's id and name; the inverse removes it.
    /// `index` is clamped to the end, so appending is `index == sheets.len()`.
    InsertSheet {
        /// Position to insert at (clamped to the current sheet count).
        index: usize,
        /// The sheet to insert.
        sheet: Box<Sheet>,
    },
    /// Remove the sheet at `index`. The inverse re-inserts the removed sheet at
    /// the same position, so a delete is fully recoverable.
    RemoveSheet {
        /// Position of the sheet to remove.
        index: usize,
    },
    /// Rename the sheet at `index`. The inverse restores the prior name.
    RenameSheet {
        /// Position of the sheet to rename.
        index: usize,
        /// The new name.
        name: String,
    },
    /// Move the sheet at `from` to position `to` (tab reorder). The inverse
    /// moves it back.
    MoveSheet {
        /// Current position.
        from: usize,
        /// Destination position.
        to: usize,
    },
    /// Set (or clear, with `None`) a sheet's tab color. The inverse restores the
    /// prior color.
    SetTabColor {
        /// Sheet index.
        sheet: usize,
        /// The new tab color (`RRGGBB`), or `None` to clear.
        color: Option<String>,
    },
    /// Replace the workbook's whole defined-name table wholesale. The
    /// universal inverse form for defining, renaming, or deleting a name —
    /// each swaps in the new list and carries the prior list back as its own
    /// inverse, mirroring [`Operation::SetSheetMetadata`].
    SetDefinedNames(Vec<DefinedName>),
    /// A group applied atomically, with a single combined inverse.
    Batch(Vec<Operation>),
}

/// Apply `op` to `workbook`, returning the inverse operation.
///
/// A `Batch` is all-or-nothing: if any member fails, the already-applied members
/// are rolled back before the error is returned.
pub fn apply(workbook: &mut Workbook, op: Operation) -> Result<Operation, TxnError> {
    match op {
        Operation::SetCell { sheet, at, cell } => {
            let previous = replace_cell(workbook, sheet, at, cell)?;
            Ok(inverse_of(sheet, at, previous))
        }
        Operation::SetValue { sheet, at, value } => {
            let previous = current_cell(workbook, sheet, at)?;
            let new_cell = Cell {
                value,
                style: previous.as_ref().and_then(|c| c.style),
                formula: None,
                ..Cell::default()
            };
            let cell = (!new_cell.is_blank()).then_some(new_cell);
            let restored = replace_cell(workbook, sheet, at, cell)?;
            Ok(inverse_of(sheet, at, restored))
        }
        Operation::SetStyle { sheet, at, style } => {
            let previous = current_cell(workbook, sheet, at)?;
            let mut new_cell = previous.unwrap_or_default();
            new_cell.style = style;
            let cell = (!new_cell.is_blank()).then_some(new_cell);
            let restored = replace_cell(workbook, sheet, at, cell)?;
            Ok(inverse_of(sheet, at, restored))
        }
        Operation::ClearCell { sheet, at } => {
            let previous = replace_cell(workbook, sheet, at, None)?;
            Ok(inverse_of(sheet, at, previous))
        }
        Operation::SetColumnWidth { sheet, col, width } => {
            let target = workbook
                .sheets
                .get_mut(sheet)
                .ok_or(TxnError::SheetNotFound { index: sheet })?;
            let previous = set_axis_override(&mut target.columns, col, width);
            Ok(Operation::SetColumnWidth {
                sheet,
                col,
                width: previous,
            })
        }
        Operation::SetRowHeight { sheet, row, height } => {
            let target = workbook
                .sheets
                .get_mut(sheet)
                .ok_or(TxnError::SheetNotFound { index: sheet })?;
            let previous = set_axis_override(&mut target.rows, row, height);
            Ok(Operation::SetRowHeight {
                sheet,
                row,
                height: previous,
            })
        }
        Operation::InsertRows { sheet, at, count } => {
            structural::insert(workbook, sheet, Axis::Row, at, count)
        }
        Operation::DeleteRows { sheet, at, count } => {
            structural::delete(workbook, sheet, Axis::Row, at, count)
        }
        Operation::InsertColumns { sheet, at, count } => {
            structural::insert(workbook, sheet, Axis::Col, at, count)
        }
        Operation::DeleteColumns { sheet, at, count } => {
            structural::delete(workbook, sheet, Axis::Col, at, count)
        }
        Operation::SetSheetMetadata {
            sheet,
            merges,
            columns,
            rows,
            hidden_rows,
            hidden_cols,
            view,
        } => {
            let target = workbook
                .sheets
                .get_mut(sheet)
                .ok_or(TxnError::SheetNotFound { index: sheet })?;
            // Swap each field in, handing back the prior contents as the inverse.
            Ok(Operation::SetSheetMetadata {
                sheet,
                merges: std::mem::replace(&mut target.merges, merges),
                columns: std::mem::replace(&mut target.columns, columns),
                rows: std::mem::replace(&mut target.rows, rows),
                hidden_rows: std::mem::replace(&mut target.hidden_rows, hidden_rows),
                hidden_cols: std::mem::replace(&mut target.hidden_cols, hidden_cols),
                view: std::mem::replace(&mut target.view, view),
            })
        }
        Operation::InsertSheet { index, sheet } => {
            let at = index.min(workbook.sheets.len());
            workbook.sheets.insert(at, *sheet);
            Ok(Operation::RemoveSheet { index: at })
        }
        Operation::RemoveSheet { index } => {
            if index >= workbook.sheets.len() {
                return Err(TxnError::SheetNotFound { index });
            }
            let removed = workbook.sheets.remove(index);
            Ok(Operation::InsertSheet {
                index,
                sheet: Box::new(removed),
            })
        }
        Operation::RenameSheet { index, name } => {
            let previous = {
                let target = workbook
                    .sheets
                    .get_mut(index)
                    .ok_or(TxnError::SheetNotFound { index })?;
                std::mem::replace(&mut target.name, name.clone())
            };
            // Follow the rename in every cross-sheet reference (`Old!A1` ->
            // `New!A1`) so a referenced sheet's formulas don't silently break.
            // The inverse renames back and this same pass reverses the rewrite.
            if previous != name {
                rename_sheet_in_formulas(workbook, &previous, &name);
            }
            Ok(Operation::RenameSheet {
                index,
                name: previous,
            })
        }
        Operation::MoveSheet { from, to } => {
            let count = workbook.sheets.len();
            if from >= count {
                return Err(TxnError::SheetNotFound { index: from });
            }
            if to >= count {
                return Err(TxnError::SheetNotFound { index: to });
            }
            let sheet = workbook.sheets.remove(from);
            workbook.sheets.insert(to, sheet);
            // Removing `from` then inserting at `to` is undone by removing `to`
            // then inserting at `from`.
            Ok(Operation::MoveSheet { from: to, to: from })
        }
        Operation::SetTabColor { sheet, color } => {
            let target = workbook
                .sheets
                .get_mut(sheet)
                .ok_or(TxnError::SheetNotFound { index: sheet })?;
            let previous = std::mem::replace(&mut target.tab_color, color);
            Ok(Operation::SetTabColor {
                sheet,
                color: previous,
            })
        }
        Operation::SetDefinedNames(names) => {
            let previous = std::mem::replace(&mut workbook.defined_names, names);
            Ok(Operation::SetDefinedNames(previous))
        }
        Operation::Batch(ops) => {
            let mut inverses = Vec::with_capacity(ops.len());
            for member in ops {
                match apply(workbook, member) {
                    Ok(inverse) => inverses.push(inverse),
                    Err(err) => {
                        while let Some(inv) = inverses.pop() {
                            let _ = apply(workbook, inv);
                        }
                        return Err(err);
                    }
                }
            }
            inverses.reverse();
            Ok(Operation::Batch(inverses))
        }
    }
}

/// Set or clear one axis override, returning the previous value (for the inverse).
fn set_axis_override(axis: &mut AxisSizing, index: u32, size: Option<i64>) -> Option<i64> {
    let previous = axis.sizes.get(&index).copied();
    match size {
        Some(value) => {
            axis.sizes.insert(index, value);
        }
        None => {
            axis.sizes.remove(&index);
        }
    }
    previous
}

/// Rewrite every workbook formula that references sheet `old` (by name) so it
/// points at `new`. Only formulas that actually change are re-stored, mirroring
/// the structural row/column rewrite pass.
fn rename_sheet_in_formulas(workbook: &mut Workbook, old: &str, new: &str) {
    let mut jobs: Vec<(usize, CellRef, Expr)> = Vec::new();
    for (idx, sheet) in workbook.sheets.iter().enumerate() {
        for (addr, cell) in sheet.cells.iter() {
            if let Some(handle) = cell.formula
                && let Some(expr) = workbook.formula(handle)
            {
                let mut rewritten = expr.clone();
                if rename_sheet_references(&mut rewritten, old, new) {
                    jobs.push((idx, addr, rewritten));
                }
            }
        }
    }
    for (idx, addr, expr) in jobs {
        let handle = workbook.store_formula(expr);
        let store = &mut workbook.sheets[idx].cells;
        if let Some(existing) = store.get(addr) {
            let mut updated = existing.clone();
            updated.formula = Some(handle);
            store.set(addr, updated);
        }
    }
}

fn inverse_of(sheet: usize, at: CellRef, previous: Option<Cell>) -> Operation {
    Operation::SetCell {
        sheet,
        at,
        cell: previous,
    }
}

fn current_cell(workbook: &Workbook, sheet: usize, at: CellRef) -> Result<Option<Cell>, TxnError> {
    let sheet = workbook
        .sheets
        .get(sheet)
        .ok_or(TxnError::SheetNotFound { index: sheet })?;
    Ok(sheet.cells.get(at).cloned())
}

fn replace_cell(
    workbook: &mut Workbook,
    sheet: usize,
    at: CellRef,
    cell: Option<Cell>,
) -> Result<Option<Cell>, TxnError> {
    let sheet = workbook
        .sheets
        .get_mut(sheet)
        .ok_or(TxnError::SheetNotFound { index: sheet })?;
    let previous = sheet.cells.get(at).cloned();
    match cell {
        Some(cell) => sheet.cells.set(at, cell),
        None => {
            sheet.cells.clear(at);
        }
    }
    Ok(previous)
}

/// Paired undo/redo stacks over [`apply`]. The host keeps one of these per
/// document session.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Operation>,
    redo: Vec<Operation>,
}

impl History {
    /// A new, empty history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply `op`, recording its inverse for undo and clearing the redo stack.
    pub fn apply(&mut self, workbook: &mut Workbook, op: Operation) -> Result<(), TxnError> {
        let inverse = apply(workbook, op)?;
        self.undo.push(inverse);
        self.redo.clear();
        Ok(())
    }

    /// Whether there is anything to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is anything to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undo the most recent operation.
    pub fn undo(&mut self, workbook: &mut Workbook) -> Result<(), TxnError> {
        if let Some(op) = self.undo.pop() {
            let inverse = apply(workbook, op)?;
            self.redo.push(inverse);
        }
        Ok(())
    }

    /// Redo the most recently undone operation.
    pub fn redo(&mut self, workbook: &mut Workbook) -> Result<(), TxnError> {
        if let Some(op) = self.redo.pop() {
            let inverse = apply(workbook, op)?;
            self.undo.push(inverse);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
