//! Array lifting: a function whose parameter is a single value, handed an
//! array, is evaluated once per element and answers an array (`XL-FEAT-131`).
//!
//! `=MOD(C1:C10,4)` spills ten remainders in Excel; it was `#VALUE!` here,
//! because every scalar function read its argument in scalar context and a
//! bare range there has no single answer. `XL-FEAT-002` lifted the unary
//! operators and `IF` by hand; this generalises it with one table rather than
//! a hundred per-function edits.
//!
//! **Which parameters lift is a property of the parameter, not of the
//! function.** `COUNTIF(range, criteria)` reads its range as a range — lifting
//! it would count each cell against itself — while its criteria is one value,
//! and `COUNTIF(A:A, B1:B3)` is three counts. So the table gives each
//! function's parameters a [`ParamKind`]; a function that is not in it takes
//! every argument exactly as written, which is the conservative default: an
//! aggregate, a lookup, a reference-inspecting function (`ROW`, `ISREF`,
//! `ISFORMULA`), a lazy one (`IF`, `IFERROR`, `CHOOSE`) and an array-shaping
//! one all stay out.
//!
//! The lifted elements reach the function through the evaluator's scope — the
//! mechanism `LET` and `LAMBDA` already use — bound to names no formula can
//! write. That keeps every element's value exact, including a blank cell,
//! which is a value (zero, empty text) and not an omitted argument.

use casual_calc_formula::Expr;

use crate::eval::Evaluator;
use crate::value::Value;

use super::{checked_array_size, dispatch};

/// How a parameter takes its argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParamKind {
    /// One value. An array argument here is lifted: the function is called
    /// once per element.
    Scalar,
    /// A range or an array, taken whole (`COUNTIF`'s range, `SUM`'s numbers).
    /// Passed through untouched.
    Array,
}

use ParamKind::{Array as A, Scalar as S};

/// Every parameter scalar — the common case.
const SCALARS: &[ParamKind] = &[S];

/// The functions that lift, alphabetical, each with its parameters' kinds.
/// The last kind repeats for any further arguments.
///
/// Conservative by construction: a function is here only if Excel documents
/// each lifted parameter as a single value and lifts it. Anything absent takes
/// its arguments as written.
const PARAM_KINDS: &[(&str, &[ParamKind])] = &[
    ("ABS", SCALARS),
    ("ACOS", SCALARS),
    ("ACOSH", SCALARS),
    ("ACOT", SCALARS),
    ("ACOTH", SCALARS),
    ("ASIN", SCALARS),
    ("ASINH", SCALARS),
    ("ATAN", SCALARS),
    ("ATAN2", SCALARS),
    ("ATANH", SCALARS),
    // The criteria is one value; the ranges are ranges.
    ("AVERAGEIF", &[A, S, A]),
    ("BASE", SCALARS),
    ("CEILING", SCALARS),
    ("CEILING.MATH", SCALARS),
    ("CEILING.PRECISE", SCALARS),
    ("CHAR", SCALARS),
    ("CLEAN", SCALARS),
    ("CODE", SCALARS),
    ("COMBIN", SCALARS),
    ("COMBINA", SCALARS),
    // CONCATENATE is the scalar one; CONCAT and TEXTJOIN join ranges.
    ("CONCATENATE", SCALARS),
    ("COS", SCALARS),
    ("COSH", SCALARS),
    ("COT", SCALARS),
    ("COTH", SCALARS),
    ("COUNTIF", &[A, S]),
    ("CSC", SCALARS),
    ("CSCH", SCALARS),
    ("DATE", SCALARS),
    ("DAY", SCALARS),
    ("DAYS", SCALARS),
    ("DECIMAL", SCALARS),
    ("DEGREES", SCALARS),
    ("EDATE", SCALARS),
    ("EOMONTH", SCALARS),
    ("EVEN", SCALARS),
    ("EXACT", SCALARS),
    ("EXP", SCALARS),
    ("FACT", SCALARS),
    ("FACTDOUBLE", SCALARS),
    ("FIND", SCALARS),
    ("FIXED", SCALARS),
    ("FLOOR", SCALARS),
    ("FLOOR.MATH", SCALARS),
    ("FLOOR.PRECISE", SCALARS),
    ("HOUR", SCALARS),
    ("INT", SCALARS),
    ("ISBLANK", SCALARS),
    ("ISERR", SCALARS),
    ("ISERROR", SCALARS),
    ("ISEVEN", SCALARS),
    ("ISLOGICAL", SCALARS),
    ("ISNA", SCALARS),
    ("ISNONTEXT", SCALARS),
    ("ISNUMBER", SCALARS),
    ("ISO.CEILING", SCALARS),
    ("ISODD", SCALARS),
    ("ISOWEEKNUM", SCALARS),
    ("ISTEXT", SCALARS),
    ("LEFT", SCALARS),
    ("LEN", SCALARS),
    ("LN", SCALARS),
    ("LOG", SCALARS),
    ("LOG10", SCALARS),
    ("LOWER", SCALARS),
    ("MID", SCALARS),
    ("MINUTE", SCALARS),
    ("MOD", SCALARS),
    ("MONTH", SCALARS),
    ("MROUND", SCALARS),
    ("NOT", SCALARS),
    ("NUMBERVALUE", SCALARS),
    ("ODD", SCALARS),
    ("PERMUT", SCALARS),
    ("POWER", SCALARS),
    ("PROPER", SCALARS),
    ("QUOTIENT", SCALARS),
    ("RADIANS", SCALARS),
    ("REPLACE", SCALARS),
    ("REPT", SCALARS),
    ("RIGHT", SCALARS),
    ("ROMAN", SCALARS),
    ("ROUND", SCALARS),
    ("ROUNDDOWN", SCALARS),
    ("ROUNDUP", SCALARS),
    ("SEARCH", SCALARS),
    ("SEC", SCALARS),
    ("SECH", SCALARS),
    ("SECOND", SCALARS),
    ("SIGN", SCALARS),
    ("SIN", SCALARS),
    ("SINH", SCALARS),
    ("SQRT", SCALARS),
    ("SQRTPI", SCALARS),
    ("SUBSTITUTE", SCALARS),
    ("SUMIF", &[A, S, A]),
    ("TAN", SCALARS),
    ("TANH", SCALARS),
    ("TEXT", SCALARS),
    ("TIME", SCALARS),
    ("TRIM", SCALARS),
    ("TRUNC", SCALARS),
    ("UNICHAR", SCALARS),
    ("UNICODE", SCALARS),
    ("UPPER", SCALARS),
    ("VALUE", SCALARS),
    ("WEEKDAY", SCALARS),
    ("WEEKNUM", SCALARS),
    ("YEAR", SCALARS),
];

/// The kinds of `name`'s parameters, or `None` when it does not lift.
fn param_kinds(name: &str) -> Option<&'static [ParamKind]> {
    PARAM_KINDS
        .binary_search_by(|(n, _)| (*n).cmp(name))
        .ok()
        .map(|i| PARAM_KINDS[i].1)
}

/// The kind of argument `index` of a function with these parameter kinds.
fn kind_at(kinds: &[ParamKind], index: usize) -> ParamKind {
    kinds
        .get(index)
        .or_else(|| kinds.last())
        .copied()
        .unwrap_or(ParamKind::Array)
}

/// Every function that lifts at least one parameter — for the tests that pin
/// the table to its intent.
#[cfg(test)]
pub(crate) fn lifted_functions() -> impl Iterator<Item = &'static str> {
    PARAM_KINDS.iter().map(|(n, _)| *n)
}

/// Whether evaluating `expr` could produce a multi-element array.
///
/// A conservative syntactic test that keeps the common case free: a literal or
/// a single-cell reference cannot, so `ROUND(A1*B1,2)` is called exactly as
/// before, with no evaluation up front and no binding. Anything that might —
/// a range, a call to an array function, a `LET` name — is evaluated.
fn may_be_array(expr: &Expr) -> bool {
    match expr {
        Expr::Number(_)
        | Expr::Bool(_)
        | Expr::Text(_)
        | Expr::Error(_)
        | Expr::Reference(_)
        | Expr::Empty
        | Expr::Raw(_)
        // A wide structured reference is `#VALUE!` in an array context too;
        // left to the function, as it always was.
        | Expr::StructuredRef { .. } => false,
        Expr::Unary { operand, .. } => may_be_array(operand),
        Expr::Binary { left, right, .. } => may_be_array(left) || may_be_array(right),
        // A lifting function answers an array only if a lifted argument is one.
        Expr::Function { name, args } => match param_kinds(name) {
            Some(kinds) => args
                .iter()
                .enumerate()
                .any(|(i, a)| kind_at(kinds, i) == ParamKind::Scalar && may_be_array(a)),
            None => true,
        },
        // `COUNTIF(r, {"a","b"})` is two counts: a constant with more than one
        // element is exactly what lifting is for.
        Expr::Array(rows) => rows.len() > 1 || rows.first().is_some_and(|r| r.len() > 1),
        Expr::Range(..) | Expr::Name(_) | Expr::Call { .. } => true,
    }
}

/// The name a lifted argument is bound to: a control character no formula can
/// contain, so it can neither collide with nor be read by a user's name.
fn bound_name(index: usize) -> String {
    format!("\u{1}lift{index}")
}

/// Call `name`, lifting its scalar parameters over any array argument.
///
/// `None` when nothing needs lifting — the caller dispatches as written.
pub(crate) fn call_lifted(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    name: &str,
    args: &[Expr],
) -> Option<Value> {
    let kinds = param_kinds(name)?;
    let positions: Vec<usize> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| kind_at(kinds, *i) == ParamKind::Scalar && may_be_array(a))
        .map(|(i, _)| i)
        .collect();
    if positions.is_empty() {
        return None;
    }

    // Each candidate is evaluated once, in array context, so a range is its
    // cells and `C1:C3*2` its three products. Evaluated once matters: the
    // values are bound, not re-read per element, so a volatile argument is
    // one draw.
    let values: Vec<Value> = positions
        .iter()
        .map(|&i| ev.eval_expr_array(sheet, &args[i]))
        .collect();

    // The call the function sees: the lifted positions read their bound
    // values; every other argument is exactly as written.
    let mut call: Vec<Expr> = args.to_vec();
    for (k, &i) in positions.iter().enumerate() {
        call[i] = Expr::Name(bound_name(k));
    }
    let mark = ev.scope_len();

    let is_block = |v: &Value| {
        let (r, c) = v.shape();
        r * c > 1
    };
    if !values.iter().any(is_block) {
        // Nothing to lift after all (`ROUND(SUM(A1:A3),2)`): one call, with
        // the values already computed.
        for (k, v) in values.into_iter().enumerate() {
            ev.bind(bound_name(k), v);
        }
        let result = dispatch(ev, sheet, name, &call);
        ev.unbind_to(mark);
        return Some(result);
    }

    let refs: Vec<&Value> = values.iter().collect();
    let (rows, cols) = crate::value::broadcast_shape(&refs);
    // The bound every array result checks, before allocating: a 1×N against
    // an M×1 is M×N, which two modest arguments can make enormous.
    if let Err(e) = checked_array_size(rows, cols) {
        return Some(Value::Error(e));
    }
    for k in 0..values.len() {
        ev.bind(bound_name(k), Value::Empty);
    }
    let mut cells = Vec::with_capacity(rows * cols);
    for r in 0..rows {
        for c in 0..cols {
            for (k, v) in values.iter().enumerate() {
                ev.rebind(mark + k, v.broadcast_at(r, c));
            }
            // One element of the answer is one value — an array a function
            // returns for a single element collapses to its corner.
            cells.push(dispatch(ev, sheet, name, &call).scalar());
        }
    }
    ev.unbind_to(mark);
    Some(Value::Array { rows, cols, cells })
}
