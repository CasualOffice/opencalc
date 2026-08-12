//! The recalc dependency graph: which formula cells must be recomputed when a
//! set of cells changes.
//!
//! This is what makes [`crate::recalculate_incremental`] touch only a changed
//! cell's transitive dependents instead of the whole workbook. The graph is
//! The graph is **kept across edits** by [`crate::Recalculator`] — step three of
//! [66](../../../docs/66-INCREMENTAL-RECALC-GRAPH.md). A value edit leaves it
//! alone, a formula edit repoints one node, and the reference-shifting edits
//! drop it. Callers without a `Recalculator` still get a per-pass rebuild, which
//! is slower and cannot go stale.
//!
//! Correctness is pinned from both ends: a differential test asserting an
//! incremental pass equals a full recalculation, and — because a kept graph
//! fails by *omission*, producing no error and one cell that quietly stopped
//! being recalculated — a property test asserting a patched graph equals one
//! rebuilt from scratch.

use std::collections::{HashMap, HashSet};

use casual_calc_formula::{CellReference, Expr};
use casual_calc_model::{CellRef, Workbook};

/// `(sheet_index, row, col)`.
pub(crate) type CellKey = (usize, u32, u32);

/// A rectangular precedent range on one sheet (inclusive), plus the formula
/// cell that reads it.
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone)]
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
#[derive(Debug)]
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
    /// What each formula cell registered, so that one node can be removed
    /// without scanning every edge.
    ///
    /// The graph above runs precedent-to-dependents, which is the direction
    /// propagation needs and the wrong one for patching: finding a dependent's
    /// own edges means scanning all of them, which is the O(formulas) walk this
    /// whole exercise exists to remove. Patching in the fast direction requires
    /// recording the slow one.
    outgoing: HashMap<CellKey, Node>,
}

/// One formula cell's edges, by where they were put rather than by what they
/// mean — this exists to be undone.
#[derive(Debug, Default)]
struct Node {
    /// Keys in `direct` under which this cell appears.
    reads: Vec<CellKey>,
    /// Slots in `ranges` belonging to this cell.
    spans: Vec<usize>,
    uses_name: bool,
}

impl Precedents {
    /// Walk every formula in the workbook and record what it reads.
    pub(crate) fn build(workbook: &Workbook) -> Self {
        let mut this = Self {
            direct: HashMap::new(),
            ranges: Vec::new(),
            name_users: Vec::new(),
            outgoing: HashMap::new(),
        };
        for (sheet_index, sheet) in workbook.sheets.iter().enumerate() {
            for (at, _) in sheet.cells.iter() {
                this.attach(workbook, (sheet_index, at.row, at.col));
            }
        }
        this
    }

    /// Bring one cell's edges up to date with what the workbook now says.
    ///
    /// The whole of step three, in one method: a value edit finds the same
    /// formula and re-derives the same edges, a formula edit replaces them, and
    /// clearing a cell removes them. The caller does not have to know which of
    /// those it did, which matters because the operation log does not say.
    pub(crate) fn repoint(&mut self, workbook: &Workbook, cell: CellKey) {
        self.detach(cell);
        self.attach(workbook, cell);
    }

    /// Record what one cell reads. Not an edit — the cell is assumed absent
    /// from the graph, which is what `build` and `repoint` both guarantee.
    fn attach(&mut self, workbook: &Workbook, cell: CellKey) {
        let (sheet_index, row, col) = cell;
        let Some(expr) = workbook
            .sheets
            .get(sheet_index)
            .and_then(|s| s.cells.get(CellRef::new(row, col)))
            .and_then(|c| c.formula)
            .and_then(|handle| workbook.formula(handle))
        else {
            return;
        };

        // Destructured so the two callbacks below borrow disjoint fields; the
        // walker takes both at once.
        let (direct, ranges) = (&mut self.direct, &mut self.ranges);
        let (mut reads, mut spans) = (Vec::new(), Vec::new());
        let mut uses_name = false;
        collect_precedents(
            expr,
            sheet_index,
            workbook,
            &mut |p| {
                direct.entry(p).or_default().push(cell);
                reads.push(p);
            },
            &mut |sheet, r0, c0, r1, c1| {
                spans.push(ranges.len());
                ranges.push(RangeEdge {
                    sheet,
                    r0,
                    c0,
                    r1,
                    c1,
                    dependent: cell,
                });
            },
            &mut uses_name,
        );
        if uses_name {
            self.name_users.push(cell);
        }
        if !reads.is_empty() || !spans.is_empty() || uses_name {
            self.outgoing.insert(
                cell,
                Node {
                    reads,
                    spans,
                    uses_name,
                },
            );
        }
    }

    /// Remove every edge one cell registered, and nothing else.
    fn detach(&mut self, cell: CellKey) {
        let Some(node) = self.outgoing.remove(&cell) else {
            return;
        };

        for p in node.reads {
            if let Some(dependents) = self.direct.get_mut(&p) {
                if let Some(i) = dependents.iter().position(|&d| d == cell) {
                    dependents.swap_remove(i);
                }
                // An empty vector left behind is a key a rebuilt graph would not
                // have, which the equality property would report as a
                // difference — correctly, since it is one.
                if dependents.is_empty() {
                    self.direct.remove(&p);
                }
            }
        }

        // Highest slot first. `swap_remove` moves the last edge into the hole,
        // so whoever owned it needs its recorded slot corrected — and going
        // downwards guarantees the edge that moves always comes from above every
        // slot still to be visited, so it can never belong to `cell`, whose
        // bookkeeping has already been taken out of `outgoing` and could not be
        // corrected.
        let mut spans = node.spans;
        spans.sort_unstable_by(|a, b| b.cmp(a));
        for slot in spans {
            let vacated = self.ranges.len() - 1;
            self.ranges.swap_remove(slot);
            if vacated != slot {
                let owner = self.ranges[slot].dependent;
                debug_assert_ne!(owner, cell, "a detached cell cannot own a moved edge");
                if let Some(s) = self
                    .outgoing
                    .get_mut(&owner)
                    .and_then(|n| n.spans.iter_mut().find(|s| **s == vacated))
                {
                    *s = slot;
                }
            }
        }

        if node.uses_name
            && let Some(i) = self.name_users.iter().position(|&d| d == cell)
        {
            self.name_users.swap_remove(i);
        }
    }
}

pub(crate) fn dirty_set(workbook: &Workbook, changed: &[CellKey]) -> HashSet<CellKey> {
    dirty_from(&Precedents::build(workbook), workbook, changed)
}

/// Propagate through a graph somebody else is keeping.
pub(crate) fn dirty_from(
    graph: &Precedents,
    workbook: &Workbook,
    changed: &[CellKey],
) -> HashSet<CellKey> {
    let Precedents {
        direct,
        ranges,
        name_users,
        ..
    } = graph;

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
    for &n in name_users {
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
        for e in ranges {
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
    use std::collections::BTreeMap;

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
    #[test]
    fn building_twice_gives_the_same_graph() {
        let wb = workbook();
        assert_eq!(
            canonical(&Precedents::build(&wb)),
            canonical(&Precedents::build(&wb))
        );
    }

    /// Edges, then bookkeeping: `direct`, `ranges`, `name_users`, and each
    /// cell's own record of what it registered.
    type Canonical = (
        BTreeMap<CellKey, Vec<CellKey>>,
        Vec<RangeEdge>,
        Vec<CellKey>,
        BTreeMap<CellKey, (Vec<CellKey>, Vec<RangeEdge>, bool)>,
    );

    /// Every edge and every piece of bookkeeping, in an order-independent form.
    ///
    /// Compared as sets because a patched graph and a built one differ in the
    /// order edges happen to sit in, and that difference means nothing. An
    /// order-sensitive comparison here would fail for a reason unrelated to
    /// correctness, which is worse than not comparing at all — it teaches you to
    /// ignore the one test standing between a stale graph and a wrong number.
    ///
    /// The reverse index is included deliberately. A leaked range slot or a
    /// `direct` key left behind changes no answer *today*; it makes the *next*
    /// patch remove the wrong edge.
    fn canonical(g: &Precedents) -> Canonical {
        let direct: BTreeMap<_, _> = g
            .direct
            .iter()
            .map(|(k, v)| {
                let mut v = v.clone();
                v.sort_unstable();
                (*k, v)
            })
            .collect();
        let mut ranges = g.ranges.clone();
        ranges.sort_unstable();
        let mut names = g.name_users.clone();
        names.sort_unstable();
        // Slots are positions in a vector that swaps on removal, so they are not
        // comparable between two graphs and the *edges they point at* are.
        let outgoing = g
            .outgoing
            .iter()
            .map(|(k, n)| {
                let mut reads = n.reads.clone();
                reads.sort_unstable();
                let mut spans: Vec<RangeEdge> =
                    n.spans.iter().map(|&i| g.ranges[i].clone()).collect();
                spans.sort_unstable();
                (*k, (reads, spans, n.uses_name))
            })
            .collect();
        (direct, ranges, names, outgoing)
    }

    fn put(wb: &mut Workbook, at: CellRef, formula: &str) {
        let handle = wb.store_formula(casual_calc_formula::parse(formula).unwrap());
        let mut cell = Cell::value(CellValue::Number(0.0));
        cell.formula = Some(handle);
        wb.sheets[0].cells.set(at, cell);
    }

    /// **The property step three rests on**: after any sequence of edits, a
    /// patched graph equals one rebuilt from scratch.
    ///
    /// The edits below are chosen to be the ones that move bookkeeping rather
    /// than the ones that look interesting: a formula replaced by another
    /// formula, a formula replaced by a plain value, a value becoming a formula,
    /// a cell cleared outright, and a range formula removed from the middle of
    /// `ranges` so the swap-on-removal has to correct somebody else's slot.
    #[test]
    fn a_patched_graph_equals_a_rebuilt_one() {
        let mut wb = workbook();
        put(&mut wb, CellRef::new(2, 1), "SUM(A1:A2)");
        put(&mut wb, CellRef::new(3, 1), "SUM(A2:A4)");
        let mut graph = Precedents::build(&wb);

        let edits: Vec<(CellRef, Option<&str>)> = vec![
            (CellRef::new(0, 1), Some("A2*3")),       // repoint a direct edge
            (CellRef::new(1, 1), Some("A1+A4")),      // a range becomes direct
            (CellRef::new(2, 1), None),               // a range formula, cleared
            (CellRef::new(0, 2), Some("SUM(A1:A4)")), // a new range, on a new cell
            (CellRef::new(3, 1), Some("7")),          // formula becomes a value
            (CellRef::new(0, 1), None),               // and cleared entirely
        ];

        for (at, formula) in edits {
            match formula {
                Some(f) => put(&mut wb, at, f),
                None => {
                    wb.sheets[0]
                        .cells
                        .set(at, Cell::value(CellValue::Number(1.0)));
                }
            }
            graph.repoint(&wb, (0, at.row, at.col));
            assert_eq!(
                canonical(&graph),
                canonical(&Precedents::build(&wb)),
                "graph diverged from a rebuild after editing {at:?}"
            );
        }
    }

    /// Removing a range edge must not corrupt the slot of the one that moves
    /// into its place.
    ///
    /// Called out separately because the property test above would catch it and
    /// would not say why: `swap_remove` is the one piece of this that is wrong
    /// by default, and a failure here names it.
    #[test]
    fn removing_a_range_edge_repairs_the_edge_that_moved() {
        let mut wb = workbook();
        put(&mut wb, CellRef::new(2, 1), "SUM(A1:A3)");
        let mut graph = Precedents::build(&wb);
        assert_eq!(graph.ranges.len(), 2);

        // Remove the *first* range edge, forcing the second to be swapped down.
        wb.sheets[0]
            .cells
            .set(CellRef::new(1, 1), Cell::value(CellValue::Number(0.0)));
        graph.repoint(&wb, (0, 1, 1));

        let survivor = (0usize, 2u32, 1u32);
        assert_eq!(graph.ranges.len(), 1);
        assert_eq!(graph.ranges[0].dependent, survivor);
        assert_eq!(
            graph.outgoing[&survivor].spans,
            vec![0],
            "the surviving edge's recorded slot follows it"
        );

        // And the repaired slot is usable: removing the survivor must empty the
        // vector rather than panic or take the wrong edge.
        wb.sheets[0]
            .cells
            .set(CellRef::new(2, 1), Cell::value(CellValue::Number(0.0)));
        graph.repoint(&wb, (0, 2, 1));
        assert!(graph.ranges.is_empty());
        assert_eq!(canonical(&graph), canonical(&Precedents::build(&wb)));
    }

    /// A cell with no formula leaves nothing behind.
    #[test]
    fn a_value_cell_holds_no_bookkeeping() {
        let mut wb = workbook();
        let graph = Precedents::build(&wb);
        assert!(!graph.outgoing.contains_key(&(0, 0, 0)), "A1 reads nothing");

        // And repointing a value cell repeatedly is a no-op, which is the common
        // case: every ordinary typed number takes this path.
        let mut graph = graph;
        let before = canonical(&graph);
        for _ in 0..3 {
            graph.repoint(&wb, (0, 0, 0));
        }
        assert_eq!(canonical(&graph), before);
        let _ = &mut wb;
    }
}
