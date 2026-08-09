//! The recalc dependency graph: which formula cells must be recomputed when a
//! set of cells changes.
//!
//! This is what makes [`crate::recalculate_incremental`] touch only a changed
//! cell's transitive dependents instead of the whole workbook. The graph is
//! rebuilt per incremental pass (one scan of all formulas) rather than
//! maintained across edits — a deliberately simple, obviously-correct first
//! step; a persistent graph is a later optimization. Correctness is pinned by a
//! differential test that asserts an incremental pass equals a full recalc.

use std::collections::{HashMap, HashSet};

use casual_calc_formula::{CellReference, Expr};
use casual_calc_model::{CellRef, Workbook};

/// `(sheet_index, row, col)`.
pub(crate) type CellKey = (usize, u32, u32);

/// A rectangular precedent range on one sheet (inclusive), plus the formula
/// cell that reads it.
struct RangeEdge {
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    dependent: CellKey,
}

/// Compute the set of formula cells that must be recomputed when `changed`
/// cells take new values. Includes the changed cells themselves when they are
/// formulas (their own formula may have just been edited) and every formula
/// that transitively references a changed cell. Formulas that use a defined
/// name are treated conservatively as always-dirty (a name's target is not
/// resolved here), which keeps the result correct if not maximally minimal.
pub(crate) fn dirty_set(workbook: &Workbook, changed: &[CellKey]) -> HashSet<CellKey> {
    // precedent cell -> formula cells that read it directly.
    let mut direct: HashMap<CellKey, Vec<CellKey>> = HashMap::new();
    // range precedents, scanned linearly (a cell may fall inside any of them).
    let mut ranges: Vec<RangeEdge> = Vec::new();
    // formulas that reference a defined name: recompute on any change.
    let mut name_users: Vec<CellKey> = Vec::new();

    for (sheet_index, sheet) in workbook.sheets.iter().enumerate() {
        for (at, cell) in sheet.cells.iter() {
            let Some(handle) = cell.formula else { continue };
            let Some(expr) = workbook.formula(handle) else {
                continue;
            };
            let dependent = (sheet_index, at.row, at.col);
            let mut uses_name = false;
            collect_precedents(
                expr,
                sheet_index,
                workbook,
                &mut |p| direct.entry(p).or_default().push(dependent),
                &mut |sheet, r0, c0, r1, c1| {
                    ranges.push(RangeEdge {
                        sheet,
                        r0,
                        c0,
                        r1,
                        c1,
                        dependent,
                    })
                },
                &mut uses_name,
            );
            if uses_name {
                name_users.push(dependent);
            }
        }
    }

    let is_formula = |k: CellKey| {
        workbook
            .sheets
            .get(k.0)
            .and_then(|s| s.cells.get(casual_calc_model::CellRef::new(k.1, k.2)))
            .is_some_and(|c| c.formula.is_some())
    };

    let mut dirty: HashSet<CellKey> = HashSet::new();
    let mut work: Vec<CellKey> = Vec::new();
    // Seed: the changed cells drive propagation; changed formula cells are
    // themselves dirty. Name-using formulas are unconditionally dirty.
    for &c in changed {
        work.push(c);
        if is_formula(c) {
            dirty.insert(c);
        }
    }
    for &n in &name_users {
        if dirty.insert(n) {
            work.push(n);
        }
    }

    while let Some(x) = work.pop() {
        if let Some(deps) = direct.get(&x) {
            for &d in deps {
                if dirty.insert(d) {
                    work.push(d);
                }
            }
        }
        for e in &ranges {
            if e.sheet == x.0
                && x.1 >= e.r0
                && x.1 <= e.r1
                && x.2 >= e.c0
                && x.2 <= e.c1
                && dirty.insert(e.dependent)
            {
                work.push(e.dependent);
            }
        }
    }
    dirty
}

/// The cells and ranges a formula reads directly — its **precedents** — as
/// `(sheet, r0, c0, r1, c1)` blocks. A single-cell reference is a 1x1 block.
///
/// Public because "what does this formula read?" is a question the *user* asks,
/// not only the recalculator: tracing precedents is how a wrong answer gets
/// diagnosed. This is the same walk the dirty-set uses, so a traced arrow can
/// never disagree with what recalculation actually followed.
pub fn precedents_of(
    workbook: &Workbook,
    sheet: usize,
    at: CellRef,
) -> Vec<(usize, u32, u32, u32, u32)> {
    let Some(cell) = workbook.sheets.get(sheet).and_then(|sh| sh.cells.get(at)) else {
        return Vec::new();
    };
    let Some(expr) = cell
        .formula
        .and_then(|id| workbook.formulas.get(id.0 as usize))
    else {
        return Vec::new();
    };
    // Two accumulators rather than one: the walker takes both callbacks at once,
    // so they cannot share a mutable borrow.
    let mut cells = Vec::new();
    let mut ranges = Vec::new();
    let mut uses_name = false;
    collect_precedents(
        expr,
        sheet,
        workbook,
        &mut |(si, r, c)| cells.push((si, r, c, r, c)),
        &mut |si, r0, c0, r1, c1| ranges.push((si, r0, c0, r1, c1)),
        &mut uses_name,
    );
    let mut out = cells;
    out.extend(ranges);
    out.sort_unstable();
    out.dedup();
    out
}

/// The formula cells that read `at`, directly — its **dependents**.
///
/// Walks every formula in the workbook rather than keeping a reverse index: the
/// trace is a deliberate, one-off action, and a persistent reverse map would have
/// to be maintained on every edit for something asked once in a while.
pub fn dependents_of(workbook: &Workbook, sheet: usize, at: CellRef) -> Vec<(usize, u32, u32)> {
    let mut out = Vec::new();
    for (si, sh) in workbook.sheets.iter().enumerate() {
        for (addr, cell) in sh.cells.iter() {
            let Some(expr) = cell
                .formula
                .and_then(|id| workbook.formulas.get(id.0 as usize))
            else {
                continue;
            };
            let mut by_cell = false;
            let mut by_range = false;
            let mut uses_name = false;
            collect_precedents(
                expr,
                si,
                workbook,
                &mut |(ps, r, c)| {
                    if ps == sheet && r == at.row && c == at.col {
                        by_cell = true;
                    }
                },
                &mut |ps, r0, c0, r1, c1| {
                    if ps == sheet && at.row >= r0 && at.row <= r1 && at.col >= c0 && at.col <= c1 {
                        by_range = true;
                    }
                },
                &mut uses_name,
            );
            if by_cell || by_range {
                out.push((si, addr.row, addr.col));
            }
        }
    }
    out.sort_unstable();
    out
}

/// Walk `expr`, reporting each single-cell precedent to `on_cell`, each range
/// precedent to `on_range`, and setting `uses_name` if a defined name appears.
fn collect_precedents(
    expr: &Expr,
    ctx_sheet: usize,
    workbook: &Workbook,
    on_cell: &mut impl FnMut(CellKey),
    on_range: &mut impl FnMut(usize, u32, u32, u32, u32),
    uses_name: &mut bool,
) {
    match expr {
        // A structured reference's dependencies cannot be resolved from the
        // expression alone — it names a table, and the table's range decides
        // the cells. Treated like a defined name: the formula recalculates on
        // any change rather than tracking a narrower dependency, which is
        // conservative and never stale.
        Expr::StructuredRef { .. } => *uses_name = true,
        Expr::Reference(r) => {
            if let Some(si) = resolve_sheet(r, ctx_sheet, workbook) {
                on_cell((si, r.row, r.col));
            }
        }
        Expr::Range(a, b) => {
            if let Some(si) = resolve_sheet(a, ctx_sheet, workbook) {
                on_range(
                    si,
                    a.row.min(b.row),
                    a.col.min(b.col),
                    a.row.max(b.row),
                    a.col.max(b.col),
                );
            }
        }
        Expr::Name(_) => *uses_name = true,
        Expr::Unary { operand, .. } => {
            collect_precedents(operand, ctx_sheet, workbook, on_cell, on_range, uses_name)
        }
        Expr::Binary { left, right, .. } => {
            collect_precedents(left, ctx_sheet, workbook, on_cell, on_range, uses_name);
            collect_precedents(right, ctx_sheet, workbook, on_cell, on_range, uses_name);
        }
        Expr::Function { args, .. } => {
            for a in args {
                collect_precedents(a, ctx_sheet, workbook, on_cell, on_range, uses_name);
            }
        }
        Expr::Number(_) | Expr::Bool(_) | Expr::Text(_) | Expr::Error(_) => {}
    }
}

/// Resolve a reference's sheet (its explicit qualifier, else the context sheet)
/// to a workbook index, or `None` if the named sheet does not exist. Matching is
/// **case-insensitive**, identical to the evaluator's `sheet_index_by_name`, so
/// the dependency graph and evaluation always resolve a qualifier to the same
/// sheet — otherwise a differently-cased qualifier (e.g. `=sheet1!A1`) would be
/// evaluated against Sheet1 but recorded as depending on nothing, leaving the
/// dependent stale after an incremental recalc.
fn resolve_sheet(r: &CellReference, ctx_sheet: usize, workbook: &Workbook) -> Option<usize> {
    match &r.sheet {
        Some(name) => workbook
            .sheets
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name)),
        None => Some(ctx_sheet),
    }
}
