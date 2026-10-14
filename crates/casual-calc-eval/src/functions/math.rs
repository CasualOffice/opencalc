//! Arithmetic, rounding and the elementary maths functions.
//!
//! Split out of the single `functions.rs` under `MNT-002`; the section
//! headings that file already carried are the seams.

use super::*;

pub(crate) fn eval_product(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match flatten_numbers(ev, sheet, args) {
        Ok(ns) if ns.is_empty() => Value::Number(0.0),
        Ok(ns) => Value::Number(ns.iter().product()),
        Err(e) => Value::Error(e),
    }
}

#[derive(Clone, Copy)]
pub(crate) enum RoundDir {
    Up,
    Down,
}

/// `ROUNDUP`/`ROUNDDOWN`: round away from / toward zero to `digits` places.
pub(crate) fn eval_round_dir(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    dir: RoundDir,
) -> Value {
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
pub(crate) fn eval_trunc(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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
pub(crate) fn eval_ceiling_floor(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    up: bool,
) -> Value {
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

pub(crate) fn eval_sign(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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

/// ISEVEN / ISODD: truncate toward zero, then test parity.
pub(crate) fn eval_parity(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    even: bool,
) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => Value::Bool(((n.trunc() as i64).rem_euclid(2) == 0) == even),
        Err(e) => Value::Error(e),
    }
}

/// SUMPRODUCT: element-wise product of same-shaped arrays, then summed.
///
/// Every argument is read in array context, so a computed block counts as
/// much as a reference: `SUMPRODUCT(--(A1:A5>2))` and
/// `SUMPRODUCT((A1:A5>2)*A1:A5)` are the canonical conditional count and sum,
/// and reading only references collapsed them to one element (`XL-FEAT-002`).
///
/// Excel's rules, each one observable:
/// - the arrays must have the same *shape*, not merely the same element count
///   — a 5×1 against a 1×5 is `#VALUE!`;
/// - an entry that is not a number — text, blank, **and logicals** — counts as
///   0, which is why `SUMPRODUCT(A1:A5>2)` is 0 and the idiom needs `--`;
/// - an error anywhere is the answer.
pub(crate) fn eval_sumproduct(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() {
        return Value::Error(ErrorValue::Value);
    }
    let mut arrays: Vec<Vec<f64>> = Vec::with_capacity(args.len());
    let mut shape = None;
    let mut error = None;
    for arg in args {
        let value = ev.eval_expr_array(sheet, arg);
        let this = value.shape();
        if *shape.get_or_insert(this) != this {
            return Value::Error(ErrorValue::Value);
        }
        let cells = match value {
            Value::Array { cells, .. } => cells,
            other => vec![other],
        };
        let mut nums = Vec::with_capacity(cells.len());
        for v in cells {
            match v {
                Value::Number(n) => nums.push(n),
                Value::Error(e) => {
                    error.get_or_insert(e);
                    nums.push(0.0);
                }
                Value::Lambda(_) => return Value::Error(ErrorValue::Value),
                _ => nums.push(0.0),
            }
        }
        arrays.push(nums);
    }
    // Shape is checked over every argument before an error is reported, so a
    // mismatch is #VALUE! whichever argument carried the error.
    if let Some(e) = error {
        return Value::Error(e);
    }
    let len = arrays[0].len();
    let mut total = 0.0;
    for i in 0..len {
        total += arrays.iter().map(|c| c[i]).product::<f64>();
    }
    Value::Number(total)
}

/// A unary function that can fail. Excel answers with an error value where IEEE
/// arithmetic would produce NaN or an infinity, so the closure returns a
/// [`Value`] rather than an `f64`.
pub(crate) fn checked(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: fn(f64) -> Value,
) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    if args.len() != 1 {
        return Value::Error(ErrorValue::Value);
    }
    match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => f(n),
        Err(e) => Value::Error(e),
    }
}

/// A result outside the function's domain is `#NUM!`, which is what Excel
/// reports for `ASIN(2)` or `LN(-1)` where the maths yields NaN.
pub(crate) fn domain(v: f64) -> Value {
    if v.is_finite() {
        Value::Number(v)
    } else {
        Value::Error(ErrorValue::Num)
    }
}

/// A non-finite result becomes `err`; used where a zero denominator means the
/// answer is a division error rather than an infinity.
pub(crate) fn finite_or(v: f64, err: ErrorValue) -> Value {
    if v.is_finite() {
        Value::Number(v)
    } else {
        Value::Error(err)
    }
}

/// Round away from zero to the next multiple of `step`, preserving sign. Zero
/// stays zero: `EVEN(0)` is 0, not 2.
pub(crate) fn round_away_to(n: f64, step: f64) -> f64 {
    if n == 0.0 {
        return 0.0;
    }
    let scaled = (n.abs() / step).ceil() * step;
    if n < 0.0 { -scaled } else { scaled }
}

/// `ODD` rounds away from zero to the next odd integer; `ODD(0)` is 1.
pub(crate) fn eval_odd(n: f64) -> f64 {
    if n == 0.0 {
        return 1.0;
    }
    let up = ((n.abs() + 1.0) / 2.0).ceil() * 2.0 - 1.0;
    if n < 0.0 { -up } else { up }
}

/// `n!` for a non-negative integer. Negative input is `#NUM!`; anything past
/// 170 overflows an `f64`, which Excel also reports as `#NUM!` rather than
/// returning an infinity.
pub(crate) fn factorial(n: f64) -> Value {
    if !(0.0..=170.0).contains(&n) {
        return Value::Error(ErrorValue::Num);
    }
    let mut acc = 1.0f64;
    for i in 2..=(n as u64) {
        acc *= i as f64;
    }
    Value::Number(acc)
}

/// The double factorial `n!!` — every other term down to 1 or 2.
pub(crate) fn factorial_double(n: f64) -> Value {
    let n = n.trunc();
    if n < -1.0 {
        return Value::Error(ErrorValue::Num);
    }
    let mut acc = 1.0f64;
    let mut i = n;
    while i > 1.0 {
        acc *= i;
        i -= 2.0;
        if !acc.is_finite() {
            return Value::Error(ErrorValue::Num);
        }
    }
    Value::Number(acc)
}

/// `ATAN2(x, y)`. OOXML orders the arguments x-then-y, the reverse of the
/// `atan2(y, x)` every maths library uses; swapping them here is the whole
/// point of the function existing separately.
pub(crate) fn eval_atan2(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [x, y] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if x == 0.0 && y == 0.0 {
        return Value::Error(ErrorValue::Div0);
    }
    Value::Number(y.atan2(x))
}

/// `LOG(number, [base])`, base 10 when omitted.
pub(crate) fn eval_log(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let n = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let base = if args.len() == 2 {
        match ev.eval_expr(sheet, &args[1]).as_number() {
            Ok(b) => b,
            Err(e) => return Value::Error(e),
        }
    } else {
        10.0
    };
    if n <= 0.0 || base <= 0.0 || base == 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    domain(n.log(base))
}

/// `QUOTIENT` — the integer part of a division, discarding the remainder.
pub(crate) fn eval_quotient(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match pair_of_numbers(ev, sheet, args) {
        Ok([_, 0.0]) => Value::Error(ErrorValue::Div0),
        Ok([a, b]) => Value::Number((a / b).trunc()),
        Err(e) => e,
    }
}

/// `MROUND` — round to the nearest multiple. Excel requires the number and the
/// multiple to share a sign, and reports `#NUM!` when they do not.
pub(crate) fn eval_mround(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match pair_of_numbers(ev, sheet, args) {
        Ok([n, m]) => {
            if m == 0.0 {
                return Value::Number(0.0);
            }
            if n.signum() != m.signum() && n != 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number((n / m).round() * m)
        }
        Err(e) => e,
    }
}

/// `COMBIN(n, k)`, or `COMBINA` for combinations with repetition.
pub(crate) fn eval_combin(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    repeat: bool,
) -> Value {
    match pair_of_numbers(ev, sheet, args) {
        Ok([n, k]) => {
            let (n, k) = (n.trunc(), k.trunc());
            if n < 0.0 || k < 0.0 || (!repeat && k > n) {
                return Value::Error(ErrorValue::Num);
            }
            // COMBINA(n, k) = COMBIN(n + k - 1, k).
            let (n, k) = if repeat { (n + k - 1.0, k) } else { (n, k) };
            binomial(n, k)
        }
        Err(e) => e,
    }
}

/// `n choose k`, accumulated term by term so the intermediate products stay
/// representable — computing `n!/(k!(n-k)!)` directly overflows well before the
/// result does.
pub(crate) fn binomial(n: f64, k: f64) -> Value {
    if k > n {
        return Value::Number(0.0);
    }
    let k = k.min(n - k);
    let mut acc = 1.0f64;
    let mut i = 0.0;
    while i < k {
        acc = acc * (n - i) / (i + 1.0);
        i += 1.0;
    }
    if acc.is_finite() {
        Value::Number(acc.round())
    } else {
        Value::Error(ErrorValue::Num)
    }
}

/// `PERMUT(n, k)`, or `PERMUTATIONA` for permutations with repetition.
pub(crate) fn eval_permut(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    repeat: bool,
) -> Value {
    match pair_of_numbers(ev, sheet, args) {
        Ok([n, k]) => {
            let (n, k) = (n.trunc(), k.trunc());
            if n < 0.0 || k < 0.0 || (!repeat && k > n) {
                return Value::Error(ErrorValue::Num);
            }
            if repeat {
                return finite_or(n.powf(k), ErrorValue::Num);
            }
            let mut acc = 1.0f64;
            let mut i = 0.0;
            while i < k {
                acc *= n - i;
                i += 1.0;
            }
            finite_or(acc, ErrorValue::Num)
        }
        Err(e) => e,
    }
}

/// `GCD` / `LCM` over every number in the arguments. Both are defined on
/// non-negative integers, and the fractional part is truncated as Excel does.
pub(crate) fn eval_gcd_lcm(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    gcd_mode: bool,
) -> Value {
    let numbers = match flatten_numbers(ev, sheet, args) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    if numbers.is_empty() {
        return Value::Error(ErrorValue::Value);
    }
    let mut acc: u64 = if gcd_mode { 0 } else { 1 };
    for n in numbers {
        if n < 0.0 {
            return Value::Error(ErrorValue::Num);
        }
        let v = n.trunc() as u64;
        acc = if gcd_mode {
            gcd(acc, v)
        } else if v == 0 {
            return Value::Number(0.0);
        } else {
            match (acc / gcd(acc, v)).checked_mul(v) {
                Some(l) => l,
                None => return Value::Error(ErrorValue::Num),
            }
        };
    }
    Value::Number(acc as f64)
}

pub(crate) fn gcd(a: u64, b: u64) -> u64 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// `MULTINOMIAL` — `(Σx)! / Πx!`, built up term by term to avoid overflowing on
/// the factorials when the result itself is small.
pub(crate) fn eval_multinomial(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let numbers = match flatten_numbers(ev, sheet, args) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    let mut acc = 1.0f64;
    let mut running = 0.0f64;
    for n in numbers {
        if n < 0.0 {
            return Value::Error(ErrorValue::Num);
        }
        let n = n.trunc();
        running += n;
        match binomial(running, n) {
            Value::Number(c) => acc *= c,
            other => return other,
        }
    }
    finite_or(acc, ErrorValue::Num)
}

/// `SERIESSUM(x, n, m, coefficients)` — the power series
/// `Σ coefficient_i · x^(n + i·m)`.
pub(crate) fn eval_seriessum(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 4 {
        return Value::Error(ErrorValue::Value);
    }
    let mut scalars = [0.0f64; 3];
    for (i, slot) in scalars.iter_mut().enumerate() {
        match ev.eval_expr(sheet, &args[i]).as_number() {
            Ok(v) => *slot = v,
            Err(e) => return Value::Error(e),
        }
    }
    let [x, n, m] = scalars;
    let coefficients = match flatten_numbers(ev, sheet, &args[3..]) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    let mut total = 0.0f64;
    for (i, c) in coefficients.iter().enumerate() {
        total += c * x.powf(n + (i as f64) * m);
    }
    finite_or(total, ErrorValue::Num)
}

/// Evaluate exactly two arguments as numbers, or the error to report instead.
pub(crate) fn pair_of_numbers(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
) -> Result<[f64; 2], Value> {
    if args.len() != 2 {
        return Err(Value::Error(ErrorValue::Value));
    }
    let a = ev
        .eval_expr(sheet, &args[0])
        .as_number()
        .map_err(Value::Error)?;
    let b = ev
        .eval_expr(sheet, &args[1])
        .as_number()
        .map_err(Value::Error)?;
    Ok([a, b])
}

// --- Logical and information helpers ---------------------------------------

/// `ISO.CEILING` and `ECMA.CEILING`. They agree on positives and differ on
/// negatives: ISO rounds toward positive infinity, ECMA away from zero.
pub(crate) fn eval_ceiling_variant(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    iso: bool,
) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let n = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let step = match args.get(1) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(v) => v,
            Err(e) => return Value::Error(e),
        },
        None => 1.0,
    };
    if step == 0.0 {
        return Value::Number(0.0);
    }
    let step = step.abs();
    Value::Number(if iso || n >= 0.0 {
        (n / step).ceil() * step
    } else {
        -((-n / step).ceil() * step)
    })
}

/// `CUMIPMT` / `CUMPRINC` — the interest or principal paid across a span of
/// periods, summed from the per-period figures so the two always agree with
/// PMT rather than being derived independently.
pub(crate) fn eval_cumulative(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    interest: bool,
) -> Value {
    if args.len() != 6 {
        return Value::Error(ErrorValue::Value);
    }
    let mut v = [0.0f64; 6];
    for (i, slot) in v.iter_mut().enumerate() {
        match ev.eval_expr(sheet, &args[i]).as_number() {
            Ok(n) => *slot = n,
            Err(e) => return Value::Error(e),
        }
    }
    let [rate, nper, pv, start, end, kind] = v;
    if rate <= 0.0 || nper <= 0.0 || pv <= 0.0 || start < 1.0 || end < start || end > nper {
        return Value::Error(ErrorValue::Num);
    }
    let Some(payment) = eval_pmt_values(rate, nper, pv, 0.0, kind) else {
        return Value::Error(ErrorValue::Num);
    };
    let mut total = 0.0;
    for per in (start as u64)..=(end as u64) {
        let per = per as f64;
        let (growth, factor) = annuity_factor(rate, per - 1.0);
        let balance = pv * growth + payment * due_factor(rate, kind) * factor;
        let mut part = -balance * rate;
        if kind != 0.0 {
            part = if per == 1.0 { 0.0 } else { part / (1.0 + rate) };
        }
        total += if interest { part } else { payment - part };
    }
    Value::Number(total)
}

/// `CEILING.MATH` / `FLOOR.MATH` (with `mode`) and `CEILING.PRECISE` /
/// `FLOOR.PRECISE` (without): round to a multiple of `|significance|`.
///
/// Unlike `CEILING`/`FLOOR` none of them refuses a mismatched sign — the
/// significance's sign is simply ignored — and a zero significance is 0. They
/// differ only on a **negative** number: the plain direction is toward +∞ for
/// the ceiling and −∞ for the floor, and a non-zero `mode` (the `.MATH`
/// functions only) flips a negative to round away from zero for the ceiling
/// and toward it for the floor.
pub(crate) fn eval_round_to_multiple(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    up: bool,
    has_mode: bool,
) -> Value {
    let max = if has_mode { 3 } else { 2 };
    if args.is_empty() || args.len() > max {
        return Value::Error(ErrorValue::Value);
    }
    let mut read = |i: usize, default: f64| -> Result<f64, ErrorValue> {
        match args.get(i) {
            None | Some(Expr::Empty) => Ok(default),
            Some(a) => ev.eval_expr(sheet, a).as_number(),
        }
    };
    let (n, step, mode) = match (read(0, 0.0), read(1, 1.0), read(2, 0.0)) {
        (Ok(n), Ok(s), Ok(m)) => (n, s.abs(), m),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => return Value::Error(e),
    };
    if step == 0.0 || n == 0.0 {
        return Value::Number(0.0);
    }
    // A negative number with a non-zero mode goes the other way: the ceiling
    // away from zero, the floor toward it.
    let up = if n < 0.0 && mode != 0.0 { !up } else { up };
    let q = n / step;
    // A quotient within a rounding error of an integer *is* that integer:
    // `CEILING.MATH(0.3, 0.1)` must not become 0.4 because 0.3/0.1 is
    // 2.9999999999999996.
    let nearest = q.round();
    let q = if (q - nearest).abs() < 1e-12 * nearest.abs().max(1.0) {
        nearest
    } else if up {
        q.ceil()
    } else {
        q.floor()
    };
    finite_or(q * step, ErrorValue::Num)
}

/// `ARABIC(text)` — a Roman numeral as a number; the inverse of `ROMAN`.
///
/// Microsoft's rules, which are looser than `ROMAN` writes: case is ignored,
/// leading and trailing spaces are ignored, a leading minus makes it negative,
/// `""` is 0, and the reading is subtractive — a numeral smaller than the one
/// after it is subtracted — so the non-standard `IIII` (4) and `IC` (99) read
/// as they look. Anything else, a number included, is `#VALUE!`, as is text
/// past 255 characters.
pub(crate) fn eval_arabic(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [arg] = args else {
        return Value::Error(ErrorValue::Value);
    };
    let text = match ev.eval_expr(sheet, arg) {
        Value::Text(t) => t,
        Value::Empty => String::new(),
        Value::Error(e) => return Value::Error(e),
        // A number or a logical is not a numeral, even one that would print
        // as digits.
        _ => return Value::Error(ErrorValue::Value),
    };
    let trimmed = text.trim_matches(' ');
    if trimmed.chars().count() > 255 {
        return Value::Error(ErrorValue::Value);
    }
    let (negative, body) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed),
    };
    let mut values = Vec::with_capacity(body.len());
    for c in body.chars() {
        let v = match c.to_ascii_uppercase() {
            'I' => 1.0,
            'V' => 5.0,
            'X' => 10.0,
            'L' => 50.0,
            'C' => 100.0,
            'D' => 500.0,
            'M' => 1000.0,
            _ => return Value::Error(ErrorValue::Value),
        };
        values.push(v);
    }
    let total: f64 = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            if values.get(i + 1).is_some_and(|next| next > v) {
                -v
            } else {
                *v
            }
        })
        .sum();
    Value::Number(if negative { -total } else { total })
}
