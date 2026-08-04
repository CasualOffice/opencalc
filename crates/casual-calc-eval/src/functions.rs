//! The built-in function library (starter subset). Aggregates flatten ranges to
//! numbers; `IF` evaluates only the taken branch.

use casual_calc_formula::Expr;
use casual_calc_model::{CellRef, ErrorValue};

use crate::eval::Evaluator;
use crate::value::Value;

/// Guard against pathological full-range aggregates (a dependency-graph with
/// range buckets is the Phase-2 optimization; this bounds the naive scan).
const MAX_RANGE_CELLS: u64 = 2_000_000;

/// Dispatch a function call by (upper-cased) name.
pub fn call_function(ev: &mut Evaluator<'_>, sheet: usize, name: &str, args: &[Expr]) -> Value {
    match name {
        "SUM" => match flatten_numbers(ev, sheet, args) {
            Ok(ns) => Value::Number(ns.iter().sum()),
            Err(e) => Value::Error(e),
        },
        "AVERAGE" => match flatten_numbers(ev, sheet, args) {
            Ok(ns) if ns.is_empty() => Value::Error(ErrorValue::Div0),
            Ok(ns) => Value::Number(ns.iter().sum::<f64>() / ns.len() as f64),
            Err(e) => Value::Error(e),
        },
        "COUNT" => match flatten_numbers(ev, sheet, args) {
            Ok(ns) => Value::Number(ns.len() as f64),
            Err(e) => Value::Error(e),
        },
        "MIN" => reduce(ev, sheet, args, f64::min),
        "MAX" => reduce(ev, sheet, args, f64::max),
        "IF" => eval_if(ev, sheet, args),
        "ABS" => scalar(ev, sheet, args, f64::abs),
        "ROUND" => eval_round(ev, sheet, args),
        _ => Value::Error(ErrorValue::Name),
    }
}

fn reduce(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], f: fn(f64, f64) -> f64) -> Value {
    match flatten_numbers(ev, sheet, args) {
        Ok(ns) if ns.is_empty() => Value::Number(0.0),
        Ok(ns) => Value::Number(ns.into_iter().reduce(f).unwrap_or(0.0)),
        Err(e) => Value::Error(e),
    }
}

fn scalar(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], f: fn(f64) -> f64) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => Value::Number(f(n)),
        Err(e) => Value::Error(e),
    }
}

fn eval_if(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    match ev.eval_expr(sheet, &args[0]).as_bool() {
        Ok(true) => ev.eval_expr(sheet, &args[1]),
        Ok(false) => {
            if args.len() == 3 {
                ev.eval_expr(sheet, &args[2])
            } else {
                Value::Bool(false)
            }
        }
        Err(e) => Value::Error(e),
    }
}

fn eval_round(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let value = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let digits = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n as i32,
        Err(e) => return Value::Error(e),
    };
    let factor = 10f64.powi(digits);
    Value::Number((value * factor).round() / factor)
}

fn flatten_numbers(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
) -> Result<Vec<f64>, ErrorValue> {
    let mut out = Vec::new();
    for arg in args {
        if let Expr::Range(a, b) = arg {
            let target = ev.resolve_sheet(&a.sheet, sheet).ok_or(ErrorValue::Ref)?;
            let (r0, r1) = (a.row.min(b.row), a.row.max(b.row));
            let (c0, c1) = (a.col.min(b.col), a.col.max(b.col));
            let area = (r1 - r0 + 1) as u64 * (c1 - c0 + 1) as u64;
            if area > MAX_RANGE_CELLS {
                return Err(ErrorValue::Num);
            }
            for row in r0..=r1 {
                for col in c0..=c1 {
                    match ev.eval_cell(target, CellRef::new(row, col)) {
                        Value::Number(n) => out.push(n),
                        Value::Bool(b) => out.push(if b { 1.0 } else { 0.0 }),
                        Value::Error(e) => return Err(e),
                        _ => {}
                    }
                }
            }
        } else {
            match ev.eval_expr(sheet, arg) {
                Value::Number(n) => out.push(n),
                Value::Bool(b) => out.push(if b { 1.0 } else { 0.0 }),
                Value::Empty => {}
                Value::Text(t) => out.push(t.trim().parse::<f64>().map_err(|_| ErrorValue::Value)?),
                Value::Error(e) => return Err(e),
            }
        }
    }
    Ok(out)
}
