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
