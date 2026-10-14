//! The incomplete gamma and beta functions.
//!
//! Kept apart from `stats` because they are numeric kernels rather than
//! statistics: several distributions are defined in terms of them, and their
//! accuracy is the accuracy of everything built on top.
//!
//! Split out of the single `functions.rs` under `MNT-002`; the section
//! headings that file already carried are the seams.

use super::*;

pub(crate) const GAMMA_ITERATIONS: usize = 300;
pub(crate) const GAMMA_EPSILON: f64 = 1e-15;

/// The regularized lower incomplete gamma `P(a, x)`.
pub(crate) fn gamma_p(a: f64, x: f64) -> f64 {
    if x <= 0.0 || a <= 0.0 {
        return 0.0;
    }
    if x < a + 1.0 {
        // Series representation.
        let mut term = 1.0 / a;
        let mut sum = term;
        let mut n = a;
        for _ in 0..GAMMA_ITERATIONS {
            n += 1.0;
            term *= x / n;
            sum += term;
            if term.abs() < sum.abs() * GAMMA_EPSILON {
                break;
            }
        }
        sum * (-x + a * x.ln() - ln_gamma(a)).exp()
    } else {
        1.0 - gamma_q_cf(a, x)
    }
}

/// The regularized upper incomplete gamma `Q(a, x)`, by continued fraction.
pub(crate) fn gamma_q_cf(a: f64, x: f64) -> f64 {
    let tiny = 1e-300;
    let mut b = x + 1.0 - a;
    let mut c = 1.0 / tiny;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..GAMMA_ITERATIONS {
        let an = -(i as f64) * (i as f64 - a);
        b += 2.0;
        d = an * d + b;
        if d.abs() < tiny {
            d = tiny;
        }
        c = b + an / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < GAMMA_EPSILON {
            break;
        }
    }
    h * (-x + a * x.ln() - ln_gamma(a)).exp()
}

/// The regularized incomplete beta `I_x(a, b)`.
pub(crate) fn beta_i(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let front =
        (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    // The continued fraction converges only for x below the distribution's
    // mode; above it, the symmetry I_x(a,b) = 1 - I_(1-x)(b,a) moves the
    // argument back into the converging half.
    if x < (a + 1.0) / (a + b + 2.0) {
        front * beta_cf(a, b, x) / a
    } else {
        1.0 - front * beta_cf(b, a, 1.0 - x) / b
    }
}

pub(crate) fn beta_cf(a: f64, b: f64, x: f64) -> f64 {
    let tiny = 1e-300;
    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < tiny {
        d = tiny;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..GAMMA_ITERATIONS {
        let m = m as f64;
        let m2 = 2.0 * m;
        // Even step.
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = 1.0 + aa / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        h *= d * c;
        // Odd step.
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = 1.0 + aa / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < GAMMA_EPSILON {
            break;
        }
    }
    h
}

/// Invert a monotone CDF by bisection.
///
/// The distributions below have no closed-form inverse, and bisection over a
/// bracket that is grown until it contains the root converges for all of them —
/// where a fixed bracket would silently return its own endpoint for extreme
/// probabilities.
pub(crate) fn invert_cdf(p: f64, cdf: impl Fn(f64) -> f64) -> Option<f64> {
    if !(0.0..=1.0).contains(&p) {
        return None;
    }
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    let mut guard = 0;
    while cdf(hi) < p {
        hi *= 2.0;
        guard += 1;
        if guard > 200 || !hi.is_finite() {
            return None;
        }
    }
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        if cdf(mid) < p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some((lo + hi) / 2.0)
}

/// `CHIDIST` is the **upper** tail, unlike almost every other `*DIST`.
pub(crate) fn eval_chidist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [x, df] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if x < 0.0 || df < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(1.0 - gamma_p(df / 2.0, x / 2.0))
}

pub(crate) fn eval_chiinv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [p, df] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !(0.0..=1.0).contains(&p) || df < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    // CHIINV inverts the upper tail, matching CHIDIST.
    match invert_cdf(1.0 - p, |x| gamma_p(df / 2.0, x / 2.0)) {
        Some(v) => Value::Number(v),
        None => Value::Error(ErrorValue::Num),
    }
}

/// The Student's t CDF.
pub(crate) fn t_cdf(t: f64, df: f64) -> f64 {
    let x = df / (df + t * t);
    let half = 0.5 * beta_i(df / 2.0, 0.5, x);
    if t > 0.0 { 1.0 - half } else { half }
}

/// `TDIST(x, df, tails)` — the legacy form, which takes only positive `x` and
/// reports a tail probability rather than a CDF.
pub(crate) fn eval_tdist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [x, df, tails] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if x < 0.0 || df < 1.0 || !(tails == 1.0 || tails == 2.0) {
        return Value::Error(ErrorValue::Num);
    }
    let upper = 1.0 - t_cdf(x, df);
    Value::Number(upper * tails)
}

pub(crate) fn eval_tinv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [p, df] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !(0.0..=1.0).contains(&p) || df < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    // TINV is two-tailed, so it inverts against 1 - p/2.
    match invert_cdf(1.0 - p / 2.0, |x| t_cdf(x, df)) {
        Some(v) => Value::Number(v),
        None => Value::Error(ErrorValue::Num),
    }
}

/// `FDIST` is the upper tail, like CHIDIST.
pub(crate) fn eval_fdist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [x, d1, d2] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if x < 0.0 || d1 < 1.0 || d2 < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(1.0 - f_cdf(x, d1, d2))
}

pub(crate) fn f_cdf(x: f64, d1: f64, d2: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    beta_i(d1 / 2.0, d2 / 2.0, d1 * x / (d1 * x + d2))
}

pub(crate) fn eval_finv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [p, d1, d2] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if !(0.0..=1.0).contains(&p) || d1 < 1.0 || d2 < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    match invert_cdf(1.0 - p, |x| f_cdf(x, d1, d2)) {
        Some(v) => Value::Number(v),
        None => Value::Error(ErrorValue::Num),
    }
}

pub(crate) fn eval_gammadist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 4 {
        return Value::Error(ErrorValue::Value);
    }
    let Some(v) = three_numbers(ev, sheet, &args[..3]) else {
        return Value::Error(ErrorValue::Value);
    };
    let [x, alpha, beta] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let cumulative = match ev.eval_expr(sheet, &args[3]).as_bool() {
        Ok(b) => b,
        Err(e) => return Value::Error(e),
    };
    if x < 0.0 || alpha <= 0.0 || beta <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(if cumulative {
        gamma_p(alpha, x / beta)
    } else {
        ((alpha - 1.0) * (x / beta).ln() - x / beta - ln_gamma(alpha)).exp() / beta
    })
}

pub(crate) fn eval_gammainv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [p, alpha, beta] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if !(0.0..=1.0).contains(&p) || alpha <= 0.0 || beta <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    match invert_cdf(p, |x| gamma_p(alpha, x / beta)) {
        Some(v) => Value::Number(v),
        None => Value::Error(ErrorValue::Num),
    }
}

pub(crate) fn eval_betadist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 || args.len() > 5 {
        return Value::Error(ErrorValue::Value);
    }
    let [x, alpha, beta, lo, hi] = match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0, 1.0])
    {
        Ok(v) => v,
        Err(e) => return e,
    };
    if alpha <= 0.0 || beta <= 0.0 || hi <= lo || x < lo || x > hi {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(beta_i(alpha, beta, (x - lo) / (hi - lo)))
}

pub(crate) fn eval_betainv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 || args.len() > 5 {
        return Value::Error(ErrorValue::Value);
    }
    let [p, alpha, beta, lo, hi] = match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0, 1.0])
    {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !(0.0..=1.0).contains(&p) || alpha <= 0.0 || beta <= 0.0 || hi <= lo {
        return Value::Error(ErrorValue::Num);
    }
    // The beta CDF lives on 0..1, so the bracket is known and bisection is
    // direct rather than needing the growing bracket the others use.
    let (mut a, mut b) = (0.0f64, 1.0f64);
    for _ in 0..200 {
        let mid = (a + b) / 2.0;
        if beta_i(alpha, beta, mid) < p {
            a = mid;
        } else {
            b = mid;
        }
    }
    Value::Number(lo + (hi - lo) * (a + b) / 2.0)
}

// --- The 2010 distribution names (XL-FEAT-013) -----------------------------
//
// Excel 2010 renamed the distributions and, in renaming them, changed what
// several of them answer: `CHIDIST`, `FDIST` and `TDIST` are upper tails, while
// `CHISQ.DIST`, `F.DIST` and `T.DIST` are the ordinary left-tailed CDF *or*
// density, chosen by a required `cumulative` flag. Aliasing the new names to
// the old functions would have answered the wrong tail without any visible
// error, so each is its own function over the same kernels.
//
// The degrees of freedom truncate, as Excel's documentation says for each of
// these; below 1 after truncation is `#NUM!`.

/// `N` numbers followed by a required logical `cumulative`.
pub(crate) fn numbers_then_flag<const N: usize>(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
) -> Result<([f64; N], bool), ErrorValue> {
    if args.len() != N + 1 {
        return Err(ErrorValue::Value);
    }
    let mut out = [0.0; N];
    for (slot, arg) in out.iter_mut().zip(args) {
        *slot = ev.eval_expr(sheet, arg).as_number()?;
    }
    let flag = ev.eval_expr(sheet, &args[N]).as_bool()?;
    Ok((out, flag))
}

/// A degrees-of-freedom argument: truncated, and at least 1.
fn degrees(df: f64) -> Option<f64> {
    let df = df.trunc();
    (df >= 1.0).then_some(df)
}

/// A finite result, or `#NUM!`.
fn number_or_num(v: f64) -> Value {
    if v.is_finite() {
        Value::Number(v)
    } else {
        Value::Error(ErrorValue::Num)
    }
}

/// The Student's t density.
pub(crate) fn t_pdf(x: f64, df: f64) -> f64 {
    (ln_gamma((df + 1.0) / 2.0)
        - ln_gamma(df / 2.0)
        - 0.5 * (df * std::f64::consts::PI).ln()
        - (df + 1.0) / 2.0 * (x * x / df).ln_1p())
    .exp()
}

/// The probability beyond `|t|` in one tail of the t distribution — computed
/// directly from the incomplete beta rather than as `1 - CDF`, which would
/// lose every significant digit of a small tail.
pub(crate) fn t_upper_tail(t: f64, df: f64) -> f64 {
    0.5 * beta_i(df / 2.0, 0.5, df / (df + t * t))
}

/// `T.DIST(x, df, cumulative)` — the left-tailed CDF or the density.
pub(crate) fn eval_t_dist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let ([x, df], cumulative) = match numbers_then_flag::<2>(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let Some(df) = degrees(df) else {
        return Value::Error(ErrorValue::Num);
    };
    number_or_num(if cumulative {
        t_cdf(x, df)
    } else {
        t_pdf(x, df)
    })
}

/// `T.DIST.2T(x, df)` (both tails, `x ≥ 0`) and `T.DIST.RT(x, df)` (right
/// tail, any `x`).
pub(crate) fn eval_t_dist_tail(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    two_tailed: bool,
) -> Value {
    let [x, df] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(df) = degrees(df) else {
        return Value::Error(ErrorValue::Num);
    };
    if two_tailed && x < 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    let tail = t_upper_tail(x, df);
    number_or_num(if two_tailed {
        2.0 * tail
    } else if x >= 0.0 {
        tail
    } else {
        1.0 - tail
    })
}

/// `T.INV(p, df)` — the left-tailed inverse, so negative below one half.
pub(crate) fn eval_t_inv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [p, df] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(df) = degrees(df) else {
        return Value::Error(ErrorValue::Num);
    };
    if p <= 0.0 || p >= 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    t_quantile(p, df).map_or(Value::Error(ErrorValue::Num), Value::Number)
}

/// The t quantile, by symmetry from the upper half `invert_cdf` searches.
pub(crate) fn t_quantile(p: f64, df: f64) -> Option<f64> {
    if p == 0.5 {
        return Some(0.0);
    }
    if p > 0.5 {
        invert_cdf(p, |x| t_cdf(x, df))
    } else {
        invert_cdf(1.0 - p, |x| t_cdf(x, df)).map(|x| -x)
    }
}

/// `F.DIST(x, d1, d2, cumulative)`.
pub(crate) fn eval_f_dist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let ([x, d1, d2], cumulative) = match numbers_then_flag::<3>(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let (Some(d1), Some(d2)) = (degrees(d1), degrees(d2)) else {
        return Value::Error(ErrorValue::Num);
    };
    if x < 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    if cumulative {
        return number_or_num(f_cdf(x, d1, d2));
    }
    if x == 0.0 {
        // The density's limit at zero: unbounded for d1 = 1, 1 for d1 = 2,
        // and 0 above.
        return match d1 {
            1.0 => Value::Error(ErrorValue::Num),
            2.0 => Value::Number(1.0),
            _ => Value::Number(0.0),
        };
    }
    let ln_beta = ln_gamma(d1 / 2.0) + ln_gamma(d2 / 2.0) - ln_gamma((d1 + d2) / 2.0);
    let ln = 0.5 * (d1 * (d1 * x).ln() + d2 * d2.ln() - (d1 + d2) * (d1 * x + d2).ln())
        - x.ln()
        - ln_beta;
    number_or_num(ln.exp())
}

/// `F.INV(p, d1, d2)` — the left-tailed inverse.
pub(crate) fn eval_f_inv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [p, d1, d2] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let (Some(d1), Some(d2)) = (degrees(d1), degrees(d2)) else {
        return Value::Error(ErrorValue::Num);
    };
    // p = 1 is the point at infinity: `#NUM!`, where bisection would stop
    // wherever the CDF first rounds to 1.
    if !(0.0..1.0).contains(&p) {
        return Value::Error(ErrorValue::Num);
    }
    if p == 0.0 {
        return Value::Number(0.0);
    }
    invert_cdf(p, |x| f_cdf(x, d1, d2)).map_or(Value::Error(ErrorValue::Num), Value::Number)
}

/// `CHISQ.DIST(x, df, cumulative)`.
pub(crate) fn eval_chisq_dist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let ([x, df], cumulative) = match numbers_then_flag::<2>(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let Some(df) = degrees(df) else {
        return Value::Error(ErrorValue::Num);
    };
    if x < 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    let k = df / 2.0;
    if cumulative {
        return number_or_num(gamma_p(k, x / 2.0));
    }
    if x == 0.0 {
        // As for F: unbounded for one degree of freedom, ½ for two, else 0.
        return match df {
            1.0 => Value::Error(ErrorValue::Num),
            2.0 => Value::Number(0.5),
            _ => Value::Number(0.0),
        };
    }
    number_or_num(((k - 1.0) * x.ln() - x / 2.0 - k * std::f64::consts::LN_2 - ln_gamma(k)).exp())
}

/// `CHISQ.INV(p, df)` — the left-tailed inverse.
pub(crate) fn eval_chisq_inv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [p, df] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(df) = degrees(df) else {
        return Value::Error(ErrorValue::Num);
    };
    // As F.INV: p = 1 is infinite, not wherever the CDF rounds to 1.
    if !(0.0..1.0).contains(&p) {
        return Value::Error(ErrorValue::Num);
    }
    if p == 0.0 {
        return Value::Number(0.0);
    }
    invert_cdf(p, |x| gamma_p(df / 2.0, x / 2.0))
        .map_or(Value::Error(ErrorValue::Num), Value::Number)
}

/// `BETA.DIST(x, alpha, beta, cumulative, [A], [B])`.
///
/// Unlike `BETADIST` the flag is required and selects the density.
pub(crate) fn eval_beta_dist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 4 || args.len() > 6 {
        return Value::Error(ErrorValue::Value);
    }
    let ([x, alpha, beta], cumulative) = match numbers_then_flag::<3>(ev, sheet, &args[..4]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let mut bound = |i: usize, default: f64| match args.get(i) {
        None | Some(Expr::Empty) => Ok(default),
        Some(a) => ev.eval_expr(sheet, a).as_number(),
    };
    let (lo, hi) = match (bound(4, 0.0), bound(5, 1.0)) {
        (Ok(lo), Ok(hi)) => (lo, hi),
        (Err(e), _) | (_, Err(e)) => return Value::Error(e),
    };
    if alpha <= 0.0 || beta <= 0.0 || hi <= lo || x < lo || x > hi {
        return Value::Error(ErrorValue::Num);
    }
    let z = (x - lo) / (hi - lo);
    if cumulative {
        return number_or_num(beta_i(alpha, beta, z));
    }
    // `(α-1)·ln z` is 0·−∞ at an edge when α is 1; the factor is 1 there.
    let part = |exponent: f64, base: f64| {
        if exponent == 0.0 {
            0.0
        } else {
            exponent * base.ln()
        }
    };
    let ln_beta = ln_gamma(alpha) + ln_gamma(beta) - ln_gamma(alpha + beta);
    number_or_num((part(alpha - 1.0, z) + part(beta - 1.0, 1.0 - z) - ln_beta).exp() / (hi - lo))
}

// --- Statistical tests -----------------------------------------------------
