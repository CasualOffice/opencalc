//! `casual-calc-eval` — the calculation engine (Phase 2).
//!
//! Increment 1: evaluates the formula ASTs the model already stores
//! (`casual-calc-formula`), resolving references by memoized recursive
//! evaluation with circular-reference detection, and recomputes each formula
//! cell's **cached value**. Supported: arithmetic/comparison/concat operators,
//! unary `+`/`-`/`%`, cell and range references (same- and cross-sheet), defined
//! names, and a starter function library (`SUM`, `AVERAGE`, `MIN`, `MAX`,
//! `COUNT`, `IF`, `ABS`, `ROUND`).
//!
//! Nothing depends on this crate except the host bridges — the model, layout,
//! and render layers build without it (they read cached values). A full
//! incremental dependency graph and the <50 ms recalc budget are later
//! increments; this is a correct full recalc.
//!
//! See `docs/40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md`.

mod eval;
mod functions;
mod value;

pub use eval::Evaluator;
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
