//! Engineering: base conversion, bit operations, complex numbers, unit
//! conversion and the Bessel family.
//!
//! Split out of the single `functions.rs` under `MNT-002`; the section
//! headings that file already carried are the seams.

use super::*;

/// The widest value the base conversions accept: ten digits in the source
/// radix, which is the ceiling OOXML sets on all of them.
pub(crate) const BASE_DIGITS: u32 = 10;

/// Parse `text` in `radix`, honouring the two's-complement convention the
/// spreadsheet base functions use.
///
/// A ten-digit value with the top digit set is negative — `1111111111` in
/// binary is -1, not 1023. Parsing it as unsigned is the single most likely
/// mistake here, and it yields a large positive number that looks plausible.
pub(crate) fn parse_in_base(text: &str, radix: u32) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() || text.len() > BASE_DIGITS as usize {
        return None;
    }
    let magnitude = i64::from_str_radix(text, radix).ok()?;
    let width = (radix as f64).log2().round() as u32 * BASE_DIGITS;
    let sign_bit = 1i64 << (width - 1);
    Some(if magnitude >= sign_bit {
        magnitude - (sign_bit << 1)
    } else {
        magnitude
    })
}

/// Format `value` in `radix` with the same two's-complement convention.
pub(crate) fn format_in_base(value: i64, radix: u32, places: Option<usize>) -> Option<String> {
    let width = (radix as f64).log2().round() as u32 * BASE_DIGITS;
    let sign_bit = 1i64 << (width - 1);
    if value >= sign_bit || value < -sign_bit {
        return None;
    }
    let encoded = if value < 0 {
        (value + (sign_bit << 1)) as u64
    } else {
        value as u64
    };
    let digits = match radix {
        2 => format!("{encoded:b}"),
        8 => format!("{encoded:o}"),
        16 => format!("{encoded:X}"),
        _ => return None,
    };
    // A negative value always occupies the full width, so `places` is ignored
    // for it — padding a two's-complement form would change its value.
    if value < 0 {
        return Some(digits);
    }
    match places {
        Some(p) if p < digits.len() => None,
        Some(p) => Some(format!("{digits:0>p$}")),
        None => Some(digits),
    }
}

pub(crate) fn text_arg(ev: &mut Evaluator<'_>, sheet: usize, expr: &Expr) -> Result<String, Value> {
    match ev.eval_expr(sheet, expr) {
        Value::Text(t) => Ok(t),
        Value::Error(e) => Err(Value::Error(e)),
        // A binary literal typed as a number reaches here as one, and its
        // digits are the text we want.
        other => other.as_number().map(number_to_text).map_err(Value::Error),
    }
}

pub(crate) fn base_to_dec(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    radix: u32,
) -> Value {
    let [arg] = args else {
        return Value::Error(ErrorValue::Value);
    };
    let text = match text_arg(ev, sheet, arg) {
        Ok(t) => t,
        Err(e) => return e,
    };
    match parse_in_base(&text, radix) {
        Some(v) => Value::Number(v as f64),
        None => Value::Error(ErrorValue::Num),
    }
}

pub(crate) fn places_arg(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
) -> Result<Option<usize>, Value> {
    match args.get(1) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) if n < 0.0 => Err(Value::Error(ErrorValue::Num)),
            Ok(n) => Ok(Some(n.trunc() as usize)),
            Err(e) => Err(Value::Error(e)),
        },
        None => Ok(None),
    }
}

pub(crate) fn dec_to_base(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    radix: u32,
) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let value = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    let places = match places_arg(ev, sheet, args) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match format_in_base(value, radix, places) {
        Some(text) => Value::Text(text),
        None => Value::Error(ErrorValue::Num),
    }
}

pub(crate) fn base_to_base(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    from: u32,
    to: u32,
) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let text = match text_arg(ev, sheet, &args[0]) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let places = match places_arg(ev, sheet, args) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let Some(value) = parse_in_base(&text, from) else {
        return Value::Error(ErrorValue::Num);
    };
    match format_in_base(value, to, places) {
        Some(text) => Value::Text(text),
        None => Value::Error(ErrorValue::Num),
    }
}

/// The bitwise operations, which are defined only on non-negative integers
/// below 2^48 — a range that fits `f64` exactly, so the result is never a
/// rounded approximation of the bits asked for.
pub(crate) fn bitwise(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: fn(u64, u64) -> u64,
) -> Value {
    let [a, b] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let limit = 2f64.powi(48);
    if a < 0.0 || b < 0.0 || a >= limit || b >= limit || a.fract() != 0.0 || b.fract() != 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(f(a as u64, b as u64) as f64)
}

pub(crate) fn bit_shift(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], left: bool) -> Value {
    let [value, shift] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let limit = 2f64.powi(48);
    if value < 0.0 || value >= limit || value.fract() != 0.0 || shift.abs() > 53.0 {
        return Value::Error(ErrorValue::Num);
    }
    // A negative shift reverses the direction, which is why the two functions
    // can share this body.
    let shift = if left { shift } else { -shift };
    let result = if shift >= 0.0 {
        (value as u64) << (shift as u32)
    } else {
        (value as u64) >> ((-shift) as u32)
    };
    if (result as f64) >= limit {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(result as f64)
}

/// `DELTA(a, [b])` — 1 when equal; `GESTEP(a, [step])` — 1 when at or above.
pub(crate) fn eval_delta(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    equality: bool,
) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let a = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let b = match args.get(1) {
        Some(arg) => match ev.eval_expr(sheet, arg).as_number() {
            Ok(n) => n,
            Err(e) => return Value::Error(e),
        },
        None => 0.0,
    };
    let hit = if equality { a == b } else { a >= b };
    Value::Number(if hit { 1.0 } else { 0.0 })
}

/// `ERF(lower, [upper])` — the error function, or the integral between two
/// bounds when an upper one is given.
pub(crate) fn eval_erf(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let lower = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    match args.get(1) {
        Some(arg) => match ev.eval_expr(sheet, arg).as_number() {
            Ok(upper) => Value::Number(erf(upper) - erf(lower)),
            Err(e) => Value::Error(e),
        },
        None => Value::Number(erf(lower)),
    }
}

// --- Financial -------------------------------------------------------------

/// A complex number as `(real, imaginary)`. A named pair rather than a struct
/// because every operation below is arithmetic on two floats and a struct would
/// add ceremony without adding meaning.
pub(crate) type Complex = (f64, f64);

/// An operation that can fail — division by zero, in practice.
pub(crate) type ComplexOp1 = fn(Complex) -> Option<Complex>;
/// A two-argument operation that can fail.
pub(crate) type ComplexOp2 = fn(Complex, Complex) -> Option<Complex>;
/// A total two-argument operation, for the folds.
pub(crate) type ComplexFold = fn(Complex, Complex) -> Complex;

/// Parse `"3+4i"`, `"-2.5j"`, `"7"` or `"i"` into `(real, imaginary, suffix)`.
pub(crate) fn parse_complex(text: &str) -> Option<(f64, f64, char)> {
    let t = text.trim();
    if t.is_empty() {
        return Some((0.0, 0.0, 'i'));
    }
    let suffix = if t.ends_with('i') {
        'i'
    } else if t.ends_with('j') {
        'j'
    } else {
        // No suffix at all: a plain real number.
        return t.parse::<f64>().ok().map(|r| (r, 0.0, 'i'));
    };
    let body = &t[..t.len() - 1];
    // Split at the sign that separates the parts, skipping a leading sign and
    // any exponent sign — `1e-3+2i` must not split at the exponent's minus.
    let bytes = body.as_bytes();
    let mut split = None;
    for i in (1..bytes.len()).rev() {
        let c = bytes[i] as char;
        if (c == '+' || c == '-') && !matches!(bytes[i - 1] as char, 'e' | 'E') {
            split = Some(i);
            break;
        }
    }
    match split {
        Some(i) => {
            let real = body[..i].parse::<f64>().ok()?;
            // "3+i" means 3 + 1i, and "3-i" means 3 - 1i: a bare sign is a
            // coefficient of one.
            let imag_text = &body[i..];
            let imag = match imag_text {
                "+" => 1.0,
                "-" => -1.0,
                other => other.parse::<f64>().ok()?,
            };
            Some((real, imag, suffix))
        }
        None => {
            let imag = match body {
                "" | "+" => 1.0,
                "-" => -1.0,
                other => other.parse::<f64>().ok()?,
            };
            Some((0.0, imag, suffix))
        }
    }
}

/// Format `(real, imaginary)` the way Excel writes one: the parts that are zero
/// are omitted, and a unit coefficient is written as a bare `i`.
pub(crate) fn format_complex(real: f64, imag: f64, suffix: char) -> String {
    let n = |v: f64| number_to_text(v);
    if imag == 0.0 {
        return n(real);
    }
    let imag_part = if imag == 1.0 {
        suffix.to_string()
    } else if imag == -1.0 {
        format!("-{suffix}")
    } else {
        format!("{}{suffix}", n(imag))
    };
    if real == 0.0 {
        return imag_part;
    }
    if imag > 0.0 {
        format!("{}+{imag_part}", n(real))
    } else {
        format!("{}{imag_part}", n(real))
    }
}

pub(crate) fn complex_arg(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    expr: &Expr,
) -> Result<(f64, f64, char), Value> {
    let text = match ev.eval_expr(sheet, expr) {
        Value::Text(t) => t,
        Value::Error(e) => return Err(Value::Error(e)),
        other => other
            .as_number()
            .map(number_to_text)
            .map_err(Value::Error)?,
    };
    parse_complex(&text).ok_or(Value::Error(ErrorValue::Num))
}

/// A function of one complex number returning a real.
pub(crate) fn complex_part(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: fn(Complex) -> f64,
) -> Value {
    let [arg] = args else {
        return Value::Error(ErrorValue::Value);
    };
    match complex_arg(ev, sheet, arg) {
        Ok((re, im, _)) => Value::Number(f((re, im))),
        Err(e) => e,
    }
}

/// A function of one complex number returning another.
pub(crate) fn complex_map(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: fn(Complex) -> Complex,
) -> Value {
    let [arg] = args else {
        return Value::Error(ErrorValue::Value);
    };
    match complex_arg(ev, sheet, arg) {
        Ok((re, im, suffix)) => {
            let (r, i) = f((re, im));
            Value::Text(format_complex(r, i, suffix))
        }
        Err(e) => e,
    }
}

/// As [`complex_map`], but the operation can fail (a division by zero).
pub(crate) fn complex_pair_self(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: ComplexOp1,
) -> Value {
    let [arg] = args else {
        return Value::Error(ErrorValue::Value);
    };
    match complex_arg(ev, sheet, arg) {
        Ok((re, im, suffix)) => match f((re, im)) {
            Some((r, i)) => Value::Text(format_complex(r, i, suffix)),
            None => Value::Error(ErrorValue::Div0),
        },
        Err(e) => e,
    }
}

/// A function of two complex numbers.
pub(crate) fn complex_pair(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: ComplexOp2,
) -> Value {
    let [a, b] = args else {
        return Value::Error(ErrorValue::Value);
    };
    let (ar, ai, suffix) = match complex_arg(ev, sheet, a) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (br, bi, _) = match complex_arg(ev, sheet, b) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match f((ar, ai), (br, bi)) {
        Some((r, i)) => Value::Text(format_complex(r, i, suffix)),
        None => Value::Error(ErrorValue::Div0),
    }
}

/// Fold a variadic list of complex numbers.
pub(crate) fn complex_fold(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: ComplexFold,
    identity: Complex,
) -> Value {
    if args.is_empty() {
        return Value::Error(ErrorValue::Value);
    }
    let mut acc = identity;
    // The first argument's suffix wins, so a sheet written in `j` stays in `j`.
    let mut suffix = 'i';
    for (n, arg) in args.iter().enumerate() {
        match complex_arg(ev, sheet, arg) {
            Ok((re, im, s)) => {
                if n == 0 {
                    suffix = s;
                }
                acc = f(acc, (re, im));
            }
            Err(e) => return e,
        }
    }
    Value::Text(format_complex(acc.0, acc.1, suffix))
}

pub(crate) fn eval_complex(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let [re, im] = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let suffix = match args.get(2) {
        Some(arg) => match ev.eval_expr(sheet, arg) {
            Value::Text(t) => match t.as_str() {
                "i" => 'i',
                "j" => 'j',
                // Only i and j are legal; anything else is a typo that would
                // otherwise produce a value nothing can parse back.
                _ => return Value::Error(ErrorValue::Value),
            },
            Value::Error(e) => return Value::Error(e),
            _ => return Value::Error(ErrorValue::Value),
        },
        None => 'i',
    };
    Value::Text(format_complex(re, im, suffix))
}

pub(crate) fn eval_impower(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let (re, im, suffix) = match complex_arg(ev, sheet, &args[0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let power = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let modulus = re.hypot(im);
    if modulus == 0.0 {
        return Value::Text(format_complex(0.0, 0.0, suffix));
    }
    // De Moivre: (r∠θ)^n = r^n ∠ nθ.
    let arg = im.atan2(re);
    let r = modulus.powf(power);
    let t = arg * power;
    Value::Text(format_complex(r * t.cos(), r * t.sin(), suffix))
}

// --- Incomplete gamma and beta ---------------------------------------------
//
// Every distribution below reduces to one of these two, so they are implemented
// once. Both use the standard series/continued-fraction split: the series
// converges quickly below the distribution's mean and stalls above it, and the
// continued fraction does the opposite. Using either alone is accurate over
// half its range and quietly wrong over the other half.

pub(crate) fn eval_bessel(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    name: &str,
    args: &[Expr],
) -> Value {
    let [x, order] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let n = order.trunc() as i32;
    if n < 0 || x < 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    // Y and K diverge at zero; J and I are defined there.
    if x == 0.0 && matches!(name, "BESSELY" | "BESSELK") {
        return Value::Error(ErrorValue::Num);
    }
    let v = match name {
        "BESSELJ" => bessel_j(n, x),
        "BESSELI" => bessel_i(n, x),
        "BESSELY" => bessel_y(n, x),
        _ => bessel_k(n, x),
    };
    if v.is_finite() {
        Value::Number(v)
    } else {
        Value::Error(ErrorValue::Num)
    }
}

/// `VDB` — declining-balance depreciation over an arbitrary span of periods,
/// switching to straight line once that gives more.
///
/// The switch is the whole point of the function and the thing `DDB` lacks:
/// declining balance never reaches the salvage value, so an asset depreciated
/// purely that way is still on the books at the end of its life. `no_switch`
/// turns it off for the jurisdictions that require pure declining balance.
///
/// Partial periods are handled by prorating the first and last, which is why
/// `start_period` and `end_period` are floats rather than counts.
pub(crate) fn eval_vdb(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 5 || args.len() > 7 {
        return Value::Error(ErrorValue::Value);
    }
    let v = match opt_numbers(ev, sheet, args, 5, [0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (cost, salvage, life, start, end, factor) = (v[0], v[1], v[2], v[3], v[4], v[5]);
    let no_switch = v[6] != 0.0;
    if cost < 0.0 || salvage < 0.0 || life <= 0.0 || start < 0.0 || end < start || factor <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }

    // Depreciation for one whole period index, given what is already written
    // off. Straight line is measured over the periods *remaining*, which is
    // what makes the two curves cross rather than run parallel.
    let period_amount = |index: f64, accumulated: f64| -> f64 {
        let book = cost - accumulated;
        let declining = (book * factor / life).min(book - salvage).max(0.0);
        if no_switch {
            return declining;
        }
        let remaining = life - index;
        let straight = if remaining > 0.0 {
            ((book - salvage) / remaining).max(0.0)
        } else {
            (book - salvage).max(0.0)
        };
        declining.max(straight).min((book - salvage).max(0.0))
    };

    // Walk whole periods, accumulating, and take the fraction of the first and
    // last that the requested span actually covers.
    let mut accumulated = 0.0;
    let mut total = 0.0;
    let last = end.ceil() as i64;
    for i in 0..last.max(0) {
        let idx = i as f64;
        let amount = period_amount(idx, accumulated);
        // How much of this period lies inside [start, end].
        let overlap = (end.min(idx + 1.0) - start.max(idx)).clamp(0.0, 1.0);
        total += amount * overlap;
        accumulated += amount;
    }
    Value::Number(total)
}

pub(crate) fn eval_convert(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let number = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let from = match ev.eval_expr(sheet, &args[1]).as_text() {
        Ok(t) => t,
        Err(e) => return Value::Error(e),
    };
    let to = match ev.eval_expr(sheet, &args[2]).as_text() {
        Ok(t) => t,
        Err(e) => return Value::Error(e),
    };

    if let Some(k) = temperature_to_kelvin(&from, number) {
        return match temperature_from_kelvin(&to, k) {
            Some(v) => Value::Number(v),
            // A temperature into anything else has no answer.
            None => Value::Error(ErrorValue::Na),
        };
    }
    let (Some((cat_from, f_from)), Some((cat_to, f_to))) =
        (convert_factor(&from), convert_factor(&to))
    else {
        return Value::Error(ErrorValue::Na);
    };
    if cat_from != cat_to {
        return Value::Error(ErrorValue::Na);
    }
    Value::Number(number * f_from / f_to)
}
