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

/// The catalog of built-in functions as `(name, signature)`, kept alphabetical.
/// This is the **single source of truth** for the function list — the host UI
/// (autocomplete / signature help) reads it via the SDK/WASM instead of keeping
/// its own copy, and a test asserts every entry has a dispatch arm in
/// `call_function` so the two never drift. Add a function in both places.
pub const FUNCTIONS: &[(&str, &str)] = &[
    ("ABS", "ABS(number)"),
    ("AND", "AND(logical1, …)"),
    ("AVERAGE", "AVERAGE(number1, …)"),
    ("AVERAGEIF", "AVERAGEIF(range, criteria, [average_range])"),
    ("AVERAGEIFS", "AVERAGEIFS(avg_range, range1, criteria1, …)"),
    ("CEILING", "CEILING(number, significance)"),
    ("CHOOSE", "CHOOSE(index, value1, …)"),
    ("COLUMN", "COLUMN([reference])"),
    ("COLUMNS", "COLUMNS(array)"),
    ("CONCAT", "CONCAT(text1, …)"),
    ("CONCATENATE", "CONCATENATE(text1, …)"),
    ("COUNT", "COUNT(value1, …)"),
    ("COUNTA", "COUNTA(value1, …)"),
    ("COUNTIF", "COUNTIF(range, criteria)"),
    ("COUNTIFS", "COUNTIFS(range1, criteria1, …)"),
    ("DATE", "DATE(year, month, day)"),
    ("DAY", "DAY(serial_number)"),
    ("EDATE", "EDATE(start_date, months)"),
    ("EOMONTH", "EOMONTH(start_date, months)"),
    ("EXACT", "EXACT(text1, text2)"),
    ("FIND", "FIND(find_text, within_text, [start])"),
    ("FLOOR", "FLOOR(number, significance)"),
    ("HLOOKUP", "HLOOKUP(lookup, table, row, [exact])"),
    ("IF", "IF(logical_test, value_if_true, value_if_false)"),
    ("IFERROR", "IFERROR(value, value_if_error)"),
    ("IFNA", "IFNA(value, value_if_na)"),
    ("IFS", "IFS(test1, value1, …)"),
    ("INDEX", "INDEX(array, row_num, [col_num])"),
    ("INT", "INT(number)"),
    ("ISBLANK", "ISBLANK(value)"),
    ("ISERR", "ISERR(value)"),
    ("ISERROR", "ISERROR(value)"),
    ("ISEVEN", "ISEVEN(number)"),
    ("ISLOGICAL", "ISLOGICAL(value)"),
    ("ISNA", "ISNA(value)"),
    ("ISNONTEXT", "ISNONTEXT(value)"),
    ("ISNUMBER", "ISNUMBER(value)"),
    ("ISODD", "ISODD(number)"),
    ("ISTEXT", "ISTEXT(value)"),
    ("LARGE", "LARGE(array, k)"),
    ("LEFT", "LEFT(text, [num_chars])"),
    ("LEN", "LEN(text)"),
    ("LOWER", "LOWER(text)"),
    ("MATCH", "MATCH(lookup, array, [match_type])"),
    ("MAX", "MAX(number1, …)"),
    ("MEDIAN", "MEDIAN(number1, …)"),
    ("MID", "MID(text, start_num, num_chars)"),
    ("MIN", "MIN(number1, …)"),
    ("MOD", "MOD(number, divisor)"),
    ("MONTH", "MONTH(serial_number)"),
    ("NA", "NA()"),
    ("NOT", "NOT(logical)"),
    ("OR", "OR(logical1, …)"),
    ("POWER", "POWER(number, power)"),
    ("PRODUCT", "PRODUCT(number1, …)"),
    ("PROPER", "PROPER(text)"),
    ("RANK", "RANK(number, ref, [order])"),
    ("REPLACE", "REPLACE(old, start, num_chars, new)"),
    ("REPT", "REPT(text, number_times)"),
    ("RIGHT", "RIGHT(text, [num_chars])"),
    ("ROUND", "ROUND(number, num_digits)"),
    ("ROUNDDOWN", "ROUNDDOWN(number, num_digits)"),
    ("ROUNDUP", "ROUNDUP(number, num_digits)"),
    ("ROW", "ROW([reference])"),
    ("ROWS", "ROWS(array)"),
    ("SEARCH", "SEARCH(find_text, within_text, [start])"),
    ("SIGN", "SIGN(number)"),
    ("SMALL", "SMALL(array, k)"),
    ("SQRT", "SQRT(number)"),
    ("STDEV", "STDEV(number1, …)"),
    ("STDEVP", "STDEVP(number1, …)"),
    ("SUBSTITUTE", "SUBSTITUTE(text, old, new, [instance])"),
    ("SUM", "SUM(number1, …)"),
    ("SUMIF", "SUMIF(range, criteria, [sum_range])"),
    ("SUMIFS", "SUMIFS(sum_range, range1, criteria1, …)"),
    ("SUMPRODUCT", "SUMPRODUCT(array1, …)"),
    (
        "SWITCH",
        "SWITCH(expression, value1, result1, …, [default])",
    ),
    ("TEXT", "TEXT(value, format_code)"),
    ("TEXTJOIN", "TEXTJOIN(delimiter, ignore_empty, text1, …)"),
    ("TRIM", "TRIM(text)"),
    ("TRUNC", "TRUNC(number, [num_digits])"),
    ("UPPER", "UPPER(text)"),
    ("VALUE", "VALUE(text)"),
    ("VLOOKUP", "VLOOKUP(lookup, table, col, [exact])"),
    ("WEEKDAY", "WEEKDAY(serial_number, [type])"),
    ("YEAR", "YEAR(serial_number)"),
];

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
        "PRODUCT" => eval_product(ev, sheet, args),
        "ROUNDUP" => eval_round_dir(ev, sheet, args, RoundDir::Up),
        "ROUNDDOWN" => eval_round_dir(ev, sheet, args, RoundDir::Down),
        "TRUNC" => eval_trunc(ev, sheet, args),
        "CEILING" => eval_ceiling_floor(ev, sheet, args, true),
        "FLOOR" => eval_ceiling_floor(ev, sheet, args, false),
        "SIGN" => eval_sign(ev, sheet, args),
        "VLOOKUP" => eval_vlookup(ev, sheet, args, true),
        "HLOOKUP" => eval_vlookup(ev, sheet, args, false),
        "INDEX" => eval_index(ev, sheet, args),
        "MATCH" => eval_match(ev, sheet, args),
        "CHOOSE" => eval_choose(ev, sheet, args),
        "SUBSTITUTE" => eval_substitute(ev, sheet, args),
        "REPLACE" => eval_replace(ev, sheet, args),
        "FIND" => eval_find_search(ev, sheet, args, true),
        "SEARCH" => eval_find_search(ev, sheet, args, false),
        "VALUE" => eval_value(ev, sheet, args),
        "PROPER" => text_op(ev, sheet, args, proper_case),
        "REPT" => eval_rept(ev, sheet, args),
        "EXACT" => eval_exact(ev, sheet, args),
        "DATE" => eval_date(ev, sheet, args),
        "YEAR" => eval_date_part(ev, sheet, args, DatePart::Year),
        "MONTH" => eval_date_part(ev, sheet, args, DatePart::Month),
        "DAY" => eval_date_part(ev, sheet, args, DatePart::Day),
        "WEEKDAY" => eval_weekday(ev, sheet, args),
        "EDATE" => eval_edate(ev, sheet, args, false),
        "EOMONTH" => eval_edate(ev, sheet, args, true),
        // --- Logical / info (M6-2) ---
        "IFS" => eval_ifs(ev, sheet, args),
        "SWITCH" => eval_switch(ev, sheet, args),
        "IFNA" => eval_ifna(ev, sheet, args),
        "NA" => Value::Error(ErrorValue::Na),
        "ISBLANK" => is_predicate(ev, sheet, args, |v| matches!(v, Value::Empty)),
        "ISNUMBER" => is_predicate(ev, sheet, args, |v| matches!(v, Value::Number(_))),
        "ISTEXT" => is_predicate(ev, sheet, args, |v| matches!(v, Value::Text(_))),
        "ISNONTEXT" => is_predicate(ev, sheet, args, |v| !matches!(v, Value::Text(_))),
        "ISLOGICAL" => is_predicate(ev, sheet, args, |v| matches!(v, Value::Bool(_))),
        "ISERROR" => is_predicate(ev, sheet, args, |v| matches!(v, Value::Error(_))),
        "ISERR" => is_predicate(
            ev,
            sheet,
            args,
            |v| matches!(v, Value::Error(e) if *e != ErrorValue::Na),
        ),
        "ISNA" => is_predicate(ev, sheet, args, |v| {
            matches!(v, Value::Error(ErrorValue::Na))
        }),
        "ISEVEN" => eval_parity(ev, sheet, args, true),
        "ISODD" => eval_parity(ev, sheet, args, false),
        // --- Statistics (M6-2) ---
        "MEDIAN" => eval_median(ev, sheet, args),
        "LARGE" => eval_large_small(ev, sheet, args, true),
        "SMALL" => eval_large_small(ev, sheet, args, false),
        "RANK" => eval_rank(ev, sheet, args),
        "STDEV" => eval_stdev(ev, sheet, args, true),
        "STDEVP" => eval_stdev(ev, sheet, args, false),
        "SUMPRODUCT" => eval_sumproduct(ev, sheet, args),
        // --- Multi-criteria aggregates (M6-2) ---
        "SUMIFS" => eval_ifs_aggregate(ev, sheet, args, IfsKind::Sum),
        "AVERAGEIFS" => eval_ifs_aggregate(ev, sheet, args, IfsKind::Average),
        "COUNTIFS" => eval_countifs(ev, sheet, args),
        // --- Shape / text (M6-2) ---
        "ROWS" => eval_dim(ev, sheet, args, true),
        "COLUMNS" => eval_dim(ev, sheet, args, false),
        "ROW" => eval_row_col(ev, args, true),
        "COLUMN" => eval_row_col(ev, args, false),
        "TEXT" => eval_text(ev, sheet, args),
        "TEXTJOIN" => eval_textjoin(ev, sheet, args),
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
/// otherwise a case-insensitive text comparison (Excel semantics). For `=`/`<>`
/// criteria whose operand contains an unescaped `*` or `?`, Excel wildcard
/// matching is used and applies to **text** cells only.
fn criterion_matches(cell: &Value, op: CritOp, operand: &str) -> bool {
    // Wildcard text matching (Excel): `*` = any run, `?` = one char, `~` escapes
    // the next `*`/`?`/`~`. Wildcards only match text cells, not numbers/blanks.
    if matches!(op, CritOp::Eq | CritOp::Ne) && has_wildcard(operand) {
        let matched = match cell {
            Value::Text(s) => wildcard_match(operand, s),
            _ => false,
        };
        return if matches!(op, CritOp::Ne) {
            !matched
        } else {
            matched
        };
    }

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
            // Unescape `~*`/`~?`/`~~` so a criterion can match a literal wildcard.
            let b = unescape_criteria(operand).to_uppercase();
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

/// True if `s` contains a `*` or `?` that is not escaped by a preceding `~`.
fn has_wildcard(s: &str) -> bool {
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '~' => {
                chars.next(); // the escaped char is literal
            }
            '*' | '?' => return true,
            _ => {}
        }
    }
    false
}

/// Remove `~` escapes before `*`/`?`/`~`, leaving other characters untouched.
fn unescape_criteria(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('~', Some(&n)) if matches!(n, '*' | '?' | '~') => {
                out.push(n);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// Case-insensitive Excel wildcard match of `pattern` against `text`.
/// `*` matches any run of characters (including empty), `?` matches exactly one
/// character, and `~` escapes the following `*`/`?`/`~` to a literal.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    enum Tok {
        Any,
        One,
        Lit(char),
    }
    // Fold case up front so both pattern literals and text compare case-insensitively.
    let pat_up = pattern.to_uppercase();
    let mut toks = Vec::new();
    let mut chars = pat_up.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '~' => match chars.peek() {
                Some(&n @ ('*' | '?' | '~')) => {
                    toks.push(Tok::Lit(n));
                    chars.next();
                }
                _ => toks.push(Tok::Lit('~')),
            },
            '*' => toks.push(Tok::Any),
            '?' => toks.push(Tok::One),
            other => toks.push(Tok::Lit(other)),
        }
    }

    let text: Vec<char> = text.to_uppercase().chars().collect();
    // Classic linear-time backtracking wildcard match.
    let (mut ti, mut pi) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None; // (pattern idx after '*', text idx)
    while ti < text.len() {
        match toks.get(pi) {
            Some(Tok::One) => {
                pi += 1;
                ti += 1;
            }
            Some(Tok::Lit(c)) if *c == text[ti] => {
                pi += 1;
                ti += 1;
            }
            Some(Tok::Any) => {
                star = Some((pi + 1, ti));
                pi += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    pi = sp;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    while matches!(toks.get(pi), Some(Tok::Any)) {
        pi += 1;
    }
    pi == toks.len()
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

// --- Extra math -----------------------------------------------------------

fn eval_product(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match flatten_numbers(ev, sheet, args) {
        Ok(ns) if ns.is_empty() => Value::Number(0.0),
        Ok(ns) => Value::Number(ns.iter().product()),
        Err(e) => Value::Error(e),
    }
}

#[derive(Clone, Copy)]
enum RoundDir {
    Up,
    Down,
}

/// `ROUNDUP`/`ROUNDDOWN`: round away from / toward zero to `digits` places.
fn eval_round_dir(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], dir: RoundDir) -> Value {
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
    let scaled = (value * factor).abs();
    let rounded = match dir {
        RoundDir::Up => scaled.ceil(),
        RoundDir::Down => scaled.floor(),
    };
    Value::Number(value.signum() * rounded / factor)
}

/// `TRUNC(number, [digits])`: truncate toward zero (digits default 0).
fn eval_trunc(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let value = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let digits = match args.get(1) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n as i32,
            Err(e) => return Value::Error(e),
        },
        None => 0,
    };
    let factor = 10f64.powi(digits);
    Value::Number((value * factor).trunc() / factor)
}

/// `CEILING`/`FLOOR`: round to the nearest multiple of `significance`.
/// Excel requires number and significance to share a sign (else `#NUM!`).
fn eval_ceiling_floor(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], up: bool) -> Value {
    let Some((num, sig)) = two_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let (num, sig) = match (num, sig) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return Value::Error(e),
    };
    if sig == 0.0 {
        return Value::Number(0.0);
    }
    if num != 0.0 && num.signum() != sig.signum() {
        return Value::Error(ErrorValue::Num);
    }
    let quotient = num / sig;
    let rounded = if up {
        quotient.ceil()
    } else {
        quotient.floor()
    };
    Value::Number(rounded * sig)
}

fn eval_sign(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) if n > 0.0 => Value::Number(1.0),
        Ok(n) if n < 0.0 => Value::Number(-1.0),
        Ok(_) => Value::Number(0.0),
        Err(e) => Value::Error(e),
    }
}

// --- Lookup / reference ---------------------------------------------------

/// A materialized rectangular block of cell values (row-major).
struct Grid {
    rows: usize,
    cols: usize,
    cells: Vec<Value>,
}

impl Grid {
    fn get(&self, row: usize, col: usize) -> &Value {
        &self.cells[row * self.cols + col]
    }
}

/// Evaluate one argument into a [`Grid`]; a scalar becomes a 1x1 block.
fn eval_range_2d(ev: &mut Evaluator<'_>, sheet: usize, arg: &Expr) -> Result<Grid, ErrorValue> {
    if let Expr::Range(a, b) = arg {
        let target = ev.resolve_sheet(&a.sheet, sheet).ok_or(ErrorValue::Ref)?;
        let (r0, r1) = (a.row.min(b.row), a.row.max(b.row));
        let (c0, c1) = (a.col.min(b.col), a.col.max(b.col));
        let area = (r1 - r0 + 1) as u64 * (c1 - c0 + 1) as u64;
        if area > MAX_RANGE_CELLS {
            return Err(ErrorValue::Num);
        }
        let rows = (r1 - r0 + 1) as usize;
        let cols = (c1 - c0 + 1) as usize;
        let mut cells = Vec::with_capacity(rows * cols);
        for row in r0..=r1 {
            for col in c0..=c1 {
                cells.push(ev.eval_cell(target, CellRef::new(row, col)));
            }
        }
        Ok(Grid { rows, cols, cells })
    } else {
        Ok(Grid {
            rows: 1,
            cols: 1,
            cells: vec![ev.eval_expr(sheet, arg)],
        })
    }
}

/// Order two values the way lookups compare: numerically when both are numeric,
/// otherwise by case-insensitive text (matches the engine's comparison rules).
fn loose_cmp(a: &Value, b: &Value) -> Option<Ordering> {
    match (numeric_of(a), numeric_of(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y),
        _ => {
            let sa = a.as_text().unwrap_or_default().to_uppercase();
            let sb = b.as_text().unwrap_or_default().to_uppercase();
            Some(sa.cmp(&sb))
        }
    }
}

fn numeric_of(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => Some(*n),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// `VLOOKUP` (`vertical` true) / `HLOOKUP` (false).
fn eval_vlookup(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], vertical: bool) -> Value {
    if args.len() < 3 || args.len() > 4 {
        return Value::Error(ErrorValue::Value);
    }
    let key = ev.eval_expr(sheet, &args[0]);
    if let Some(e) = key.as_error() {
        return Value::Error(e);
    }
    let grid = match eval_range_2d(ev, sheet, &args[1]) {
        Ok(g) => g,
        Err(e) => return Value::Error(e),
    };
    let index = match ev.eval_expr(sheet, &args[2]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let approximate = match args.get(3) {
        Some(a) => match ev.eval_expr(sheet, a).as_bool() {
            Ok(b) => b,
            Err(e) => return Value::Error(e),
        },
        None => true,
    };
    if index < 1 {
        return Value::Error(ErrorValue::Value);
    }
    let index = index as usize;
    // Length along the search axis, and bound for the return index.
    let (search_len, index_bound) = if vertical {
        (grid.rows, grid.cols)
    } else {
        (grid.cols, grid.rows)
    };
    if index > index_bound {
        return Value::Error(ErrorValue::Ref);
    }
    // Value in the search line at position `i`.
    let at = |g: &Grid, i: usize| -> Value {
        if vertical {
            g.get(i, 0).clone()
        } else {
            g.get(0, i).clone()
        }
    };
    let found = if approximate {
        // Largest entry <= key, assuming the line is sorted ascending.
        let mut best: Option<usize> = None;
        for i in 0..search_len {
            match loose_cmp(&at(&grid, i), &key) {
                Some(Ordering::Less) | Some(Ordering::Equal) => best = Some(i),
                _ => break,
            }
        }
        best
    } else {
        (0..search_len).find(|&i| loose_cmp(&at(&grid, i), &key) == Some(Ordering::Equal))
    };
    match found {
        Some(i) if vertical => grid.get(i, index - 1).clone(),
        Some(i) => grid.get(index - 1, i).clone(),
        None => Value::Error(ErrorValue::Na),
    }
}

/// `INDEX(range, row, [col])`. Row/col are 1-based; a single index selects
/// along the sole axis of a one-dimensional range.
fn eval_index(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let grid = match eval_range_2d(ev, sheet, &args[0]) {
        Ok(g) => g,
        Err(e) => return Value::Error(e),
    };
    let first = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let second = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => Some(n as i64),
            Err(e) => return Value::Error(e),
        },
        None => None,
    };
    let (row, col) = match second {
        Some(c) => (first, c),
        None => {
            // One index: pick the axis that has more than one line.
            if grid.rows == 1 {
                (1, first)
            } else if grid.cols == 1 {
                (first, 1)
            } else {
                return Value::Error(ErrorValue::Ref);
            }
        }
    };
    if row < 1 || col < 1 || row as usize > grid.rows || col as usize > grid.cols {
        return Value::Error(ErrorValue::Ref);
    }
    grid.get(row as usize - 1, col as usize - 1).clone()
}

/// `MATCH(lookup, range, [type])`. Type 1 (default) ascending, 0 exact,
/// -1 descending. Returns the 1-based position, or `#N/A`.
fn eval_match(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let key = ev.eval_expr(sheet, &args[0]);
    if let Some(e) = key.as_error() {
        return Value::Error(e);
    }
    let grid = match eval_range_2d(ev, sheet, &args[1]) {
        Ok(g) => g,
        Err(e) => return Value::Error(e),
    };
    let match_type = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n as i64,
            Err(e) => return Value::Error(e),
        },
        None => 1,
    };
    // MATCH operates on a single row or column.
    let line: Vec<&Value> = grid.cells.iter().collect();
    let found = match match_type {
        0 => line
            .iter()
            .position(|v| loose_cmp(v, &key) == Some(Ordering::Equal)),
        1 => {
            // Largest value <= key (ascending order assumed).
            let mut best = None;
            for (i, v) in line.iter().enumerate() {
                match loose_cmp(v, &key) {
                    Some(Ordering::Less) | Some(Ordering::Equal) => best = Some(i),
                    _ => break,
                }
            }
            best
        }
        _ => {
            // -1: smallest value >= key (descending order assumed).
            let mut best = None;
            for (i, v) in line.iter().enumerate() {
                match loose_cmp(v, &key) {
                    Some(Ordering::Greater) | Some(Ordering::Equal) => best = Some(i),
                    _ => break,
                }
            }
            best
        }
    };
    match found {
        Some(i) => Value::Number(i as f64 + 1.0),
        None => Value::Error(ErrorValue::Na),
    }
}

/// `CHOOSE(index, value1, value2, ...)`. Only the selected value is evaluated.
fn eval_choose(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 {
        return Value::Error(ErrorValue::Value);
    }
    let index = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let choices = &args[1..];
    if index < 1 || index as usize > choices.len() {
        return Value::Error(ErrorValue::Value);
    }
    ev.eval_expr(sheet, &choices[index as usize - 1])
}

// --- Extra text -----------------------------------------------------------

/// `SUBSTITUTE(text, old, new, [instance])`.
fn eval_substitute(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 || args.len() > 4 {
        return Value::Error(ErrorValue::Value);
    }
    let text = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let old = match ev.eval_expr(sheet, &args[1]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let new = match ev.eval_expr(sheet, &args[2]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    if old.is_empty() {
        return Value::Text(text);
    }
    let instance = match args.get(3) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) if n < 1.0 => return Value::Error(ErrorValue::Value),
            Ok(n) => Some(n as usize),
            Err(e) => return Value::Error(e),
        },
        None => None,
    };
    match instance {
        None => Value::Text(text.replace(&old, &new)),
        Some(target) => {
            let mut out = String::with_capacity(text.len());
            let mut rest = text.as_str();
            let mut seen = 0usize;
            while let Some(pos) = rest.find(&old) {
                seen += 1;
                if seen == target {
                    out.push_str(&rest[..pos]);
                    out.push_str(&new);
                    out.push_str(&rest[pos + old.len()..]);
                    return Value::Text(out);
                }
                out.push_str(&rest[..pos + old.len()]);
                rest = &rest[pos + old.len()..];
            }
            out.push_str(rest);
            Value::Text(out)
        }
    }
}

/// `REPLACE(old_text, start, num_chars, new_text)` (1-based, over characters).
fn eval_replace(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 4 {
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
    let count = match ev.eval_expr(sheet, &args[2]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let new = match ev.eval_expr(sheet, &args[3]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    if start < 1 || count < 0 {
        return Value::Error(ErrorValue::Value);
    }
    let chars: Vec<char> = text.chars().collect();
    let begin = (start as usize - 1).min(chars.len());
    let end = (begin + count as usize).min(chars.len());
    let mut out: String = chars[..begin].iter().collect();
    out.push_str(&new);
    out.extend(chars[end..].iter());
    Value::Text(out)
}

/// `FIND` (case-sensitive) / `SEARCH` (case-insensitive). 1-based; `#VALUE!`
/// when not found or `start` is out of range.
fn eval_find_search(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    case_sensitive: bool,
) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let needle = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let haystack = match ev.eval_expr(sheet, &args[1]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let start = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n as i64,
            Err(e) => return Value::Error(e),
        },
        None => 1,
    };
    let hay_chars: Vec<char> = haystack.chars().collect();
    if start < 1 || start as usize > hay_chars.len() + 1 {
        return Value::Error(ErrorValue::Value);
    }
    let skip = start as usize - 1;
    let (needle, tail): (String, String) = if case_sensitive {
        (needle, hay_chars[skip..].iter().collect())
    } else {
        (
            needle.to_uppercase(),
            hay_chars[skip..].iter().collect::<String>().to_uppercase(),
        )
    };
    match tail.find(&needle) {
        Some(byte_pos) => {
            // Convert the byte offset within `tail` to a character offset.
            let char_off = tail[..byte_pos].chars().count();
            Value::Number((skip + char_off + 1) as f64)
        }
        None => Value::Error(ErrorValue::Value),
    }
}

/// `VALUE(text)`: parse text as a number.
fn eval_value(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let text = match ev.eval_expr(sheet, arg).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    match text.trim().parse::<f64>() {
        Ok(n) => Value::Number(n),
        Err(_) => Value::Error(ErrorValue::Value),
    }
}

/// `PROPER`: capitalize the first letter of each word, lowercase the rest.
fn proper_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at_word_start = true;
    for ch in s.chars() {
        if ch.is_alphabetic() {
            if at_word_start {
                out.extend(ch.to_uppercase());
            } else {
                out.extend(ch.to_lowercase());
            }
            at_word_start = false;
        } else {
            out.push(ch);
            at_word_start = true;
        }
    }
    out
}

/// `REPT(text, count)`.
fn eval_rept(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let text = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let count = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    if count < 0 {
        return Value::Error(ErrorValue::Value);
    }
    Value::Text(text.repeat(count as usize))
}

/// `EXACT(a, b)`: case-sensitive text equality.
fn eval_exact(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let a = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let b = match ev.eval_expr(sheet, &args[1]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    Value::Bool(a == b)
}

// --- Dates (deterministic, 1900 serial system) ----------------------------

/// Days from the civil date `(y, m, d)` to 1970-01-01 (Howard Hinnant's
/// algorithm). Proleptic Gregorian; the inverse of [`serial_to_ymd`].
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Convert a civil date to an Excel (1900-system) serial day number.
fn ymd_to_serial(y: i64, m: i64, d: i64) -> i64 {
    days_from_civil(y, m, d) + 25_569
}

/// Convert an Excel serial day number to `(year, month, day)`.
fn serial_to_ymd(serial_days: i64) -> (i64, i64, i64) {
    let mut z = serial_days - 25_569 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    z -= era * 146_097;
    let doe = z;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    // Normalize so month 13 rolls to January of the next year, etc.
    let ny = y + (m - 1).div_euclid(12);
    let nm = (m - 1).rem_euclid(12) + 1;
    let next = if nm == 12 {
        ymd_to_serial(ny + 1, 1, 1)
    } else {
        ymd_to_serial(ny, nm + 1, 1)
    };
    next - ymd_to_serial(ny, nm, 1)
}

/// `DATE(year, month, day)`. Month/day overflow rolls into adjacent months
/// (Excel semantics); years 0-1899 are offset by 1900.
fn eval_date(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let mut year = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let month = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let day = match ev.eval_expr(sheet, &args[2]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    if (0..1900).contains(&year) {
        year += 1900;
    }
    // Normalize the month into 1..=12, carrying into the year, then add the
    // day offset (which itself may push across month boundaries).
    let ny = year + (month - 1).div_euclid(12);
    let nm = (month - 1).rem_euclid(12) + 1;
    let serial = ymd_to_serial(ny, nm, 1) + (day - 1);
    if serial < 0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(serial as f64)
}

#[derive(Clone, Copy)]
enum DatePart {
    Year,
    Month,
    Day,
}

fn eval_date_part(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], part: DatePart) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let serial = match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    if serial < 0 {
        return Value::Error(ErrorValue::Num);
    }
    let (y, m, d) = serial_to_ymd(serial);
    let out = match part {
        DatePart::Year => y,
        DatePart::Month => m,
        DatePart::Day => d,
    };
    Value::Number(out as f64)
}

/// `WEEKDAY(serial, [type])`. Type 1 (default) Sun=1..Sat=7, type 2
/// Mon=1..Sun=7, type 3 Mon=0..Sun=6.
fn eval_weekday(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let serial = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    let kind = match args.get(1) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n as i64,
            Err(e) => return Value::Error(e),
        },
        None => 1,
    };
    if serial < 0 {
        return Value::Error(ErrorValue::Num);
    }
    // Days since the Unix epoch; 1970-01-01 was a Thursday.
    let unix = serial - 25_569;
    let dow_sun0 = (unix + 4).rem_euclid(7); // 0 = Sunday .. 6 = Saturday
    let out = match kind {
        1 => dow_sun0 + 1,
        2 => (dow_sun0 + 6).rem_euclid(7) + 1,
        3 => (dow_sun0 + 6).rem_euclid(7),
        _ => return Value::Error(ErrorValue::Num),
    };
    Value::Number(out as f64)
}

/// `EDATE` (`eomonth` false) advances by whole months keeping the day (clamped
/// to the month length). `EOMONTH` (true) returns the last day of that month.
fn eval_edate(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], eomonth: bool) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let serial = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    let months = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    if serial < 0 {
        return Value::Error(ErrorValue::Num);
    }
    let (y, m, d) = serial_to_ymd(serial);
    let total = m - 1 + months;
    let ny = y + total.div_euclid(12);
    let nm = total.rem_euclid(12) + 1;
    let last = days_in_month(ny, nm);
    let day = if eomonth { last } else { d.min(last) };
    let out = ymd_to_serial(ny, nm, day);
    if out < 0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(out as f64)
}

// --- M6-2 built-ins --------------------------------------------------------

/// The IS-family: evaluate the single argument and test the resulting value.
fn is_predicate(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    test: fn(&Value) -> bool,
) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    Value::Bool(test(&ev.eval_expr(sheet, arg)))
}

/// ISEVEN / ISODD: truncate toward zero, then test parity.
fn eval_parity(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], even: bool) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => Value::Bool(((n.trunc() as i64).rem_euclid(2) == 0) == even),
        Err(e) => Value::Error(e),
    }
}

/// IFS(test1, value1, test2, value2, …): first TRUE test's value, else #N/A.
fn eval_ifs(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || !args.len().is_multiple_of(2) {
        return Value::Error(ErrorValue::Value);
    }
    for pair in args.chunks(2) {
        match ev.eval_expr(sheet, &pair[0]).as_bool() {
            Ok(true) => return ev.eval_expr(sheet, &pair[1]),
            Ok(false) => {}
            Err(e) => return Value::Error(e),
        }
    }
    Value::Error(ErrorValue::Na)
}

/// SWITCH(expr, v1, r1, …, [default]).
fn eval_switch(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 {
        return Value::Error(ErrorValue::Value);
    }
    let subject = ev.eval_expr(sheet, &args[0]);
    let rest = &args[1..];
    let mut i = 0;
    while i + 1 < rest.len() {
        let candidate = ev.eval_expr(sheet, &rest[i]);
        if values_equal(&subject, &candidate) {
            return ev.eval_expr(sheet, &rest[i + 1]);
        }
        i += 2;
    }
    // A trailing odd argument is the default.
    if rest.len() % 2 == 1 {
        return ev.eval_expr(sheet, &rest[rest.len() - 1]);
    }
    Value::Error(ErrorValue::Na)
}

/// Excel equality for SWITCH: numeric when both numeric, else case-insensitive text.
fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y,
        _ => a
            .as_text()
            .unwrap_or_default()
            .eq_ignore_ascii_case(&b.as_text().unwrap_or_default()),
    }
}

/// IFNA(value, value_if_na): substitute only on #N/A.
fn eval_ifna(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    match ev.eval_expr(sheet, &args[0]) {
        Value::Error(ErrorValue::Na) => ev.eval_expr(sheet, &args[1]),
        v => v,
    }
}

/// MEDIAN over all numeric arguments.
fn eval_median(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match flatten_numbers(ev, sheet, args) {
        Ok(mut ns) if !ns.is_empty() => {
            ns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
            let m = ns.len() / 2;
            let med = if ns.len() % 2 == 1 {
                ns[m]
            } else {
                (ns[m - 1] + ns[m]) / 2.0
            };
            Value::Number(med)
        }
        Ok(_) => Value::Error(ErrorValue::Num),
        Err(e) => Value::Error(e),
    }
}

/// LARGE(array, k) / SMALL(array, k): k-th largest/smallest (1-based).
fn eval_large_small(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], large: bool) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let k = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    let mut ns = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    if k < 1 || k as usize > ns.len() {
        return Value::Error(ErrorValue::Num);
    }
    ns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let idx = if large {
        ns.len() - k as usize
    } else {
        k as usize - 1
    };
    Value::Number(ns[idx])
}

/// RANK(number, ref, [order]): position of `number` within `ref` (1-based).
/// `order` 0/omitted = descending, non-zero = ascending. Ties share a rank.
fn eval_rank(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let target = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let ns = match flatten_numbers(ev, sheet, &args[1..2]) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    let ascending = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n != 0.0,
            Err(e) => return Value::Error(e),
        },
        None => false,
    };
    if !ns.contains(&target) {
        return Value::Error(ErrorValue::Na);
    }
    let rank = if ascending {
        1 + ns.iter().filter(|&&n| n < target).count()
    } else {
        1 + ns.iter().filter(|&&n| n > target).count()
    };
    Value::Number(rank as f64)
}

/// STDEV (sample, n-1) / STDEVP (population, n).
fn eval_stdev(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], sample: bool) -> Value {
    match flatten_numbers(ev, sheet, args) {
        Ok(ns) => {
            let n = ns.len();
            if n < if sample { 2 } else { 1 } {
                return Value::Error(ErrorValue::Div0);
            }
            let mean = ns.iter().sum::<f64>() / n as f64;
            let ss: f64 = ns.iter().map(|x| (x - mean).powi(2)).sum();
            let denom = if sample { (n - 1) as f64 } else { n as f64 };
            Value::Number((ss / denom).sqrt())
        }
        Err(e) => Value::Error(e),
    }
}

/// SUMPRODUCT: element-wise product of equal-length arrays, then summed.
fn eval_sumproduct(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() {
        return Value::Error(ErrorValue::Value);
    }
    let mut cols: Vec<Vec<f64>> = Vec::new();
    for arg in args {
        let mut nums = Vec::new();
        for v in flatten_values(ev, sheet, arg) {
            match v {
                Value::Number(n) => nums.push(n),
                Value::Bool(b) => nums.push(if b { 1.0 } else { 0.0 }),
                Value::Error(e) => return Value::Error(e),
                _ => nums.push(0.0), // text/empty contribute 0, per Excel
            }
        }
        cols.push(nums);
    }
    let len = cols[0].len();
    if cols.iter().any(|c| c.len() != len) {
        return Value::Error(ErrorValue::Value);
    }
    let mut total = 0.0;
    for i in 0..len {
        total += cols.iter().map(|c| c[i]).product::<f64>();
    }
    Value::Number(total)
}

/// ROWS / COLUMNS: the row/column count of a range (a lone cell ref is 1×1).
fn eval_dim(_ev: &mut Evaluator<'_>, _sheet: usize, args: &[Expr], rows: bool) -> Value {
    match args.first() {
        Some(Expr::Range(a, b)) => {
            let n = if rows {
                a.row.max(b.row) - a.row.min(b.row) + 1
            } else {
                a.col.max(b.col) - a.col.min(b.col) + 1
            };
            Value::Number(n as f64)
        }
        Some(Expr::Reference(_)) => Value::Number(1.0),
        _ => Value::Error(ErrorValue::Value),
    }
}

/// TEXTJOIN(delimiter, ignore_empty, text1, …).
/// TEXT(value, format_code): format a number with a SpreadsheetML format code,
/// via the same engine the grid uses to display cells (so they never drift).
/// A non-numeric first argument is returned as its text unchanged (Excel's
/// behavior when the value is already text).
fn eval_text(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let code = match ev.eval_expr(sheet, &args[1]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    match ev.eval_expr(sheet, &args[0]) {
        Value::Error(e) => Value::Error(e),
        Value::Number(n) => Value::Text(casual_calc_layout::format_number(n, &code)),
        Value::Bool(b) => Value::Text(if b { "TRUE" } else { "FALSE" }.to_owned()),
        // Text (or empty) already prints as itself.
        v => Value::Text(v.as_text().unwrap_or_default()),
    }
}

fn eval_textjoin(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 {
        return Value::Error(ErrorValue::Value);
    }
    let delim = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let ignore_empty = match ev.eval_expr(sheet, &args[1]).as_bool() {
        Ok(b) => b,
        Err(e) => return Value::Error(e),
    };
    let mut parts = Vec::new();
    for arg in &args[2..] {
        for v in flatten_values(ev, sheet, arg) {
            if let Value::Error(e) = v {
                return Value::Error(e);
            }
            let s = v.as_text().unwrap_or_default();
            if ignore_empty && s.is_empty() {
                continue;
            }
            parts.push(s);
        }
    }
    Value::Text(parts.join(&delim))
}

/// Which aggregate a `*IFS` call computes over the matched positions.
enum IfsKind {
    Sum,
    Average,
}

/// SUMIFS / AVERAGEIFS: an aggregate range followed by (range, criteria) pairs.
fn eval_ifs_aggregate(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], kind: IfsKind) -> Value {
    if args.len() < 3 || args.len().is_multiple_of(2) {
        return Value::Error(ErrorValue::Value);
    }
    let agg = flatten_values(ev, sheet, &args[0]);
    let keep = match ifs_matches(ev, sheet, &args[1..], agg.len()) {
        Ok(m) => m,
        Err(e) => return Value::Error(e),
    };
    let mut picked = Vec::new();
    for (i, &k) in keep.iter().enumerate() {
        if !k {
            continue;
        }
        match agg.get(i) {
            Some(Value::Number(n)) => picked.push(*n),
            Some(Value::Bool(b)) => picked.push(if *b { 1.0 } else { 0.0 }),
            Some(Value::Error(e)) => return Value::Error(*e),
            _ => {}
        }
    }
    match kind {
        IfsKind::Sum => Value::Number(picked.iter().sum()),
        IfsKind::Average if picked.is_empty() => Value::Error(ErrorValue::Div0),
        IfsKind::Average => Value::Number(picked.iter().sum::<f64>() / picked.len() as f64),
    }
}

/// COUNTIFS: count positions satisfying every (range, criteria) pair.
fn eval_countifs(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || !args.len().is_multiple_of(2) {
        return Value::Error(ErrorValue::Value);
    }
    let first = flatten_values(ev, sheet, &args[0]);
    match ifs_matches(ev, sheet, args, first.len()) {
        Ok(m) => Value::Number(m.iter().filter(|&&k| k).count() as f64),
        Err(e) => Value::Error(e),
    }
}

/// Fold consecutive (range, criteria) pairs into a per-position keep mask of
/// length `len` (logical AND across pairs). Ranges must all match `len`.
fn ifs_matches(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    pairs: &[Expr],
    len: usize,
) -> Result<Vec<bool>, ErrorValue> {
    let mut keep = vec![true; len];
    let mut i = 0;
    while i + 1 < pairs.len() {
        let (op, operand) = parse_criteria(&ev.eval_expr(sheet, &pairs[i + 1]));
        let range = flatten_values(ev, sheet, &pairs[i]);
        if range.len() != len {
            return Err(ErrorValue::Value);
        }
        for (j, cell) in range.iter().enumerate() {
            if matches!(cell, Value::Empty) || !criterion_matches(cell, op, &operand) {
                keep[j] = false;
            }
        }
        i += 2;
    }
    Ok(keep)
}

/// ROW / COLUMN: the 1-based row/column of a reference (top-left of a range),
/// or of the calling cell when no argument is given.
fn eval_row_col(ev: &mut Evaluator<'_>, args: &[Expr], row: bool) -> Value {
    let index = match args.first() {
        None => match ev.current_cell() {
            Some((_, at)) => {
                if row {
                    at.row
                } else {
                    at.col
                }
            }
            None => return Value::Error(ErrorValue::Value),
        },
        Some(Expr::Reference(r)) => {
            if row {
                r.row
            } else {
                r.col
            }
        }
        Some(Expr::Range(a, _)) => {
            if row {
                a.row
            } else {
                a.col
            }
        }
        Some(_) => return Value::Error(ErrorValue::Value),
    };
    Value::Number((index + 1) as f64)
}
