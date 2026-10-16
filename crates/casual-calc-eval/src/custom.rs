//! Calling a custom worksheet function (`MACRO-04`).
//!
//! The function itself is JavaScript and never runs here: this builds the
//! call's key, looks for an answer the host already supplied, and otherwise
//! asks for one and answers `#BUSY!`. See
//! [`CustomFunctionRuntime`](casual_calc_model::CustomFunctionRuntime) for the
//! model, and docs/99 for why the JavaScript is kept out of the engine.
//!
//! The argument rules are ONLYOFFICE's (`sdkjs` `cell/model/WorkbookElems.js`,
//! `CCustomFunctionEngine._add`): a call with too few or too many arguments is
//! `#VALUE!`, and an argument that is an error is the call's answer without
//! the function being asked. A reference argument also carries its address,
//! sheet-qualified (`Sheet1!A2`), which the function reads as
//! `this.args[i].address`; the calling cell is `this.address` in the same
//! form.

use casual_calc_formula::Expr;
use casual_calc_formula::stored::StoredRef;
use casual_calc_model::{
    CustomCallKey, CustomFunctionSpec, CustomValue, ErrorValue, MAX_CUSTOM_ARG_CELLS,
};

use crate::eval::Evaluator;
use crate::value::Value;

/// Evaluate a call to the registered function `spec`.
pub(crate) fn call(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    spec: &CustomFunctionSpec,
    args: &[Expr],
) -> Value {
    if args.len() < spec.min_args || args.len() > spec.max_args {
        return Value::Error(ErrorValue::Value);
    }
    let mut values = Vec::with_capacity(args.len());
    let mut addresses = Vec::with_capacity(args.len());
    let mut carried = 0usize;
    for arg in args {
        let (value, address) = argument(ev, sheet, arg);
        let value = match value {
            Ok(v) => v,
            Err(e) => return Value::Error(e),
        };
        carried = carried.saturating_add(value.cell_count());
        if carried > MAX_CUSTOM_ARG_CELLS {
            return Value::Error(ErrorValue::Value);
        }
        values.push(value);
        addresses.push(address);
    }
    // Outside a cell (a formula evaluated for a dialog's preview) there is no
    // cell to answer into and no `this.address` to give: busy, and not asked.
    let Some((at_sheet, at)) = ev.current_cell() else {
        return Value::Error(ErrorValue::Busy);
    };
    let workbook = ev.workbook();
    let key = CustomCallKey {
        name: spec.name.clone(),
        version: spec.version.clone(),
        cell: (at_sheet, at.row, at.col),
        address: format!(
            "{}{}{}",
            sheet_prefix(workbook, at_sheet),
            casual_calc_formula::column_to_letters(at.col),
            at.row + 1
        ),
        args: values,
        arg_addresses: addresses,
        epoch: spec.volatile.then_some(workbook.volatile_seed),
    };
    match workbook.custom.lookup(&key) {
        Some(answer) => answer_value(answer),
        None => {
            ev.want_custom(key);
            Value::Error(ErrorValue::Busy)
        }
    }
}

/// One argument's value and, for a reference, its address.
fn argument(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    arg: &Expr,
) -> (Result<CustomValue, ErrorValue>, Option<String>) {
    match arg {
        // Left out: the function receives its default, or nothing.
        Expr::Empty => (Ok(CustomValue::Empty), None),
        Expr::Reference(r) => {
            let address = reference_address(ev, sheet, r, None);
            (scalar(ev.eval_expr(sheet, arg)), address)
        }
        Expr::Range(a, b) => {
            let address = reference_address(ev, sheet, a, Some(b));
            (block(ev.eval_expr_array(sheet, arg)), address)
        }
        _ => (block(ev.eval_expr_array(sheet, arg)), None),
    }
}

fn scalar(value: Value) -> Result<CustomValue, ErrorValue> {
    match value {
        Value::Empty => Ok(CustomValue::Empty),
        Value::Number(n) => Ok(CustomValue::Number(n)),
        Value::Text(s) => Ok(CustomValue::Text(s)),
        Value::Bool(b) => Ok(CustomValue::Bool(b)),
        Value::Error(e) => Err(e),
        // A function value has nothing to give JavaScript.
        Value::Lambda(_) => Err(ErrorValue::Value),
        Value::Array { cells, .. } => scalar(cells.into_iter().next().unwrap_or(Value::Empty)),
    }
}

/// An array argument: an error *inside* the block travels as an element (the
/// function may want to see it); an error *as* the argument is the answer.
fn block(value: Value) -> Result<CustomValue, ErrorValue> {
    match value {
        Value::Array { rows, cols, cells } => Ok(CustomValue::Array {
            rows,
            cols,
            cells: cells
                .into_iter()
                .map(|c| match scalar(c) {
                    Ok(v) => v,
                    Err(e) => CustomValue::Error(e),
                })
                .collect(),
        }),
        other => scalar(other),
    }
}

/// An answer as a value: an array within the engine's array bound, or a
/// scalar. A shape the bound refuses is `#NUM!`, as `SEQUENCE`'s is; a block
/// whose cells do not fill its shape, or that nests, is `#VALUE!`.
pub(crate) fn answer_value(answer: &CustomValue) -> Value {
    match answer {
        CustomValue::Empty => Value::Empty,
        CustomValue::Number(n) if n.is_finite() => Value::Number(*n),
        CustomValue::Number(_) => Value::Error(ErrorValue::Num),
        CustomValue::Text(s) => Value::Text(s.clone()),
        CustomValue::Bool(b) => Value::Bool(*b),
        CustomValue::Error(e) => Value::Error(*e),
        CustomValue::Array { rows, cols, cells } => {
            let n = match crate::functions::checked_array_size(*rows, *cols) {
                Ok(n) => n,
                Err(e) => return Value::Error(e),
            };
            if n == 0 || cells.len() != n {
                return Value::Error(ErrorValue::Value);
            }
            let mut out = Vec::with_capacity(n);
            for c in cells {
                if matches!(c, CustomValue::Array { .. }) {
                    return Value::Error(ErrorValue::Value);
                }
                out.push(answer_value(c));
            }
            Value::Array {
                rows: *rows,
                cols: *cols,
                cells: out,
            }
        }
    }
}

/// `Sheet1!A2`, or `Sheet1!A2:B3` — ONLYOFFICE's form for
/// `this.args[i].address`: always sheet-qualified, the name quoted when it
/// needs to be.
fn reference_address(
    ev: &Evaluator<'_>,
    sheet: usize,
    a: &StoredRef,
    b: Option<&StoredRef>,
) -> Option<String> {
    let target = ev.resolve_sheet(&a.sheet, sheet)?;
    let prefix = sheet_prefix(ev.workbook(), target);
    let cell =
        |row: u32, col: u32| format!("{}{}", casual_calc_formula::column_to_letters(col), row + 1);
    match b {
        None => {
            let at = a.resolve(ev.origin())?;
            Some(format!("{prefix}{}", cell(at.row, at.col)))
        }
        Some(b) => {
            let (r0, c0, r1, c1) = ev.range_bounds(target, a, b);
            Some(format!("{prefix}{}:{}", cell(r0, c0), cell(r1, c1)))
        }
    }
}

/// `Name!`, or `'My Sheet'!` when the name is not a plain word.
fn sheet_prefix(workbook: &casual_calc_model::Workbook, sheet: usize) -> String {
    let name = workbook.sheets.get(sheet).map_or("", |s| s.name.as_str());
    let plain = name
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.');
    if plain {
        format!("{name}!")
    } else {
        format!("'{}'!", name.replace('\'', "''"))
    }
}
