//! `casual-calc-eval` — the calculation engine (Phase 2).
//!
//! Increment 1: evaluates the formula ASTs the model already stores
//! (`casual-calc-formula`), resolving references by memoized recursive
//! evaluation with circular-reference detection, and recomputes each formula
//! cell's **cached value**. Supported: arithmetic/comparison/concat operators,
//! unary `+`/`-`/`%`, cell and range references (same- and cross-sheet), defined
//! names, and a function library: math/stats (`SUM`, `AVERAGE`, `COUNT`,
//! `COUNTA`, `MIN`, `MAX`, `ABS`, `INT`, `MOD`, `POWER`, `SQRT`, `ROUND`,
//! `ROUNDUP`, `ROUNDDOWN`, `CEILING`, `FLOOR`, `TRUNC`, `SIGN`, `PRODUCT`),
//! logical (`IF`, `IFERROR`, `AND`, `OR`, `NOT`), criteria aggregates
//! (`COUNTIF`, `SUMIF`, `AVERAGEIF`), lookup/reference (`VLOOKUP`, `HLOOKUP`,
//! `INDEX`, `MATCH`, `CHOOSE`), text (`CONCATENATE`/`CONCAT`, `LEN`, `LEFT`,
//! `RIGHT`, `MID`, `UPPER`, `LOWER`, `TRIM`, `SUBSTITUTE`, `REPLACE`, `FIND`,
//! `SEARCH`, `VALUE`, `PROPER`, `REPT`, `EXACT`), and deterministic dates on the
//! 1900 serial system (`DATE`, `YEAR`, `MONTH`, `DAY`, `WEEKDAY`, `EDATE`,
//! `EOMONTH`).
//!
//! Nothing depends on this crate except the host bridges — the model, layout,
//! and render layers build without it (they read cached values). A full
//! incremental dependency graph and the <50 ms recalc budget are later
//! increments; this is a correct full recalc.
//!
//! See `docs/40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md`.

mod eval;
mod functions;
mod graph;
pub mod pivot;
mod ranges;

pub use graph::{dependents_of, precedents_of};
mod value;

pub use eval::Evaluator;
pub use functions::FUNCTIONS;
pub use value::Value;

use casual_calc_model::{CellFlags, CellRef, CellValue, ErrorValue, Workbook};

/// Recompute every formula cell's cached value in `workbook` (a full recalc).
///
/// Deterministic: evaluation order does not affect results (memoized recursion
/// over the reference graph), and identical input yields identical cached values.
pub fn recalculate(workbook: &mut Workbook) {
    // A spilled cell is written *after* evaluation, so a formula that reads
    // into a spill range sees nothing on the pass that creates it. Rather than
    // teach the dependency graph about extents that are only known once the
    // anchor has been evaluated — which is circular — the pass simply runs
    // again when something spilled.
    //
    // It terminates: the second pass evaluates the same anchors from the same
    // inputs, so the extents are identical and nothing new appears. A sheet
    // where an anchor depends on its own spill is circular, and the second
    // pass's answer is taken rather than iterating towards one.
    if recalculate_once(workbook) {
        recalculate_once(workbook);
    }
    iterate_to_convergence(workbook);
}

/// Run further passes while `<calcPr iterate>` asks for them and the values are
/// still moving.
///
/// A workbook with iteration on is one whose author meant the loop — a balance
/// that depends on the interest it accrues, a rate that depends on the balance.
/// The single pass above has already produced one round of it, with each
/// self-referential cell reading its predecessor's value; this repeats that
/// until either nothing changes by more than `iterateDelta` or `iterateCount`
/// passes have been made.
///
/// **Both stopping conditions are needed.** Convergence is the one that
/// matters, and a divergent or oscillating model has none — so the count is
/// what guarantees this returns at all. Excel's defaults (100 and 0.001) apply
/// when the file enables iteration without saying how much.
///
/// Costs nothing when iteration is off, which is almost every workbook: one
/// map lookup, then return.
fn iterate_to_convergence(workbook: &mut Workbook) {
    let iteration = workbook.settings.iteration();
    if !iteration.enabled || iteration.max_count == 0 {
        return;
    }
    // The first pass already happened, so this budget is for the rest.
    for _ in 1..iteration.max_count {
        let before = formula_values(workbook);
        recalculate_once(workbook);
        let after = formula_values(workbook);
        if converged(&before, &after, iteration.max_change) {
            return;
        }
    }
}

/// Every formula cell's value, in a stable order, for comparing two passes.
fn formula_values(workbook: &Workbook) -> Vec<CellValue> {
    let mut out = Vec::new();
    for sheet in &workbook.sheets {
        for (_, cell) in sheet.cells.iter() {
            if cell.formula.is_some() {
                out.push(cell.value.clone());
            }
        }
    }
    out
}

/// Whether no value moved by more than `max_change` between two passes.
///
/// A numeric pair converges when the difference is within the tolerance. Any
/// other change — a value becoming an error, a string changing, a cell
/// appearing — counts as *not* converged, because the tolerance is a statement
/// about arithmetic and says nothing about those.
fn converged(before: &[CellValue], after: &[CellValue], max_change: f64) -> bool {
    if before.len() != after.len() {
        return false;
    }
    before.iter().zip(after).all(|(a, b)| match (a, b) {
        (CellValue::Number(x), CellValue::Number(y)) => {
            // Both non-finite compare equal; one of each does not.
            if x.is_nan() && y.is_nan() {
                return true;
            }
            (x - y).abs() <= max_change
        }
        _ => a == b,
    })
}

/// One evaluate-and-write cycle. Returns whether any array spilled.
fn recalculate_once(workbook: &mut Workbook) -> bool {
    // Phase 1: compute new values without mutating the workbook.
    let updates = {
        let mut evaluator = Evaluator::new(workbook);
        let mut updates: Vec<(usize, CellRef, Value)> = Vec::new();
        for (sheet_index, sheet) in workbook.sheets.iter().enumerate() {
            for (at, cell) in sheet.cells.iter() {
                if cell.formula.is_some() {
                    // The array form: only the spilling pass wants the whole
                    // block, and it is the thing about to run.
                    let value = evaluator.eval_cell_array(sheet_index, at);
                    updates.push((sheet_index, at, value));
                }
            }
        }
        updates
    };

    // Phase 2: write the new cached values back (interning any text results),
    // spilling any array results into their neighbours.
    clear_spill_children(workbook);
    let mut spilled = false;
    for (sheet_index, at, value) in updates {
        spilled |= write_result(workbook, sheet_index, at, value);
    }
    spilled
}

/// Remove every cell that a previous pass filled by spilling.
///
/// Done wholesale before writing, because a formula that used to produce a
/// 3×3 and now produces a 2×2 has to give back the cells it no longer covers.
/// Tracking which anchor owned which cell would be a second bookkeeping
/// structure to keep in step with the first; clearing and re-spilling cannot
/// drift.
fn clear_spill_children(workbook: &mut Workbook) {
    for sheet in workbook.sheets.iter_mut() {
        let children: Vec<CellRef> = sheet
            .cells
            .iter()
            .filter(|(_, c)| c.flags.contains(CellFlags::SPILL_CHILD))
            .map(|(at, _)| at)
            .collect();
        for at in children {
            sheet.cells.clear(at);
        }
    }
}

/// Write one formula result, spilling an array into the cells below and right.
///
/// The spill refuses rather than overwrites: if any target holds something, the
/// anchor becomes `#SPILL!` and nothing is written. Excel does the same, and
/// the alternative — silently replacing a value the user typed — is the one
/// behaviour a spreadsheet must never have.
/// Returns whether this result spilled into neighbouring cells.
fn write_result(workbook: &mut Workbook, sheet_index: usize, at: CellRef, value: Value) -> bool {
    let Value::Array { rows, cols, cells } = value else {
        let cell_value = value_to_cell(workbook, value);
        if let Some(existing) = workbook.sheets[sheet_index].cells.get(at) {
            let mut updated = existing.clone();
            updated.value = cell_value;
            // No longer an anchor if it ever was.
            updated.flags.remove(CellFlags::SPILL_ANCHOR);
            workbook.sheets[sheet_index].cells.set(at, updated);
        }
        return false;
    };
    // A 1×1 array is just a value; spilling it would flag a cell for nothing.
    if rows <= 1 && cols <= 1 {
        let first = cells.into_iter().next().unwrap_or(Value::Empty);
        return write_result(workbook, sheet_index, at, first);
    }

    let blocked = {
        let Some(sheet) = workbook.sheets.get(sheet_index) else {
            return false;
        };
        (0..rows).any(|r| {
            (0..cols).any(|c| {
                if r == 0 && c == 0 {
                    return false; // the anchor itself
                }
                let target = CellRef::new(at.row + r as u32, at.col + c as u32);
                sheet.cells.get(target).is_some_and(|cell| {
                    // A cell filled by a spill is not an obstruction — it is
                    // this formula's own output from the previous pass, about
                    // to be reclaimed. Treating it as data made a spilling
                    // formula turn itself into #SPILL! the next time anything
                    // on the sheet was edited.
                    !cell.is_blank() && !cell.flags.contains(CellFlags::SPILL_CHILD)
                })
            })
        })
    };
    if blocked {
        write_result(workbook, sheet_index, at, Value::Error(ErrorValue::Spill));
        // Ask for another pass. The obstruction may itself be a spill child of
        // a formula that has since shrunk, and the wholesale clear at the start
        // of a pass is what releases it — without this a #SPILL! could outlive
        // the thing that caused it.
        return true;
    }

    for (i, item) in cells.into_iter().enumerate() {
        let (r, c) = (i / cols, i % cols);
        let target = CellRef::new(at.row + r as u32, at.col + c as u32);
        let cell_value = value_to_cell(workbook, item);
        let sheet = &mut workbook.sheets[sheet_index];
        if r == 0 && c == 0 {
            if let Some(existing) = sheet.cells.get(target) {
                let mut updated = existing.clone();
                updated.value = cell_value;
                updated.flags.insert(CellFlags::SPILL_ANCHOR);
                sheet.cells.set(target, updated);
            }
        } else {
            let mut child = casual_calc_model::Cell::value(cell_value);
            // Flagged, not merely written: the flag is what tells the next
            // pass this cell belongs to a spill and may be reclaimed, and what
            // stops the editor letting someone type over half an array.
            child.flags.insert(CellFlags::SPILL_CHILD);
            sheet.cells.set(target, child);
        }
    }
    true
}

/// Recompute only the formula cells that (transitively) depend on `changed`,
/// leaving every other cached value untouched. Given a workbook whose caches
/// were correct before `changed` took their new values, this produces the same
/// result as [`recalculate`] but evaluates only the affected subgraph.
///
/// `changed` is `(sheet_index, cell)` for each cell an edit set a new value on
/// (or a formula on). Caller guarantees those new values/formulas are already
/// written into `workbook`.
/// Recalculation that can keep its precedent graph between edits.
///
/// Step two of [66](../../../docs/66-INCREMENTAL-RECALC-GRAPH.md), and it is
/// **all plumbing on purpose**. It rebuilds the graph on every call, exactly as
/// the free function does, so nothing about the answer or the cost changes yet.
/// What changes is ownership: there is now somewhere for a kept graph to live,
/// and a host that will hold it.
///
/// Separated from step three deliberately. Moving where state lives and changing
/// when it is invalidated are two changes, and the second is the one where a
/// mistake is silent — a stale graph does not fail, it just stops dirtying a
/// cell that should have been dirtied, and the wrong number appears somewhere
/// nobody was looking.
///
/// [`invalidate`](Self::invalidate) exists now and does nothing yet, so callers
/// can be taught the discipline before it matters: a structural edit shifts
/// every reference past its insertion point, and after step three a graph that
/// survives one is a graph that describes a document that no longer exists.
#[derive(Debug, Default)]
pub struct Recalculator {
    /// Kept from step three onward. `None` means "build one when next needed",
    /// which is what every call does today.
    graph: Option<graph::Precedents>,
}

impl Recalculator {
    /// A recalculator with nothing remembered.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Recalculate the cells that `changed` affects.
    ///
    /// The graph is built on the first call and **kept** after it. Each changed
    /// cell is repointed — a value edit re-derives the same edges, a formula
    /// edit replaces them, a cleared cell loses them — and propagation then runs
    /// against a graph nobody rebuilt.
    ///
    /// The caller's obligation is [`invalidate`](Self::invalidate), and it is
    /// the whole safety argument: this sees the cells it is told about and
    /// nothing else, so an edit that moves references without being reported
    /// leaves a graph describing a document that no longer exists. A stale graph
    /// does not produce an error, it produces a cell that stopped being
    /// recalculated.
    pub fn recalculate(&mut self, workbook: &mut Workbook, changed: &[(usize, CellRef)]) {
        let keys: Vec<graph::CellKey> = changed.iter().map(|(s, c)| (*s, c.row, c.col)).collect();
        let graph = self
            .graph
            .get_or_insert_with(|| graph::Precedents::build(workbook));
        for &key in &keys {
            graph.repoint(workbook, key);
        }
        let dirty = graph::dirty_from(graph, workbook, &keys);
        apply_dirty(workbook, &dirty);
    }

    /// Forget the graph, because the document moved under it.
    ///
    /// Called for the edits that shift references workbook-wide — inserting or
    /// deleting rows and columns — and for undo and redo, which replay them.
    pub fn invalidate(&mut self) {
        self.graph = None;
    }
}

pub fn recalculate_incremental(workbook: &mut Workbook, changed: &[(usize, CellRef)]) {
    let keys: Vec<(usize, u32, u32)> = changed.iter().map(|(s, c)| (*s, c.row, c.col)).collect();
    let dirty = graph::dirty_set(workbook, &keys);
    apply_dirty(workbook, &dirty);
}

/// Evaluate a dirty set and write the results back.
///
/// Shared by the rebuild-every-pass path and the kept-graph one, so the only
/// thing that can differ between them is *which cells are dirty* — which is the
/// one thing keeping a graph is allowed to change.
fn apply_dirty(workbook: &mut Workbook, dirty: &std::collections::HashSet<(usize, u32, u32)>) {
    if dirty.is_empty() {
        return;
    }

    // Evaluated in a fixed order, which is correctness rather than tidiness.
    //
    // `HashSet` iteration order depends on `RandomState`, which is seeded per
    // process — and evaluation order is *observable*. `Evaluator::next_random`
    // draws from a counter incremented per draw, so the order these cells are
    // visited in decides which cell receives which `RAND()` value; the same
    // order also arbitrates which of two colliding spills reaches its target
    // first. Identical input therefore produced a different workbook, and
    // different saved bytes, on every run — priority 2 in AGENTS.md, and the
    // one property a spreadsheet engine cannot negotiate.
    //
    // Sheet, then row, then column: the order `recalculate_once` already walks,
    // so the incremental and full paths cannot disagree about it either.
    let mut order: Vec<(usize, u32, u32)> = dirty.iter().copied().collect();
    order.sort_unstable();

    // Phase 1: evaluate the dirty formula cells (clean precedents read cache).
    let updates = {
        let mut evaluator = Evaluator::with_dirty(workbook, dirty);
        let mut updates: Vec<(usize, CellRef, Value)> = Vec::new();
        for (sheet_index, row, col) in order {
            let at = CellRef::new(row, col);
            let is_formula = workbook
                .sheets
                .get(sheet_index)
                .and_then(|s| s.cells.get(at))
                .is_some_and(|c| c.formula.is_some());
            if is_formula {
                // The array form here too: an edit that creates a spilling
                // formula has to spill, and the incremental path is the one an
                // edit actually takes — spilling only on a full recalculation
                // meant a freshly typed FILTER showed one value.
                let value = evaluator.eval_cell_array(sheet_index, at);
                updates.push((sheet_index, at, value));
            }
        }
        updates
    };

    // Phase 2: write the new cached values back, spilling any arrays. Shares
    // `write_result` with the full recalculation so the two cannot disagree
    // about what a spill does.
    let mut spilled = false;
    for (sheet_index, at, value) in updates {
        spilled |= write_result(workbook, sheet_index, at, value);
    }
    // A spill changes cells the dirty set never knew about, so anything reading
    // into the new range needs another look. The full pass is the honest way to
    // find them: the alternative is to guess which formulas point inside a
    // range that did not exist when the graph was built.
    if spilled {
        recalculate(workbook);
    }
}

fn value_to_cell(workbook: &mut Workbook, value: Value) -> CellValue {
    match value {
        Value::Empty => CellValue::Empty,
        Value::Number(n) => CellValue::Number(n),
        Value::Bool(b) => CellValue::Bool(b),
        Value::Error(e) => CellValue::Error(e),
        Value::Text(s) => CellValue::InlineString(workbook.intern_string(&s)),
        // A function stored in a cell has no value to show; Excel puts #CALC!
        // there, which says "this is not finished" rather than naming a type.
        Value::Lambda(_) => CellValue::Error(ErrorValue::Calc),
        // `write_result` unwraps arrays before reaching here, so a block at
        // this point is a value with nowhere to go — its corner is the cell's.
        Value::Array { cells, .. } => {
            let first = cells.into_iter().next().unwrap_or(Value::Empty);
            value_to_cell(workbook, first)
        }
    }
}

#[cfg(test)]
mod pivot_tests;
#[cfg(test)]
mod tests;
