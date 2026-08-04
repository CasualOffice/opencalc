//! The built-in function library (starter subset). Aggregates flatten ranges to
//! numbers; `IF` evaluates only the taken branch.

use std::cmp::Ordering;

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
        "COUNTA" => eval_counta(ev, sheet, args),
        "MIN" => reduce(ev, sheet, args, f64::min),
        "MAX" => reduce(ev, sheet, args, f64::max),
        "IF" => eval_if(ev, sheet, args),
        "IFERROR" => eval_iferror(ev, sheet, args),
        "AND" => eval_and_or(ev, sheet, args, true),
        "OR" => eval_and_or(ev, sheet, args, false),
        "NOT" => eval_not(ev, sheet, args),
        "COUNTIF" => eval_countif(ev, sheet, args),
        "SUMIF" => eval_sumif(ev, sheet, args),
        "AVERAGEIF" => eval_averageif(ev, sheet, args),
        "ABS" => scalar(ev, sheet, args, f64::abs),
        "INT" => scalar(ev, sheet, args, f64::floor),
        "SQRT" => eval_sqrt(ev, sheet, args),
        "MOD" => eval_mod(ev, sheet, args),
        "POWER" => eval_power(ev, sheet, args),
        "ROUND" => eval_round(ev, sheet, args),
        "CONCATENATE" | "CONCAT" => eval_concat(ev, sheet, args),
        "LEN" => eval_len(ev, sheet, args),
        "LEFT" => eval_left(ev, sheet, args),
        "RIGHT" => eval_right(ev, sheet, args),
        "MID" => eval_mid(ev, sheet, args),
        "UPPER" => text_op(ev, sheet, args, |s| s.to_uppercase()),
        "LOWER" => text_op(ev, sheet, args, |s| s.to_lowercase()),
        "TRIM" => text_op(ev, sheet, args, trim_excel),
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

fn eval_iferror(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let value = ev.eval_expr(sheet, &args[0]);
    if value.as_error().is_some() {
        ev.eval_expr(sheet, &args[1])
    } else {
        value
    }
}

/// `AND`/`OR`. `require_all` true means `AND` (every truthy), false means `OR`.
fn eval_and_or(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], require_all: bool) -> Value {
    let mut any = false;
    let mut acc = require_all;
    for arg in args {
        for value in flatten_values(ev, sheet, arg) {
            // Ignore blanks (matches Excel's treatment of empty cells in ranges).
            if matches!(value, Value::Empty) {
                continue;
            }
            let b = match value.as_bool() {
                Ok(b) => b,
                Err(e) => return Value::Error(e),
            };
            any = true;
            if require_all {
                acc = acc && b;
            } else {
                acc = acc || b;
            }
        }
    }
    if !any {
        return Value::Error(ErrorValue::Value);
    }
    Value::Bool(acc)
}

fn eval_not(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 1 {
        return Value::Error(ErrorValue::Value);
    }
    match ev.eval_expr(sheet, &args[0]).as_bool() {
        Ok(b) => Value::Bool(!b),
        Err(e) => Value::Error(e),
    }
}

fn eval_counta(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let mut count = 0u64;
    for arg in args {
        for value in flatten_values(ev, sheet, arg) {
            if !matches!(value, Value::Empty) {
                count += 1;
            }
        }
    }
    Value::Number(count as f64)
}

fn eval_sqrt(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) if n < 0.0 => Value::Error(ErrorValue::Num),
        Ok(n) => Value::Number(n.sqrt()),
        Err(e) => Value::Error(e),
    }
}

fn eval_mod(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some((a, b)) = two_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let (a, b) = match (a, b) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return Value::Error(e),
    };
    if b == 0.0 {
        return Value::Error(ErrorValue::Div0);
    }
    // Excel MOD has the sign of the divisor: a - b*floor(a/b).
    Value::Number(a - b * (a / b).floor())
}

fn eval_power(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some((a, b)) = two_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let (a, b) = match (a, b) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return Value::Error(e),
    };
    let result = a.powf(b);
    if result.is_nan() {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(result)
}

/// Evaluate exactly two numeric args, or `None` if the arity is wrong.
#[allow(clippy::type_complexity)]
fn two_numbers(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
) -> Option<(Result<f64, ErrorValue>, Result<f64, ErrorValue>)> {
    if args.len() != 2 {
        return None;
    }
    let a = ev.eval_expr(sheet, &args[0]).as_number();
    let b = ev.eval_expr(sheet, &args[1]).as_number();
    Some((a, b))
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

fn eval_concat(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let mut out = String::new();
    for arg in args {
        for value in flatten_values(ev, sheet, arg) {
            match value.as_text() {
                Ok(s) => out.push_str(&s),
                Err(e) => return Value::Error(e),
            }
        }
    }
    Value::Text(out)
}

fn eval_len(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_text() {
        Ok(s) => Value::Number(s.chars().count() as f64),
        Err(e) => Value::Error(e),
    }
}

/// Shared helper for `LEFT`/`RIGHT`: read `(text, count)` with `count` default 1.
fn text_and_count(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
) -> Result<(String, i64), ErrorValue> {
    if args.is_empty() || args.len() > 2 {
        return Err(ErrorValue::Value);
    }
    let text = ev.eval_expr(sheet, &args[0]).as_text()?;
    let count = match args.get(1) {
        Some(a) => ev.eval_expr(sheet, a).as_number()? as i64,
        None => 1,
    };
    if count < 0 {
        return Err(ErrorValue::Value);
    }
    Ok((text, count))
}

fn eval_left(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match text_and_count(ev, sheet, args) {
        Ok((text, count)) => Value::Text(text.chars().take(count as usize).collect()),
        Err(e) => Value::Error(e),
    }
}

fn eval_right(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match text_and_count(ev, sheet, args) {
        Ok((text, count)) => {
            let total = text.chars().count();
            let skip = total.saturating_sub(count as usize);
            Value::Text(text.chars().skip(skip).collect())
        }
        Err(e) => Value::Error(e),
    }
}

fn eval_mid(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let text = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let start = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let len = match ev.eval_expr(sheet, &args[2]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    // Excel: start is 1-based and must be >= 1; length must be >= 0.
    if start < 1 || len < 0 {
        return Value::Error(ErrorValue::Value);
    }
    let skip = (start - 1) as usize;
    Value::Text(text.chars().skip(skip).take(len as usize).collect())
}

fn text_op(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], f: fn(&str) -> String) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_text() {
        Ok(s) => Value::Text(f(&s)),
        Err(e) => Value::Error(e),
    }
}

/// `TRIM`: strip leading/trailing spaces and collapse internal runs to one.
fn trim_excel(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// --- Criteria-based aggregates (COUNTIF / SUMIF / AVERAGEIF) --------------

fn eval_countif(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let (op, operand) = parse_criteria(&ev.eval_expr(sheet, &args[1]));
    let range = flatten_values(ev, sheet, &args[0]);
    let count = range
        .iter()
        .filter(|v| !matches!(v, Value::Empty) && criterion_matches(v, op, &operand))
        .count();
    Value::Number(count as f64)
}

fn eval_sumif(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match conditional_values(ev, sheet, args) {
        Ok(picked) => Value::Number(picked.iter().sum()),
        Err(e) => Value::Error(e),
    }
}

fn eval_averageif(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match conditional_values(ev, sheet, args) {
        Ok(picked) if picked.is_empty() => Value::Error(ErrorValue::Div0),
        Ok(picked) => Value::Number(picked.iter().sum::<f64>() / picked.len() as f64),
        Err(e) => Value::Error(e),
    }
}

/// Shared `SUMIF`/`AVERAGEIF` core: for each cell in the criteria range that
/// matches, collect the corresponding numeric value from the sum range (or the
/// criteria range itself when no third argument is given).
fn conditional_values(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
) -> Result<Vec<f64>, ErrorValue> {
    if args.len() != 2 && args.len() != 3 {
        return Err(ErrorValue::Value);
    }
    let (op, operand) = parse_criteria(&ev.eval_expr(sheet, &args[1]));
    let range = flatten_values(ev, sheet, &args[0]);
    let sum_range = match args.get(2) {
        Some(a) => flatten_values(ev, sheet, a),
        None => range.clone(),
    };
    let mut out = Vec::new();
    for (i, cell) in range.iter().enumerate() {
        if matches!(cell, Value::Empty) || !criterion_matches(cell, op, &operand) {
            continue;
        }
        let Some(target) = sum_range.get(i) else {
            continue;
        };
        match target {
            Value::Number(n) => out.push(*n),
            Value::Bool(b) => out.push(if *b { 1.0 } else { 0.0 }),
            Value::Error(e) => return Err(*e),
            _ => {}
        }
    }
    Ok(out)
}

/// A comparison operator parsed from a criteria string.
#[derive(Clone, Copy)]
enum CritOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

/// Split a criteria value into a comparison operator and an operand string.
/// A bare value (no leading operator) means equality.
fn parse_criteria(v: &Value) -> (CritOp, String) {
    let s = v.as_text().unwrap_or_default();
    let (op, rest) = if let Some(r) = s.strip_prefix(">=") {
        (CritOp::Ge, r)
    } else if let Some(r) = s.strip_prefix("<=") {
        (CritOp::Le, r)
    } else if let Some(r) = s.strip_prefix("<>") {
        (CritOp::Ne, r)
    } else if let Some(r) = s.strip_prefix('>') {
        (CritOp::Gt, r)
    } else if let Some(r) = s.strip_prefix('<') {
        (CritOp::Lt, r)
    } else if let Some(r) = s.strip_prefix('=') {
        (CritOp::Eq, r)
    } else {
        (CritOp::Eq, s.as_str())
    };
    (op, rest.to_owned())
}

/// Does `cell` satisfy `op operand`? Numeric when both sides are numeric,
/// otherwise a case-insensitive text comparison (Excel semantics).
fn criterion_matches(cell: &Value, op: CritOp, operand: &str) -> bool {
    let operand_num = operand.trim().parse::<f64>().ok();
    let cell_num = match cell {
        Value::Number(n) => Some(*n),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::Text(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    let ordering = match (cell_num, operand_num) {
        (Some(a), Some(b)) => a.partial_cmp(&b),
        _ => {
            let a = cell.as_text().unwrap_or_default().to_uppercase();
            let b = operand.to_uppercase();
            Some(a.cmp(&b))
        }
    };
    let Some(ordering) = ordering else {
        return false;
    };
    match op {
        CritOp::Eq => ordering == Ordering::Equal,
        CritOp::Ne => ordering != Ordering::Equal,
        CritOp::Gt => ordering == Ordering::Greater,
        CritOp::Ge => ordering != Ordering::Less,
        CritOp::Lt => ordering == Ordering::Less,
        CritOp::Le => ordering != Ordering::Greater,
    }
}

// --- Range flattening -----------------------------------------------------

/// Flatten one argument into a flat list of values, expanding a range to every
/// cell it covers (in row-major order). A scalar argument yields one value; an
/// error encountered while evaluating a cell becomes a single `Error` value.
fn flatten_values(ev: &mut Evaluator<'_>, sheet: usize, arg: &Expr) -> Vec<Value> {
    if let Expr::Range(a, b) = arg {
        let Some(target) = ev.resolve_sheet(&a.sheet, sheet) else {
            return vec![Value::Error(ErrorValue::Ref)];
        };
        let (r0, r1) = (a.row.min(b.row), a.row.max(b.row));
        let (c0, c1) = (a.col.min(b.col), a.col.max(b.col));
        let area = (r1 - r0 + 1) as u64 * (c1 - c0 + 1) as u64;
        if area > MAX_RANGE_CELLS {
            return vec![Value::Error(ErrorValue::Num)];
        }
        let mut out = Vec::new();
        for row in r0..=r1 {
            for col in c0..=c1 {
                out.push(ev.eval_cell(target, CellRef::new(row, col)));
            }
        }
        out
    } else {
        vec![ev.eval_expr(sheet, arg)]
    }
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
