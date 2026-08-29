//! The transform for a concurrent **line** move — `COL-44`.
//!
//! Until this existed, [`Operation::MoveColumns`] and [`Operation::MoveRows`]
//! met every concurrent operation with [`TransformError::Unsupported`]: safe,
//! because the alternative fall-through ("`against` moves nothing, so
//! `subject`'s coordinates still mean what they meant") is *false* for a move
//! and would have diverged two documents silently, but a refusal is still a
//! user losing their edit.
//!
//! # Why `Band` cannot express it, and what replaces it
//!
//! A [`Band`](super::Band) is *insert n at p* or *delete n at p* — a map on
//! line indices that is monotonic and changes the axis length. A move changes
//! nothing's length and is **not monotonic**: it is exactly
//! `insert(count, landing)` composed with `delete(count, at)`, on the same
//! axis, in that order. So the normal form here is that composition, [`Mv`],
//! which is the same `LineMove` [`crate::apply`] already reduces a move to.
//! Using the *same* normal form is deliberate rather than tidy: a transform
//! that models the move differently from the way `apply` performs it converges
//! on paper and diverges in the document.
//!
//! # The two mappings a move induces
//!
//! A structural transform needs two different maps, and conflating them is the
//! classic OT bug — the sheet-position code in the parent module already says
//! so for its own axis:
//!
//! - [`Mv::map`] — where a **line** goes. Total; a move destroys nothing, so
//!   unlike a delete it never answers `None`. A cell address, a column width
//!   and a hidden-line index all follow this.
//! - [`Mv::map_gap`] — where a **position between lines** goes. The gap before
//!   the band and the gap after it are the *same gap* once the band has left,
//!   so both edges map together; a gap strictly inside the band travels with
//!   it; and a gap that coincides with the band's destination is a genuine tie,
//!   broken by the order the server settled on exactly as
//!   [`map_sheet_position`](super::map_sheet_position) breaks its own.
//!
//! # What is decided by a rule rather than by arithmetic
//!
//! Each of these is refused, and each refusal is argued from convergence rather
//! than from effort:
//!
//! 1. **[`Operation::MoveRange`]** — a rectangle move is not a permutation. It
//!    overwrites its destination, so it destroys cells, and a concurrent delete
//!    can cut its source rectangle into a shape no rectangle can express.
//!    Unchanged from before; nothing here makes it worse.
//! 2. **Two moves whose bands overlap without being equal.** If the other move
//!    takes lines out of the middle of this one's band, or drops its own band
//!    into the middle of it, this move's lines are no longer contiguous — and a
//!    `Move*` operation can only name a contiguous band. Disjoint bands, which
//!    is what two people reordering different columns actually do, transform.
//! 3. **Two moves of the *same* band.** That is contention, not independence,
//!    and it is resolved the way two writes of one cell are: the settled order
//!    decides and the earlier one yields entirely. Treating it as independent
//!    relocates the band twice and leaves it wherever the second application
//!    happened to put it — which is a different place on each replica.

use casual_calc_model::CellRef;

use super::{
    SheetNames, Shift, Side, TransformError, band_of, noop, rebase_cell_formula, sheet_of,
    unsupported,
};
use crate::{
    Operation,
    structural::{Axis, ShiftKind},
};

/// A line move in the normal form `apply` reduces it to.
///
/// `landing` is a **post-removal** coordinate: where the band begins once it
/// has been lifted out. That is not the `before` the operation carries, and
/// keeping the two apart is most of the arithmetic in this file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mv {
    at: u32,
    count: u32,
    landing: u32,
}

impl Mv {
    /// The move a `(at, count, before)` request describes, or `None` when it
    /// asks for nothing. Mirrors `structural::LineMove::plan` exactly, because
    /// a plan the transform and `apply` disagree about is a divergence.
    fn plan(at: u32, count: u32, before: u32) -> Option<Self> {
        if count == 0 {
            return None;
        }
        let end = at.saturating_add(count);
        if before > at && before < end {
            return None;
        }
        let landing = if before <= at { before } else { before - count };
        (landing != at).then_some(Self { at, count, landing })
    }

    /// The `before` that plans back to this move — the inverse of [`Mv::plan`],
    /// and what puts the answer back on the wire as a `Move*`.
    const fn before(self) -> u32 {
        if self.landing <= self.at {
            self.landing
        } else {
            self.landing + self.count
        }
    }

    const fn end(self) -> u32 {
        self.at.saturating_add(self.count)
    }

    const fn inside(self, x: u32) -> bool {
        x >= self.at && x < self.end()
    }

    /// Where a single line ends up. Total: a move destroys nothing.
    fn map(self, x: u32) -> u32 {
        if self.inside(x) {
            return (x - self.at).saturating_add(self.landing);
        }
        let removed = if x < self.at {
            x
        } else {
            x.saturating_sub(self.count)
        };
        if removed >= self.landing {
            removed.saturating_add(self.count)
        } else {
            removed
        }
    }

    /// Where a **position between lines** ends up.
    ///
    /// `side` decides only the one genuine tie: a position that coincides with
    /// where the band lands. The operation ordered **later** yields and sits
    /// after the arriving band; the **earlier** one keeps the gap it asked for.
    /// Both replicas are told the same order, so both compute the same answer —
    /// which is the whole reason a server-mediated transform is allowed a
    /// tiebreak at all (ADR-011).
    fn map_gap(self, p: u32, side: Side) -> u32 {
        // Strictly inside the band: the gap is between two lines that are
        // travelling together, so it travels with them.
        if p > self.at && p < self.end() {
            return self.landing + (p - self.at);
        }
        // Both edges of the band collapse to the same gap once it has left.
        let q = if p <= self.at { p } else { p - self.count };
        match q.cmp(&self.landing) {
            std::cmp::Ordering::Less => q,
            std::cmp::Ordering::Greater => q + self.count,
            std::cmp::Ordering::Equal => {
                if side == Side::Later {
                    q + self.count
                } else {
                    q
                }
            }
        }
    }

    /// The image of the contiguous run `[lo, hi)` under this permutation, as
    /// maximal runs, ascending.
    ///
    /// At most three, and computed from the endpoints of the three pieces the
    /// move treats differently rather than by enumeration — a delete of a
    /// million rows must not cost a million evaluations.
    fn image_runs(self, lo: u32, hi: u32) -> Vec<(u32, u32)> {
        let mut runs: Vec<(u32, u32)> = Vec::new();
        let mut push = |a: u32, b: u32| {
            if a < b {
                runs.push((a, b));
            }
        };
        // Below the band. Monotone, but the landing may split it.
        let (a0, a1) = (lo, hi.min(self.at));
        if a0 < a1 {
            // `x < at` maps to `x`, or to `x + count` once `x >= landing`.
            let split = self.landing.clamp(a0, a1);
            push(a0, split);
            push(split + self.count, a1 + self.count);
        }
        // Inside the band: translated rigidly.
        let (b0, b1) = (lo.max(self.at), hi.min(self.end()));
        if b0 < b1 {
            push(self.landing + (b0 - self.at), self.landing + (b1 - self.at));
        }
        // Above the band. After removal these are `x - count`; the landing may
        // split that image too.
        let (c0, c1) = (lo.max(self.end()), hi);
        if c0 < c1 {
            let (r0, r1) = (c0 - self.count, c1 - self.count);
            let split = self.landing.clamp(r0, r1);
            push(r0, split);
            push(split + self.count, r1 + self.count);
        }
        runs.sort_unstable();
        // Merge anything that came out adjacent, so "one run" is detected
        // rather than missed on a boundary.
        let mut merged: Vec<(u32, u32)> = Vec::with_capacity(runs.len());
        for (a, b) in runs {
            match merged.last_mut() {
                Some(last) if last.1 == a => last.1 = b,
                _ => merged.push((a, b)),
            }
        }
        merged
    }

    /// The formula rewrite this move implies, in the terms [`Shift`] carries.
    fn shift(self, sheet: usize, axis: Axis) -> Shift {
        Shift {
            sheet,
            axis,
            kind: ShiftKind::Move {
                landing: self.landing,
            },
            at: self.at,
            count: self.count,
            against: match axis {
                Axis::Row => "MoveRows",
                Axis::Col => "MoveColumns",
            },
        }
    }
}

/// The move an operation is, when it is a line move that does something.
fn line_move(op: &Operation) -> Option<(usize, Axis, Mv)> {
    match *op {
        Operation::MoveColumns {
            sheet,
            at,
            count,
            before,
        } => Mv::plan(at, count, before).map(|mv| (sheet, Axis::Col, mv)),
        Operation::MoveRows {
            sheet,
            at,
            count,
            before,
        } => Mv::plan(at, count, before).map(|mv| (sheet, Axis::Row, mv)),
        _ => None,
    }
}

/// Whether an operation permutes lines — the two that have a transform here.
pub(super) fn is_line_move(op: &Operation) -> bool {
    matches!(
        op,
        Operation::MoveColumns { .. } | Operation::MoveRows { .. }
    )
}

/// Whether an operation moves a **rectangle**, which has no transform.
pub(super) fn is_range_move(op: &Operation) -> bool {
    matches!(op, Operation::MoveRange { .. })
}

fn move_op(sheet: usize, axis: Axis, mv: Mv) -> Operation {
    match axis {
        Axis::Col => Operation::MoveColumns {
            sheet,
            at: mv.at,
            count: mv.count,
            before: mv.before(),
        },
        Axis::Row => Operation::MoveRows {
            sheet,
            at: mv.at,
            count: mv.count,
            before: mv.before(),
        },
    }
}

fn delete_op(sheet: usize, axis: Axis, at: u32, count: u32) -> Operation {
    match axis {
        Axis::Col => Operation::DeleteColumns { sheet, at, count },
        Axis::Row => Operation::DeleteRows { sheet, at, count },
    }
}

fn insert_op(sheet: usize, axis: Axis, at: u32, count: u32) -> Operation {
    match axis {
        Axis::Col => Operation::InsertColumns { sheet, at, count },
        Axis::Row => Operation::InsertRows { sheet, at, count },
    }
}

/// Rebase a pair in which at least one side is a move.
///
/// Called by [`transform`](super::transform) once batches, sheet-level
/// renumbering and same-cell contention have been dealt with, so both operands
/// here are single operations on the grid.
///
/// # Errors
///
/// [`TransformError::Unsupported`] for the three narrowings in the module docs.
pub(super) fn rebase(
    subject: &Operation,
    against: &Operation,
    side: Side,
    sheets: &SheetNames,
    formulas: &mut dyn super::FormulaTable,
) -> Result<Operation, TransformError> {
    // Narrowing (1): a rectangle move is not a permutation.
    if is_range_move(subject) || is_range_move(against) {
        return Err(unsupported(subject, against));
    }

    // **Degenerate moves, before anything else.** A `Move*` whose band is
    // empty, or which was dropped back onto itself, plans to nothing — `apply`
    // returns an empty batch for it. Both directions have to agree about that
    // or the pair converges on nothing: a no-op *subject* rebases to a no-op,
    // and a no-op *against* leaves the subject exactly as it was. Getting the
    // second of those wrong turned every pair involving a self-drop into two
    // empty batches, which is TP1-convergent only because both sides threw the
    // work away.
    if is_line_move(subject) && line_move(subject).is_none() {
        return Ok(noop());
    }
    if is_line_move(against) && line_move(against).is_none() {
        return Ok(subject.clone());
    }

    if let Some((sheet, axis, mv)) = line_move(against) {
        return rebase_onto_move(subject, sheet, axis, mv, side, sheets, formulas);
    }
    // `subject` is the move and `against` is not; a non-planning move was
    // answered above.
    let Some((sheet, axis, mv)) = line_move(subject) else {
        unreachable!("rebase is only called when one side is a move")
    };
    rebase_move_onto(subject, sheet, axis, mv, against, side)
}

/// Rebase anything at all across a line move.
fn rebase_onto_move(
    subject: &Operation,
    sheet: usize,
    axis: Axis,
    mv: Mv,
    side: Side,
    sheets: &SheetNames,
    formulas: &mut dyn super::FormulaTable,
) -> Result<Operation, TransformError> {
    let moved = move_op(sheet, axis, mv);
    // A move on another sheet permutes nothing here — with the one exception
    // every structural operation has, a formula naming that other sheet, which
    // `apply` rewrites wherever it lives (`COL-46`).
    let same_sheet = sheet_of(subject) == Some(sheet);
    if !same_sheet {
        let Operation::SetCell {
            sheet: home,
            at,
            cell,
        } = subject.clone()
        else {
            return Ok(subject.clone());
        };
        return Ok(Operation::SetCell {
            sheet: home,
            at,
            cell: rebase_cell_formula(cell, home, at, at, mv.shift(sheet, axis), sheets, formulas)?,
        });
    }
    let line = |at: CellRef| match axis {
        Axis::Row => CellRef::new(mv.map(at.row), at.col),
        Axis::Col => CellRef::new(at.row, mv.map(at.col)),
    };
    Ok(match subject.clone() {
        // Cell-addressed operations follow their cell. A move destroys no line,
        // so unlike a delete none of these can become a no-op.
        Operation::SetCell { sheet, at, cell } => {
            let to = line(at);
            Operation::SetCell {
                sheet,
                at: to,
                cell: rebase_cell_formula(
                    cell,
                    sheet,
                    at,
                    to,
                    mv.shift(sheet, axis),
                    sheets,
                    formulas,
                )?,
            }
        }
        Operation::SetValue { sheet, at, value } => Operation::SetValue {
            sheet,
            at: line(at),
            value,
        },
        Operation::SetStyle { sheet, at, style } => Operation::SetStyle {
            sheet,
            at: line(at),
            style,
        },
        Operation::ClearCell { sheet, at } => Operation::ClearCell {
            sheet,
            at: line(at),
        },

        // Axis sizing follows its line, on its own axis only.
        Operation::SetColumnWidth { sheet, col, width } => Operation::SetColumnWidth {
            sheet,
            col: if axis == Axis::Col { mv.map(col) } else { col },
            width,
        },
        Operation::SetRowHeight { sheet, row, height } => Operation::SetRowHeight {
            sheet,
            row: if axis == Axis::Row { mv.map(row) } else { row },
            height,
        },

        // An insert names a *gap*, not a line.
        Operation::InsertRows { sheet, at, count } if axis == Axis::Row => {
            insert_op(sheet, Axis::Row, mv.map_gap(at, side), count)
        }
        Operation::InsertColumns { sheet, at, count } if axis == Axis::Col => {
            insert_op(sheet, Axis::Col, mv.map_gap(at, side), count)
        }

        // A delete names a set of lines, and their image under a permutation
        // need not be contiguous — so the answer is up to three deletes,
        // emitted highest first so that no earlier one renumbers a later one.
        Operation::DeleteRows { sheet, at, count } if axis == Axis::Row => {
            deletes_after_move(sheet, Axis::Row, mv, at, count)
        }
        Operation::DeleteColumns { sheet, at, count } if axis == Axis::Col => {
            deletes_after_move(sheet, Axis::Col, mv, at, count)
        }

        // The bundle is the sheet's positional state, and a move permutes it —
        // the sizing map above all, which is what a concurrent column resize
        // travels in. It is shifted by `move_metadata`, the very pass `apply`
        // runs on the sheet itself, so the two cannot disagree; the chart
        // series and pivot sources it also carries follow through the same
        // `shift_bundle_references` the band path uses (`FID-28`).
        //
        // Refusing this arm was the one thing the out-of-tree prototype could
        // not avoid — `move_metadata` is `pub(crate)` and it was not in the
        // crate. Refusing it here would mean a concurrent column resize
        // silently un-permuting the whole sizing map.
        Operation::SetSheetMetadata {
            sheet,
            mut data,
            changed,
            restore,
        } => {
            // Planned through `apply`'s own constructor rather than assembled
            // here, so the two normal forms are the same object and not two
            // spellings that might drift apart.
            if let Some(planned) = crate::structural::LineMove::plan(mv.at, mv.count, mv.before()) {
                crate::structural::move_metadata(data.as_mut(), axis, planned);
            }
            if let (Some(target), Some(home)) = (sheets.get(sheet), sheets.get(sheet)) {
                crate::structural::shift_bundle_references(
                    data.as_mut(),
                    &crate::structural::BundleShift {
                        target_name: &target.0,
                        target_id: target.1,
                        home_name: &home.0,
                        axis,
                        kind: ShiftKind::Move {
                            landing: mv.landing,
                        },
                        at: mv.at,
                        count: mv.count,
                    },
                );
            }
            Operation::SetSheetMetadata {
                sheet,
                data,
                changed,
                restore,
            }
        }

        // Two moves.
        Operation::MoveColumns { .. } | Operation::MoveRows { .. } => {
            let Some((mine_sheet, mine_axis, mine)) = line_move(subject) else {
                unreachable!("a non-planning move was answered above")
            };
            if mine_axis != axis {
                // Row order and column order are independent.
                return Ok(subject.clone());
            }
            move_after_move(mine_sheet, axis, mine, mv, side)
                .ok_or_else(|| unsupported(subject, &moved))?
        }

        // Positionally inert: a tab colour, a rename, the defined-name table,
        // and any cross-axis structural pair.
        other => other,
    })
}

/// The image of `[at, at + count)` under `mv`, as deletes.
fn deletes_after_move(sheet: usize, axis: Axis, mv: Mv, at: u32, count: u32) -> Operation {
    if count == 0 {
        return noop();
    }
    let mut runs = mv.image_runs(at, at.saturating_add(count));
    // Highest first: a delete renumbers everything above it and nothing below.
    runs.reverse();
    if let [(lo, hi)] = runs.as_slice() {
        return delete_op(sheet, axis, *lo, hi - lo);
    }
    Operation::Batch(
        runs.into_iter()
            .map(|(lo, hi)| delete_op(sheet, axis, lo, hi - lo))
            .collect(),
    )
}

/// One move rebased across another on the same axis, or `None` when the answer
/// is not a single contiguous band — narrowings (2) and (3).
fn move_after_move(sheet: usize, axis: Axis, mine: Mv, other: Mv, side: Side) -> Option<Operation> {
    // **The same band.** Two people dragged the *same* columns to two
    // different places. Contention, resolved as two writes of one cell are.
    if mine.at == other.at && mine.count == other.count {
        if side == Side::Earlier {
            return Some(noop());
        }
        let before = other.map_gap(mine.before(), side);
        return Some(match Mv::plan(other.landing, mine.count, before) {
            Some(planned) => move_op(sheet, axis, planned),
            None => noop(),
        });
    }

    // **Bands that overlap without being equal.** One move took some of this
    // one's lines away, or this one is a sub-band of that one. Either way the
    // two intents are about overlapping sets of lines and there is no reading
    // of "both happened" that a single `Move*` can express.
    if mine.at < other.end() && other.at < mine.end() {
        return None;
    }

    let runs = other.image_runs(mine.at, mine.end());
    let &[(lo, hi)] = runs.as_slice() else {
        // Disjoint bands, but the other move dropped its band into the middle
        // of this one, so this one's lines are no longer contiguous.
        return None;
    };
    debug_assert_eq!(hi - lo, mine.count);
    let before = other.map_gap(mine.before(), side);
    // `plan` answering `None` is not a failure: it is the other move having
    // already put this band where this one wanted it, which is a no-op.
    Some(match Mv::plan(lo, hi - lo, before) {
        Some(planned) => move_op(sheet, axis, planned),
        None => noop(),
    })
}

/// A line move rebased across something that is not a move.
fn rebase_move_onto(
    subject: &Operation,
    sheet: usize,
    axis: Axis,
    mv: Mv,
    against: &Operation,
    side: Side,
) -> Result<Operation, TransformError> {
    let Some(band) = band_of(against) else {
        // `against` moves no line: a cell write, a style, a tab colour, a
        // resize, a defined-name table, a metadata bundle. None of them
        // renumbers anything, and a move contends with none of them — it writes
        // no cell, it permutes. The bundle is answered by the *other*
        // direction, where the bundle is what gets permuted.
        return Ok(subject.clone());
    };
    if band.axis != axis || band.sheet != Some(sheet) {
        return Ok(subject.clone());
    }

    if band.inserting {
        let n = band.count;
        let p = band.at;
        // The destination is a gap, and an insert at exactly that gap is a
        // genuine tie: both want to occupy it. Later yields.
        let before = match p.cmp(&mv.before()) {
            std::cmp::Ordering::Less => mv.before() + n,
            std::cmp::Ordering::Greater => mv.before(),
            std::cmp::Ordering::Equal => {
                if side == Side::Later {
                    mv.before() + n
                } else {
                    mv.before()
                }
            }
        };
        let at = if p <= mv.at { mv.at + n } else { mv.at };
        // Inserted strictly inside the moving band: the new lines sit between
        // two lines that are travelling together, so they travel too and the
        // band widens. The mirror rule — `map_gap` putting the insert inside
        // the moved band — is what makes the pair converge; neither is right
        // alone.
        let count = if p > mv.at && p < mv.end() {
            mv.count + n
        } else {
            mv.count
        };
        return Ok(match Mv::plan(at, count, before) {
            Some(planned) => move_op(sheet, axis, planned),
            None => noop(),
        });
    }

    // A delete. The band shrinks by whatever was removed from it — and the
    // survivors of a contiguous band minus a contiguous band are still
    // contiguous, which is why a delete, unlike another move, never scatters
    // this one.
    let (p, n) = (band.at, band.count);
    let del_end = p.saturating_add(n);
    let overlap = del_end.min(mv.end()).saturating_sub(p.max(mv.at));
    let count = mv.count - overlap;
    if count == 0 {
        return Ok(noop());
    }
    let removed_before = del_end.min(mv.at).saturating_sub(p.min(mv.at));
    let at = mv.at - removed_before;
    // The destination gap, mapped through the delete. A gap strictly inside the
    // deleted band collapses onto its start.
    let raw = mv.before();
    let before = if raw <= p {
        raw
    } else if raw >= del_end {
        raw - n
    } else {
        p
    };
    Ok(match Mv::plan(at, count, before) {
        Some(planned) => move_op(sheet, axis, planned),
        None => noop(),
    })
}

/// Whether a batch, or a bare operation, contains a move anywhere in it.
pub(super) fn involves_move(op: &Operation) -> bool {
    match op {
        Operation::Batch(members) => members.iter().any(involves_move),
        other => is_line_move(other) || is_range_move(other),
    }
}
