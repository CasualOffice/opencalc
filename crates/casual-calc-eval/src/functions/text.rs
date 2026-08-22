//! Text: extraction, joining, case, encoding width and `TEXT` formatting.
//!
//! Split out of the single `functions.rs` under `MNT-002`; the section
//! headings that file already carried are the seams.

use super::*;

/// `SUBSTITUTE(text, old, new, [instance])`.
pub(crate) fn eval_substitute(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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
pub(crate) fn eval_replace(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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
pub(crate) fn eval_find_search(
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
pub(crate) fn eval_value(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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
pub(crate) fn proper_case(s: &str) -> String {
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
pub(crate) fn eval_rept(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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
pub(crate) fn eval_exact(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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

/// TEXTJOIN(delimiter, ignore_empty, text1, …).
/// TEXT(value, format_code): format a number with a SpreadsheetML format code,
/// via the same engine the grid uses to display cells (so they never drift).
/// A non-numeric first argument is returned as its text unchanged (Excel's
/// behavior when the value is already text).
pub(crate) fn eval_text(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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

pub(crate) fn eval_textjoin(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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

/// `CHAR` and `UNICHAR`.
///
/// They differ in range, not in kind: `CHAR` takes 1..=255 and `UNICHAR` any
/// valid code point. Treating them as the same function would accept `CHAR(955)`
/// and return λ, which Excel refuses.
pub(crate) fn eval_char(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    unicode: bool,
) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let code = match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => n.trunc(),
        Err(e) => return Value::Error(e),
    };
    if code < 1.0 || (!unicode && code > 255.0) {
        return Value::Error(ErrorValue::Value);
    }
    match u32::try_from(code as i64).ok().and_then(char::from_u32) {
        Some(ch) => Value::Text(ch.to_string()),
        None => Value::Error(ErrorValue::Value),
    }
}

/// `CODE` and `UNICODE` — the code point of the first character.
pub(crate) fn eval_code(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    unicode: bool,
) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let text = match ev.eval_expr(sheet, arg) {
        Value::Text(t) => t,
        Value::Error(e) => return Value::Error(e),
        other => match other.as_number() {
            Ok(n) => number_to_text(n),
            Err(e) => return Value::Error(e),
        },
    };
    let Some(ch) = text.chars().next() else {
        // Excel reports #VALUE! for empty text rather than 0.
        return Value::Error(ErrorValue::Value);
    };
    let code = ch as u32;
    if !unicode && code > 255 {
        // CODE is byte-oriented; a character it cannot express is #VALUE!.
        return Value::Error(ErrorValue::Value);
    }
    Value::Number(f64::from(code))
}

/// `CLEAN(text)` — drop the non-printable control characters.
pub(crate) fn eval_clean(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg) {
        Value::Error(e) => Value::Error(e),
        other => {
            let text = match other {
                Value::Text(t) => t,
                Value::Empty => String::new(),
                v => match v.as_number() {
                    Ok(n) => number_to_text(n),
                    Err(e) => return Value::Error(e),
                },
            };
            Value::Text(text.chars().filter(|c| !c.is_control()).collect())
        }
    }
}

/// `FIXED(number, [decimals], [no_commas])` — fixed-point text.
pub(crate) fn eval_fixed(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let value = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let decimals = match args.get(1) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n.trunc() as i32,
            Err(e) => return Value::Error(e),
        },
        None => 2,
    };
    let no_commas = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_bool() {
            Ok(b) => b,
            Err(e) => return Value::Error(e),
        },
        None => false,
    };
    // A negative `decimals` rounds to the left of the point: FIXED(1234.5,-2)
    // is "1,200". Clamping it to zero instead would quietly change the answer.
    let (rounded, places) = if decimals < 0 {
        let factor = 10f64.powi(-decimals);
        ((value / factor).round() * factor, 0usize)
    } else {
        (value, decimals as usize)
    };
    let mut text = format!("{rounded:.places$}");
    if !no_commas {
        text = group_thousands(&text);
    }
    Value::Text(text)
}

/// `DOLLAR(number, [decimals])` — like FIXED, always grouped, with a currency
/// symbol and parentheses for negatives, as the accounting format uses.
pub(crate) fn eval_dollar(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let value = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let decimals = match args.get(1) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n.trunc() as i32,
            Err(e) => return Value::Error(e),
        },
        None => 2,
    };
    let (rounded, places) = if decimals < 0 {
        let factor = 10f64.powi(-decimals);
        ((value / factor).round() * factor, 0usize)
    } else {
        (value, decimals as usize)
    };
    let body = group_thousands(&format!("{:.places$}", rounded.abs()));
    Value::Text(if rounded < 0.0 {
        format!("($={body})").replace("$=", "$")
    } else {
        format!("${body}")
    })
}

/// Insert thousands separators into the integer part of a formatted number.
pub(crate) fn group_thousands(text: &str) -> String {
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text),
    };
    let (int, frac) = match rest.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (rest, None),
    };
    let mut grouped = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    match frac {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    }
}

/// `NUMBERVALUE(text, [decimal], [group])` — parse a number written with
/// explicit separators, rather than guessing at the locale.
pub(crate) fn eval_numbervalue(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let text = match ev.eval_expr(sheet, &args[0]) {
        Value::Text(t) => t,
        Value::Error(e) => return Value::Error(e),
        other => match other.as_number() {
            Ok(n) => return Value::Number(n),
            Err(e) => return Value::Error(e),
        },
    };
    let mut separator = |i: usize, default: char| -> Result<char, Value> {
        match args.get(i) {
            Some(a) => match ev.eval_expr(sheet, a) {
                Value::Text(t) => Ok(t.chars().next().unwrap_or(default)),
                Value::Error(e) => Err(Value::Error(e)),
                _ => Ok(default),
            },
            None => Ok(default),
        }
    };
    let decimal = match separator(1, '.') {
        Ok(c) => c,
        Err(e) => return e,
    };
    let group = match separator(2, ',') {
        Ok(c) => c,
        Err(e) => return e,
    };
    let mut cleaned = String::new();
    for ch in text.chars() {
        if ch == group || ch.is_whitespace() {
            continue;
        }
        cleaned.push(if ch == decimal { '.' } else { ch });
    }
    // A trailing percent scales the result, which is the one piece of
    // interpretation the function does beyond separators.
    let percents = cleaned.chars().rev().take_while(|c| *c == '%').count();
    let body = cleaned.trim_end_matches('%');
    match body.parse::<f64>() {
        Ok(n) => Value::Number(n / 100f64.powi(percents as i32)),
        Err(_) => Value::Error(ErrorValue::Value),
    }
}

// --- Statistics ------------------------------------------------------------

/// `ASC` and `JIS` — full-width ↔ half-width conversion.
///
/// Only the ASCII range and the katakana that have both forms convert; anything
/// else passes through, which is what Excel does and what stops the function
/// mangling text that has no half-width equivalent.
pub(crate) fn eval_width_convert(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    to_full: bool,
) -> Value {
    if args.len() != 1 {
        return Value::Error(ErrorValue::Value);
    }
    let text = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(t) => t,
        Err(e) => return Value::Error(e),
    };
    let converted: String = text
        .chars()
        .map(|c| {
            let code = c as u32;
            if to_full {
                match code {
                    // ASCII printable → its full-width twin.
                    0x21..=0x7E => char::from_u32(code + 0xFEE0).unwrap_or(c),
                    0x20 => '\u{3000}', // the ideographic space
                    _ => c,
                }
            } else {
                match code {
                    0xFF01..=0xFF5E => char::from_u32(code - 0xFEE0).unwrap_or(c),
                    0x3000 => ' ',
                    _ => c,
                }
            }
        })
        .collect();
    Value::Text(converted)
}

/// `BAHTTEXT` — a number as Thai baht in words.
///
/// Thai number words are positional with two irregularities that make a naive
/// digit-by-digit rendering wrong: a tens digit of 1 is `สิบ` rather than
/// `หนึ่งสิบ`, and a units digit of 1 after any tens is `เอ็ด` rather than
/// `หนึ่ง`. Both are the difference between correct Thai and something that
/// reads as a foreigner's guess.
pub(crate) fn thai_number(mut n: u64) -> String {
    const DIGITS: [&str; 10] = [
        "",
        "หนึ่ง",
        "สอง",
        "สาม",
        "สี่",
        "ห้า",
        "หก",
        "เจ็ด",
        "แปด",
        "เก้า",
    ];
    const PLACES: [&str; 6] = ["", "สิบ", "ร้อย", "พัน", "หมื่น", "แสน"];
    if n == 0 {
        return "ศูนย์".to_owned();
    }
    let mut out = String::new();
    // Above a million the whole millions part is spoken then suffixed, which
    // recurses rather than needing place names beyond แสน.
    if n >= 1_000_000 {
        out.push_str(&thai_number(n / 1_000_000));
        out.push_str("ล้าน");
        n %= 1_000_000;
        if n == 0 {
            return out;
        }
    }
    let digits: Vec<u32> = n
        .to_string()
        .chars()
        .filter_map(|c| c.to_digit(10))
        .collect();
    let len = digits.len();
    for (i, d) in digits.iter().enumerate() {
        if *d == 0 {
            continue;
        }
        let place = len - 1 - i;
        if place == 1 && *d == 1 {
            out.push_str(PLACES[1]); // สิบ, not หนึ่งสิบ
        } else if place == 1 && *d == 2 {
            out.push_str("ยี่");
            out.push_str(PLACES[1]);
        } else if place == 0 && *d == 1 && len > 1 {
            out.push_str("เอ็ด"); // the special unit after any tens
        } else {
            out.push_str(DIGITS[*d as usize]);
            out.push_str(PLACES[place]);
        }
    }
    out
}

pub(crate) fn eval_bahttext(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 1 {
        return Value::Error(ErrorValue::Value);
    }
    let n = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let negative = n < 0.0;
    let abs = n.abs();
    let baht = abs.trunc() as u64;
    // Satang are hundredths, rounded — a half-satang has nowhere to go.
    let satang = ((abs - abs.trunc()) * 100.0).round() as u64;
    let mut out = String::new();
    if negative {
        out.push_str("ลบ");
    }
    out.push_str(&thai_number(baht));
    out.push_str("บาท");
    if satang == 0 {
        out.push_str("ถ้วน"); // "exactly", which Thai requires when there is no change
    } else {
        out.push_str(&thai_number(satang));
        out.push_str("สตางค์");
    }
    Value::Text(out)
}

/// The Bessel functions of the first and second kind, and their modified forms.
///
/// Series expansions rather than a table: the series converge quickly for the
/// arguments a spreadsheet sees, and a table would be an approximation with an
/// arbitrary cut-off rather than one with a stated error.
pub(crate) fn bessel_j(n: i32, x: f64) -> f64 {
    // Ascending series: sum (-1)^k (x/2)^(2k+n) / (k! (k+n)!).
    let mut term = (x / 2.0).powi(n) / factorial_f64(n as u32);
    let mut sum = term;
    let half_sq = (x / 2.0) * (x / 2.0);
    for k in 1..200 {
        term *= -half_sq / (k as f64 * (k + n) as f64);
        sum += term;
        if term.abs() < 1e-18 * sum.abs().max(1e-300) {
            break;
        }
    }
    sum
}

pub(crate) fn bessel_i(n: i32, x: f64) -> f64 {
    // The modified form is the same series without the alternating sign.
    let mut term = (x / 2.0).powi(n) / factorial_f64(n as u32);
    let mut sum = term;
    let half_sq = (x / 2.0) * (x / 2.0);
    for k in 1..300 {
        term *= half_sq / (k as f64 * (k + n) as f64);
        sum += term;
        if term.abs() < 1e-18 * sum.abs().max(1e-300) {
            break;
        }
    }
    sum
}

/// `n!` as a float, for the Bessel series. Named apart from the spreadsheet
/// `FACT`, which returns a `Value` and validates its domain.
pub(crate) fn factorial_f64(n: u32) -> f64 {
    (1..=n).map(f64::from).product::<f64>().max(1.0)
}

/// `BESSELY` and `BESSELK` — the second-kind pair, built from the first.
///
/// Both diverge at zero and are undefined for negative arguments, which is a
/// `#NUM!` rather than an infinity: a spreadsheet showing `1E+308` for an
/// undefined value is worse than one that says so.
pub(crate) fn bessel_y(n: i32, x: f64) -> f64 {
    // Y_n via the limit form, using the recurrence from Y0 and Y1 computed by
    // their standard series with the Euler–Mascheroni term.
    const EULER: f64 = 0.577_215_664_901_532_9;
    let y0 = {
        let mut sum = 0.0;
        let mut term = 1.0;
        let half_sq = (x / 2.0) * (x / 2.0);
        let mut harmonic = 0.0;
        for k in 1..200 {
            term *= -half_sq / (k as f64 * k as f64);
            harmonic += 1.0 / k as f64;
            sum += term * harmonic;
            if term.abs() < 1e-18 {
                break;
            }
        }
        2.0 / std::f64::consts::PI * ((x / 2.0).ln() + EULER) * bessel_j(0, x)
            - 2.0 / std::f64::consts::PI * sum
    };
    if n == 0 {
        return y0;
    }
    let y1 = 2.0 / std::f64::consts::PI * (bessel_j(1, x) * ((x / 2.0).ln() + EULER) - 1.0 / x)
        - bessel_series_y1_correction(x);
    if n == 1 {
        return y1;
    }
    // Upward recurrence, which is stable for Y.
    let (mut prev, mut cur) = (y0, y1);
    for k in 1..n {
        let next = 2.0 * k as f64 / x * cur - prev;
        prev = cur;
        cur = next;
    }
    cur
}

/// The series part of `Y1` that is not expressible through `J1`.
pub(crate) fn bessel_series_y1_correction(x: f64) -> f64 {
    let half = x / 2.0;
    let half_sq = half * half;
    let mut term = half;
    let mut sum = 0.0;
    let mut h_k = 0.0;
    let mut h_k1 = 1.0;
    for k in 0..200 {
        if k > 0 {
            term *= -half_sq / (k as f64 * (k + 1) as f64);
            h_k += 1.0 / k as f64;
            h_k1 += 1.0 / (k + 1) as f64;
        }
        sum += term * (h_k + h_k1);
        if term.abs() < 1e-18 {
            break;
        }
    }
    sum / std::f64::consts::PI
}

pub(crate) fn bessel_k(n: i32, x: f64) -> f64 {
    // K via the integral-free relation to I, using the standard K0/K1 series
    // and upward recurrence.
    const EULER: f64 = 0.577_215_664_901_532_9;
    let k0 = {
        let mut sum = 0.0;
        let mut term = 1.0;
        let half_sq = (x / 2.0) * (x / 2.0);
        let mut harmonic = 0.0;
        for k in 1..300 {
            term *= half_sq / (k as f64 * k as f64);
            harmonic += 1.0 / k as f64;
            sum += term * harmonic;
            if term.abs() < 1e-18 {
                break;
            }
        }
        -((x / 2.0).ln() + EULER) * bessel_i(0, x) + sum
    };
    if n == 0 {
        return k0;
    }
    let k1 = (1.0 / x) * (1.0 - x * k0 * 0.0) + {
        // K1 = 1/x + ln(x/2)·I1 − series; assembled from the same pieces.
        let half = x / 2.0;
        let half_sq = half * half;
        let mut term = half;
        let mut sum = 0.0;
        let mut h_k = 0.0;
        let mut h_k1 = 1.0;
        for k in 0..300 {
            if k > 0 {
                term *= half_sq / (k as f64 * (k + 1) as f64);
                h_k += 1.0 / k as f64;
                h_k1 += 1.0 / (k + 1) as f64;
            }
            sum += term * (h_k + h_k1);
            if term.abs() < 1e-18 {
                break;
            }
        }
        ((x / 2.0).ln() + EULER) * bessel_i(1, x) - sum / 2.0
    };
    if n == 1 {
        return k1;
    }
    let (mut prev, mut cur) = (k0, k1);
    for k in 1..n {
        let next = 2.0 * k as f64 / x * cur + prev;
        prev = cur;
        cur = next;
    }
    cur
}
