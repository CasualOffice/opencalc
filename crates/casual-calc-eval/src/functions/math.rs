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

/// SUMPRODUCT: element-wise product of equal-length arrays, then summed.
pub(crate) fn eval_sumproduct(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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
