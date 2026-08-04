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

use casual_calc_model::{Cell, CellRef, CellValue, StyleId, Workbook};

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
