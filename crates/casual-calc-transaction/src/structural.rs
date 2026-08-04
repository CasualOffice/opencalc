//! Structural row/column insert & delete with formula-reference rewriting.
//!
//! Inserting or deleting whole rows/columns has two coupled effects that must
//! stay in lock-step or the model silently corrupts:
//!
//! 1. **Geometry** — every populated cell on or after the insertion/deletion
//!    band shifts, and the sheet's [`CellStore`] is rebuilt at the new
//!    coordinates.
//! 2. **References** — every formula *anywhere in the workbook* that targets the
//!    affected sheet has its cell/range references rewritten so they keep
//!    pointing at the same logical cells (or collapse to `#REF!` when their
//!    target is deleted).
//!
//! A reference *targets* the operation's sheet `S` when it is explicitly
//! qualified with `S`'s name, or when it is unqualified and lives in a formula
//! whose home sheet *is* `S`. Cross-sheet references to `S` from other sheets
//! shift too.
//!
//! Both operations return an exact inverse, so undo is inverse replay (see
//! [`crate::apply`]). Insert's inverse is the matching delete — exact because an
//! insert opens an empty band no reference can point into. Delete's inverse is a
//! [`Operation::Batch`] of the re-insert plus `SetCell` restores of every cell
//! and formula the delete could have touched, snapshotted *before* mutation.

use casual_calc_formula::{CellReference, Expr};
use casual_calc_model::{CellRef, CellStore, Workbook};

use crate::{Operation, TxnError};

/// Which axis a structural operation runs along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    /// Rows — the vertical axis; coordinate is `CellRef::row` / `CellReference::row`.
    Row,
    /// Columns — the horizontal axis; coordinate is `CellRef::col` / `CellReference::col`.
    Col,
}

impl Axis {
    /// The along-axis coordinate of a stored cell address.
    fn coord(self, at: CellRef) -> u32 {
        match self {
            Axis::Row => at.row,
            Axis::Col => at.col,
        }
    }

    /// A copy of `at` with its along-axis coordinate replaced by `value`.
    fn with_coord(self, at: CellRef, value: u32) -> CellRef {
        match self {
            Axis::Row => CellRef::new(value, at.col),
            Axis::Col => CellRef::new(at.row, value),
        }
    }

    /// The along-axis coordinate of a formula reference.
    fn ref_coord(self, reference: &CellReference) -> u32 {
        match self {
            Axis::Row => reference.row,
            Axis::Col => reference.col,
        }
    }

    /// Set the along-axis coordinate of a formula reference in place.
    fn set_ref_coord(self, reference: &mut CellReference, value: u32) {
        match self {
            Axis::Row => reference.row = value,
            Axis::Col => reference.col = value,
        }
    }
}

/// Whether a rewrite is shifting for an insert or a delete.
#[derive(Debug, Clone, Copy)]
enum ShiftKind {
    Insert,
    Delete,
}

/// Apply an insert of `count` lines at `at` along `axis` on sheet `sheet`.
/// Returns the inverse (the matching delete).
pub(crate) fn insert(
    workbook: &mut Workbook,
    sheet: usize,
    axis: Axis,
    at: u32,
    count: u32,
) -> Result<Operation, TxnError> {
    let target = sheet_name(workbook, sheet)?;
    shift_cells_insert(workbook, sheet, axis, at, count);
    rewrite_all_formulas(workbook, &target, axis, ShiftKind::Insert, at, count);
    Ok(delete_op(sheet, axis, at, count))
}

/// Apply a delete of `count` lines starting at `at` along `axis` on sheet
/// `sheet`. Returns the inverse: a [`Operation::Batch`] of the re-insert plus
/// `SetCell` restores captured *before* mutation.
pub(crate) fn delete(
    workbook: &mut Workbook,
    sheet: usize,
    axis: Axis,
    at: u32,
    count: u32,
) -> Result<Operation, TxnError> {
    let target = sheet_name(workbook, sheet)?;
    // Snapshot everything the delete can change, before we mutate anything.
    let restores = snapshot_for_delete(workbook, sheet, axis, at);
    shift_cells_delete(workbook, sheet, axis, at, count);
    rewrite_all_formulas(workbook, &target, axis, ShiftKind::Delete, at, count);

    let mut ops = Vec::with_capacity(restores.len() + 1);
    ops.push(insert_op(sheet, axis, at, count));
    ops.extend(restores);
    Ok(Operation::Batch(ops))
}

/// Clone the target sheet's name, erroring if the sheet index is out of range.
fn sheet_name(workbook: &Workbook, sheet: usize) -> Result<String, TxnError> {
    workbook
        .sheets
        .get(sheet)
        .map(|s| s.name.clone())
        .ok_or(TxnError::SheetNotFound { index: sheet })
}

/// The `Insert*` operation for `axis`.
fn insert_op(sheet: usize, axis: Axis, at: u32, count: u32) -> Operation {
    match axis {
        Axis::Row => Operation::InsertRows { sheet, at, count },
        Axis::Col => Operation::InsertColumns { sheet, at, count },
    }
}

/// The `Delete*` operation for `axis`.
fn delete_op(sheet: usize, axis: Axis, at: u32, count: u32) -> Operation {
    match axis {
        Axis::Row => Operation::DeleteRows { sheet, at, count },
        Axis::Col => Operation::DeleteColumns { sheet, at, count },
    }
}

/// Rebuild the sheet's cell store, shifting every cell on or after `at` by
/// `+count` along `axis`. The sheet index was already validated by the caller.
fn shift_cells_insert(workbook: &mut Workbook, sheet: usize, axis: Axis, at: u32, count: u32) {
    let store = &mut workbook.sheets[sheet].cells;
    let old = std::mem::take(store);
    let mut rebuilt = CellStore::new();
    for (addr, cell) in old.iter() {
        let coord = axis.coord(addr);
        let new_addr = if coord >= at {
            axis.with_coord(addr, coord.saturating_add(count))
        } else {
            addr
        };
        rebuilt.set(new_addr, cell.clone());
    }
    *store = rebuilt;
}

/// Rebuild the sheet's cell store, dropping cells in the deleted band
/// `[at, at+count)` and shifting cells past it back by `count` along `axis`.
fn shift_cells_delete(workbook: &mut Workbook, sheet: usize, axis: Axis, at: u32, count: u32) {
    let end = at.saturating_add(count);
    let store = &mut workbook.sheets[sheet].cells;
    let old = std::mem::take(store);
    let mut rebuilt = CellStore::new();
    for (addr, cell) in old.iter() {
        let coord = axis.coord(addr);
        if coord < at {
            rebuilt.set(addr, cell.clone());
        } else if coord >= end {
            let new_addr = axis.with_coord(addr, coord - count);
            rebuilt.set(new_addr, cell.clone());
        }
        // Cells inside the deleted band are dropped.
    }
    *store = rebuilt;
}

/// Snapshot, as `SetCell` restore ops, every cell a delete could disturb: on the
/// target sheet, cells on/after `at` (they move or vanish) plus any formula
/// cell (its references may be rewritten); on every other sheet, formula cells
/// (their cross-sheet references may be rewritten). Captured before mutation so
/// each op restores the exact original cell, including its original formula
/// handle.
fn snapshot_for_delete(workbook: &Workbook, sheet: usize, axis: Axis, at: u32) -> Vec<Operation> {
    let mut restores = Vec::new();
    for (idx, s) in workbook.sheets.iter().enumerate() {
        for (addr, cell) in s.cells.iter() {
            let touched = if idx == sheet {
                axis.coord(addr) >= at || cell.formula.is_some()
            } else {
                cell.formula.is_some()
            };
            if touched {
                restores.push(Operation::SetCell {
                    sheet: idx,
                    at: addr,
                    cell: Some(cell.clone()),
                });
            }
        }
    }
    restores
}

/// A formula cell scheduled for reference rewriting.
struct RewriteJob {
    sheet: usize,
    at: CellRef,
    home: String,
    expr: Expr,
}

/// Context for rewriting one formula's references.
struct RewriteCtx<'a> {
    /// Name of the sheet the structural op runs on.
    target: &'a str,
    /// Home sheet of the formula being rewritten (resolves unqualified refs).
    home: &'a str,
    axis: Axis,
    kind: ShiftKind,
    at: u32,
    count: u32,
}

/// Rewrite the references of every formula in the workbook that targets
/// `target`. Formulas are collected first (immutable borrow), then transformed
/// and re-stored; only formulas that actually change get a new arena entry and
/// an updated cell handle. Cached values are intentionally left stale for the
/// caller to recalculate.
fn rewrite_all_formulas(
    workbook: &mut Workbook,
    target: &str,
    axis: Axis,
    kind: ShiftKind,
    at: u32,
    count: u32,
) {
    let mut jobs: Vec<RewriteJob> = Vec::new();
    for (idx, s) in workbook.sheets.iter().enumerate() {
        for (addr, cell) in s.cells.iter() {
            if let Some(handle) = cell.formula
                && let Some(expr) = workbook.formula(handle)
            {
                jobs.push(RewriteJob {
                    sheet: idx,
                    at: addr,
                    home: s.name.clone(),
                    expr: expr.clone(),
                });
            }
        }
    }

    for job in jobs {
        let ctx = RewriteCtx {
            target,
            home: &job.home,
            axis,
            kind,
            at,
            count,
        };
        let rewritten = rewrite_expr(&job.expr, &ctx);
        if rewritten != job.expr {
            let handle = workbook.store_formula(rewritten);
            let store = &mut workbook.sheets[job.sheet].cells;
            if let Some(existing) = store.get(job.at) {
                let mut updated = existing.clone();
                updated.formula = Some(handle);
                store.set(job.at, updated);
            }
        }
    }
}

/// Recursively rewrite references within an expression.
fn rewrite_expr(expr: &Expr, ctx: &RewriteCtx) -> Expr {
    match expr {
        Expr::Reference(reference) => rewrite_reference(reference, ctx),
        Expr::Range(a, b) => rewrite_range(a, b, ctx),
        Expr::Unary { op, operand } => Expr::Unary {
            op: *op,
            operand: Box::new(rewrite_expr(operand, ctx)),
        },
        Expr::Binary { op, left, right } => Expr::Binary {
            op: *op,
            left: Box::new(rewrite_expr(left, ctx)),
            right: Box::new(rewrite_expr(right, ctx)),
        },
        Expr::Function { name, args } => Expr::Function {
            name: name.clone(),
            args: args.iter().map(|arg| rewrite_expr(arg, ctx)).collect(),
        },
        // Literals and defined names carry no cell coordinates.
        Expr::Number(_) | Expr::Bool(_) | Expr::Text(_) | Expr::Error(_) | Expr::Name(_) => {
            expr.clone()
        }
    }
}

/// Whether a reference targets the operation's sheet: qualified with its name,
/// or unqualified inside a formula whose home sheet is the target.
fn targets(reference: &CellReference, ctx: &RewriteCtx) -> bool {
    match &reference.sheet {
        Some(name) => name == ctx.target,
        None => ctx.home == ctx.target,
    }
}

/// The `#REF!` error expression a collapsed reference becomes.
fn ref_error() -> Expr {
    Expr::Error("#REF!".to_owned())
}

/// Rewrite a single (non-range) cell reference.
fn rewrite_reference(reference: &CellReference, ctx: &RewriteCtx) -> Expr {
    if !targets(reference, ctx) {
        return Expr::Reference(reference.clone());
    }
    let coord = ctx.axis.ref_coord(reference);
    match ctx.kind {
        ShiftKind::Insert => {
            let mut shifted = reference.clone();
            if coord >= ctx.at {
                ctx.axis
                    .set_ref_coord(&mut shifted, coord.saturating_add(ctx.count));
            }
            Expr::Reference(shifted)
        }
        ShiftKind::Delete => {
            let end = ctx.at.saturating_add(ctx.count);
            if coord >= ctx.at && coord < end {
                // The referenced line was deleted.
                ref_error()
            } else if coord >= end {
                let mut shifted = reference.clone();
                ctx.axis.set_ref_coord(&mut shifted, coord - ctx.count);
                Expr::Reference(shifted)
            } else {
                Expr::Reference(reference.clone())
            }
        }
    }
}

/// Rewrite a range. Targeting is decided from the first endpoint (the qualifier
/// on a sheet-qualified range); both endpoints then move together.
fn rewrite_range(a: &CellReference, b: &CellReference, ctx: &RewriteCtx) -> Expr {
    if !targets(a, ctx) {
        return Expr::Range(a.clone(), b.clone());
    }
    match ctx.kind {
        ShiftKind::Insert => {
            Expr::Range(shift_endpoint_insert(a, ctx), shift_endpoint_insert(b, ctx))
        }
        ShiftKind::Delete => rewrite_range_delete(a, b, ctx),
    }
}

/// Shift one range endpoint for an insert: bump its along-axis coordinate if it
/// is on or after the insertion point.
fn shift_endpoint_insert(endpoint: &CellReference, ctx: &RewriteCtx) -> CellReference {
    let mut shifted = endpoint.clone();
    let coord = ctx.axis.ref_coord(endpoint);
    if coord >= ctx.at {
        ctx.axis
            .set_ref_coord(&mut shifted, coord.saturating_add(ctx.count));
    }
    shifted
}

/// Rewrite a range for a delete, clamping endpoints that fall in the deleted
/// band and collapsing to `#REF!` when nothing survives.
fn rewrite_range_delete(a: &CellReference, b: &CellReference, ctx: &RewriteCtx) -> Expr {
    let ca = ctx.axis.ref_coord(a);
    let cb = ctx.axis.ref_coord(b);
    let (lo, hi, a_is_lo) = if ca <= cb {
        (ca, cb, true)
    } else {
        (cb, ca, false)
    };
    match map_range_delete(lo, hi, ctx.at, ctx.count) {
        None => ref_error(),
        Some((new_lo, new_hi)) => {
            let mut na = a.clone();
            let mut nb = b.clone();
            let (lo_ref, hi_ref) = if a_is_lo {
                (&mut na, &mut nb)
            } else {
                (&mut nb, &mut na)
            };
            ctx.axis.set_ref_coord(lo_ref, new_lo);
            ctx.axis.set_ref_coord(hi_ref, new_hi);
            Expr::Range(na, nb)
        }
    }
}

/// Map a normalized `[lo, hi]` coordinate span through a delete of the band
/// `[at, at+count)`, returning the surviving span or `None` for `#REF!`.
///
/// A deleted low endpoint clamps up to `at` (the line the band collapses onto);
/// a deleted high endpoint clamps down to `at-1` (the last kept line before the
/// band). Kept endpoints past the band shift back by `count`. If the whole span
/// is inside the band — or the mapping would invert — nothing survives.
fn map_range_delete(lo: u32, hi: u32, at: u32, count: u32) -> Option<(u32, u32)> {
    let end = at.saturating_add(count);
    let lo_deleted = lo >= at && lo < end;
    let hi_deleted = hi >= at && hi < end;
    if lo_deleted && hi_deleted {
        // The entire span lies within the deleted band (covers `at == 0` too).
        return None;
    }
    let new_lo = if lo < at {
        lo
    } else if lo_deleted {
        at
    } else {
        lo - count
    };
    let new_hi = if hi < at {
        hi
    } else if hi_deleted {
        // `hi_deleted` with `lo` kept implies `lo < at`, hence `at >= 1`.
        at - 1
    } else {
        hi - count
    };
    if new_lo > new_hi {
        return None;
    }
    Some((new_lo, new_hi))
}
