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

use crate::{
    Operation, SheetFields,
    structural::{Axis, CarriedShift, ShiftKind},
};
use casual_calc_formula::{Expr, stored::Origin};
use casual_calc_model::{Cell, CellRef, CellValue, FormulaHandle};

mod line_move;

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
        | Operation::MoveColumns { sheet, .. }
        | Operation::MoveRows { sheet, .. }
        | Operation::MoveRange { sheet, .. }
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
    /// The sheet the structural operation runs on. Not on the wire — derived
    /// from the operation here, so the metadata arm can ask what that index
    /// actually names (`FID-28`).
    sheet: Option<usize>,
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
        sheet: sheet_of(op),
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
/// What each sheet is called and which sheet it is, indexed by sheet number.
///
/// The transform is a pure function over operations, and an operation carries a
/// sheet *index*: enough to move a position, not enough to decide whether a
/// chart's `Sheet1!$D$2:$D$11` or a pivot's `source_sheet` names the sheet a
/// concurrent insert is shifting. Rather than make every structural operation
/// repeat that identity on the wire, the callers — which hold the workbook —
/// pass it in. A pure function is allowed to take more arguments; it is not
/// allowed to go and look something up (`FID-28`).
///
/// An empty slice means "identity unknown", and the two shifts that need it are
/// skipped rather than guessed.
pub type SheetNames = [(String, casual_calc_model::SheetId)];

/// The formula table a rebase needs when the operation it is moving **carries**
/// a formula.
///
/// `SheetNames` is the same idea for sheet identity: the transform is a pure
/// function and is handed what it cannot look up (`FID-28`). A formula needs
/// more than a lookup, though — rewriting one produces a *new* tree, and a
/// [`Cell`] refers to its formula by an arena handle,
/// so the new tree has to be interned somewhere before a handle for it exists.
/// This is that capability and nothing else: read one tree, intern the tree it
/// becomes. It is deliberately not `&mut Workbook`, which would let the
/// transform go looking for anything at all.
///
/// # Why it can refuse
///
/// [`transform`] has no table, so it answers `None` here and the pair is
/// refused rather than merged wrongly — see [`NoFormulas`].
pub trait FormulaTable {
    /// Read the tree behind `handle`, put it through `rewrite`, and intern the
    /// result.
    ///
    /// `None` when this table cannot — an unknown handle, or no table at all.
    /// The caller turns that into [`TransformError::Unsupported`]; it must
    /// never turn it into "carry the formula over unchanged", which is the
    /// silent divergence `COL-46` was.
    fn rebase(
        &mut self,
        handle: FormulaHandle,
        rewrite: &dyn Fn(&Expr) -> Expr,
    ) -> Option<FormulaHandle>;
}

/// A [`FormulaTable`] that holds nothing, so every rebase of a carried formula
/// is refused.
///
/// What plain [`transform`] uses. A caller with a workbook should pass it
/// instead — [`transform_with_formulas`] — and get the pair answered.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoFormulas;

impl FormulaTable for NoFormulas {
    fn rebase(&mut self, _: FormulaHandle, _: &dyn Fn(&Expr) -> Expr) -> Option<FormulaHandle> {
        None
    }
}

impl FormulaTable for casual_calc_model::Workbook {
    fn rebase(
        &mut self,
        handle: FormulaHandle,
        rewrite: &dyn Fn(&Expr) -> Expr,
    ) -> Option<FormulaHandle> {
        let expr = self.formula(handle)?.clone();
        Some(self.store_formula(rewrite(&expr)))
    }
}

/// [`transform`], with a table to rebase carried formulas through.
///
/// The pair `transform` refuses for want of one — an unacknowledged
/// [`Operation::SetCell`] holding a formula, meeting a concurrent insert or
/// delete — is answered here. Every caller that holds a workbook should use
/// this one; [`session`](crate::session) does.
///
/// # Errors
///
/// As [`transform`], plus [`TransformError::Unsupported`] when `formulas` does
/// not know a handle the operation carries, or when `sheets` does not say what
/// the operation's sheet index is called (the rewrite has to decide whether a
/// `Sheet1!$D$2` names it).
pub fn transform_with_formulas(
    subject: &Operation,
    against: &Operation,
    side: Side,
    sheets: &SheetNames,
    formulas: &mut dyn FormulaTable,
) -> Result<Operation, TransformError> {
    transform_inner(subject, against, side, sheets, formulas)
}

/// # Errors
///
/// [`TransformError::Unsupported`] for the pairs listed in the module docs, and
/// for any operation carrying a formula that a concurrent insert or delete
/// moves — this entry point has no formula table to rewrite it through. Use
/// [`transform_with_formulas`] when a workbook is at hand.
pub fn transform(
    subject: &Operation,
    against: &Operation,
    side: Side,
    sheets: &SheetNames,
) -> Result<Operation, TransformError> {
    transform_inner(subject, against, side, sheets, &mut NoFormulas)
}

fn transform_inner(
    subject: &Operation,
    against: &Operation,
    side: Side,
    sheets: &SheetNames,
    formulas: &mut dyn FormulaTable,
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
            let rebased = transform_inner(member, &other, side, sheets, formulas)?;
            other = transform_inner(&other, member, side.flip(), sheets, formulas)?;
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
            current = transform_inner(&current, member, side, sheets, formulas)?;
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

    // **A move is a permutation, which `Band` cannot express**, so the pair
    // goes to the module that can: reordering a band is a delete and an insert
    // at once on the same axis, and the fall-through here ("`against` moves
    // nothing, so `subject`'s coordinates still mean what they meant") is false
    // for it. A *rectangle* move is still refused, and so are the two shapes of
    // concurrent reorder that no single `Move*` can name — [`line_move`] argues
    // each from convergence. Concurrency only: an uncontended move never
    // reaches `transform` at all.
    if line_move::involves_move(subject) || line_move::involves_move(against) {
        return line_move::rebase(subject, against, side, sheets, formulas);
    }

    let band = band_of(against);

    // Different sheets never interact **positionally**, and that was read as
    // "never interact at all". A formula is the exception and the only one: a
    // reference qualified with another sheet's name — `S!$D$1` sitting on sheet
    // `T` — is moved by a structural operation on `S`, because `apply` rewrites
    // every formula in the *workbook* that targets the banded sheet and not
    // merely the ones living on it. So a cell edit carrying a formula still
    // goes through the band; nothing else does, and its address does not move.
    match (sheet_of(subject), sheet_of(against)) {
        (Some(a), Some(b)) if a != b => {
            return match band {
                Some(band) if carries_formula(subject) => {
                    rebase_onto_band(subject, band, sheets, formulas)
                }
                _ => Ok(subject.clone()),
            };
        }
        _ => {}
    }

    let Some(band) = band else {
        // `against` moves no *lines*. It may still have moved a **name**: a
        // concurrent `RenameSheet` rewrites every qualified reference in the
        // workbook, so an operation carrying one has to follow it or the two
        // replicas end up with `S!$C$3` and `renamed0!$C$3` for the same cell.
        // The `COL-46` shape again, one axis over — a carried reference that
        // was not rebased, diverging with nothing raised.
        let renamed = rebase_across_rename(subject, against, sheets, formulas)?;
        // What is left is contention for the same target, which the settled
        // order decides.
        return Ok(resolve_contention(&renamed, against, side));
    };

    rebase_onto_band(subject, band, sheets, formulas)
}

/// Whether an operation writes a cell that holds a formula.
fn carries_formula(op: &Operation) -> bool {
    matches!(op, Operation::SetCell { cell: Some(cell), .. } if cell.formula.is_some())
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
        // These name a sheet, and vanish with it — except that a rename does
        // not only touch the sheet it names. It rewrites every `Old!A1` in the
        // workbook, on every other sheet and in the defined names, so dropping
        // it to a no-op keeps the sheet list right and loses that rewrite: one
        // replica renamed `S` and then removed it, leaving the references
        // reading `renamed0!`, and the other removed `S` first and left them
        // reading `S!`. Same shape as the band above, refused for the same
        // reason — there is no operation that performs only the rewrite.
        Operation::RenameSheet { index, .. } => match map_sheet_index(*index, against) {
            Some(mapped) => *index = mapped,
            None => return Err(unsupported(subject, against)),
        },
        Operation::RemoveSheet { index } => match map_sheet_index(*index, against) {
            Some(mapped) => *index = mapped,
            None => return Ok(noop()),
        },
        _ => {
            if let Some(sheet) = sheet_field_mut(&mut rebased) {
                match map_sheet_index(*sheet, against) {
                    Some(mapped) => *sheet = mapped,
                    // **The sheet this operation was about is gone — but a
                    // structural one did not only act on that sheet.** An
                    // insert or a delete rewrites every formula in the
                    // *workbook* that points at the sheet it runs on, plus the
                    // defined names, so dropping it to a no-op keeps the cells
                    // right and silently loses that rewrite. The replica that
                    // ordered the insert first shifted `T!` formulas reading
                    // `S!$B$2` to `S!$B$3` and then removed `S`; the one that
                    // removed `S` first never shifted them. Both end with `S`
                    // gone and with different formulas in `T`.
                    //
                    // There is no operation in the closed set that performs
                    // only that rewrite, so there is no answer to give and the
                    // pair is refused (`COL-49`). Found by TP1 the first time
                    // the seed carried a cross-sheet reference; every other
                    // operation really is confined to its own sheet, which is
                    // why the no-op is right for them and only for them.
                    None if band_of(&rebased).is_some() => {
                        return Err(unsupported(subject, against));
                    }
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
        | Operation::MoveColumns { sheet, .. }
        | Operation::MoveRows { sheet, .. }
        | Operation::MoveRange { sheet, .. }
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
        Operation::MoveColumns { .. } => "MoveColumns",
        Operation::MoveRows { .. } => "MoveRows",
        Operation::MoveRange { .. } => "MoveRange",
        Operation::SetTabColor { .. } => "SetTabColor",
        Operation::SetDefinedNames(_) => "SetDefinedNames",
        Operation::Batch(_) => "Batch",
    }
}

/// Follow a concurrent [`Operation::RenameSheet`] through the references an
/// operation **carries**.
///
/// A rename is not a position, so no band describes it and the fall-through
/// used to carry the operation over untouched. That is wrong for the same
/// reason `COL-46` was: `apply` rewrites every `Old!A1` in the workbook when
/// the rename lands, so a pending `SetDefinedNames` or a pending cell formula
/// that still says `Old!` means something different on the replica that
/// ordered the rename first.
///
/// Everything without a sheet qualifier is untouched, which is most of what
/// exists — and cheaply so, since `rename_sheet_references` reports whether it
/// changed anything.
///
/// # Errors
///
/// [`TransformError::Unsupported`] when `sheets` does not say what the renamed
/// index was called (the rewrite matches on the *old* name, which only the
/// caller knows), or when `formulas` cannot rewrite a tree the operation
/// carries.
fn rebase_across_rename(
    subject: &Operation,
    against: &Operation,
    sheets: &SheetNames,
    formulas: &mut dyn FormulaTable,
) -> Result<Operation, TransformError> {
    let Operation::RenameSheet { index, name } = against else {
        return Ok(subject.clone());
    };
    // Nothing an operation can carry names a sheet, so nothing can move.
    if !carries_a_reference(subject) {
        return Ok(subject.clone());
    }
    let refuse = || unsupported(subject, against);
    let old = sheets.get(*index).ok_or_else(refuse)?.0.clone();
    // **The snapshot already shows the new name, so the old one is unknowable.**
    // `RenameSheet` carries only the name it is moving *to*; the name it is
    // moving *from* is a fact about state, and the only place it can come from
    // is `sheets`. `ClientSession` builds that from a workbook the arrival has
    // not been applied to yet, so it holds the old name and the rewrite works.
    // `ServerSession` builds it from a workbook the committed history *has*
    // been applied to, so for a rename inside that history it already reads the
    // new name — and a rewrite keyed on it would match nothing and leave the
    // pending formula pointing at a sheet that no longer answers to that name.
    //
    // Refused rather than passed through, because passed through is the
    // divergence: the submitting client rewrites its own copy when the rename
    // reaches it and the server does not. A rename genuinely to the same name
    // is a no-op and loses nothing by being refused with it.
    if old == *name {
        return Err(refuse());
    }
    let rename = |expr: &Expr| {
        let mut renamed = expr.clone();
        casual_calc_formula::rename_sheet_references(&mut renamed, &old, name);
        renamed
    };
    Ok(match subject.clone() {
        Operation::SetCell {
            sheet,
            at,
            cell: Some(mut cell),
        } => {
            if let Some(handle) = cell.formula {
                cell.formula = Some(formulas.rebase(handle, &rename).ok_or_else(refuse)?);
            }
            Operation::SetCell {
                sheet,
                at,
                cell: Some(cell),
            }
        }
        Operation::SetDefinedNames(names) => Operation::SetDefinedNames(
            names
                .into_iter()
                .map(|mut defined| {
                    defined.formula = rename(&defined.formula);
                    defined
                })
                .collect(),
        ),
        other => other,
    })
}

/// Whether an operation carries a formula, and so can name a sheet by name.
///
/// A [`Operation::Batch`] never reaches here — `transform` has already split it
/// into members — so it deliberately answers `false` rather than recursing into
/// something that cannot arrive.
fn carries_a_reference(op: &Operation) -> bool {
    match op {
        Operation::SetCell { cell, .. } => cell.as_ref().is_some_and(|c| c.formula.is_some()),
        Operation::SetDefinedNames(_) => true,
        _ => false,
    }
}

/// One structural change to the grid, in the terms a formula rewrite needs.
///
/// A [`Band`] and a line move are different operations that do the same thing
/// to a formula — [`crate::structural`] already reduces all three to one
/// [`ShiftKind`] — so the rewrite is described once here and both paths build
/// it. Keeping them apart is how the move transform and the band transform
/// would drift into rewriting formulas differently.
#[derive(Debug, Clone, Copy)]
pub(super) struct Shift {
    /// The sheet the operation runs on, whose references move.
    sheet: usize,
    axis: Axis,
    kind: ShiftKind,
    at: u32,
    count: u32,
    /// The operation's name, for the refusal when the context is not enough.
    against: &'static str,
}

impl Band {
    /// The formula rewrite this band implies, or `None` when the band's own
    /// sheet is unknown — which cannot happen for the four operations
    /// [`band_of`] accepts, all of which name a sheet.
    const fn shift(self) -> Option<Shift> {
        let Some(sheet) = self.sheet else {
            return None;
        };
        Some(Shift {
            sheet,
            axis: self.axis,
            kind: if self.inserting {
                ShiftKind::Insert
            } else {
                ShiftKind::Delete
            },
            at: self.at,
            count: self.count,
            against: match (self.axis, self.inserting) {
                (Axis::Row, true) => "InsertRows",
                (Axis::Row, false) => "DeleteRows",
                (Axis::Col, true) => "InsertColumns",
                (Axis::Col, false) => "DeleteColumns",
            },
        })
    }
}

/// How `shift` rewrites the formulas of a document, or `None` when `sheets`
/// does not say what the indices involved are called.
///
/// Both names are needed and they are not the same question. `target` is the
/// sheet the operation runs on — what a qualified `Sheet1!$D$2` has to name to
/// be moved. `home` is the sheet the formula lives on, which is what decides an
/// *unqualified* reference. They are equal whenever the edit is on the banded
/// sheet and differ for the cross-sheet case, which is the whole reason the
/// rewrite takes two.
fn carried_shift<'a>(
    shift: Shift,
    home_name: &'a str,
    sheets: &'a SheetNames,
) -> Option<CarriedShift<'a>> {
    let target = sheets.get(shift.sheet)?;
    Some(CarriedShift {
        target_name: &target.0,
        home_name,
        axis: shift.axis,
        kind: shift.kind,
        at: shift.at,
        count: shift.count,
    })
}

/// Rebase the formula a [`Operation::SetCell`] carries, so it means on the
/// rebased state what it meant on the state it was written against.
///
/// This is `COL-46`. The cell address was already being moved and the formula
/// was carried verbatim, so a replica that applied the concurrent insert first
/// kept `=$D$1` where the other had been rewritten to `=$E$1` — two replicas of
/// one document holding different formulas, with nothing raised anywhere.
///
/// **It is not only the `$`-anchored references that move**, which is the part
/// that reads wrong until it is worked through. A relative reference is stored
/// as an offset from its own cell (`PERF-11`), so it survives a band that lies
/// outside the span between the reference and the cell — and diverges just as
/// loudly for one that lies inside it. `=A1` in `B2` across an
/// `InsertColumns{at:1}` is the smallest example.
///
/// # Errors
///
/// [`TransformError::Unsupported`] when `formulas` cannot rewrite the tree, or
/// when `sheets` does not name the sheet the rewrite has to decide against.
/// Refusing is the whole point: the alternative is the divergence above.
pub(super) fn rebase_cell_formula(
    cell: Option<Cell>,
    sheet: usize,
    from: CellRef,
    to: CellRef,
    shift: Shift,
    sheets: &SheetNames,
    formulas: &mut dyn FormulaTable,
) -> Result<Option<Cell>, TransformError> {
    let Some(mut cell) = cell else {
        return Ok(None);
    };
    let Some(handle) = cell.formula else {
        return Ok(Some(cell));
    };
    let refuse = || TransformError::Unsupported {
        subject: "SetCell",
        against: shift.against,
    };
    let home = sheets.get(sheet).ok_or_else(refuse)?;
    let carried = carried_shift(shift, &home.0, sheets).ok_or_else(refuse)?;
    let (from, to) = (Origin::at(from.row, from.col), Origin::at(to.row, to.col));
    cell.formula = Some(
        formulas
            .rebase(handle, &|expr| carried.cell_formula(expr, from, to))
            .ok_or_else(refuse)?,
    );
    Ok(Some(cell))
}

/// Rebase an operation across a structural band on the same sheet.
///
/// # Errors
///
/// [`TransformError::Unsupported`] when the operation carries a formula that
/// the band moves and `formulas` cannot rewrite it — see [`FormulaTable`].
fn rebase_onto_band(
    subject: &Operation,
    band: Band,
    sheets: &SheetNames,
    formulas: &mut dyn FormulaTable,
) -> Result<Operation, TransformError> {
    let out =
        match subject.clone() {
            // Cell-addressed operations follow their cell, or vanish with it — and
            // a cell carrying a **formula** carries coordinates of its own, which
            // the band moved just as surely as it moved the address (`COL-46`).
            Operation::SetCell { sheet, at, cell } => {
                // The **address** moves only when the band is on this cell's own
                // sheet. The **formula** moves either way: a reference qualified
                // with the banded sheet's name is rewritten by `apply` wherever it
                // lives, so an edit on `T` carrying `S!$D$1` is reached by an
                // insert on `S` and its address is not.
                let to = if band.sheet == Some(sheet) {
                    match shift_cell(at, band) {
                        Some(to) => to,
                        None => return Ok(noop()),
                    }
                } else {
                    at
                };
                let Some(shift) = band.shift() else {
                    return Ok(Operation::SetCell {
                        sheet,
                        at: to,
                        cell,
                    });
                };
                Operation::SetCell {
                    sheet,
                    at: to,
                    cell: rebase_cell_formula(cell, sheet, at, to, shift, sheets, formulas)?,
                }
            }
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
                    return Ok(Operation::SetColumnWidth { sheet, col, width });
                }
                match shift_index(col, band) {
                    Some(col) => Operation::SetColumnWidth { sheet, col, width },
                    None => noop(),
                }
            }
            Operation::SetRowHeight { sheet, row, height } => {
                if band.axis != Axis::Row {
                    return Ok(Operation::SetRowHeight { sheet, row, height });
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
                rebase_delete(band, at, count).map_or_else(noop, |(at, count)| {
                    Operation::DeleteRows { sheet, at, count }
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
                // Bytes, not positions. A retained chart part has nothing to shift.
                restore,
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
                // A chart's series is a reference *string* and a pivot's source is
                // on another sheet, so neither can be decided from a sheet index
                // alone — which is why `shift_metadata_*` above leaves them. The
                // caller holds the workbook and passed what the indices name, so
                // they can be shifted here rather than reinstating a pre-insert
                // reference when this bundle lands (`FID-28`).
                if let (Some(target), Some(home)) =
                    (band.sheet.and_then(|i| sheets.get(i)), sheets.get(sheet))
                {
                    crate::structural::shift_bundle_references(
                        data.as_mut(),
                        &crate::structural::BundleShift {
                            target_name: &target.0,
                            target_id: target.1,
                            home_name: &home.0,
                            axis: band.axis,
                            kind: if band.inserting {
                                ShiftKind::Insert
                            } else {
                                ShiftKind::Delete
                            },
                            at: band.at,
                            count: band.count,
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

            // **A defined name is a formula with no cell**, and it was being
            // carried across a band verbatim — found by this crate's TP1 property
            // the first time that property could see a formula at all (`COL-50`,
            // the second head of `COL-46`). `apply` rewrites every name targeting
            // the sheet a band runs on, so `Anchor = S!$C$3` written concurrently
            // with `InsertRows{at:0}` settled as `$C$4` on the replica that
            // ordered the name first and `$C$3` on the other.
            //
            // No formula table is needed and none is asked for: a name carries its
            // tree inline rather than by handle, so the rewrite is pure.
            Operation::SetDefinedNames(names) => {
                let Some(shift) = band.shift() else {
                    return Ok(Operation::SetDefinedNames(names));
                };
                let refuse = || TransformError::Unsupported {
                    subject: "SetDefinedNames",
                    against: shift.against,
                };
                let mut shifted = Vec::with_capacity(names.len());
                for mut name in names {
                    // A name's home is the sheet it is *scoped* to, which is what
                    // resolves an unqualified reference inside it. A
                    // workbook-scoped name has none, and the empty name it gets
                    // here is the same answer `rewrite_defined_names` reaches —
                    // deliberately no sheet at all, so an unqualified reference is
                    // left alone rather than rewritten against a sheet picked
                    // arbitrarily.
                    let home = name
                        .sheet
                        .and_then(|id| sheets.iter().find(|(_, sheet_id)| *sheet_id == id))
                        .map_or("", |(sheet_name, _)| sheet_name.as_str());
                    let carried = carried_shift(shift, home, sheets).ok_or_else(refuse)?;
                    name.formula = carried.free_formula(&name.formula);
                    shifted.push(name);
                }
                Operation::SetDefinedNames(shifted)
            }

            // Everything else is positionally inert: a tab colour, a rename, and
            // any cross-axis structural pair.
            other => other,
        };
    Ok(out)
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
            restore,
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
            restore: restore.clone(),
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
            restore,
        } = subject
            && let Some(patched) = concede_one_line(data, *changed, against)
        {
            return Operation::SetSheetMetadata {
                sheet: *sheet,
                data: Box::new(patched),
                changed: *changed,
                restore: restore.clone(),
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
