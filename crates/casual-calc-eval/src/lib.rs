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

pub use graph::{dependents_of, precedents_of};
mod value;

pub use eval::Evaluator;
pub use functions::FUNCTIONS;
pub use value::Value;

use casual_calc_model::{CellRef, CellValue, Workbook};

/// Recompute every formula cell's cached value in `workbook` (a full recalc).
///
/// Deterministic: evaluation order does not affect results (memoized recursion
/// over the reference graph), and identical input yields identical cached values.
pub fn recalculate(workbook: &mut Workbook) {
    // Phase 1: compute new values without mutating the workbook.
    let updates = {
        let mut evaluator = Evaluator::new(workbook);
        let mut updates: Vec<(usize, CellRef, Value)> = Vec::new();
        for (sheet_index, sheet) in workbook.sheets.iter().enumerate() {
            for (at, cell) in sheet.cells.iter() {
                if cell.formula.is_some() {
                    let value = evaluator.eval_cell(sheet_index, at);
                    updates.push((sheet_index, at, value));
                }
            }
        }
        updates
    };

    // Phase 2: write the new cached values back (interning any text results).
    for (sheet_index, at, value) in updates {
        let cell_value = value_to_cell(workbook, value);
        if let Some(existing) = workbook.sheets[sheet_index].cells.get(at) {
            let mut updated = existing.clone();
            updated.value = cell_value;
            workbook.sheets[sheet_index].cells.set(at, updated);
        }
    }
}

/// Recompute only the formula cells that (transitively) depend on `changed`,
/// leaving every other cached value untouched. Given a workbook whose caches
/// were correct before `changed` took their new values, this produces the same
/// result as [`recalculate`] but evaluates only the affected subgraph.
///
/// `changed` is `(sheet_index, cell)` for each cell an edit set a new value on
/// (or a formula on). Caller guarantees those new values/formulas are already
/// written into `workbook`.
pub fn recalculate_incremental(workbook: &mut Workbook, changed: &[(usize, CellRef)]) {
    let keys: Vec<(usize, u32, u32)> = changed.iter().map(|(s, c)| (*s, c.row, c.col)).collect();
    let dirty = graph::dirty_set(workbook, &keys);
    if dirty.is_empty() {
        return;
    }

    // Phase 1: evaluate the dirty formula cells (clean precedents read cache).
    let updates = {
        let mut evaluator = Evaluator::with_dirty(workbook, &dirty);
        let mut updates: Vec<(usize, CellRef, Value)> = Vec::new();
        for &(sheet_index, row, col) in &dirty {
            let at = CellRef::new(row, col);
            let is_formula = workbook
                .sheets
                .get(sheet_index)
                .and_then(|s| s.cells.get(at))
                .is_some_and(|c| c.formula.is_some());
            if is_formula {
                let value = evaluator.eval_cell(sheet_index, at);
                updates.push((sheet_index, at, value));
            }
        }
        updates
    };

    // Phase 2: write the new cached values back.
    for (sheet_index, at, value) in updates {
        let cell_value = value_to_cell(workbook, value);
        if let Some(existing) = workbook.sheets[sheet_index].cells.get(at) {
            let mut updated = existing.clone();
            updated.value = cell_value;
            workbook.sheets[sheet_index].cells.set(at, updated);
        }
    }
}

fn value_to_cell(workbook: &mut Workbook, value: Value) -> CellValue {
    match value {
        Value::Empty => CellValue::Empty,
        Value::Number(n) => CellValue::Number(n),
        Value::Bool(b) => CellValue::Bool(b),
        Value::Error(e) => CellValue::Error(e),
        Value::Text(s) => CellValue::InlineString(workbook.intern_string(&s)),
    }
}

#[cfg(test)]
mod tests;
