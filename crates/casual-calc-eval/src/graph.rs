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
#[derive(PartialEq, Eq, Debug)]
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
/// Which formulas read what, for one workbook.
///
/// Step one of [66](../../../docs/66-INCREMENTAL-RECALC-GRAPH.md): the same
/// three collections `dirty_set` has always built per pass, given a name and a
/// constructor so that a later step can **keep** one instead of rebuilding it on
/// every edit. Nothing yet keeps one — this is a refactor, and the measurement
/// it exists to fix is unchanged until step three.
///
/// Extracted rather than rewritten on purpose. The propagation below is the part
/// where being wrong is silent, so the change that introduces the type must not
/// also change how the type is filled.
pub(crate) struct Precedents {
    /// Precedent cell to the formula cells that read it directly.
    direct: HashMap<CellKey, Vec<CellKey>>,
    /// Range precedents, scanned linearly: a changed cell may fall inside any.
    ///
    /// The linear scan is what step four replaces with row-band buckets. It is
    /// correct and it is why a workbook of range formulas costs more per edit
    /// than one of cell references.
    ranges: Vec<RangeEdge>,
    /// Formulas that reference a defined name, recomputed on any change.
    ///
    /// Conservative, and staying that way: a name's target can be an expression,
    /// so resolving it precisely is a second dependency problem for a small
    /// population.
    name_users: Vec<CellKey>,
}

impl Precedents {
    /// Walk every formula in the workbook and record what it reads.
    pub(crate) fn build(workbook: &Workbook) -> Self {
        let mut this = Self {
            direct: HashMap::new(),
            ranges: Vec::new(),
            name_users: Vec::new(),
        };
        this.fill_from(workbook);
        this
    }

    fn fill_from(&mut self, workbook: &Workbook) {
        let (direct, ranges, name_users) =
            (&mut self.direct, &mut self.ranges, &mut self.name_users);
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
    }
}

pub(crate) fn dirty_set(workbook: &Workbook, changed: &[CellKey]) -> HashSet<CellKey> {
    let Precedents {
        direct,
        ranges,
        name_users,
    } = Precedents::build(workbook);

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
        // Unreadable text may reference anything, so it is treated as a name:
        // recalculate on any change rather than track a dependency that cannot
        // be derived. Conservative, never stale.
        Expr::Raw(_) => *uses_name = true,
        // Nothing there, so nothing to depend on.
        Expr::Empty => {}
        Expr::Call { callee, args } => {
            collect_precedents(callee, ctx_sheet, workbook, on_cell, on_range, uses_name);
            for a in args {
                collect_precedents(a, ctx_sheet, workbook, on_cell, on_range, uses_name);
            }
        }
        Expr::Reference(r) => {
            if let Some(si) = resolve_sheet(r, ctx_sheet, workbook) {
                on_cell((si, r.row, r.col));
            }
        }
        Expr::Range(a, b) => {
            // An open range (`A:A`) covers whatever the sheet grows into, so a
            // dependency span computed from today's extent goes stale the
            // moment a cell appears below it. Treated like a defined name
            // instead: recalculate on any change. Conservative, never wrong.
            if crate::ranges::is_open(a, b) {
                *uses_name = true;
            } else if let Some(si) = resolve_sheet(a, ctx_sheet, workbook) {
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
        Expr::Function { name, args } => {
            // A function whose target is computed from a string cannot have its
            // precedents read off the expression: `INDIRECT("A"&B1)` depends on
            // whatever that string names, which is only known once it is
            // evaluated. Walking the arguments finds B1 but not the cell the
            // formula actually reads, so the result would go stale when that
            // cell changed. Flagged like a defined name instead — recalculate
            // on any change, which is conservative and never wrong.
            // A volatile function depends on nothing in the sheet and changes
            // anyway, so a dependency-driven recalculation would never reach
            // it — `=TODAY()` would keep yesterday's date until something
            // unrelated happened to touch it. Same flag, opposite reason.
            if matches!(name.as_str(), "TODAY" | "NOW" | "RAND" | "RANDBETWEEN") {
                *uses_name = true;
            }
            if matches!(name.as_str(), "INDIRECT" | "OFFSET") {
                *uses_name = true;
            }
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

#[cfg(test)]
mod precedents_tests {
    use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

    use super::*;

    /// A sheet with one of each edge the graph distinguishes.
    fn workbook() -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        for row in 0..4u32 {
            sheet.cells.set(
                CellRef::new(row, 0),
                Cell::value(CellValue::Number(f64::from(row))),
            );
        }
        wb.sheets.push(sheet);

        let put = |wb: &mut Workbook, at: CellRef, formula: &str| {
            let handle = wb.store_formula(casual_calc_formula::parse(formula).unwrap());
            let mut cell = Cell::value(CellValue::Number(0.0));
            cell.formula = Some(handle);
            wb.sheets[0].cells.set(at, cell);
        };
        put(&mut wb, CellRef::new(0, 1), "A1*2"); // a direct edge
        put(&mut wb, CellRef::new(1, 1), "SUM(A1:A4)"); // a range edge
        wb
    }

    /// The three collections, asserted as themselves.
    ///
    /// `dirty_set` has always exercised these indirectly, which is enough while
    /// they are rebuilt from scratch every time and not enough once step three
    /// starts *mutating* them: a patch that puts an edge in the wrong collection
    /// still produces the right answer for the edit that made it, and the wrong
    /// one later. This is the baseline that catches that.
    #[test]
    fn the_graph_separates_direct_edges_from_ranges() {
        let wb = workbook();
        let graph = Precedents::build(&wb);

        // A1 is read directly by B1, and is inside the range B2 reads — the
        // direct edge must not silently also be a range edge, or removing one
        // hides the loss of the other.
        let a1 = (0usize, 0u32, 0u32);
        assert_eq!(
            graph.direct.get(&a1).map(Vec::as_slice),
            Some([(0usize, 0u32, 1u32)].as_slice()),
            "A1 is read directly by exactly B1"
        );
        assert_eq!(graph.ranges.len(), 1, "and by exactly one range");
        assert_eq!(graph.ranges[0].dependent, (0, 1, 1), "which B2 reads");
        assert!(
            graph.name_users.is_empty(),
            "nothing here uses a defined name"
        );
    }

    /// Rebuilt from the same document, the graph is the same graph.
    ///
    /// Trivial today — nothing keeps one yet — and the property step three has
    /// to preserve, so it is written now while it is obviously true rather than
    /// after it stops being.
    #[test]
    fn building_twice_gives_the_same_graph() {
        let wb = workbook();
        let (a, b) = (Precedents::build(&wb), Precedents::build(&wb));
        assert_eq!(a.direct, b.direct);
        assert_eq!(a.name_users, b.name_users);
        assert_eq!(a.ranges.len(), b.ranges.len());
    }
}
