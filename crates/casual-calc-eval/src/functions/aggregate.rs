//! Aggregates that take a criterion: `COUNTIF`, `SUMIF`, the `*IFS`
//! family, `SUBTOTAL` and the `D` database functions.
//!
//! Split out of the single `functions.rs` under `MNT-002`; the section
//! headings that file already carried are the seams.

use super::*;

pub(crate) fn eval_countif(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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

pub(crate) fn eval_sumif(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match conditional_values(ev, sheet, args) {
        Ok(picked) => Value::Number(picked.iter().sum()),
        Err(e) => Value::Error(e),
    }
}

pub(crate) fn eval_averageif(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match conditional_values(ev, sheet, args) {
        Ok(picked) if picked.is_empty() => Value::Error(ErrorValue::Div0),
        Ok(picked) => Value::Number(picked.iter().sum::<f64>() / picked.len() as f64),
        Err(e) => Value::Error(e),
    }
}

/// Shared `SUMIF`/`AVERAGEIF` core: for each cell in the criteria range that
/// matches, collect the corresponding numeric value from the sum range (or the
/// criteria range itself when no third argument is given).
pub(crate) fn conditional_values(
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
pub(crate) enum CritOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

/// Split a criteria value into a comparison operator and an operand string.
/// A bare value (no leading operator) means equality.
pub(crate) fn parse_criteria(v: &Value) -> (CritOp, String) {
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
pub(crate) fn criterion_matches(cell: &Value, op: CritOp, operand: &str) -> bool {
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
pub(crate) fn has_wildcard(s: &str) -> bool {
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
pub(crate) fn unescape_criteria(s: &str) -> String {
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
pub(crate) fn wildcard_match(pattern: &str, text: &str) -> bool {
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

/// SUMIFS / AVERAGEIFS: an aggregate range followed by (range, criteria) pairs.
pub(crate) fn eval_ifs_aggregate(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    kind: IfsKind,
) -> Value {
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
        // MAXIFS and MINIFS of nothing are **0**, not an error — Excel's
        // choice, and different from AVERAGEIFS because a maximum of no
        // numbers has a defensible answer where a mean does not.
        IfsKind::Max if picked.is_empty() => Value::Number(0.0),
        IfsKind::Min if picked.is_empty() => Value::Number(0.0),
        IfsKind::Max => Value::Number(picked.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
        IfsKind::Min => Value::Number(picked.iter().copied().fold(f64::INFINITY, f64::min)),
    }
}

/// COUNTIFS: count positions satisfying every (range, criteria) pair.
pub(crate) fn eval_countifs(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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
pub(crate) fn ifs_matches(
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

/// `ROMAN(number, [form])` — classic form only; the four "concise" forms
/// differ in how they abbreviate and are not modelled, so a non-zero form is
/// refused rather than silently answered in the classic one.
pub(crate) fn eval_roman(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let n = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(v) => v.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    if let Some(a) = args.get(1) {
        match ev.eval_expr(sheet, a).as_number() {
            Ok(f) if f != 0.0 => return Value::Error(ErrorValue::Value),
            Ok(_) => {}
            Err(e) => return Value::Error(e),
        }
    }
    if !(0..=3999).contains(&n) {
        return Value::Error(ErrorValue::Value);
    }
    const TABLE: [(i64, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut left = n;
    let mut out = String::new();
    for (value, glyph) in TABLE {
        while left >= value {
            out.push_str(glyph);
            left -= value;
        }
    }
    Value::Text(out)
}

/// `ymd_to_serial` for tests in sibling modules, which cannot see a private fn.
#[cfg(test)]
pub(crate) fn ymd_to_serial_for_test(y: i64, m: i64, d: i64) -> i64 {
    ymd_to_serial(y, m, d)
}

/// Whether an expression calls `SUBTOTAL` or `AGGREGATE` anywhere.
///
/// A cell whose formula does is a *subtotal*, and both functions skip it so a
/// total over a block of subtotals does not count each figure twice.
pub(crate) fn contains_subtotal(expr: &Expr) -> bool {
    match expr {
        Expr::Function { name, args } => {
            name == "SUBTOTAL" || name == "AGGREGATE" || args.iter().any(contains_subtotal)
        }
        Expr::Call { callee, args } => {
            contains_subtotal(callee) || args.iter().any(contains_subtotal)
        }
        Expr::Unary { operand, .. } => contains_subtotal(operand),
        Expr::Binary { left, right, .. } => contains_subtotal(left) || contains_subtotal(right),
        _ => false,
    }
}

/// `AGGREGATE(function_num, options, ref1, …)` and
/// `AGGREGATE(function_num, options, array, k)`.
///
/// Nineteen aggregates under one name, each able to skip what `options`
/// selects: hidden rows (1, 3, 5, 7), error values (2, 3, 6, 7), and nested
/// `SUBTOTAL`/`AGGREGATE` cells (0–3). The skipping is the point of the
/// function — `AGGREGATE(14,6,r/(c>5),1)` is the usual way to take a maximum
/// of the rows a condition keeps — so it is applied as the values are read,
/// not afterwards.
///
/// Two forms, and Excel restricts each:
///
/// * `1`–`13` take **references**; a computed array is `#VALUE!`.
/// * `14`–`19` take one array and a required `k` (`#VALUE!` without it). The
///   array may be computed, but then — Microsoft's words — "the function will
///   not ignore hidden rows, nested subtotals or nested aggregates": a
///   computed value has no row to be hidden and no formula to inspect. Errors
///   in it can still be skipped.
pub(crate) fn eval_aggregate(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 {
        return Value::Error(ErrorValue::Value);
    }
    let code = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n.trunc(),
        Err(e) => return Value::Error(e),
    };
    let options = match &args[1] {
        Expr::Empty => 0.0,
        a => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n.trunc(),
            Err(e) => return Value::Error(e),
        },
    };
    if !(1.0..=19.0).contains(&code) || !(0.0..=7.0).contains(&options) {
        return Value::Error(ErrorValue::Value);
    }
    let (code, options) = (code as u8, options as u8);
    let skip = Skip {
        hidden: matches!(options, 1 | 3 | 5 | 7),
        errors: matches!(options, 2 | 3 | 6 | 7),
        nested: options <= 3,
    };
    if code <= 13 {
        let mut values = Vec::new();
        for arg in &args[2..] {
            if !matches!(
                arg,
                Expr::Reference(_) | Expr::Range(..) | Expr::StructuredRef { .. }
            ) {
                return Value::Error(ErrorValue::Value);
            }
            if let Err(e) = aggregate_values(ev, sheet, arg, skip, &mut values) {
                return Value::Error(e);
            }
        }
        return aggregate_reduce(code, &values);
    }
    if args.len() != 4 {
        return Value::Error(ErrorValue::Value);
    }
    let mut values = Vec::new();
    if let Err(e) = aggregate_values(ev, sheet, &args[2], skip, &mut values) {
        return Value::Error(e);
    }
    let k = match ev.eval_expr(sheet, &args[3]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let mut ns: Vec<f64> = values
        .iter()
        .filter_map(|v| match v {
            Value::Number(n) => Some(*n),
            _ => None,
        })
        .collect();
    ns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let result = match code {
        14 | 15 => {
            let k = k.trunc();
            if k < 1.0 || k as usize > ns.len() {
                None
            } else {
                let k = k as usize;
                Some(if code == 14 {
                    ns[ns.len() - k]
                } else {
                    ns[k - 1]
                })
            }
        }
        16 => percentile_inclusive(&ns, k),
        17 => quartile_k(k).and_then(|q| percentile_inclusive(&ns, q)),
        18 => percentile_exclusive(&ns, k),
        _ => quartile_k(k).and_then(|q| percentile_exclusive(&ns, q)),
    };
    match result {
        Some(v) => Value::Number(v),
        None => Value::Error(ErrorValue::Num),
    }
}

/// What an `AGGREGATE` option says to leave out.
#[derive(Clone, Copy)]
struct Skip {
    hidden: bool,
    errors: bool,
    nested: bool,
}

/// Read one `AGGREGATE` argument into `out`, leaving out what `skip` says.
///
/// A reference is walked cell by cell, so a hidden row or a subtotal cell can
/// be recognised; anything else is evaluated as an array and only its errors
/// can be skipped. An error that is not skipped is the answer.
fn aggregate_values(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    arg: &Expr,
    skip: Skip,
    out: &mut Vec<Value>,
) -> Result<(), ErrorValue> {
    let cells = match arg {
        Expr::Reference(r) => {
            let target = ev.resolve_sheet(&r.sheet, sheet).ok_or(ErrorValue::Ref)?;
            let at = r.resolve(ev.origin()).ok_or(ErrorValue::Ref)?;
            Some((target, vec![CellRef::new(at.row, at.col)]))
        }
        Expr::Range(..) | Expr::StructuredRef { .. } => {
            Some(range_cells(ev, sheet, arg).ok_or(ErrorValue::Ref)?)
        }
        _ => None,
    };
    let Some((target, cells)) = cells else {
        let items = match ev.eval_expr_array(sheet, arg) {
            Value::Array { cells, .. } => cells,
            other => vec![other],
        };
        for v in items {
            match v {
                Value::Error(_) if skip.errors => {}
                Value::Error(e) => return Err(e),
                other => out.push(other),
            }
        }
        return Ok(());
    };
    for at in cells {
        let workbook = ev.workbook();
        let Some(sheet_ref) = workbook.sheets.get(target) else {
            return Err(ErrorValue::Ref);
        };
        if skip.hidden && sheet_ref.is_row_hidden(at.row) {
            continue;
        }
        if skip.nested
            && sheet_ref
                .cells
                .get(at)
                .and_then(|c| c.formula)
                .and_then(|h| workbook.formula(h))
                .is_some_and(contains_subtotal)
        {
            continue;
        }
        match ev.eval_cell(target, at) {
            Value::Error(_) if skip.errors => {}
            Value::Error(e) => return Err(e),
            v => out.push(v),
        }
    }
    Ok(())
}

/// The reference-form reduction, `function_num` 1–13, over the values read.
///
/// Each answers as the function it stands for does over a reference: numbers
/// only (COUNTA aside), and the same empty-input answers — a SUM or MAX of
/// nothing is 0, a mean of nothing `#DIV/0!`.
fn aggregate_reduce(code: u8, values: &[Value]) -> Value {
    if code == 3 {
        let n = values.iter().filter(|v| !matches!(v, Value::Empty)).count();
        return Value::Number(n as f64);
    }
    let ns: Vec<f64> = values
        .iter()
        .filter_map(|v| match v {
            Value::Number(n) => Some(*n),
            _ => None,
        })
        .collect();
    let some = |v: Option<f64>, empty: ErrorValue| match v {
        Some(v) if v.is_finite() => Value::Number(v),
        Some(_) => Value::Error(ErrorValue::Num),
        None => Value::Error(empty),
    };
    match code {
        1 => some((!ns.is_empty()).then(|| mean(&ns)), ErrorValue::Div0),
        2 => Value::Number(ns.len() as f64),
        4 => Value::Number(ns.iter().copied().reduce(f64::max).unwrap_or(0.0)),
        5 => Value::Number(ns.iter().copied().reduce(f64::min).unwrap_or(0.0)),
        6 if ns.is_empty() => Value::Number(0.0),
        6 => some(Some(ns.iter().product()), ErrorValue::Num),
        7 => some(variance(&ns, true).map(f64::sqrt), ErrorValue::Div0),
        8 => some(variance(&ns, false).map(f64::sqrt), ErrorValue::Div0),
        9 => Value::Number(ns.iter().sum()),
        10 => some(variance(&ns, true), ErrorValue::Div0),
        11 => some(variance(&ns, false), ErrorValue::Div0),
        12 => {
            let mut sorted = ns;
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
            some(percentile_inclusive(&sorted, 0.5), ErrorValue::Num)
        }
        _ => some(mode_of(&ns), ErrorValue::Na),
    }
}

/// `QUARTILE`'s `quart` as the fraction it names, or `None` outside 0..=4.
pub(crate) fn quartile_k(quart: f64) -> Option<f64> {
    (0.0..5.0).contains(&quart).then(|| quart.trunc() / 4.0)
}

/// `PERCENTILE.INC` over sorted data, or `None` for no data or `k` outside
/// `0..=1`.
pub(crate) fn percentile_inclusive(sorted: &[f64], k: f64) -> Option<f64> {
    if sorted.is_empty() || !(0.0..=1.0).contains(&k) {
        return None;
    }
    Some(percentile_of(sorted, k))
}

/// `PERCENTILE.EXC` over sorted data: the rank is `k·(n+1)`, and it must fall
/// within `1..=n` — the exclusive form never extrapolates past the data, so a
/// `k` below `1/(n+1)` or above `n/(n+1)` has no answer.
pub(crate) fn percentile_exclusive(sorted: &[f64], k: f64) -> Option<f64> {
    let n = sorted.len();
    if n == 0 || k <= 0.0 || k >= 1.0 {
        return None;
    }
    let rank = k * (n as f64 + 1.0);
    // A rank within a rounding error of an integer is that integer: `1/11`
    // times 11 must not land a hair below 1 and be refused.
    let rank = if (rank - rank.round()).abs() < 1e-12 {
        rank.round()
    } else {
        rank
    };
    if rank < 1.0 || rank > n as f64 {
        return None;
    }
    let lower = rank.floor() as usize;
    if lower >= n {
        return Some(sorted[n - 1]);
    }
    let frac = rank - lower as f64;
    Some(sorted[lower - 1] + frac * (sorted[lower] - sorted[lower - 1]))
}
