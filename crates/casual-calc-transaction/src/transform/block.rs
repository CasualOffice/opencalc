//! Rebasing across [`Operation::ShiftCells`] — Insert Cells and Delete Cells
//! with a shift (`XL-UX-173`).
//!
//! A block shift is a whole-line insert or delete **restricted to a band of the
//! other axis**: Shift cells down on `B3:C4` is `InsertRows{at: 2, count: 2}`
//! for columns B and C only. So most of the arithmetic is the band's, and what
//! this module adds is the restriction — and the honest list of pairs that
//! restriction leaves without an answer.
//!
//! # What is answered
//!
//! - A cell edit follows its cell when the cell is in the block's columns
//!   (rows), and vanishes with it when the block deleted it. A carried formula
//!   is rewritten by the same rule `apply` uses, through [`Shift`] with the
//!   band set.
//! - A defined-name table is rewritten the same way.
//! - A whole-line insert or delete on the *cross* axis widens, narrows or moves
//!   the block's band; on the *same* axis, one wholly **before** the block
//!   moves it, and the block leaves that line edit alone.
//! - Two block shifts on the same axis whose bands do not meet commute.
//! - Everything positionally inert (sizes, tab colour, macros, renames).
//!
//! # What is refused, and why
//!
//! [`TransformError::Unsupported`] — the session's existing "cannot merge"
//! path — for: a same-axis line insert or delete at or after the block's first
//! line (a whole row inserted *inside* a partially shifted region has no single
//! position on the other replica: the block's columns and the rest disagree
//! about which row it is); a cross-axis delete that cuts the band partway; two
//! blocks that meet or run on different axes; a block meeting a line or range
//! move; and a block meeting a metadata bundle that carries positional state.
//! Each is a pair where any answer is a guess, and a guessed transform is the
//! silent divergence this layer exists to rule out.

use casual_calc_model::{CellRange, CellRef};

use super::{
    FormulaTable, SheetNames, Shift, TransformError, line_move, noop, rebase_cell_formula,
    unsupported,
};
use crate::{
    CellShift, Operation, SheetFields,
    structural::{Axis, ShiftKind},
};

/// Whether an operation is a block shift.
pub(super) fn involves(op: &Operation) -> bool {
    matches!(op, Operation::ShiftCells { .. })
}

/// A block shift, reduced to its band.
#[derive(Debug, Clone, Copy)]
struct Block {
    sheet: usize,
    axis: Axis,
    at: u32,
    count: u32,
    /// The lines of the *other* axis that move, inclusive.
    cross: (u32, u32),
    inserting: bool,
}

fn block_of(op: &Operation) -> Option<Block> {
    let Operation::ShiftCells {
        sheet,
        range,
        shift,
    } = op
    else {
        return None;
    };
    let (r0, r1) = (
        range.start.row.min(range.end.row),
        range.start.row.max(range.end.row),
    );
    let (c0, c1) = (
        range.start.col.min(range.end.col),
        range.start.col.max(range.end.col),
    );
    let (axis, at, count, cross) = match shift {
        CellShift::Down | CellShift::Up => (Axis::Row, r0, r1 - r0 + 1, (c0, c1)),
        CellShift::Right | CellShift::Left => (Axis::Col, c0, c1 - c0 + 1, (r0, r1)),
    };
    Some(Block {
        sheet: *sheet,
        axis,
        at,
        count,
        cross,
        inserting: matches!(shift, CellShift::Down | CellShift::Right),
    })
}

impl Block {
    fn op(self) -> Operation {
        let last = self.at + self.count - 1;
        let (range, shift) = match (self.axis, self.inserting) {
            (Axis::Row, ins) => (
                CellRange::new(
                    CellRef::new(self.at, self.cross.0),
                    CellRef::new(last, self.cross.1),
                ),
                if ins { CellShift::Down } else { CellShift::Up },
            ),
            (Axis::Col, ins) => (
                CellRange::new(
                    CellRef::new(self.cross.0, self.at),
                    CellRef::new(self.cross.1, last),
                ),
                if ins {
                    CellShift::Right
                } else {
                    CellShift::Left
                },
            ),
        };
        Operation::ShiftCells {
            sheet: self.sheet,
            range,
            shift,
        }
    }

    fn shift(self) -> Shift {
        Shift {
            sheet: self.sheet,
            axis: self.axis,
            kind: if self.inserting {
                ShiftKind::Insert
            } else {
                ShiftKind::Delete
            },
            at: self.at,
            count: self.count,
            cross: Some(self.cross),
            against: "ShiftCells",
        }
    }

    /// Where a cell lands, or `None` when the block deleted it.
    fn map(self, at: CellRef) -> Option<CellRef> {
        let (along, across) = match self.axis {
            Axis::Row => (at.row, at.col),
            Axis::Col => (at.col, at.row),
        };
        if across < self.cross.0 || across > self.cross.1 {
            return Some(at);
        }
        let moved = if self.inserting {
            if along >= self.at {
                along + self.count
            } else {
                along
            }
        } else if along >= self.at + self.count {
            along - self.count
        } else if along >= self.at {
            return None;
        } else {
            along
        };
        Some(match self.axis {
            Axis::Row => CellRef::new(moved, at.col),
            Axis::Col => CellRef::new(at.row, moved),
        })
    }
}

/// A whole-line structural operation: `(sheet, axis, at, count, inserting)`.
fn line_of(op: &Operation) -> Option<(usize, Axis, u32, u32, bool)> {
    Some(match *op {
        Operation::InsertRows { sheet, at, count } => (sheet, Axis::Row, at, count, true),
        Operation::DeleteRows { sheet, at, count } => (sheet, Axis::Row, at, count, false),
        Operation::InsertColumns { sheet, at, count } => (sheet, Axis::Col, at, count, true),
        Operation::DeleteColumns { sheet, at, count } => (sheet, Axis::Col, at, count, false),
        _ => return None,
    })
}

/// The bundle fields a block shift rewrites on its own sheet.
const POSITIONAL: SheetFields = SheetFields::MERGES
    .union(SheetFields::AUTO_FILTER)
    .union(SheetFields::VALIDATIONS)
    .union(SheetFields::CONDITIONAL_FORMATS)
    .union(SheetFields::COMMENTS)
    .union(SheetFields::HYPERLINKS)
    .union(SheetFields::TABLES)
    .union(SheetFields::CHARTS)
    .union(SheetFields::PIVOTS);

/// Whether a metadata bundle and a block shift contend. A chart's series and a
/// pivot's source can name the block's sheet from any sheet, so those two
/// fields contend wherever the bundle is.
fn bundle_contends(op: &Operation, block: Block) -> bool {
    let Operation::SetSheetMetadata { sheet, changed, .. } = op else {
        return false;
    };
    (*sheet == block.sheet && changed.intersects(POSITIONAL))
        || changed.intersects(SheetFields::CHARTS.union(SheetFields::PIVOTS))
}

/// Whether two block shifts commute: same sheet, same axis, bands apart.
fn blocks_commute(a: Block, b: Block) -> bool {
    a.sheet != b.sheet || (a.axis == b.axis && (a.cross.1 < b.cross.0 || b.cross.1 < a.cross.0))
}

/// Whether a same-axis line insert at `at` sits wholly before `block`.
fn before_block(at: u32, block: Block) -> bool {
    if block.inserting {
        at <= block.at
    } else {
        at < block.at
    }
}

/// Rebase `subject` across `against` when either is a block shift.
pub(super) fn rebase(
    subject: &Operation,
    against: &Operation,
    sheets: &SheetNames,
    formulas: &mut dyn FormulaTable,
) -> Result<Operation, TransformError> {
    let refuse = || unsupported(subject, against);
    if line_move::involves_move(subject) || line_move::involves_move(against) {
        return Err(refuse());
    }
    if let Some(block) = block_of(against) {
        return across_block(subject, block, sheets, formulas).map_err(|()| refuse());
    }
    let Some(mut block) = block_of(subject) else {
        return Ok(subject.clone());
    };
    if let Some(other) = block_of(against) {
        return if blocks_commute(block, other) {
            Ok(subject.clone())
        } else {
            Err(refuse())
        };
    }
    if bundle_contends(against, block) {
        return Err(refuse());
    }
    let Some((sheet, axis, at, count, inserting)) = line_of(against) else {
        return Ok(subject.clone());
    };
    if sheet != block.sheet {
        return Ok(subject.clone());
    }
    let end = at.saturating_add(count);
    if axis == block.axis {
        // Only a line edit wholly before the block has one answer. An insert
        // exactly on a *deleted* block's first line is the `COL-50` shape —
        // a range ending there is grown by one order and clamped by the
        // other — so it is refused rather than answered two ways.
        if inserting && before_block(at, block) {
            block.at += count;
        } else if !inserting && end <= block.at {
            block.at -= count;
        } else {
            return Err(refuse());
        }
        return Ok(block.op());
    }
    let (c0, c1) = block.cross;
    if inserting {
        if at <= c0 {
            block.cross = (c0 + count, c1 + count);
        } else if at <= c1 {
            // Inside the band: the new lines are in the shifted region too.
            block.cross = (c0, c1 + count);
        }
    } else if end <= c0 {
        block.cross = (c0 - count, c1 - count);
    } else if at > c1 {
        // After the band: nothing moves.
    } else if at <= c0 && end > c1 {
        // The band's lines are gone, and every cell it would have moved with them.
        return Ok(noop());
    } else {
        return Err(refuse());
    }
    Ok(block.op())
}

/// `subject` rebased across a block shift, or `Err(())` for a refused pair.
fn across_block(
    subject: &Operation,
    block: Block,
    sheets: &SheetNames,
    formulas: &mut dyn FormulaTable,
) -> Result<Operation, ()> {
    let same = |sheet: usize| sheet == block.sheet;
    Ok(match subject.clone() {
        Operation::SetCell { sheet, at, cell } => {
            let to = if same(sheet) {
                match block.map(at) {
                    Some(to) => to,
                    None => return Ok(noop()),
                }
            } else {
                at
            };
            let cell = rebase_cell_formula(cell, sheet, at, to, block.shift(), sheets, formulas)
                .map_err(|_| ())?;
            Operation::SetCell {
                sheet,
                at: to,
                cell,
            }
        }
        Operation::SetValue { sheet, at, value } if same(sheet) => match block.map(at) {
            Some(at) => Operation::SetValue { sheet, at, value },
            None => noop(),
        },
        Operation::SetStyle { sheet, at, style } if same(sheet) => match block.map(at) {
            Some(at) => Operation::SetStyle { sheet, at, style },
            None => noop(),
        },
        Operation::ClearCell { sheet, at } if same(sheet) => match block.map(at) {
            Some(at) => Operation::ClearCell { sheet, at },
            None => noop(),
        },
        Operation::SetDefinedNames(names) => {
            let shift = block.shift();
            let mut shifted = Vec::with_capacity(names.len());
            for mut name in names {
                let home = name
                    .sheet
                    .and_then(|id| sheets.iter().find(|(_, sheet_id)| *sheet_id == id))
                    .map_or("", |(sheet_name, _)| sheet_name.as_str());
                let carried = super::carried_shift(shift, home, sheets).ok_or(())?;
                name.formula = carried.free_formula(&name.formula);
                shifted.push(name);
            }
            Operation::SetDefinedNames(shifted)
        }
        op @ Operation::ShiftCells { .. } => {
            let other = block_of(&op).ok_or(())?;
            if blocks_commute(other, block) {
                op
            } else {
                return Err(());
            }
        }
        op @ Operation::SetSheetMetadata { .. } => {
            if bundle_contends(&op, block) {
                return Err(());
            }
            op
        }
        op => {
            if let Some((sheet, axis, at, count, inserting)) = line_of(&op)
                && same(sheet)
                && axis == block.axis
            {
                // The mirror of the arm in `rebase`: a line edit wholly
                // before the block is untouched by it, anything else refused.
                let before = if inserting {
                    before_block(at, block)
                } else {
                    at.saturating_add(count) <= block.at
                };
                if !before {
                    return Err(());
                }
            }
            op
        }
    })
}
