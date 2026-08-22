//! Financial: annuities, depreciation, bonds, coupons and bill discounting.
//!
//! Split out of the single `functions.rs` under `MNT-002`; the section
//! headings that file already carried are the seams.

use super::*;

/// Read up to `max` numeric arguments, filling absent ones from `defaults`.
pub(crate) fn opt_numbers<const N: usize>(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    required: usize,
    defaults: [f64; N],
) -> Result<[f64; N], Value> {
    if args.len() < required || args.len() > N {
        return Err(Value::Error(ErrorValue::Value));
    }
    let mut out = defaults;
    for (i, slot) in out.iter_mut().enumerate() {
        if let Some(arg) = args.get(i) {
            *slot = ev.eval_expr(sheet, arg).as_number().map_err(Value::Error)?;
        }
    }
    Ok(out)
}

/// `(1 + rate)^nper`, and the annuity factor `((1+r)^n - 1) / r`.
///
/// The zero-rate case is a genuine limit, not an edge case to reject: a
/// no-interest loan is an ordinary thing to model, and `0/0` here would make
/// PMT report an error for it.
pub(crate) fn annuity_factor(rate: f64, nper: f64) -> (f64, f64) {
    if rate == 0.0 {
        return (1.0, nper);
    }
    let growth = (1.0 + rate).powf(nper);
    (growth, (growth - 1.0) / rate)
}

/// `type` is 1 when payments fall at the start of the period, which advances
/// every payment by one period's interest.
pub(crate) fn due_factor(rate: f64, kind: f64) -> f64 {
    if kind != 0.0 { 1.0 + rate } else { 1.0 }
}

pub(crate) fn eval_fv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [rate, nper, pmt, pv, kind] =
        match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0, 0.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    let (growth, factor) = annuity_factor(rate, nper);
    Value::Number(-(pv * growth + pmt * due_factor(rate, kind) * factor))
}

pub(crate) fn eval_pv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [rate, nper, pmt, fv, kind] =
        match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0, 0.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    let (growth, factor) = annuity_factor(rate, nper);
    Value::Number(-(fv + pmt * due_factor(rate, kind) * factor) / growth)
}

pub(crate) fn eval_pmt(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [rate, nper, pv, fv, kind] =
        match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0, 0.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    let (growth, factor) = annuity_factor(rate, nper);
    if factor == 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(-(pv * growth + fv) / (due_factor(rate, kind) * factor))
}

pub(crate) fn eval_nper(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [rate, pmt, pv, fv, kind] = match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0, 0.0])
    {
        Ok(v) => v,
        Err(e) => return e,
    };
    if rate == 0.0 {
        if pmt == 0.0 {
            return Value::Error(ErrorValue::Num);
        }
        return Value::Number(-(pv + fv) / pmt);
    }
    let adjusted = pmt * due_factor(rate, kind);
    let numerator = adjusted - fv * rate;
    let denominator = pv * rate + adjusted;
    if numerator / denominator <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((numerator / denominator).ln() / (1.0 + rate).ln())
}

/// `RATE` has no closed form, so it is solved numerically.
///
/// Newton from the caller's guess, falling back to bisection when Newton
/// wanders — the derivative is near zero around rate 0, where Newton alone
/// diverges on exactly the ordinary case of a nearly interest-free loan.
pub(crate) fn eval_rate(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [nper, pmt, pv, fv, kind, guess] =
        match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0, 0.0, 0.1]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    let residual = |rate: f64| {
        let (growth, factor) = annuity_factor(rate, nper);
        pv * growth + pmt * due_factor(rate, kind) * factor + fv
    };
    let mut rate = guess;
    for _ in 0..64 {
        let f = residual(rate);
        if f.abs() < 1e-10 {
            return Value::Number(rate);
        }
        // Numeric derivative: the analytic one is long and its algebra is a
        // ready source of sign errors that only show as slow convergence.
        let h = 1e-7;
        let slope = (residual(rate + h) - f) / h;
        if slope.abs() < 1e-14 {
            break;
        }
        let next = rate - f / slope;
        if !next.is_finite() {
            break;
        }
        rate = next;
    }
    // Bisection over a wide bracket, which converges wherever a root exists.
    let (mut lo, mut hi) = (-0.999_999, 10.0);
    let (mut flo, fhi) = (residual(lo), residual(hi));
    if flo * fhi > 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        let fmid = residual(mid);
        if fmid.abs() < 1e-12 {
            return Value::Number(mid);
        }
        if flo * fmid < 0.0 {
            hi = mid;
        } else {
            lo = mid;
            flo = fmid;
        }
    }
    Value::Number((lo + hi) / 2.0)
}

/// The interest (or principal) part of one payment.
pub(crate) fn eval_ipmt(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    interest: bool,
) -> Value {
    let [rate, per, nper, pv, fv, kind] =
        match opt_numbers(ev, sheet, args, 4, [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    if per < 1.0 || per > nper {
        return Value::Error(ErrorValue::Num);
    }
    let payment = match eval_pmt_values(rate, nper, pv, fv, kind) {
        Some(p) => p,
        None => return Value::Error(ErrorValue::Num),
    };
    // The balance carried into this period, which is what the interest accrues
    // on — computed as the future value of the loan after `per - 1` payments.
    let (growth, factor) = annuity_factor(rate, per - 1.0);
    let balance = pv * growth + payment * due_factor(rate, kind) * factor;
    let mut interest_part = -balance * rate;
    // A payment due at the start of its period accrues no interest for it.
    if kind != 0.0 && per > 1.0 {
        interest_part /= 1.0 + rate;
    }
    if kind != 0.0 && per == 1.0 {
        interest_part = 0.0;
    }
    Value::Number(if interest {
        interest_part
    } else {
        payment - interest_part
    })
}

pub(crate) fn eval_pmt_values(rate: f64, nper: f64, pv: f64, fv: f64, kind: f64) -> Option<f64> {
    let (growth, factor) = annuity_factor(rate, nper);
    let denominator = due_factor(rate, kind) * factor;
    (denominator != 0.0).then(|| -(pv * growth + fv) / denominator)
}

/// `ISPMT` — the interest of a straight-line loan, which is *not* the same as
/// IPMT: the principal repays evenly rather than on an amortization schedule.
pub(crate) fn eval_ispmt(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [rate, per, nper, pv] = match opt_numbers(ev, sheet, args, 4, [0.0, 0.0, 0.0, 0.0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if nper == 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(pv * rate * (per / nper - 1.0))
}

pub(crate) fn eval_npv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 {
        return Value::Error(ErrorValue::Value);
    }
    let rate = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let flows = match flatten_numbers(ev, sheet, &args[1..]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if rate == -1.0 {
        return Value::Error(ErrorValue::Num);
    }
    // NPV discounts the *first* flow by one period: it treats every value as
    // arriving at the end of a period, unlike XNPV which dates them.
    let total: f64 = flows
        .iter()
        .enumerate()
        .map(|(i, v)| v / (1.0 + rate).powi(i as i32 + 1))
        .sum();
    Value::Number(total)
}

pub(crate) fn npv_at(rate: f64, flows: &[f64]) -> f64 {
    flows
        .iter()
        .enumerate()
        .map(|(i, v)| v / (1.0 + rate).powi(i as i32))
        .sum()
}

pub(crate) fn eval_irr(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let flows = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    // Without both signs there is no root, and a solver would wander until it
    // gave up rather than saying so.
    if !flows.iter().any(|v| *v > 0.0) || !flows.iter().any(|v| *v < 0.0) {
        return Value::Error(ErrorValue::Num);
    }
    match solve_rate(|r| npv_at(r, &flows)) {
        Some(r) => Value::Number(r),
        None => Value::Error(ErrorValue::Num),
    }
}

/// Bisect for a rate where `f` crosses zero, over the range a rate can take.
pub(crate) fn solve_rate(f: impl Fn(f64) -> f64) -> Option<f64> {
    let (mut lo, mut hi) = (-0.999_999, 10.0);
    let (mut flo, fhi) = (f(lo), f(hi));
    if !flo.is_finite() || !fhi.is_finite() || flo * fhi > 0.0 {
        return None;
    }
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        let fmid = f(mid);
        if fmid.abs() < 1e-12 {
            return Some(mid);
        }
        if flo * fmid < 0.0 {
            hi = mid;
        } else {
            lo = mid;
            flo = fmid;
        }
    }
    Some((lo + hi) / 2.0)
}

pub(crate) fn eval_mirr(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let flows = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let (finance, reinvest) = match pair_of_numbers(ev, sheet, &args[1..3]) {
        Ok([a, b]) => (a, b),
        Err(e) => return e,
    };
    let n = flows.len() as f64;
    if n < 2.0 {
        return Value::Error(ErrorValue::Div0);
    }
    // Negatives discounted at the finance rate, positives compounded at the
    // reinvestment rate — the whole point of MIRR over IRR.
    let negatives: f64 = flows
        .iter()
        .enumerate()
        .filter(|(_, v)| **v < 0.0)
        .map(|(i, v)| v / (1.0 + finance).powi(i as i32))
        .sum();
    let positives: f64 = flows
        .iter()
        .enumerate()
        .filter(|(_, v)| **v > 0.0)
        .map(|(i, v)| v * (1.0 + reinvest).powi((n as i32 - 1) - i as i32))
        .sum();
    if negatives == 0.0 {
        return Value::Error(ErrorValue::Div0);
    }
    Value::Number((-positives / negatives).powf(1.0 / (n - 1.0)) - 1.0)
}

pub(crate) fn eval_xnpv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let rate = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let (flows, dates) = match dated_flows(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let start = dates[0];
    Value::Number(
        flows
            .iter()
            .zip(&dates)
            .map(|(v, d)| v / (1.0 + rate).powf((d - start) / 365.0))
            .sum(),
    )
}

pub(crate) fn eval_xirr(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let (flows, dates) = match dated_flows(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !flows.iter().any(|v| *v > 0.0) || !flows.iter().any(|v| *v < 0.0) {
        return Value::Error(ErrorValue::Num);
    }
    let start = dates[0];
    let f = |rate: f64| -> f64 {
        flows
            .iter()
            .zip(&dates)
            .map(|(v, d)| v / (1.0 + rate).powf((d - start) / 365.0))
            .sum()
    };
    match solve_rate(f) {
        Some(r) => Value::Number(r),
        None => Value::Error(ErrorValue::Num),
    }
}

/// The `(values, dates)` pair XNPV and XIRR share, validated together.
pub(crate) fn dated_flows(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
) -> Result<(Vec<f64>, Vec<f64>), Value> {
    // XNPV takes the rate first, XIRR does not.
    let offset = usize::from(args.len() == 3 && !matches!(args[0], Expr::Range(..)));
    let flows = flatten_numbers(ev, sheet, &args[offset..offset + 1]).map_err(Value::Error)?;
    let dates = flatten_numbers(ev, sheet, &args[offset + 1..offset + 2]).map_err(Value::Error)?;
    if flows.len() != dates.len() || flows.is_empty() {
        return Err(Value::Error(ErrorValue::Num));
    }
    Ok((flows, dates))
}

pub(crate) fn eval_fvschedule(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let principal = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let schedule = match flatten_numbers(ev, sheet, &args[1..]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    Value::Number(schedule.iter().fold(principal, |acc, r| acc * (1.0 + r)))
}

pub(crate) fn eval_sln(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [cost, salvage, life] = match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if life == 0.0 {
        return Value::Error(ErrorValue::Div0);
    }
    Value::Number((cost - salvage) / life)
}

pub(crate) fn eval_syd(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [cost, salvage, life, per] = match opt_numbers(ev, sheet, args, 4, [0.0, 0.0, 0.0, 0.0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if life <= 0.0 || per < 1.0 || per > life {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((cost - salvage) * (life - per + 1.0) * 2.0 / (life * (life + 1.0)))
}

/// `DB` — fixed-declining balance, whose rate is rounded to three decimals.
///
/// That rounding is in the definition, not an implementation shortcut: leaving
/// it out changes every period's figure by a little, which is exactly the kind
/// of error that survives review.
pub(crate) fn eval_db(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [cost, salvage, life, period, month] =
        match opt_numbers(ev, sheet, args, 4, [0.0, 0.0, 0.0, 0.0, 12.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    if cost <= 0.0 || life <= 0.0 || period < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    // The rate is rounded to three decimals *by definition*, not as an
    // implementation shortcut: omitting the rounding shifts every period's
    // figure slightly, which is the kind of error that survives review.
    let rate = ((1.0 - (salvage / cost).powf(1.0 / life)) * 1000.0).round() / 1000.0;
    let first = cost * rate * month / 12.0;
    if period == 1.0 {
        return Value::Number(first);
    }
    let mut total = first;
    let mut current = 0.0;
    for _ in 2..=(period as u64) {
        current = (cost - total) * rate;
        total += current;
    }
    // The final period covers only the remaining months of the year.
    if period > life {
        current = (cost - total + current) * rate * (12.0 - month) / 12.0;
    }
    Value::Number(current)
}

/// `DDB` — double-declining balance, never depreciating below the salvage.
pub(crate) fn eval_ddb(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [cost, salvage, life, period, factor] =
        match opt_numbers(ev, sheet, args, 4, [0.0, 0.0, 0.0, 0.0, 2.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    if cost < 0.0 || life <= 0.0 || period < 1.0 || factor <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    let mut total = 0.0;
    let mut current = 0.0;
    for _ in 1..=(period as u64) {
        current = ((cost - total) * factor / life)
            .min(cost - salvage - total)
            .max(0.0);
        total += current;
    }
    Value::Number(current)
}

/// `EFFECT` and `NOMINAL`, which are inverses of each other.
pub(crate) fn eval_effect(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    to_effective: bool,
) -> Value {
    let [rate, periods] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let periods = periods.trunc();
    if rate <= 0.0 || periods < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(if to_effective {
        (1.0 + rate / periods).powf(periods) - 1.0
    } else {
        ((1.0 + rate).powf(1.0 / periods) - 1.0) * periods
    })
}

pub(crate) fn eval_rri(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [nper, pv, fv] = match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if nper <= 0.0 || pv <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((fv / pv).powf(1.0 / nper) - 1.0)
}

pub(crate) fn eval_pduration(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [rate, pv, fv] = match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if rate <= 0.0 || pv <= 0.0 || fv <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((fv.ln() - pv.ln()) / (1.0 + rate).ln())
}

/// `DOLLARDE` / `DOLLARFR` — prices written as whole units plus a fraction,
/// as bond quotes are.
pub(crate) fn eval_dollar_frac(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    to_decimal: bool,
) -> Value {
    let [value, fraction] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let fraction = fraction.trunc();
    if fraction < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    let whole = value.trunc();
    let rest = value - whole;
    // The fractional part is written in base `fraction` but *positioned* by
    // decimal digits: at 16ths, 1.02 means 1 + 2/16 and 1.15 means 1 + 15/16.
    // So the scale is 10^(digits in `fraction`), not the fraction itself.
    let digits = fraction.log10().floor() + 1.0;
    let scale = 10f64.powf(digits);
    Value::Number(if to_decimal {
        whole + rest * scale / fraction
    } else {
        whole + rest * fraction / scale
    })
}

// --- Complex numbers -------------------------------------------------------
//
// A complex number is *text* in a spreadsheet — "3+4i" — not a value type. So
// every function here parses its arguments and formats its result, and the
// imaginary suffix travels with the value: a workbook using `j` throughout must
// not come back using `i`.

/// `DISC` — the discount rate implied by a price.
pub(crate) fn eval_disc(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 4 || args.len() > 5 {
        return Value::Error(ErrorValue::Value);
    }
    let [settle, mature, price, redemption, basis] =
        match opt_numbers(ev, sheet, args, 4, [0.0, 0.0, 0.0, 0.0, 0.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    if price <= 0.0 || redemption <= 0.0 || mature <= settle {
        return Value::Error(ErrorValue::Num);
    }
    let frac = year_fraction(settle, mature, basis as i64);
    if frac <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((redemption - price) / redemption / frac)
}

/// `INTRATE` and `RECEIVED`, which invert each other.
pub(crate) fn eval_intrate(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    received: bool,
) -> Value {
    if args.len() < 4 || args.len() > 5 {
        return Value::Error(ErrorValue::Value);
    }
    let [settle, mature, investment, other, basis] =
        match opt_numbers(ev, sheet, args, 4, [0.0, 0.0, 0.0, 0.0, 0.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    if investment <= 0.0 || mature <= settle {
        return Value::Error(ErrorValue::Num);
    }
    let frac = year_fraction(settle, mature, basis as i64);
    if frac <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    if received {
        let denominator = 1.0 - other * frac;
        if denominator == 0.0 {
            return Value::Error(ErrorValue::Num);
        }
        return Value::Number(investment / denominator);
    }
    Value::Number((other - investment) / investment / frac)
}

/// The three Treasury-bill functions, which all use the 360-day actual basis
/// the bill market quotes on.
pub(crate) fn eval_tbill(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], which: u8) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [settle, mature, third] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let days = mature.trunc() - settle.trunc();
    // A bill runs at most a year; beyond that the quoting convention does not
    // apply and Excel refuses rather than extrapolating.
    if days <= 0.0 || days > 366.0 {
        return Value::Error(ErrorValue::Num);
    }
    match which {
        0 => {
            if third <= 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number(100.0 * (1.0 - third * days / 360.0))
        }
        1 => {
            if third <= 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number((100.0 - third) / third * (360.0 / days))
        }
        _ => {
            if third <= 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number(365.0 * third / (360.0 - third * days))
        }
    }
}

/// The year fraction between two serials on an OOXML day-count basis.
pub(crate) fn year_fraction(start: f64, end: f64, basis: i64) -> f64 {
    let (a, b) = (start.trunc() as i64, end.trunc() as i64);
    match basis {
        0 => eval_days360_serials(a, b, false).unwrap_or(0) as f64 / 360.0,
        1 => (b - a) as f64 / average_year_length(a, b),
        2 => (b - a) as f64 / 360.0,
        3 => (b - a) as f64 / 365.0,
        4 => eval_days360_serials(a, b, true).unwrap_or(0) as f64 / 360.0,
        _ => 0.0,
    }
}

/// The `D` functions: an aggregate over the rows of a table that satisfy a
/// criteria block.
///
/// All twelve are one shape — `Dxxx(database, field, criteria)` — differing
/// only in what they do with the picked column, so they share everything up to
/// that point. Writing them separately is how twelve copies of the criteria
/// rules drift apart.
///
/// The criteria block is the part worth stating: its first row names fields,
/// each following row is a set of conditions, conditions **across a row are
/// AND** and **rows are OR**. An empty criteria cell is not a condition at all
/// — reading it as "equals blank" would exclude every row.
pub(crate) fn eval_database(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    name: &str,
    args: &[Expr],
) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let db = match eval_range_2d(ev, sheet, &args[0]) {
        Ok(g) => g,
        Err(e) => return Value::Error(e),
    };
    let crit = match eval_range_2d(ev, sheet, &args[2]) {
        Ok(g) => g,
        Err(e) => return Value::Error(e),
    };
    // A table with only a header row has no rows to aggregate, and a criteria
    // block with only a header row selects everything.
    if db.rows < 1 || db.cols == 0 || crit.rows < 1 || crit.cols == 0 {
        return Value::Error(ErrorValue::Value);
    }

    let header = |g: &Grid, c: usize| g.get(0, c).as_text().unwrap_or_default().trim().to_owned();
    let db_headers: Vec<String> = (0..db.cols).map(|c| header(&db, c)).collect();

    // `field` is a column name, a 1-based index, or a reference to a header
    // cell — Excel accepts all three, and a file written by someone else will
    // use whichever they preferred.
    let field_value = ev.eval_expr(sheet, &args[1]);
    let field_col: Option<usize> = match &field_value {
        Value::Number(n) => {
            let i = *n as i64;
            if i >= 1 && (i as usize) <= db.cols {
                Some(i as usize - 1)
            } else {
                None
            }
        }
        other => {
            let want = other.as_text().unwrap_or_default();
            let want = want.trim();
            db_headers.iter().position(|h| h.eq_ignore_ascii_case(want))
        }
    };
    // DCOUNTA is the one that allows an absent field: it then counts rows.
    let counting_rows = field_col.is_none() && name == "DCOUNTA";
    if field_col.is_none() && !counting_rows {
        return Value::Error(ErrorValue::Value);
    }

    let mut picked: Vec<Value> = Vec::new();
    for r in 1..db.rows {
        let mut any_row_matched = false;
        for cr in 1..crit.rows {
            let mut all = true;
            let mut had_condition = false;
            for cc in 0..crit.cols {
                let cell = crit.get(cr, cc);
                let text = cell.as_text().unwrap_or_default();
                if text.trim().is_empty() {
                    continue; // not a condition
                }
                had_condition = true;
                let Some(col) = db_headers
                    .iter()
                    .position(|h| h.eq_ignore_ascii_case(header(&crit, cc).trim()))
                else {
                    // A criteria column naming no field cannot be satisfied.
                    all = false;
                    break;
                };
                let (op, operand) = parse_criteria(cell);
                if !criterion_matches(db.get(r, col), op, &operand) {
                    all = false;
                    break;
                }
            }
            // A criteria row with no conditions at all matches everything,
            // which is what an empty row under the headers means.
            if all && (had_condition || crit.cols > 0) {
                any_row_matched = true;
                break;
            }
        }
        if any_row_matched {
            picked.push(if counting_rows {
                Value::Number(1.0)
            } else {
                db.get(r, field_col.expect("checked")).clone()
            });
        }
    }

    let numbers: Vec<f64> = picked
        .iter()
        .filter_map(|v| match v {
            Value::Number(n) => Some(*n),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        })
        .collect();

    match name {
        // DCOUNT counts numbers; DCOUNTA counts anything that is not blank.
        // The pair is the same distinction as COUNT and COUNTA, and swapping
        // them silently changes what a report totals.
        "DCOUNT" => Value::Number(numbers.len() as f64),
        "DCOUNTA" => {
            Value::Number(picked.iter().filter(|v| !matches!(v, Value::Empty)).count() as f64)
        }
        "DGET" => match picked.len() {
            // Excel's own answers: nothing matched is #VALUE!, more than one
            // match is #NUM!. Returning the first would be a plausible wrong
            // answer, which is the worst kind.
            0 => Value::Error(ErrorValue::Value),
            1 => picked.into_iter().next().expect("one"),
            _ => Value::Error(ErrorValue::Num),
        },
        _ if numbers.is_empty() => match name {
            "DSUM" | "DPRODUCT" => Value::Number(0.0),
            _ => Value::Error(ErrorValue::Div0),
        },
        "DSUM" => Value::Number(numbers.iter().sum()),
        "DPRODUCT" => Value::Number(numbers.iter().product()),
        "DAVERAGE" => Value::Number(numbers.iter().sum::<f64>() / numbers.len() as f64),
        "DMAX" => Value::Number(numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
        "DMIN" => Value::Number(numbers.iter().copied().fold(f64::INFINITY, f64::min)),
        "DVAR" | "DSTDEV" => {
            // Sample statistics need two points; one has no spread to measure.
            if numbers.len() < 2 {
                return Value::Error(ErrorValue::Div0);
            }
            let mean = numbers.iter().sum::<f64>() / numbers.len() as f64;
            let var = numbers.iter().map(|n| (n - mean).powi(2)).sum::<f64>()
                / (numbers.len() - 1) as f64;
            Value::Number(if name == "DVAR" { var } else { var.sqrt() })
        }
        "DVARP" | "DSTDEVP" => {
            let mean = numbers.iter().sum::<f64>() / numbers.len() as f64;
            let var =
                numbers.iter().map(|n| (n - mean).powi(2)).sum::<f64>() / numbers.len() as f64;
            Value::Number(if name == "DVARP" { var } else { var.sqrt() })
        }
        _ => Value::Error(ErrorValue::Name),
    }
}

/// The coupon-schedule quantities every bond formula needs: number of periods,
/// and the settlement's position inside its coupon period.
///
/// Returned together because they must come from one schedule — deriving `n`
/// from one calculation and `dsc/e` from another is how a price and a yield
/// stop being inverses of each other.
pub(crate) fn bond_terms(
    settle: i64,
    mature: i64,
    freq: i64,
    basis: i64,
) -> Option<(f64, f64, f64)> {
    let (prev, next) = coupon_period(settle, mature, freq)?;
    let period = |a: i64, b: i64| -> f64 {
        match basis {
            0 => eval_days360_serials(a, b, false).unwrap_or(0) as f64,
            4 => eval_days360_serials(a, b, true).unwrap_or(0) as f64,
            _ => (b - a) as f64,
        }
    };
    let e = match basis {
        0 | 2 | 4 => 360.0 / freq as f64,
        1 => (next - prev) as f64,
        _ => 365.0 / freq as f64,
    };
    if e <= 0.0 {
        return None;
    }
    let (my, mm, _) = serial_to_ymd(mature);
    let (ny, nm, _) = serial_to_ymd(next);
    let n = ((my * 12 + mm) - (ny * 12 + nm)) / (12 / freq) + 1;
    // `a/e` is how far into the period settlement sits — the accrued fraction.
    Some((n as f64, period(settle, next) / e, period(prev, settle) / e))
}

/// The clean price of a bond per 100 face, given a yield.
///
/// The last term is the accrued interest: a buyer settling mid-period pays the
/// seller for the days they held it, and the *clean* price is what is quoted.
/// Omitting it prices the bond as though coupons only ever land on settlement.
pub(crate) fn bond_price(
    rate: f64,
    yld: f64,
    redemption: f64,
    freq: f64,
    n: f64,
    dsc_e: f64,
    a_e: f64,
) -> f64 {
    let coupon = 100.0 * rate / freq;
    let k = 1.0 + yld / freq;
    let mut price = redemption / k.powf(n - 1.0 + dsc_e);
    for i in 1..=(n as i64) {
        price += coupon / k.powf(i as f64 - 1.0 + dsc_e);
    }
    price - coupon * a_e
}

/// The bond functions that need the coupon schedule: PRICE, YIELD, DURATION,
/// MDURATION.
///
/// `YIELD` has no closed form, so it is solved numerically against `bond_price`
/// — the same function `PRICE` uses, which is what makes the two exact
/// inverses rather than approximately so.
pub(crate) fn eval_bond(ev: &mut Evaluator<'_>, sheet: usize, name: &str, args: &[Expr]) -> Value {
    let wants = if matches!(name, "PRICE" | "YIELD") {
        6
    } else {
        5
    };
    if args.len() < wants || args.len() > wants + 1 {
        return Value::Error(ErrorValue::Value);
    }
    let nums = match opt_numbers(ev, sheet, args, wants, [0.0; 7]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (settle, mature) = (nums[0].trunc() as i64, nums[1].trunc() as i64);
    let (rate, third) = (nums[2], nums[3]);
    let (redemption, freq, basis) = if wants == 6 {
        (nums[4], nums[5], nums[6] as i64)
    } else {
        (100.0, nums[4], nums[5] as i64)
    };
    if rate < 0.0 || freq <= 0.0 || !(0..=4).contains(&basis) {
        return Value::Error(ErrorValue::Num);
    }
    let Some((n, dsc_e, a_e)) = bond_terms(settle, mature, freq as i64, basis) else {
        return Value::Error(ErrorValue::Num);
    };

    match name {
        "PRICE" => {
            if third < 0.0 || redemption <= 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number(bond_price(rate, third, redemption, freq, n, dsc_e, a_e))
        }
        "YIELD" => {
            let price = third;
            if price <= 0.0 || redemption <= 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            // Bisection over a bracket wide enough for any real bond. Slower
            // than Newton and immune to the derivative blowing up near zero
            // yield, which is where a bond priced at par sits.
            let f = |y: f64| bond_price(rate, y, redemption, freq, n, dsc_e, a_e) - price;
            let (mut lo, mut hi) = (-0.99, 10.0);
            if f(lo) * f(hi) > 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            for _ in 0..200 {
                let mid = (lo + hi) / 2.0;
                if f(lo) * f(mid) <= 0.0 {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            Value::Number((lo + hi) / 2.0)
        }
        "DURATION" | "MDURATION" => {
            let yld = third;
            let k = 1.0 + yld / freq;
            let coupon = 100.0 * rate / freq;
            let (mut pv_sum, mut weighted) = (0.0, 0.0);
            for i in 1..=(n as i64) {
                let periods = i as f64 - 1.0 + dsc_e;
                let cash = coupon + if i as f64 == n { 100.0 } else { 0.0 };
                let pv = cash / k.powf(periods);
                pv_sum += pv;
                weighted += pv * periods / freq;
            }
            if pv_sum == 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            let macaulay = weighted / pv_sum;
            Value::Number(if name == "DURATION" {
                macaulay
            } else {
                // Modified duration discounts Macaulay by one period's yield —
                // it answers "how much does the price move", not "when is the
                // money".
                macaulay / k
            })
        }
        _ => Value::Error(ErrorValue::Name),
    }
}

/// The bond functions that need no coupon schedule, because the instrument has
/// no coupons or pays only at maturity.
pub(crate) fn eval_bond_simple(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    name: &str,
    args: &[Expr],
) -> Value {
    let wants = match name {
        "ACCRINTM" => 4,
        "PRICEDISC" | "YIELDDISC" => 4,
        _ => 5, // PRICEMAT, YIELDMAT
    };
    if args.len() < wants || args.len() > wants + 1 {
        return Value::Error(ErrorValue::Value);
    }
    let v = match opt_numbers(ev, sheet, args, wants, [0.0; 6]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let basis = v[wants] as i64;
    if !(0..=4).contains(&basis) {
        return Value::Error(ErrorValue::Num);
    }
    match name {
        "ACCRINTM" => {
            let (issue, settle, rate, par) = (v[0], v[1], v[2], v[3]);
            if rate <= 0.0 || par <= 0.0 || settle <= issue {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number(par * rate * year_fraction(issue, settle, basis))
        }
        "PRICEDISC" => {
            let (settle, mature, discount, redemption) = (v[0], v[1], v[2], v[3]);
            if discount <= 0.0 || redemption <= 0.0 || mature <= settle {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number(redemption - discount * redemption * year_fraction(settle, mature, basis))
        }
        "YIELDDISC" => {
            let (settle, mature, price, redemption) = (v[0], v[1], v[2], v[3]);
            let frac = year_fraction(settle, mature, basis);
            if price <= 0.0 || redemption <= 0.0 || mature <= settle || frac <= 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number((redemption / price - 1.0) / frac)
        }
        "PRICEMAT" | "YIELDMAT" => {
            let (settle, mature, issue, rate, fourth) = (v[0], v[1], v[2], v[3], v[4]);
            if rate < 0.0 || mature <= settle || settle <= issue {
                return Value::Error(ErrorValue::Num);
            }
            // Interest accrues from *issue*, not from settlement: the buyer
            // pays the seller for the part of the term already elapsed.
            let fim = year_fraction(issue, mature, basis);
            let fsm = year_fraction(settle, mature, basis);
            let fis = year_fraction(issue, settle, basis);
            if name == "PRICEMAT" {
                let denom = 1.0 + fsm * fourth;
                if denom == 0.0 {
                    return Value::Error(ErrorValue::Num);
                }
                Value::Number((100.0 + fim * rate * 100.0) / denom - fis * rate * 100.0)
            } else {
                let price = fourth;
                if price <= 0.0 || fsm <= 0.0 {
                    return Value::Error(ErrorValue::Num);
                }
                Value::Number(
                    ((100.0 + fim * rate * 100.0) / (price + fis * rate * 100.0) - 1.0) / fsm,
                )
            }
        }
        _ => Value::Error(ErrorValue::Name),
    }
}

/// The `.INTL` variants of NETWORKDAYS and WORKDAY, where the caller says which
/// days are the weekend.
///
/// `weekend` is either one of Excel's numbered presets or a seven-character
/// mask starting on **Monday** — `"0000011"` is Saturday and Sunday. The mask
/// starting on Monday while `WEEKDAY` counts from Sunday is the trap: reading
/// the mask with a Sunday origin shifts every weekend by a day.
pub(crate) fn eval_workdays_intl(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    advance: bool,
) -> Value {
    if args.len() < 2 || args.len() > 4 {
        return Value::Error(ErrorValue::Value);
    }
    let (start, second) = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok([a, b]) => (a.trunc() as i64, b.trunc()),
        Err(e) => return e,
    };

    // Monday-origin mask, `true` where the day is a weekend.
    let mut mask = [false, false, false, false, false, true, true];
    if let Some(arg) = args.get(2) {
        let value = ev.eval_expr(sheet, arg);
        match &value {
            Value::Text(s) if s.len() == 7 && s.bytes().all(|b| b == b'0' || b == b'1') => {
                for (i, b) in s.bytes().enumerate() {
                    mask[i] = b == b'1';
                }
                if mask.iter().all(|d| *d) {
                    // Every day a weekend never terminates in WORKDAY and
                    // counts nothing in NETWORKDAYS; Excel rejects it.
                    return Value::Error(ErrorValue::Value);
                }
            }
            Value::Number(_) | Value::Bool(_) | Value::Empty => {
                let code = value.as_number().unwrap_or(1.0) as i64;
                // 1..=7 are the two-day weekends starting Sat/Sun, 11..=17 the
                // single-day ones starting Sunday.
                mask = [false; 7];
                match code {
                    1..=7 => {
                        // Preset 1 is Sat+Sun; each step moves the pair on by a
                        // day. In Monday-origin indices, 1 → {5,6}.
                        let first = (code + 3).rem_euclid(7) as usize;
                        mask[first] = true;
                        mask[(first + 1) % 7] = true;
                    }
                    11..=17 => {
                        // 11 is Sunday only, which is index 6 Monday-origin.
                        mask[((code - 11 + 6) % 7) as usize] = true;
                    }
                    _ => return Value::Error(ErrorValue::Num),
                }
            }
            Value::Error(e) => return Value::Error(*e),
            _ => return Value::Error(ErrorValue::Value),
        }
    }

    let holidays: Vec<i64> = match args.get(3) {
        Some(_) => match flatten_numbers(ev, sheet, &args[3..]) {
            Ok(ns) => ns.into_iter().map(|n| n.trunc() as i64).collect(),
            Err(e) => return Value::Error(e),
        },
        None => Vec::new(),
    };
    let is_workday = |serial: i64| {
        // `weekday_of` is Sunday-origin (0 = Sunday); the mask is Monday-origin.
        let monday_origin = (weekday_of(serial) + 6) % 7;
        !mask[monday_origin as usize] && !holidays.contains(&serial)
    };

    if advance {
        let mut remaining = second as i64;
        if remaining == 0 {
            return Value::Number(start as f64);
        }
        let step = if remaining > 0 { 1 } else { -1 };
        let mut at = start;
        let mut guard = 0;
        while remaining != 0 && guard < 4_000_000 {
            at += step;
            if is_workday(at) {
                remaining -= step;
            }
            guard += 1;
        }
        return Value::Number(at as f64);
    }
    let end = second as i64;
    let (lo, hi) = (start.min(end), start.max(end));
    let count = (lo..=hi).filter(|d| is_workday(*d)).count() as f64;
    Value::Number(if end < start { -count } else { count })
}

/// `ACCRINT` — interest accrued on a security that pays periodically.
///
/// `calc_method` decides where accrual starts: `TRUE` (the default) from
/// **issue**, `FALSE` from the first interest date. The difference matters
/// exactly when settlement is past the first coupon, which is when anyone
/// bothers to pass the argument.
pub(crate) fn eval_accrint(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 6 || args.len() > 8 {
        return Value::Error(ErrorValue::Value);
    }
    let v = match opt_numbers(ev, sheet, args, 6, [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (issue, first, settle, rate, par, freq) = (v[0], v[1], v[2], v[3], v[4], v[5]);
    let basis = v[6] as i64;
    let from_issue = v[7] != 0.0;
    if rate <= 0.0 || par <= 0.0 || !matches!(freq as i64, 1 | 2 | 4) || !(0..=4).contains(&basis) {
        return Value::Error(ErrorValue::Num);
    }
    let start = if from_issue { issue } else { first.max(issue) };
    if settle <= start {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(par * rate * year_fraction(start, settle, basis))
}

/// `AMORLINC` and `AMORDEGRC` — the French depreciation systems.
///
/// Both prorate the first period from the purchase date to the end of the first
/// accounting period, which is why they take dates where the other
/// depreciation functions take counts. `AMORDEGRC` additionally applies a
/// coefficient set by the asset's life, and forces 50% then 100% in the last
/// two periods — a rule of the tax code rather than of arithmetic, which is
/// why it cannot be derived and has to be written down.
pub(crate) fn eval_amor(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    degressive: bool,
) -> Value {
    if args.len() < 6 || args.len() > 7 {
        return Value::Error(ErrorValue::Value);
    }
    let v = match opt_numbers(ev, sheet, args, 6, [0.0; 7]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (cost, purchased, first_period, salvage, period, rate) =
        (v[0], v[1], v[2], v[3], v[4], v[5]);
    let basis = v[6] as i64;
    if cost <= 0.0 || rate <= 0.0 || salvage < 0.0 || period < 0.0 || !(0..=4).contains(&basis) {
        return Value::Error(ErrorValue::Num);
    }
    let life = 1.0 / rate;
    let coefficient = if !degressive {
        1.0
    } else {
        // The coefficients are fixed by the life in years, not computed.
        match life {
            l if l < 3.0 => 1.0,
            l if l <= 4.0 => 1.5,
            l if l <= 5.0 => 2.0,
            _ => 2.5,
        }
    };
    let effective_rate = rate * coefficient;
    // The first period runs from purchase to the end of the first accounting
    // period, so it is a fraction of a year rather than a whole one.
    let first_fraction = year_fraction(purchased, first_period, basis);
    let mut book = cost;
    let mut amount = (cost * effective_rate * first_fraction).round();
    if period == 0.0 {
        return Value::Number(amount.min(cost - salvage).max(0.0));
    }
    book -= amount;
    for p in 1..=(period as i64) {
        let remaining_life = life - first_fraction - (p - 1) as f64;
        amount = if !degressive {
            // AMORLINC is *linear*: every full period writes off the same
            // `cost × rate`. Applying the rate to the declining book instead
            // makes it degressive, which is the other function.
            cost * rate
        } else if remaining_life <= 2.0 {
            // The last two periods are forced: half, then whatever is left. A
            // rule of the tax code rather than of arithmetic.
            if remaining_life <= 1.0 {
                book - salvage
            } else {
                (book - salvage) / 2.0
            }
        } else {
            book * effective_rate
        };
        amount = amount.min((book - salvage).max(0.0)).max(0.0);
        if p as f64 == period {
            return Value::Number(amount);
        }
        book -= amount;
    }
    Value::Number(0.0)
}

/// `CELL(info_type, [reference])` — properties of a cell rather than its value.
///
/// Most of the types describe the *reference*, not the value at it, which is
/// why the argument has to stay an expression: evaluating it first would leave
/// only a number and lose the address entirely.
///
/// The types this cannot answer honestly return `#N/A` rather than a plausible
/// value. `"filename"` in particular: there is no path here, and returning an
/// empty string would read as "an unsaved workbook", which is a different
/// claim.
pub(crate) fn eval_cell_info(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let kind = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(t) => t.trim().to_ascii_lowercase(),
        Err(e) => return Value::Error(e),
    };
    // With no reference, Excel reports on the cell holding the formula.
    let target = match args.get(1) {
        Some(Expr::Reference(r)) => match r.resolve(ev.origin()) {
            Some(at) => (r.sheet.clone(), at.row, at.col),
            None => return Value::Error(ErrorValue::Ref),
        },
        Some(Expr::Range(a, _)) => match a.resolve(ev.origin()) {
            Some(at) => (a.sheet.clone(), at.row, at.col),
            None => return Value::Error(ErrorValue::Ref),
        },
        Some(_) => return Value::Error(ErrorValue::Value),
        None => match ev.current_cell() {
            Some((_, at)) => (None, at.row, at.col),
            None => return Value::Error(ErrorValue::Value),
        },
    };
    let (sheet_name, row, col) = target;
    let index = match ev.resolve_sheet(&sheet_name, sheet) {
        Some(i) => i,
        None => return Value::Error(ErrorValue::Ref),
    };
    let at = CellRef::new(row, col);
    let wb = ev.workbook();
    let cell = wb.sheets.get(index).and_then(|s| s.cells.get(at));

    match kind.as_str() {
        "address" => Value::Text(format!(
            "${}${}",
            casual_calc_formula::column_to_letters(col),
            row + 1
        )),
        "col" => Value::Number(col as f64 + 1.0),
        "row" => Value::Number(row as f64 + 1.0),
        "contents" => match cell {
            Some(c) => crate::value::value_from_cell(&c.value, &wb.strings),
            None => Value::Number(0.0),
        },
        // `"type"` is about the *kind* of content: b(lank), l(abel), v(alue).
        "type" => Value::Text(
            match cell.map(|c| &c.value) {
                None | Some(casual_calc_model::CellValue::Empty) => "b",
                Some(casual_calc_model::CellValue::SharedString(_))
                | Some(casual_calc_model::CellValue::InlineString(_)) => "l",
                _ => "v",
            }
            .to_owned(),
        ),
        "prefix" => {
            // The alignment prefix character: ' left, " right, ^ centre, empty
            // for anything else. Reported from the alignment, which is where a
            // reader of the file would find it.
            let style = cell.and_then(|c| c.style).and_then(|id| wb.styles.get(id));
            Value::Text(
                match style.and_then(|s| s.align) {
                    Some(casual_calc_model::HAlign::Left) => "'",
                    Some(casual_calc_model::HAlign::Right) => "\"",
                    Some(casual_calc_model::HAlign::Center) => "^",
                    Some(casual_calc_model::HAlign::Fill) => "\\",
                    _ => "",
                }
                .to_owned(),
            )
        }
        "protect" => {
            // 1 when locked, which is OOXML's default for a cell that says
            // nothing — the same default the protection guard relies on.
            let locked = cell
                .and_then(|c| c.style)
                .and_then(|id| wb.styles.get(id))
                .and_then(|s| s.locked)
                .unwrap_or(true);
            Value::Number(if locked { 1.0 } else { 0.0 })
        }
        "width" => {
            // Excel reports the width in characters of the default font;
            // the model stores it in the same unit scaled by 256, as OOXML
            // does, so it is divided back rather than reported raw.
            const DEFAULT_COL: i64 = 8 * 256;
            let width = wb
                .sheets
                .get(index)
                .map(|s| s.columns.size(col, DEFAULT_COL))
                .unwrap_or(DEFAULT_COL);
            Value::Number((width as f64 / 256.0).round())
        }
        "format" => {
            // Excel's format codes are a small closed vocabulary, not the
            // number-format string. Only the ones that map unambiguously are
            // reported; anything else is "G", which is what Excel gives for a
            // format it has no letter for.
            let code = cell
                .and_then(|c| casual_calc_layout::cell_number_format(wb, c).map(str::to_owned))
                .unwrap_or_default();
            Value::Text(
                if code.is_empty() || code == "General" {
                    "G"
                } else if code.contains('%') {
                    "P0"
                } else if code.contains('$') {
                    "C0"
                } else if code.contains('y') || code.contains('d') {
                    "D1"
                } else if code.contains('h') || code.contains('s') {
                    "D9"
                } else if code.contains("0.00") {
                    "F2"
                } else {
                    "G"
                }
                .to_owned(),
            )
        }
        "color" | "parentheses" => {
            // Both report whether the *negative* section of the format does
            // something special, which needs the section split.
            let code = cell
                .and_then(|c| casual_calc_layout::cell_number_format(wb, c).map(str::to_owned))
                .unwrap_or_default();
            let negative = code.split(';').nth(1).unwrap_or_default();
            let flag = if kind == "color" {
                negative.contains('[')
            } else {
                negative.contains('(')
            };
            Value::Number(if flag { 1.0 } else { 0.0 })
        }
        // No path, no window, nothing to report — and an empty string would
        // read as "an unsaved workbook", which is a different claim.
        "filename" => Value::Error(ErrorValue::Na),
        _ => Value::Error(ErrorValue::Value),
    }
}

pub(crate) fn eval_odd_bond(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    name: &str,
    args: &[Expr],
) -> Value {
    // ODDF* take an issue *and* a first coupon; ODDL* take only a last
    // interest date, so the argument counts differ by one.
    let first = name.starts_with("ODDF");
    let wants = if first { 8 } else { 7 };
    if args.len() < wants || args.len() > wants + 1 {
        return Value::Error(ErrorValue::Value);
    }
    let v = match opt_numbers(ev, sheet, args, wants, [0.0; 9]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (settle, mature) = (v[0].trunc() as i64, v[1].trunc() as i64);
    // ODDF: settle, mature, issue, first_coupon, rate, yld/pr, redemption, freq
    // ODDL: settle, mature, last_interest, rate, yld/pr, redemption, freq
    let (boundary, rate, third, redemption, freq, basis) = if first {
        (
            v[3].trunc() as i64,
            v[4],
            v[5],
            v[6],
            v[7] as i64,
            v[8] as i64,
        )
    } else {
        (
            v[2].trunc() as i64,
            v[3],
            v[4],
            v[5],
            v[6] as i64,
            v[7] as i64,
        )
    };
    if rate < 0.0 || redemption <= 0.0 || !matches!(freq, 1 | 2 | 4) || !(0..=4).contains(&basis) {
        return Value::Error(ErrorValue::Num);
    }
    let terms = OddBond {
        settle,
        mature,
        boundary,
        rate,
        redemption,
        freq,
        basis,
        first,
    };

    if name.ends_with("PRICE") {
        return match odd_price(&terms, third) {
            Some(p) => Value::Number(p),
            None => Value::Error(ErrorValue::Num),
        };
    }
    // The yields: solved against the price function above, so each pair
    // inverts exactly rather than approximately.
    let price = third;
    if price <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    let f = |y: f64| odd_price(&terms, y).map(|p| p - price);
    let (Some(mut lo_v), Some(mut hi_v)) = (f(-0.99), f(10.0)) else {
        return Value::Error(ErrorValue::Num);
    };
    if lo_v * hi_v > 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    let (mut lo, mut hi) = (-0.99, 10.0);
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        let Some(mid_v) = f(mid) else {
            return Value::Error(ErrorValue::Num);
        };
        if lo_v * mid_v <= 0.0 {
            hi = mid;
            hi_v = mid_v;
        } else {
            lo = mid;
            lo_v = mid_v;
        }
        let _ = hi_v;
    }
    Value::Number((lo + hi) / 2.0)
}

/// `MDETERM` — the determinant of a square array.
///
/// LU decomposition with partial pivoting rather than cofactor expansion:
/// expansion is O(n!) and a 10×10 array — small for a spreadsheet — would take
/// millions of operations. Pivoting is what keeps it stable when the leading
/// entry is small.
///
/// This is the one matrix function that returns a scalar, which is why it can
/// land before the array-spilling work the rest of the family needs.
pub(crate) fn eval_mdeterm(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 1 {
        return Value::Error(ErrorValue::Value);
    }
    let grid = match eval_range_2d(ev, sheet, &args[0]) {
        Ok(g) => g,
        Err(e) => return Value::Error(e),
    };
    let n = grid.rows;
    if n == 0 || n != grid.cols {
        return Value::Error(ErrorValue::Value);
    }
    let mut m = vec![0.0f64; n * n];
    for r in 0..n {
        for c in 0..n {
            match grid.get(r, c).as_number() {
                Ok(v) => m[r * n + c] = v,
                Err(e) => return Value::Error(e),
            }
        }
    }

    let mut det = 1.0f64;
    for col in 0..n {
        // Partial pivot: the largest magnitude in the column, which is what
        // stops a small leading entry amplifying rounding error.
        let mut pivot = col;
        for r in (col + 1)..n {
            if m[r * n + col].abs() > m[pivot * n + col].abs() {
                pivot = r;
            }
        }
        if m[pivot * n + col] == 0.0 {
            return Value::Number(0.0); // singular
        }
        if pivot != col {
            for c in 0..n {
                m.swap(col * n + c, pivot * n + c);
            }
            det = -det; // each row exchange flips the sign
        }
        det *= m[col * n + col];
        for r in (col + 1)..n {
            let factor = m[r * n + col] / m[col * n + col];
            for c in col..n {
                m[r * n + c] -= factor * m[col * n + c];
            }
        }
    }
    Value::Number(det)
}

// The `ODD*` bond functions — securities whose first or last coupon period is
// not a whole one.
//
// These are the awkward corner of bond maths, and the property that pins them
// down is that **a regular period must reduce to `PRICE`**: an odd-first bond
// whose first coupon falls exactly one period after issue is an ordinary bond,
// and if the formula says otherwise the formula is wrong. The tests assert
// exactly that rather than quoting numbers I cannot independently check.
//
// The yields are solved against their own price functions, so each pair
// inverts exactly — the same reason `YIELD` is solved against `PRICE`.
//
// A plain comment, not a doc comment: it describes the family below rather than
// the one item that happened to follow it. In the single file the two ran
// together with no blank line, so this prose was rendered as `OddBond`'s own
// documentation — which is not what it says.

/// The terms of an odd-period bond, gathered because nine positional
/// parameters is a signature nobody can call correctly twice.
pub(crate) struct OddBond {
    pub(crate) settle: i64,
    pub(crate) mature: i64,
    /// The first coupon date for an odd *first* bond, the last interest date
    /// for an odd *last* one.
    pub(crate) boundary: i64,
    pub(crate) rate: f64,
    pub(crate) redemption: f64,
    pub(crate) freq: i64,
    pub(crate) basis: i64,
    /// Whether the odd period is the first one.
    pub(crate) first: bool,
}

pub(crate) fn odd_price(b: &OddBond, yld: f64) -> Option<f64> {
    let (settle, mature, boundary) = (b.settle, b.mature, b.boundary);
    let (rate, redemption, freq, basis, first) = (b.rate, b.redemption, b.freq, b.basis, b.first);
    let f = freq as f64;
    let coupon = 100.0 * rate / f;
    let k = 1.0 + yld / f;
    let span = |a: i64, b: i64| year_fraction(a as f64, b as f64, basis) * f;

    if first {
        // `boundary` is the first coupon date. The odd period runs from issue
        // to it; everything after is regular.
        let (_, next) = coupon_period(settle, mature, freq)?;
        let (n, dsc_e, a_e) = bond_terms(settle, mature, freq, basis)?;
        // The odd first coupon is prorated by how long that period actually is.
        let odd_fraction = span(boundary.min(next), boundary).abs().max(0.0);
        let mut price = redemption / k.powf(n - 1.0 + dsc_e);
        // The first coupon carries the odd fraction; the rest are whole.
        price += coupon * (1.0 + odd_fraction) / k.powf(dsc_e);
        for i in 2..=(n as i64) {
            price += coupon / k.powf(i as f64 - 1.0 + dsc_e);
        }
        Some(price - coupon * a_e)
    } else {
        // `boundary` is the last interest date; the odd period runs from it to
        // maturity, and there are no regular periods left after settlement.
        let dcs = span(boundary, mature).max(0.0); // whole odd period
        let dsc = span(settle, mature).max(0.0); // settlement to maturity
        let accrued = span(boundary, settle).max(0.0);
        let denominator = 1.0 + (yld / f) * dsc;
        if denominator == 0.0 {
            return None;
        }
        Some((redemption + coupon * dcs) / denominator - coupon * accrued)
    }
}
