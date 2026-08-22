//! Descriptive statistics, distributions, regression and the significance
//! tests.
//!
//! Split out of the single `functions.rs` under `MNT-002`; the section
//! headings that file already carried are the seams.

use super::*;

/// MEDIAN over all numeric arguments.
pub(crate) fn eval_median(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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
pub(crate) fn eval_large_small(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    large: bool,
) -> Value {
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
pub(crate) fn eval_rank(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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
pub(crate) fn eval_stdev(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    sample: bool,
) -> Value {
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

/// Run `f` over the flattened numeric arguments. An empty sample, or an `f`
/// returning `None`, is `#NUM!` — the value a statistic has no meaning for.
pub(crate) fn stat_over(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: fn(&[f64]) -> Option<f64>,
) -> Value {
    match flatten_numbers(ev, sheet, args) {
        Ok(ns) if ns.is_empty() => Value::Error(ErrorValue::Num),
        Ok(ns) => match f(&ns) {
            Some(v) if v.is_finite() => Value::Number(v),
            _ => Value::Error(ErrorValue::Num),
        },
        Err(e) => Value::Error(e),
    }
}

pub(crate) fn mean(ns: &[f64]) -> f64 {
    ns.iter().sum::<f64>() / ns.len() as f64
}

/// Variance, sample (`n-1`) or population (`n`).
///
/// The divisor is the whole difference between VAR and VARP, and using the
/// wrong one gives an answer close enough to pass a glance on any large sample.
pub(crate) fn variance(ns: &[f64], sample: bool) -> Option<f64> {
    let n = ns.len();
    if sample && n < 2 {
        return None;
    }
    let m = mean(ns);
    let sum: f64 = ns.iter().map(|x| (x - m).powi(2)).sum();
    Some(sum / if sample { (n - 1) as f64 } else { n as f64 })
}

/// The most frequent value, or `None` when every value occurs once — Excel
/// reports `#N/A` for that, not the first value.
pub(crate) fn mode_of(ns: &[f64]) -> Option<f64> {
    let mut best: Option<(f64, usize)> = None;
    for candidate in ns {
        let count = ns.iter().filter(|n| *n == candidate).count();
        if count > best.map_or(0, |(_, c)| c) {
            best = Some((*candidate, count));
        }
    }
    best.filter(|(_, count)| *count > 1).map(|(v, _)| v)
}

pub(crate) fn skew_of(ns: &[f64]) -> Option<f64> {
    let n = ns.len();
    if n < 3 {
        return None;
    }
    let m = mean(ns);
    let sd = variance(ns, true)?.sqrt();
    if sd == 0.0 {
        return None;
    }
    let n = n as f64;
    let sum: f64 = ns.iter().map(|x| ((x - m) / sd).powi(3)).sum();
    Some(n / ((n - 1.0) * (n - 2.0)) * sum)
}

pub(crate) fn kurt_of(ns: &[f64]) -> Option<f64> {
    let count = ns.len();
    if count < 4 {
        return None;
    }
    let m = mean(ns);
    let sd = variance(ns, true)?.sqrt();
    if sd == 0.0 {
        return None;
    }
    let n = count as f64;
    let sum: f64 = ns.iter().map(|x| ((x - m) / sd).powi(4)).sum();
    Some(
        n * (n + 1.0) / ((n - 1.0) * (n - 2.0) * (n - 3.0)) * sum
            - 3.0 * (n - 1.0).powi(2) / ((n - 2.0) * (n - 3.0)),
    )
}

/// `PERCENTILE(array, k)`, or `QUARTILE(array, q)` with `q` in 0..=4.
///
/// Linear interpolation between order statistics, which is what Excel's
/// inclusive percentile does; a nearest-rank implementation disagrees on most
/// samples.
pub(crate) fn eval_percentile(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    quartile: bool,
) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let mut ns = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    let k = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => {
            if quartile {
                if !(0.0..=4.0).contains(&n) {
                    return Value::Error(ErrorValue::Num);
                }
                n.trunc() / 4.0
            } else {
                n
            }
        }
        Err(e) => return Value::Error(e),
    };
    if ns.is_empty() || !(0.0..=1.0).contains(&k) {
        return Value::Error(ErrorValue::Num);
    }
    ns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    Value::Number(percentile_of(&ns, k))
}

pub(crate) fn percentile_of(sorted: &[f64], k: f64) -> f64 {
    let position = k * (sorted.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        return sorted[lower];
    }
    sorted[lower] + (position - lower as f64) * (sorted[upper] - sorted[lower])
}

/// `PERCENTRANK(array, x, [significance])` — the inverse of PERCENTILE.
pub(crate) fn eval_percentrank(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let mut ns = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    let x = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    if ns.is_empty() {
        return Value::Error(ErrorValue::Num);
    }
    ns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    if x < ns[0] || x > ns[ns.len() - 1] {
        return Value::Error(ErrorValue::Na);
    }
    let below = ns.iter().filter(|n| **n < x).count() as f64;
    let equal = ns.iter().filter(|n| **n == x).count() as f64;
    let rank = if equal > 0.0 {
        below / (ns.len() - 1) as f64
    } else {
        // Between two observations: interpolate, as Excel does.
        let lower = ns.iter().rev().find(|n| **n < x).copied().unwrap_or(ns[0]);
        let upper = ns.iter().find(|n| **n > x).copied().unwrap_or(x);
        let base = ns.iter().filter(|n| **n < x).count() as f64 - 1.0;
        (base + (x - lower) / (upper - lower)) / (ns.len() - 1) as f64
    };
    let digits = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n.trunc() as i32,
            Err(e) => return Value::Error(e),
        },
        None => 3,
    };
    let factor = 10f64.powi(digits);
    // Truncated, not rounded: PERCENTRANK reports significant digits rather
    // than a rounded value.
    Value::Number((rank * factor).trunc() / factor)
}

/// `TRIMMEAN(array, percent)` — the mean after discarding the extremes.
pub(crate) fn eval_trimmean(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let mut ns = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    let percent = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    if ns.is_empty() || !(0.0..1.0).contains(&percent) {
        return Value::Error(ErrorValue::Num);
    }
    ns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    // The count to drop is rounded *down to an even number*, so the same many
    // are discarded from each end.
    let drop = ((ns.len() as f64 * percent / 2.0).floor() as usize) * 2;
    let keep = &ns[drop / 2..ns.len() - drop / 2];
    if keep.is_empty() {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(mean(keep))
}

/// `COUNTBLANK(range)` — cells with no content, which is not the same as cells
/// holding an empty string.
pub(crate) fn eval_countblank(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let Some((target, cells)) = range_cells(ev, sheet, arg) else {
        return Value::Error(ErrorValue::Value);
    };
    let count = cells
        .into_iter()
        .filter(|at| matches!(ev.eval_cell(target, *at), Value::Empty))
        .count();
    Value::Number(count as f64)
}

pub(crate) fn eval_standardize(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [x, m, sd] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if sd <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((x - m) / sd)
}

/// Evaluate two ranges of equal length and hand them to `f`.
pub(crate) fn paired(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: fn(&[f64], &[f64]) -> Option<f64>,
) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let xs = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let ys = match flatten_numbers(ev, sheet, &args[1..2]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    // Mismatched lengths are #N/A rather than being zipped to the shorter one,
    // which would silently answer over part of the data.
    if xs.len() != ys.len() || xs.is_empty() {
        return Value::Error(ErrorValue::Na);
    }
    match f(&xs, &ys) {
        Some(v) if v.is_finite() => Value::Number(v),
        _ => Value::Error(ErrorValue::Div0),
    }
}

pub(crate) fn correlation(xs: &[f64], ys: &[f64]) -> Option<f64> {
    let (mx, my) = (mean(xs), mean(ys));
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    let mut syy = 0.0;
    for (x, y) in xs.iter().zip(ys) {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx).powi(2);
        syy += (y - my).powi(2);
    }
    let denominator = (sxx * syy).sqrt();
    (denominator != 0.0).then(|| sxy / denominator)
}

pub(crate) fn slope(ys: &[f64], xs: &[f64]) -> Option<f64> {
    let (mx, my) = (mean(xs), mean(ys));
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    for (x, y) in xs.iter().zip(ys) {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx).powi(2);
    }
    (sxx != 0.0).then(|| sxy / sxx)
}

pub(crate) fn steyx(ys: &[f64], xs: &[f64]) -> Option<f64> {
    if ys.len() < 3 {
        return None;
    }
    let m = slope(ys, xs)?;
    let b = mean(ys) - m * mean(xs);
    let sse: f64 = xs
        .iter()
        .zip(ys)
        .map(|(x, y)| (y - (m * x + b)).powi(2))
        .sum();
    Some((sse / (ys.len() - 2) as f64).sqrt())
}

/// `FORECAST(x, known_y, known_x)` — the regression line evaluated at `x`.
pub(crate) fn eval_forecast(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let x = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let ys = match flatten_numbers(ev, sheet, &args[1..2]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let xs = match flatten_numbers(ev, sheet, &args[2..3]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if xs.len() != ys.len() || xs.is_empty() {
        return Value::Error(ErrorValue::Na);
    }
    match slope(&ys, &xs) {
        Some(m) => Value::Number(mean(&ys) - m * mean(&xs) + m * x),
        None => Value::Error(ErrorValue::Div0),
    }
}

pub(crate) fn three_numbers(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
) -> Option<Result<[f64; 3], ErrorValue>> {
    if args.len() != 3 {
        return None;
    }
    let mut out = [0.0; 3];
    for (i, slot) in out.iter_mut().enumerate() {
        match ev.eval_expr(sheet, &args[i]).as_number() {
            Ok(v) => *slot = v,
            Err(e) => return Some(Err(e)),
        }
    }
    Some(Ok(out))
}

/// The standard normal CDF, via the error function.
pub(crate) fn standard_normal_cdf(z: f64) -> f64 {
    0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2))
}

#[allow(clippy::excessive_precision)]
/// Abramowitz & Stegun 7.1.26 — about 1.5e-7 absolute error, which is finer
/// than the 15 significant digits a spreadsheet displays can distinguish for
/// probabilities.
pub(crate) fn erf(x: f64) -> f64 {
    // Exact at zero by construction. The rational approximation returns about
    // 1e-9 there, which makes NORMSDIST(0) read 0.5000000005 — a wart in the
    // one place every user checks first.
    if x == 0.0 {
        return 0.0;
    }
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-x * x).exp();
    sign * y
}

/// The inverse standard normal CDF (Acklam's rational approximation), refined
/// by one Halley step so the result is accurate to full double precision.
///
/// The coefficients are transcribed at their published precision, which is
/// finer than `f64` holds. They are left as printed so they can be checked
/// against the source rather than against a rounded copy of it.
#[allow(clippy::excessive_precision)]
pub(crate) fn normal_quantile(p: f64) -> f64 {
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_690e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996,
        3.754_408_661_907_416,
    ];
    const LOW: f64 = 0.024_25;
    let x = if p < LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= 1.0 - LOW {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    // One Halley refinement against the CDF, which lifts the approximation's
    // ~1e-9 relative error to machine precision.
    let e = standard_normal_cdf(x) - p;
    let u = e * (2.0 * std::f64::consts::PI).sqrt() * (x * x / 2.0).exp();
    x - u / (1.0 + x * u / 2.0)
}

/// The Lanczos approximation to `ln Γ(x)`.
///
/// Coefficients transcribed at published precision; see [`normal_quantile`].
#[allow(clippy::excessive_precision)]
pub(crate) fn ln_gamma(x: f64) -> f64 {
    const G: [f64; 9] = [
        0.999_999_999_999_809_93,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        // Reflection, since the series converges only for x > 0.5.
        return (std::f64::consts::PI / (std::f64::consts::PI * x).sin()).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = G[0];
    let t = x + 7.5;
    for (i, g) in G.iter().enumerate().skip(1) {
        a += g / (x + i as f64);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

pub(crate) fn eval_normdist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 4 {
        return Value::Error(ErrorValue::Value);
    }
    let Some(v) = three_numbers(ev, sheet, &args[..3]) else {
        return Value::Error(ErrorValue::Value);
    };
    let [x, m, sd] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let cumulative = match ev.eval_expr(sheet, &args[3]).as_bool() {
        Ok(b) => b,
        Err(e) => return Value::Error(e),
    };
    if sd <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    let z = (x - m) / sd;
    Value::Number(if cumulative {
        standard_normal_cdf(z)
    } else {
        (-z * z / 2.0).exp() / (sd * (2.0 * std::f64::consts::PI).sqrt())
    })
}

pub(crate) fn eval_norminv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [p, m, sd] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if sd <= 0.0 || p <= 0.0 || p >= 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(m + sd * normal_quantile(p))
}

pub(crate) fn eval_expondist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let (x, lambda) = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok([a, b]) => (a, b),
        Err(e) => return e,
    };
    let cumulative = match ev.eval_expr(sheet, &args[2]).as_bool() {
        Ok(b) => b,
        Err(e) => return Value::Error(e),
    };
    if x < 0.0 || lambda <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(if cumulative {
        1.0 - (-lambda * x).exp()
    } else {
        lambda * (-lambda * x).exp()
    })
}

pub(crate) fn eval_poisson(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let (x, m) = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok([a, b]) => (a.trunc(), b),
        Err(e) => return e,
    };
    let cumulative = match ev.eval_expr(sheet, &args[2]).as_bool() {
        Ok(b) => b,
        Err(e) => return Value::Error(e),
    };
    if x < 0.0 || m < 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    // Summed in log space: m^k / k! overflows for quite ordinary means long
    // before the probability itself becomes unrepresentable.
    let term = |k: f64| (-m + k * m.ln() - ln_gamma(k + 1.0)).exp();
    Value::Number(if cumulative {
        (0..=(x as u64)).map(|k| term(k as f64)).sum()
    } else {
        term(x)
    })
}

pub(crate) fn eval_binomdist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 4 {
        return Value::Error(ErrorValue::Value);
    }
    let Some(v) = three_numbers(ev, sheet, &args[..3]) else {
        return Value::Error(ErrorValue::Value);
    };
    let [s, n, p] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let cumulative = match ev.eval_expr(sheet, &args[3]).as_bool() {
        Ok(b) => b,
        Err(e) => return Value::Error(e),
    };
    let (s, n) = (s.trunc(), n.trunc());
    if s < 0.0 || s > n || !(0.0..=1.0).contains(&p) {
        return Value::Error(ErrorValue::Num);
    }
    let term = |k: f64| {
        // Log space again: C(n,k) overflows well before the probability does.
        (ln_gamma(n + 1.0) - ln_gamma(k + 1.0) - ln_gamma(n - k + 1.0)
            + k * p.ln()
            + (n - k) * (1.0 - p).ln())
        .exp()
    };
    Value::Number(if cumulative {
        (0..=(s as u64)).map(|k| term(k as f64)).sum()
    } else {
        term(s)
    })
}

/// As [`stat_over`], but over the `A` family's coercion: text counts as 0 and
/// logicals as 0 or 1, rather than being skipped.
pub(crate) fn stat_over_a(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: fn(&[f64]) -> Option<f64>,
) -> Value {
    let mut values = Vec::new();
    for arg in args {
        match range_cells(ev, sheet, arg) {
            Some((target, cells)) => {
                for at in cells {
                    match ev.eval_cell(target, at) {
                        Value::Number(n) => values.push(n),
                        Value::Bool(b) => values.push(if b { 1.0 } else { 0.0 }),
                        // Text is zero, not skipped: that is the whole point of
                        // the A variants, and it drags an average down.
                        Value::Text(_) => values.push(0.0),
                        Value::Error(e) => return Value::Error(e),
                        Value::Empty => {}
                        Value::Array { .. } | Value::Lambda(_) => {
                            return Value::Error(ErrorValue::Value);
                        }
                    }
                }
            }
            None => match ev.eval_expr(sheet, arg) {
                Value::Number(n) => values.push(n),
                Value::Bool(b) => values.push(if b { 1.0 } else { 0.0 }),
                Value::Text(_) => values.push(0.0),
                Value::Error(e) => return Value::Error(e),
                Value::Empty => {}
                Value::Array { .. } | Value::Lambda(_) => return Value::Error(ErrorValue::Value),
            },
        }
    }
    if values.is_empty() {
        return Value::Error(ErrorValue::Div0);
    }
    match f(&values) {
        Some(v) if v.is_finite() => Value::Number(v),
        _ => Value::Error(ErrorValue::Num),
    }
}

pub(crate) fn eval_lognormdist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [x, m, sd] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if x <= 0.0 || sd <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(standard_normal_cdf((x.ln() - m) / sd))
}

pub(crate) fn eval_loginv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [p, m, sd] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if sd <= 0.0 || p <= 0.0 || p >= 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((m + sd * normal_quantile(p)).exp())
}

pub(crate) fn eval_weibull(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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
    let scaled = (x / beta).powf(alpha);
    Value::Number(if cumulative {
        1.0 - (-scaled).exp()
    } else {
        alpha / beta.powf(alpha) * x.powf(alpha - 1.0) * (-scaled).exp()
    })
}

pub(crate) fn eval_negbinomdist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [f, s, p] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let (f, s) = (f.trunc(), s.trunc());
    if f < 0.0 || s < 1.0 || !(0.0..=1.0).contains(&p) {
        return Value::Error(ErrorValue::Num);
    }
    let log = ln_gamma(f + s) - ln_gamma(f + 1.0) - ln_gamma(s) + s * p.ln() + f * (1.0 - p).ln();
    Value::Number(log.exp())
}

pub(crate) fn eval_hypgeomdist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 4 {
        return Value::Error(ErrorValue::Value);
    }
    let mut v = [0.0f64; 4];
    for (i, slot) in v.iter_mut().enumerate() {
        match ev.eval_expr(sheet, &args[i]).as_number() {
            Ok(n) => *slot = n.trunc(),
            Err(e) => return Value::Error(e),
        }
    }
    let [k, n, successes, population] = v;
    if k < 0.0 || k > n || k > successes || n > population || successes > population {
        return Value::Error(ErrorValue::Num);
    }
    let log_choose = |a: f64, b: f64| ln_gamma(a + 1.0) - ln_gamma(b + 1.0) - ln_gamma(a - b + 1.0);
    Value::Number(
        (log_choose(successes, k) + log_choose(population - successes, n - k)
            - log_choose(population, n))
        .exp(),
    )
}

/// `CRITBINOM(trials, p, alpha)` — the smallest k whose cumulative binomial
/// probability reaches `alpha`.
pub(crate) fn eval_critbinom(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [trials, p, alpha] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let trials = trials.trunc();
    if trials < 0.0 || !(0.0..=1.0).contains(&p) || !(0.0..=1.0).contains(&alpha) {
        return Value::Error(ErrorValue::Num);
    }
    let mut cumulative = 0.0;
    for k in 0..=(trials as u64) {
        let k = k as f64;
        cumulative += (ln_gamma(trials + 1.0) - ln_gamma(k + 1.0) - ln_gamma(trials - k + 1.0)
            + k * p.ln()
            + (trials - k) * (1.0 - p).ln())
        .exp();
        if cumulative >= alpha {
            return Value::Number(k);
        }
    }
    Value::Number(trials)
}

pub(crate) fn eval_confidence(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [alpha, sd, size] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if alpha <= 0.0 || alpha >= 1.0 || sd <= 0.0 || size < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    // Two-tailed, so the quantile is at 1 - alpha/2.
    Value::Number(normal_quantile(1.0 - alpha / 2.0) * sd / size.trunc().sqrt())
}

// --- Engineering: base conversion and bit operations -----------------------

pub(crate) fn eval_ztest(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let sample = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let x = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    if sample.is_empty() {
        return Value::Error(ErrorValue::Num);
    }
    // Without a stated sigma the sample's own standard deviation stands in,
    // which is what makes ZTEST usable on a sample rather than a population.
    let sigma = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n,
            Err(e) => return Value::Error(e),
        },
        None => match variance(&sample, true) {
            Some(v) => v.sqrt(),
            None => return Value::Error(ErrorValue::Div0),
        },
    };
    if sigma <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    let z = (mean(&sample) - x) / (sigma / (sample.len() as f64).sqrt());
    // One-tailed, upper: ZTEST reports the probability of a value this high.
    Value::Number(1.0 - standard_normal_cdf(z))
}

pub(crate) fn eval_ttest(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 4 {
        return Value::Error(ErrorValue::Value);
    }
    let xs = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let ys = match flatten_numbers(ev, sheet, &args[1..2]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let (tails, kind) = match pair_of_numbers(ev, sheet, &args[2..4]) {
        Ok([a, b]) => (a, b),
        Err(e) => return e,
    };
    if !(tails == 1.0 || tails == 2.0) || !(1.0..=3.0).contains(&kind) {
        return Value::Error(ErrorValue::Num);
    }
    let (t, df) = match kind as i32 {
        // Paired: the test is on the differences, so the samples must line up.
        1 => {
            if xs.len() != ys.len() || xs.len() < 2 {
                return Value::Error(ErrorValue::Na);
            }
            let diffs: Vec<f64> = xs.iter().zip(&ys).map(|(x, y)| x - y).collect();
            let sd = match variance(&diffs, true) {
                Some(v) => v.sqrt(),
                None => return Value::Error(ErrorValue::Div0),
            };
            if sd == 0.0 {
                return Value::Error(ErrorValue::Div0);
            }
            let n = diffs.len() as f64;
            (mean(&diffs) / (sd / n.sqrt()), n - 1.0)
        }
        // Equal variance: pooled.
        2 => {
            let (n1, n2) = (xs.len() as f64, ys.len() as f64);
            if n1 < 2.0 || n2 < 2.0 {
                return Value::Error(ErrorValue::Div0);
            }
            let (v1, v2) = (variance(&xs, true).unwrap(), variance(&ys, true).unwrap());
            let pooled = ((n1 - 1.0) * v1 + (n2 - 1.0) * v2) / (n1 + n2 - 2.0);
            let se = (pooled * (1.0 / n1 + 1.0 / n2)).sqrt();
            if se == 0.0 {
                return Value::Error(ErrorValue::Div0);
            }
            ((mean(&xs) - mean(&ys)) / se, n1 + n2 - 2.0)
        }
        // Unequal variance: Welch, whose degrees of freedom are not an integer.
        _ => {
            let (n1, n2) = (xs.len() as f64, ys.len() as f64);
            if n1 < 2.0 || n2 < 2.0 {
                return Value::Error(ErrorValue::Div0);
            }
            let (v1, v2) = (variance(&xs, true).unwrap(), variance(&ys, true).unwrap());
            let se2 = v1 / n1 + v2 / n2;
            if se2 == 0.0 {
                return Value::Error(ErrorValue::Div0);
            }
            let df = se2 * se2 / ((v1 / n1).powi(2) / (n1 - 1.0) + (v2 / n2).powi(2) / (n2 - 1.0));
            ((mean(&xs) - mean(&ys)) / se2.sqrt(), df)
        }
    };
    Value::Number((1.0 - t_cdf(t.abs(), df)) * tails)
}

pub(crate) fn eval_ftest(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let xs = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let ys = match flatten_numbers(ev, sheet, &args[1..2]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let (Some(v1), Some(v2)) = (variance(&xs, true), variance(&ys, true)) else {
        return Value::Error(ErrorValue::Div0);
    };
    if v1 == 0.0 || v2 == 0.0 {
        return Value::Error(ErrorValue::Div0);
    }
    // The larger variance goes on top so the ratio is ≥ 1 and the tail is the
    // upper one; the other order gives the complement.
    let (hi, lo, dfh, dfl) = if v1 > v2 {
        (v1, v2, xs.len() as f64 - 1.0, ys.len() as f64 - 1.0)
    } else {
        (v2, v1, ys.len() as f64 - 1.0, xs.len() as f64 - 1.0)
    };
    Value::Number(2.0 * (1.0 - f_cdf(hi / lo, dfh, dfl)))
}

pub(crate) fn eval_chitest(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let actual = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let expected = match flatten_numbers(ev, sheet, &args[1..2]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if actual.len() != expected.len() || actual.is_empty() {
        return Value::Error(ErrorValue::Na);
    }
    let mut chi = 0.0;
    for (a, e) in actual.iter().zip(&expected) {
        if *e == 0.0 {
            return Value::Error(ErrorValue::Div0);
        }
        chi += (a - e).powi(2) / e;
    }
    let df = (actual.len() - 1) as f64;
    Value::Number(1.0 - gamma_p(df / 2.0, chi / 2.0))
}

pub(crate) fn eval_prob(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 || args.len() > 4 {
        return Value::Error(ErrorValue::Value);
    }
    let xs = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let ps = match flatten_numbers(ev, sheet, &args[1..2]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if xs.len() != ps.len() || xs.is_empty() {
        return Value::Error(ErrorValue::Na);
    }
    // The probabilities must be a distribution; Excel refuses otherwise rather
    // than normalizing, since a list that does not sum to 1 is a data error.
    let total: f64 = ps.iter().sum();
    if (total - 1.0).abs() > 1e-9 || ps.iter().any(|p| *p <= 0.0 || *p > 1.0) {
        return Value::Error(ErrorValue::Num);
    }
    let lower = match ev.eval_expr(sheet, &args[2]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let upper = match args.get(3) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n,
            Err(e) => return Value::Error(e),
        },
        None => lower,
    };
    let (lo, hi) = (lower.min(upper), lower.max(upper));
    Value::Number(
        xs.iter()
            .zip(&ps)
            .filter(|(x, _)| **x >= lo && **x <= hi)
            .map(|(_, p)| p)
            .sum(),
    )
}

/// `SUBTOTAL(fn, ranges…)` — the aggregate a table's totals row uses.
///
/// Codes 1..11 include manually hidden rows; 101..111 exclude them. The
/// distinction is the whole point of the function: a filtered list must not
/// report a total that includes what is hidden.
pub(crate) fn eval_subtotal(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 {
        return Value::Error(ErrorValue::Value);
    }
    let code = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n.trunc() as i32,
        Err(e) => return Value::Error(e),
    };
    let ignore_hidden = code > 100;
    let op = if ignore_hidden { code - 100 } else { code };
    if !(1..=11).contains(&op) {
        return Value::Error(ErrorValue::Value);
    }
    // Gather the values, skipping hidden rows for the 100-series.
    let mut values = Vec::new();
    for arg in &args[1..] {
        match range_cells(ev, sheet, arg) {
            Some((target, cells)) => {
                for at in cells {
                    if ignore_hidden {
                        let hidden = ev
                            .workbook()
                            .sheets
                            .get(target)
                            .is_some_and(|sh| sh.is_row_hidden(at.row));
                        if hidden {
                            continue;
                        }
                    }
                    match ev.eval_cell(target, at) {
                        Value::Number(n) => values.push(n),
                        Value::Error(e) => return Value::Error(e),
                        _ => {}
                    }
                }
            }
            None => match ev.eval_expr(sheet, arg) {
                Value::Number(n) => values.push(n),
                Value::Error(e) => return Value::Error(e),
                _ => {}
            },
        }
    }
    if values.is_empty() && op != 2 && op != 3 {
        return Value::Error(ErrorValue::Div0);
    }
    Value::Number(match op {
        1 => mean(&values),
        2 | 3 => values.len() as f64,
        4 => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        5 => values.iter().copied().fold(f64::INFINITY, f64::min),
        6 => values.iter().product(),
        7 => match variance(&values, true) {
            Some(v) => v.sqrt(),
            None => return Value::Error(ErrorValue::Div0),
        },
        8 => match variance(&values, false) {
            Some(v) => v.sqrt(),
            None => return Value::Error(ErrorValue::Div0),
        },
        9 => values.iter().sum(),
        10 => match variance(&values, true) {
            Some(v) => v,
            None => return Value::Error(ErrorValue::Div0),
        },
        _ => match variance(&values, false) {
            Some(v) => v,
            None => return Value::Error(ErrorValue::Div0),
        },
    })
}

/// The modern dynamic-array functions: XLOOKUP, XMATCH, FILTER, UNIQUE, SORT,
/// SORTBY and SEQUENCE.
///
/// None is in ECMA-376 — the standard predates them — but a spreadsheet
/// without XLOOKUP and FILTER is not a current one, and they only became
/// possible once results could spill.
///
/// How a value compares to a lookup key, for the ordered match modes.
pub(crate) fn lookup_compare(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => {
            let (x, y) = (
                a.as_text().unwrap_or_default().to_lowercase(),
                b.as_text().unwrap_or_default().to_lowercase(),
            );
            x.cmp(&y)
        }
    }
}

/// Whether two values are equal for lookup purposes, with `match_mode = 2`
/// meaning the needle may contain wildcards.
pub(crate) fn lookup_equal(cell: &Value, needle: &Value, wildcard: bool) -> bool {
    if wildcard {
        let pattern = needle.as_text().unwrap_or_default();
        if has_wildcard(&pattern) {
            return match cell {
                Value::Text(s) => wildcard_match(&pattern, s),
                _ => false,
            };
        }
    }
    lookup_compare(cell, needle) == std::cmp::Ordering::Equal
}

/// The index `XLOOKUP` and `XMATCH` settle on, or `None`.
///
/// `match_mode` −1 and 1 mean "next smaller" and "next larger" and require the
/// *best* candidate rather than the first acceptable one — taking the first
/// would return whichever end of the data the scan started from, which is a
/// different answer for the same question.
pub(crate) fn lookup_index(
    values: &[Value],
    needle: &Value,
    match_mode: i64,
    search_mode: i64,
) -> Option<usize> {
    use std::cmp::Ordering;
    let indices: Vec<usize> = if search_mode < 0 {
        (0..values.len()).rev().collect()
    } else {
        (0..values.len()).collect()
    };
    match match_mode {
        0 | 2 => indices
            .into_iter()
            .find(|&i| lookup_equal(&values[i], needle, match_mode == 2)),
        -1 | 1 => {
            let mut best: Option<(usize, &Value)> = None;
            for i in indices {
                let ord = lookup_compare(&values[i], needle);
                if ord == Ordering::Equal {
                    return Some(i); // exact always wins
                }
                let acceptable = if match_mode == -1 {
                    ord == Ordering::Less
                } else {
                    ord == Ordering::Greater
                };
                if !acceptable {
                    continue;
                }
                let better = match best {
                    None => true,
                    Some((_, b)) => {
                        let c = lookup_compare(&values[i], b);
                        if match_mode == -1 {
                            c == Ordering::Greater // the largest below
                        } else {
                            c == Ordering::Less // the smallest above
                        }
                    }
                };
                if better {
                    best = Some((i, &values[i]));
                }
            }
            best.map(|(i, _)| i)
        }
        _ => None,
    }
}

/// A grid's values as a list of rows, each a list of values.
pub(crate) fn grid_rows(g: &Grid) -> Vec<Vec<Value>> {
    (0..g.rows)
        .map(|r| (0..g.cols).map(|c| g.get(r, c).clone()).collect())
        .collect()
}

/// Build an array value from rows, or a scalar when it is 1×1.
pub(crate) fn rows_to_value(rows: Vec<Vec<Value>>) -> Value {
    let height = rows.len();
    let width = rows.first().map_or(0, Vec::len);
    if height == 0 || width == 0 {
        return Value::Error(ErrorValue::Value);
    }
    if height == 1 && width == 1 {
        return rows
            .into_iter()
            .next()
            .and_then(|r| r.into_iter().next())
            .unwrap_or(Value::Empty);
    }
    Value::Array {
        rows: height,
        cols: width,
        cells: rows.into_iter().flatten().collect(),
    }
}
