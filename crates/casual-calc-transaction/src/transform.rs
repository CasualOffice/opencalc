//! Operational transform over the closed op set — the concurrency primitive
//! from [ADR-011](../../../docs/56-COLLABORATION-CONCURRENCY-DESIGN.md).
//!
//! [`transform(a, b)`](transform) answers one question: *`a` was written against
//! a state where `b` had not happened; what is `a` on a state where it has?*
//! With a server imposing a total order, that is the whole of the algorithm —
//! a client rebases its pending ops onto each revision as it arrives, and the
//! server rebases an incoming op onto everything committed since the revision
//! it was based on.
//!
//! # The property this must satisfy
//!
//! **TP1**, convergence:
//!
//! ```text
//! apply(apply(S, a), transform(b, a)) == apply(apply(S, b), transform(a, b))
//! ```
//!
//! Two clients that saw the same state and edited concurrently end up with the
//! same state, whichever order the server picked. It is asserted as a property
//! over generated op pairs rather than by example, because transform functions
//! fail on the pair nobody thought of and that is the standard way OT ships
//! broken.
//!
//! TP2 — the property peer-to-peer OT needs — is deliberately **not** provided.
//! A server orders everything, which is what removes the obligation, and that
//! is the trade ADR-011 makes on purpose.
//!
//! # What is not handled yet
//!
//! **Concurrent sheet reordering.** An ordinary edit follows its sheet across
//! an insert, a remove or a move; what is refused is a [`Operation::MoveSheet`]
//! being rebased across another sheet-level operation. A move is defined by the
//! sheets it lands between, and a bare pair of indices does not record that, so
//! two concurrent reorderings have no answer that is obviously the intended
//! one. It is also rare, which is why refusing costs little.
//!
//! **A [`Operation::SetCell`] carrying a formula, ordered before a concurrent
//! style change to the same cell.** [`Operation::SetValue`] is the only
//! operation that writes content while leaving the style alone, and it carries
//! a value, so the formula has nowhere to go.
//!
//! Both return [`TransformError::Unsupported`] rather than an answer. Returning
//! the untransformed op would be silently wrong, and silently wrong is the one
//! outcome this layer must not produce: the two clients would diverge and
//! nothing would say so.

use crate::{Operation, SheetFields, structural::Axis};
use casual_calc_model::{CellRef, CellValue};

/// Where `subject` sits relative to `against` in the order the server settled
/// on.
///
/// Needed because both replicas run this function and each must reach the same
/// answer about who wins a contested target. "Whoever is transforming" cannot
/// decide it — that is the one fact the two sides disagree about. The final
/// order is the shared fact, so it is what the tiebreak reads.
///
/// A client rebasing its own unacknowledged op onto an arriving server op uses
/// [`Side::Later`] for its own; rebasing the server's op onto its pending one
/// uses [`Side::Earlier`] for the server's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// `subject` is ordered before `against`, so it loses a contested target.
    Earlier,
    /// `subject` is ordered after `against`, so it wins one.
    Later,
}

impl Side {
    /// The same relationship seen from the other operation.
    #[must_use]
    pub const fn flip(self) -> Self {
        match self {
            Self::Earlier => Self::Later,
            Self::Later => Self::Earlier,
        }
    }
}

/// Why a pair could not be transformed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransformError {
    /// This pair has no transform yet. The caller must serialise the two
    /// operations some other way — refuse the edit, or make the client reload.
    ///
    /// Deliberately an error and not a best guess: an untransformed op applied
    /// to a state it was not written against diverges the replicas *quietly*.
    Unsupported {
        /// The operation being transformed.
        subject: &'static str,
        /// The operation it is being transformed against.
        against: &'static str,
    },
}

impl core::fmt::Display for TransformError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unsupported { subject, against } => {
                write!(f, "cannot transform {subject} against {against} yet")
            }
        }
    }
}

impl core::error::Error for TransformError {}

/// An operation that does nothing. An empty batch, so no variant has to exist
/// for it and `apply` needs no special case.
fn noop() -> Operation {
    Operation::Batch(Vec::new())
}

/// Whether an operation would change nothing if applied.
#[must_use]
pub fn is_noop(op: &Operation) -> bool {
    match op {
        Operation::Batch(ops) => ops.iter().all(is_noop),
        Operation::SetSheetMetadata { changed, .. } => changed.is_empty(),
        _ => false,
    }
}

/// The sheet an operation acts on, when it acts on exactly one.
fn sheet_of(op: &Operation) -> Option<usize> {
    match op {
        Operation::SetCell { sheet, .. }
        | Operation::SetValue { sheet, .. }
        | Operation::SetStyle { sheet, .. }
        | Operation::ClearCell { sheet, .. }
        | Operation::SetColumnWidth { sheet, .. }
        | Operation::SetRowHeight { sheet, .. }
        | Operation::InsertRows { sheet, .. }
        | Operation::DeleteRows { sheet, .. }
        | Operation::InsertColumns { sheet, .. }
        | Operation::DeleteColumns { sheet, .. }
        | Operation::SetSheetMetadata { sheet, .. }
        | Operation::SetTabColor { sheet, .. } => Some(*sheet),
        _ => None,
    }
}

/// A structural band: which axis, where it starts, how many lines, inserting
/// or deleting.
#[derive(Debug, Clone, Copy)]
struct Band {
    axis: Axis,
    at: u32,
    count: u32,
    inserting: bool,
}

fn band_of(op: &Operation) -> Option<Band> {
    let (axis, at, count, inserting) = match *op {
        Operation::InsertRows { at, count, .. } => (Axis::Row, at, count, true),
        Operation::DeleteRows { at, count, .. } => (Axis::Row, at, count, false),
        Operation::InsertColumns { at, count, .. } => (Axis::Col, at, count, true),
        Operation::DeleteColumns { at, count, .. } => (Axis::Col, at, count, false),
        _ => return None,
    };
    Some(Band {
        axis,
        at,
        count,
        inserting,
    })
}

/// Where a single line index ends up after `band` is applied, or `None` when
/// the band deleted it.
fn shift_index(index: u32, band: Band) -> Option<u32> {
    if band.inserting {
        // An insert at exactly `index` pushes `index` down: the new empty lines
        // occupy `[at, at + count)`.
        Some(if index >= band.at {
            index + band.count
        } else {
            index
        })
    } else {
        let end = band.at.saturating_add(band.count);
        if index >= end {
            Some(index - band.count)
        } else if index >= band.at {
            None // inside the deleted band — the line is gone
        } else {
            Some(index)
        }
    }
}

/// Move a cell address across a band, or report that its line was deleted.
fn shift_cell(at: CellRef, band: Band) -> Option<CellRef> {
    Some(match band.axis {
        Axis::Row => CellRef::new(shift_index(at.row, band)?, at.col),
        Axis::Col => CellRef::new(at.row, shift_index(at.col, band)?),
    })
}

/// Rebase `subject` so it can be applied after `against` already has been.
///
/// Both must have been written against the same state. Returns an operation
/// with the same intent, expressed in the coordinates the state now has — or a
/// no-op when `against` removed the thing `subject` was about.
///
/// # Errors
///
/// [`TransformError::Unsupported`] for the pairs listed in the module docs.
/// Callers must treat that as "these two cannot be merged", not as "carry on".
pub fn transform(
    subject: &Operation,
    against: &Operation,
    side: Side,
) -> Result<Operation, TransformError> {
    // A batch is transformed member by member, **threading** the other side
    // through as it goes.
    //
    // Transforming each member against the same unchanged `against` is the
    // obvious implementation and it is wrong, which the TP1 property caught
    // rather than review: member two was written against the state that member
    // one produced, not against the state the batch started from. Rebasing it
    // onto the original `against` therefore lands it a member's worth of shift
    // out of place. `against` has to advance past each member as the member is
    // consumed — exactly as it would if the members had arrived as separate
    // operations, which is what they are.
    if let Operation::Batch(members) = subject {
        let mut out = Vec::with_capacity(members.len());
        let mut other = against.clone();
        for member in members {
            let rebased = transform(member, &other, side)?;
            other = transform(&other, member, side.flip())?;
            if !is_noop(&rebased) {
                out.push(rebased);
            }
        }
        return Ok(Operation::Batch(out));
    }
    if let Operation::Batch(members) = against {
        // The mirror image, and the direction that needs no threading: the
        // members were applied in this order, so folding through them in that
        // order is already the composition.
        let mut current = subject.clone();
        for member in members {
            current = transform(&current, member, side)?;
            if is_noop(&current) {
                return Ok(noop());
            }
        }
        return Ok(current);
    }

    // Anything that renumbers sheets, or a bundle meeting a structural op, has
    // no answer yet — and a wrong answer here is invisible divergence.
    // Sheet-level operations renumber every `sheet` index in the workbook, which
    // is a second transform axis on top of the row/column one.
    if renumbers_sheets(against) {
        return rebase_across_sheets(subject, against, side);
    }
    // A sheet-level operation being rebased across a *cell-level* one needs
    // nothing: adding or removing a sheet is unaffected by what is in the cells.
    if renumbers_sheets(subject) {
        return Ok(subject.clone());
    }
    if side == Side::Earlier && same_target(subject, against) && loses_a_formula(subject, against) {
        return Err(unsupported(subject, against));
    }

    // Different sheets never interact: no operation here addresses two.
    match (sheet_of(subject), sheet_of(against)) {
        (Some(a), Some(b)) if a != b => return Ok(subject.clone()),
        _ => {}
    }

    let Some(band) = band_of(against) else {
        // `against` moves nothing, so `subject`'s coordinates still mean what
        // they meant. What is left is contention for the same target, which the
        // settled order decides.
        return Ok(resolve_contention(subject, against, side));
    };

    Ok(rebase_onto_band(subject, band))
}

/// Whether an operation changes what a `sheet` index refers to.
fn renumbers_sheets(op: &Operation) -> bool {
    matches!(
        op,
        Operation::InsertSheet { .. } | Operation::RemoveSheet { .. } | Operation::MoveSheet { .. }
    )
}

/// Where the sheet at position `index` ends up after `against` runs, or `None`
/// when `against` removed it.
fn map_sheet_index(index: usize, against: &Operation) -> Option<usize> {
    match *against {
        Operation::InsertSheet { index: at, .. } => {
            Some(if index >= at { index + 1 } else { index })
        }
        Operation::RemoveSheet { index: at } => match index.cmp(&at) {
            core::cmp::Ordering::Equal => None,
            core::cmp::Ordering::Greater => Some(index - 1),
            core::cmp::Ordering::Less => Some(index),
        },
        // A move is a remove followed by an insert, and mapping it as those two
        // steps is both simpler to follow and impossible to get inconsistent
        // with `apply`, which does exactly that.
        Operation::MoveSheet { from, to } => {
            if index == from {
                return Some(to);
            }
            let after_remove = if index > from { index - 1 } else { index };
            Some(if after_remove >= to {
                after_remove + 1
            } else {
                after_remove
            })
        }
        _ => Some(index),
    }
}

/// Where an insertion *position* — not a sheet — ends up after `against` runs.
///
/// A position and an element shift differently at the boundary: inserting at
/// the same index as another insert still means "before whatever is there now",
/// so it shifts; an element at that index is the thing being pushed along.
///
/// **The tie needs `side`, and that is the whole subtlety.** For rows and
/// columns it does not: inserted lines are empty and interchangeable, so which
/// of two concurrent inserts at the same index ends up first is not an
/// observable fact. A sheet carries identity and content, so it is. While both
/// sides shifted on `position >= at`, two clients each adding a sheet at tab 1
/// each put their own sheet *after* the other's — one replica held `[S, X, Y]`
/// and the other `[S, Y, X]`, TP1 broken, and every later `sheet:`-indexed
/// operation then addressed a different sheet on each. Nothing errored, and
/// nothing later disagreed loudly enough to reveal it.
///
/// So the tie is broken by the order the server imposed: the operation ordered
/// **later** yields and shifts right, the **earlier** one keeps the position it
/// asked for. Both replicas compute the same answer because both are told the
/// same order, which is exactly what a server-mediated transform is for
/// (ADR-011).
fn map_sheet_position(position: usize, against: &Operation, side: Side) -> Option<usize> {
    match *against {
        Operation::InsertSheet { index: at, .. } => {
            let shifts = match position.cmp(&at) {
                std::cmp::Ordering::Greater => true,
                std::cmp::Ordering::Less => false,
                // The tie. Later yields, earlier holds.
                std::cmp::Ordering::Equal => side == Side::Later,
            };
            Some(if shifts { position + 1 } else { position })
        }
        Operation::RemoveSheet { index: at } => Some(if position > at {
            position - 1
        } else {
            position
        }),
        // A move changes what sits where without changing how many sheets there
        // are, so an insertion position is only well defined relative to the
        // sheets around it — which a bare index does not record. Refused.
        Operation::MoveSheet { .. } => None,
        _ => Some(position),
    }
}

/// Rebase an operation across one that renumbers sheets.
fn rebase_across_sheets(
    subject: &Operation,
    against: &Operation,
    side: Side,
) -> Result<Operation, TransformError> {
    // Two sheet-level operations meeting is the subtle corner — concurrent
    // reordering especially — and it is rare enough that refusing beats
    // guessing. The common case, and the one that matters, is an ordinary edit
    // meeting someone else adding or removing a sheet.
    if let Operation::MoveSheet { .. } = subject {
        return Err(unsupported(subject, against));
    }

    let mut rebased = subject.clone();
    match &mut rebased {
        // An insertion position, not a sheet.
        Operation::InsertSheet { index, .. } => match map_sheet_position(*index, against, side) {
            Some(mapped) => *index = mapped,
            None => return Err(unsupported(subject, against)),
        },
        // These name a sheet, and vanish with it.
        Operation::RemoveSheet { index } | Operation::RenameSheet { index, .. } => {
            match map_sheet_index(*index, against) {
                Some(mapped) => *index = mapped,
                None => return Ok(noop()),
            }
        }
        _ => {
            if let Some(sheet) = sheet_field_mut(&mut rebased) {
                match map_sheet_index(*sheet, against) {
                    Some(mapped) => *sheet = mapped,
                    None => return Ok(noop()),
                }
            }
        }
    }

    // Renumbering moved nothing *within* a sheet, so once the index is right the
    // only question left is contention — and two operations on what is now the
    // same sheet can still contend, e.g. two renames of it.
    Ok(resolve_contention(&rebased, against, side))
}

/// The `sheet` field of an operation that has one.
fn sheet_field_mut(op: &mut Operation) -> Option<&mut usize> {
    match op {
        Operation::SetCell { sheet, .. }
        | Operation::SetValue { sheet, .. }
        | Operation::SetStyle { sheet, .. }
        | Operation::ClearCell { sheet, .. }
        | Operation::SetColumnWidth { sheet, .. }
        | Operation::SetRowHeight { sheet, .. }
        | Operation::InsertRows { sheet, .. }
        | Operation::DeleteRows { sheet, .. }
        | Operation::InsertColumns { sheet, .. }
        | Operation::DeleteColumns { sheet, .. }
        | Operation::SetSheetMetadata { sheet, .. }
        | Operation::SetTabColor { sheet, .. } => Some(sheet),
        _ => None,
    }
}

fn unsupported(subject: &Operation, against: &Operation) -> TransformError {
    TransformError::Unsupported {
        subject: variant_name(subject),
        against: variant_name(against),
    }
}

fn variant_name(op: &Operation) -> &'static str {
    match op {
        Operation::SetCell { .. } => "SetCell",
        Operation::SetValue { .. } => "SetValue",
        Operation::SetStyle { .. } => "SetStyle",
        Operation::ClearCell { .. } => "ClearCell",
        Operation::SetColumnWidth { .. } => "SetColumnWidth",
        Operation::SetRowHeight { .. } => "SetRowHeight",
        Operation::InsertRows { .. } => "InsertRows",
        Operation::DeleteRows { .. } => "DeleteRows",
        Operation::InsertColumns { .. } => "InsertColumns",
        Operation::DeleteColumns { .. } => "DeleteColumns",
        Operation::SetSheetMetadata { .. } => "SetSheetMetadata",
        Operation::InsertSheet { .. } => "InsertSheet",
        Operation::RemoveSheet { .. } => "RemoveSheet",
        Operation::RenameSheet { .. } => "RenameSheet",
        Operation::MoveSheet { .. } => "MoveSheet",
        Operation::SetTabColor { .. } => "SetTabColor",
        Operation::SetDefinedNames(_) => "SetDefinedNames",
        Operation::Batch(_) => "Batch",
    }
}

/// Rebase an operation across a structural band on the same sheet.
fn rebase_onto_band(subject: &Operation, band: Band) -> Operation {
    match subject.clone() {
        // Cell-addressed operations follow their cell, or vanish with it.
        Operation::SetCell { sheet, at, cell } => match shift_cell(at, band) {
            Some(at) => Operation::SetCell { sheet, at, cell },
            None => noop(),
        },
        Operation::SetValue { sheet, at, value } => match shift_cell(at, band) {
            Some(at) => Operation::SetValue { sheet, at, value },
            None => noop(),
        },
        Operation::SetStyle { sheet, at, style } => match shift_cell(at, band) {
            Some(at) => Operation::SetStyle { sheet, at, style },
            None => noop(),
        },
        Operation::ClearCell { sheet, at } => match shift_cell(at, band) {
            Some(at) => Operation::ClearCell { sheet, at },
            None => noop(),
        },

        // Axis sizing follows its line — but only when the band is on the same
        // axis. A column width is untouched by an inserted row.
        Operation::SetColumnWidth { sheet, col, width } => {
            if band.axis != Axis::Col {
                return Operation::SetColumnWidth { sheet, col, width };
            }
            match shift_index(col, band) {
                Some(col) => Operation::SetColumnWidth { sheet, col, width },
                None => noop(),
            }
        }
        Operation::SetRowHeight { sheet, row, height } => {
            if band.axis != Axis::Row {
                return Operation::SetRowHeight { sheet, row, height };
            }
            match shift_index(row, band) {
                Some(row) => Operation::SetRowHeight { sheet, row, height },
                None => noop(),
            }
        }

        // Structural against structural. Cross-axis pairs are independent —
        // inserting a row does not move a column — so only the same axis needs
        // arithmetic.
        Operation::InsertRows { sheet, at, count } if band.axis == Axis::Row => {
            rebase_insert(band, at, count).map_or_else(noop, |at| Operation::InsertRows {
                sheet,
                at,
                count,
            })
        }
        Operation::InsertColumns { sheet, at, count } if band.axis == Axis::Col => {
            rebase_insert(band, at, count).map_or_else(noop, |at| Operation::InsertColumns {
                sheet,
                at,
                count,
            })
        }
        Operation::DeleteRows { sheet, at, count } if band.axis == Axis::Row => {
            rebase_delete(band, at, count).map_or_else(noop, |(at, count)| Operation::DeleteRows {
                sheet,
                at,
                count,
            })
        }
        Operation::DeleteColumns { sheet, at, count } if band.axis == Axis::Col => {
            rebase_delete(band, at, count).map_or_else(noop, |(at, count)| {
                Operation::DeleteColumns { sheet, at, count }
            })
        }

        // A metadata bundle carries positional state of its own — merges, axis
        // sizing, hidden lines, the freeze band, the outline — all of which the
        // structural op moved out from under it. Shifting the bundle by the
        // same band is what keeps a pending resize pointing at the column the
        // user resized. It reuses the shift `apply` performs on the sheet
        // itself, so the two cannot disagree.
        Operation::SetSheetMetadata {
            sheet,
            mut data,
            changed,
        } => {
            match band.inserting {
                true => crate::structural::shift_metadata_insert(
                    data.as_mut(),
                    band.axis,
                    band.at,
                    band.count,
                ),
                false => crate::structural::shift_metadata_delete(
                    data.as_mut(),
                    band.axis,
                    band.at,
                    band.count,
                ),
            }
            Operation::SetSheetMetadata {
                sheet,
                data,
                changed,
            }
        }

        // Everything else is positionally inert: a tab colour, a rename, the
        // defined-name table, and any cross-axis structural pair.
        other => other,
    }
}

/// Where an insert at `at` lands after `band`, or `None` when the band swallowed
/// the position it was inserting at.
fn rebase_insert(band: Band, at: u32, _count: u32) -> Option<u32> {
    if band.inserting {
        // Ties shift. Both sides compute the same function, so both agree.
        return Some(if at >= band.at { at + band.count } else { at });
    }
    let end = band.at.saturating_add(band.count);
    if at > band.at && at < end {
        // Strictly inside a deleted band: the position no longer exists. The
        // matching rule below widens that delete to cover these lines, and the
        // two must agree or the replicas diverge. Boundary cases are *not*
        // inside — an insert at the band's first line sits before it and
        // survives, which is what pushes the delete along.
        None
    } else if at >= end {
        Some(at - band.count)
    } else {
        Some(at)
    }
}

/// Where a delete of `[at, at + count)` lands after `band`, or `None` when
/// nothing is left to delete.
fn rebase_delete(band: Band, at: u32, count: u32) -> Option<(u32, u32)> {
    let end = at.saturating_add(count);
    let band_end = band.at.saturating_add(band.count);

    if band.inserting {
        if band.at <= at {
            // The insert is at or before the band: everything shifts down.
            return Some((at + band.count, count));
        }
        if band.at >= end {
            return Some((at, count));
        }
        // Strictly inside: the new lines are empty and sit within the band the
        // delete means to remove, so the delete widens to take them. The
        // matching rule above turns that insert into a no-op; the pair is only
        // convergent together.
        return Some((at, count + band.count));
    }

    // Delete against delete: whatever the other one already removed is not
    // ours to remove again.
    let overlap = min(end, band_end).saturating_sub(max(at, band.at));
    let remaining = count.saturating_sub(overlap);
    if remaining == 0 {
        return None;
    }
    // Lines the other delete removed from before our start.
    let removed_before = min(band_end, at).saturating_sub(band.at);
    Some((at - removed_before, remaining))
}

const fn min(a: u32, b: u32) -> u32 {
    if a < b { a } else { b }
}

const fn max(a: u32, b: u32) -> u32 {
    if a > b { a } else { b }
}

/// Decide two operations that write the same thing.
///
/// Cell granularity, matching Excel and Sheets: two people editing one cell is
/// one of them winning, not a merge of the characters. The **earlier** operation
/// yields — it becomes a no-op, because the later one has already overwritten
/// whatever it was going to do. Only one side ever yields, so the pair
/// converges however the two replicas got here.
///
/// Sheet metadata is the exception, and the reason [`SheetFields`] exists: the
/// bundle is resolved **per field**, so a column resize and a comment merge
/// instead of one discarding the other. Only the fields both touch are yielded.
fn resolve_contention(subject: &Operation, against: &Operation, side: Side) -> Operation {
    if let (
        Operation::SetSheetMetadata {
            sheet,
            data,
            changed,
        },
        Operation::SetSheetMetadata { changed: other, .. },
    ) = (subject, against)
    {
        if side == Side::Later {
            return subject.clone();
        }
        // Keep the fields the winner is not touching.
        let kept = SheetFields(changed.0 & !other.0);
        return Operation::SetSheetMetadata {
            sheet: *sheet,
            data: data.clone(),
            changed: kept,
        };
    }

    // A bundle and a single-line resize write the same field at different
    // granularities, so neither "the earlier yields the field" nor "they do not
    // collide" is right. The bundle is the whole map and the resize is one key
    // in it, so the earlier one concedes exactly that key.
    if side == Side::Earlier {
        if let Operation::SetSheetMetadata {
            sheet,
            data,
            changed,
        } = subject
            && let Some(patched) = concede_one_line(data, *changed, against)
        {
            return Operation::SetSheetMetadata {
                sheet: *sheet,
                data: Box::new(patched),
                changed: *changed,
            };
        }
        // The mirror: a resize ordered before a bundle that replaces the whole
        // map is overwritten by it wholesale, so it has nothing left to do.
        if matches!(
            subject,
            Operation::SetColumnWidth { .. } | Operation::SetRowHeight { .. }
        ) && matches!(against, Operation::SetSheetMetadata { .. })
            && subject.sheet_fields().intersects(against.sheet_fields())
        {
            return noop();
        }
    }

    if side == Side::Later || !same_target(subject, against) {
        return subject.clone();
    }
    yield_cell_aspects(subject, against)
}

/// Patch a bundle so the line a concurrent resize owns carries the resize's
/// value instead of the bundle's.
///
/// Returns `None` when the pair is not a bundle meeting a resize of a field the
/// bundle actually writes.
fn concede_one_line(
    data: &crate::SheetMetadata,
    changed: SheetFields,
    against: &Operation,
) -> Option<crate::SheetMetadata> {
    let (field, index, size) = match *against {
        Operation::SetColumnWidth { col, width, .. } => (SheetFields::COLUMNS, col, width),
        Operation::SetRowHeight { row, height, .. } => (SheetFields::ROWS, row, height),
        _ => return None,
    };
    if !changed.contains(field) {
        return None;
    }
    let mut patched = data.clone();
    let sizing = if field == SheetFields::COLUMNS {
        &mut patched.columns
    } else {
        &mut patched.rows
    };
    match size {
        Some(size) => sizing.sizes.insert(index, size),
        None => sizing.sizes.remove(&index),
    };
    Some(patched)
}

/// A cell is written in two independent aspects, and an operation writes one or
/// both.
///
/// This distinction is forced by what `apply` actually does, not invented for
/// symmetry: `SetStyle` keeps the value and the formula, and `SetValue` keeps
/// the style. So "the earlier operation yields" is too blunt — the earlier one
/// must yield **only the aspects the later one overwrites**, or a concurrent
/// bold and a concurrent typed value destroy each other for no reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Aspects {
    /// The value and the formula, which move together: `SetValue` clears the
    /// formula, so nothing writes one without the other.
    content: bool,
    style: bool,
}

fn aspects_of(op: &Operation) -> Aspects {
    match op {
        Operation::SetCell { .. } | Operation::ClearCell { .. } => Aspects {
            content: true,
            style: true,
        },
        Operation::SetValue { .. } => Aspects {
            content: true,
            style: false,
        },
        Operation::SetStyle { .. } => Aspects {
            content: false,
            style: true,
        },
        _ => Aspects {
            content: true,
            style: true,
        },
    }
}

/// Shrink the earlier operation to the aspects the later one leaves alone.
fn yield_cell_aspects(subject: &Operation, against: &Operation) -> Operation {
    let (mine, theirs) = (aspects_of(subject), aspects_of(against));
    let keep = Aspects {
        content: mine.content && !theirs.content,
        style: mine.style && !theirs.style,
    };
    if keep == mine {
        return subject.clone();
    }
    if !keep.content && !keep.style {
        return noop();
    }
    let Some((sheet, at)) = cell_target(subject) else {
        return noop();
    };
    if keep.style {
        // Only the style survives.
        let style = match subject {
            Operation::SetCell { cell, .. } => cell.as_ref().and_then(|c| c.style),
            Operation::SetStyle { style, .. } => *style,
            _ => None,
        };
        return Operation::SetStyle { sheet, at, style };
    }
    // Only the content survives. `SetValue` is precisely "content, keeping the
    // style", which is what is needed — except that it cannot carry a formula,
    // so the one case it cannot express is refused above rather than silently
    // dropping one.
    let value = match subject {
        Operation::SetCell { cell, .. } => {
            cell.as_ref().map_or(CellValue::Empty, |c| c.value.clone())
        }
        Operation::SetValue { value, .. } => value.clone(),
        _ => CellValue::Empty,
    };
    Operation::SetValue { sheet, at, value }
}

/// Whether shrinking `subject` to content-only would drop a formula.
///
/// `SetValue` is the only operation that writes content while leaving the style
/// alone, and it carries a value, not a formula. So a `SetCell` bearing a
/// formula, ordered before a concurrent style change to the same cell, has no
/// expressible rebase. Refused rather than degraded to the formula's last
/// value, because a formula quietly becoming a literal is exactly the silent
/// loss this layer exists to prevent.
fn loses_a_formula(subject: &Operation, against: &Operation) -> bool {
    let (mine, theirs) = (aspects_of(subject), aspects_of(against));
    if !(mine.content && !theirs.content && mine.style && theirs.style) {
        return false;
    }
    matches!(subject, Operation::SetCell { cell: Some(c), .. } if c.formula.is_some())
}

/// Whether two operations write the same thing, and so cannot both stand.
fn same_target(a: &Operation, b: &Operation) -> bool {
    match (cell_target(a), cell_target(b)) {
        (Some(x), Some(y)) => return x == y,
        (Some(_), None) | (None, Some(_)) => return false,
        (None, None) => {}
    }
    match (a, b) {
        (Operation::SetColumnWidth { col: x, .. }, Operation::SetColumnWidth { col: y, .. }) => {
            x == y
        }
        (Operation::SetRowHeight { row: x, .. }, Operation::SetRowHeight { row: y, .. }) => x == y,
        // Both name a sheet by index, and `sheet_of` does not see a rename's
        // index — so without this, renaming two *different* sheets read as
        // contention and one of them was silently dropped.
        (Operation::RenameSheet { index: x, .. }, Operation::RenameSheet { index: y, .. }) => {
            x == y
        }
        // Sheet already matched by the caller, and the defined-name table is
        // one workbook-wide value.
        (Operation::SetTabColor { .. }, Operation::SetTabColor { .. })
        | (Operation::SetDefinedNames(_), Operation::SetDefinedNames(_)) => true,
        _ => false,
    }
}

/// The cell an operation writes, when it writes exactly one.
fn cell_target(op: &Operation) -> Option<(usize, CellRef)> {
    match *op {
        Operation::SetCell { sheet, at, .. }
        | Operation::SetValue { sheet, at, .. }
        | Operation::SetStyle { sheet, at, .. }
        | Operation::ClearCell { sheet, at } => Some((sheet, at)),
        _ => None,
    }
}

/// Whether two operations touch anything in common — the question the server
/// asks before deciding it can commit both without ordering them.
///
/// Conservative: `true` when unsure. A false negative here is two edits merged
/// that should have been ordered, which is data loss.
#[must_use]
pub fn conflicts(a: &Operation, b: &Operation) -> bool {
    match (sheet_of(a), sheet_of(b)) {
        (Some(x), Some(y)) if x != y => return false,
        _ => {}
    }
    let (fa, fb) = (a.sheet_fields(), b.sheet_fields());
    if fa != SheetFields::NONE && fb != SheetFields::NONE {
        return fa.intersects(fb);
    }
    true
}
