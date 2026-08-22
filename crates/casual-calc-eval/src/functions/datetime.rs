//! Dates and times on the deterministic 1900 serial system.
//!
//! Split out of the single `functions.rs` under `MNT-002`; the section
//! headings that file already carried are the seams.

use super::*;

/// Days from the civil date `(y, m, d)` to 1970-01-01 (Howard Hinnant's
/// algorithm). Proleptic Gregorian; the inverse of [`serial_to_ymd`].
pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The serial Excel gives to a day that never happened.
///
/// Lotus 1-2-3 treated 1900 as a leap year. Excel reproduced the bug on purpose
/// to keep those files' arithmetic working, and every spreadsheet since has
/// reproduced Excel. So serial 60 is **1900-02-29**, a date the Gregorian
/// calendar does not have, and every serial below it is one less than a
/// straight day count would give.
///
/// This engine computed a proleptic Gregorian offset and skipped all of it, so
/// `DATE(1900,1,1)` was 2 where Excel says 1 and `DAY(59)` was 27 where Excel
/// says 28. Wrong only for serials 1–60 — but a workbook holding one of those
/// dates reported the wrong day, and a `DATE()` result written back was off by
/// one against every other spreadsheet.
pub(crate) const PHANTOM_LEAP_DAY: i64 = 60;

/// Convert a civil date to an Excel (1900-system) serial day number.
///
/// The correction lives here and in [`serial_to_ymd`], and deliberately nowhere
/// else: anything derived by subtracting two serials — [`days_in_month`],
/// date differences, the coupon schedules — then inherits Excel's quirk for
/// free and in the same direction. That is why `days_in_month(1900, 2)` reports
/// 29, which is wrong about history and right about Excel.
pub(crate) fn ymd_to_serial(y: i64, m: i64, d: i64) -> i64 {
    // The phantom day itself has no civil date to convert, so it is named
    // rather than computed. `days_from_civil` would roll it to 1900-03-01.
    if (y, m, d) == (1900, 2, 29) {
        return PHANTOM_LEAP_DAY;
    }
    let serial = days_from_civil(y, m, d) + 25_569;
    if serial > PHANTOM_LEAP_DAY {
        serial
    } else {
        serial - 1
    }
}

/// Convert an Excel serial day number to `(year, month, day)`.
pub(crate) fn serial_to_ymd(serial_days: i64) -> (i64, i64, i64) {
    if serial_days == PHANTOM_LEAP_DAY {
        return (1900, 2, 29);
    }
    // Below the phantom day, undo the shift applied on the way in; above it,
    // the two systems already agree.
    let serial_days = if serial_days < PHANTOM_LEAP_DAY {
        serial_days + 1
    } else {
        serial_days
    };
    let mut z = serial_days - 25_569 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    z -= era * 146_097;
    let doe = z;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

pub(crate) fn days_in_month(y: i64, m: i64) -> i64 {
    // Normalize so month 13 rolls to January of the next year, etc.
    let ny = y + (m - 1).div_euclid(12);
    let nm = (m - 1).rem_euclid(12) + 1;
    let next = if nm == 12 {
        ymd_to_serial(ny + 1, 1, 1)
    } else {
        ymd_to_serial(ny, nm + 1, 1)
    };
    next - ymd_to_serial(ny, nm, 1)
}

/// `DATE(year, month, day)`. Month/day overflow rolls into adjacent months
/// (Excel semantics); years 0-1899 are offset by 1900.
pub(crate) fn eval_date(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let mut year = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let month = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let day = match ev.eval_expr(sheet, &args[2]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    if (0..1900).contains(&year) {
        year += 1900;
    }
    // Normalize the month into 1..=12, carrying into the year, then add the
    // day offset (which itself may push across month boundaries).
    let ny = year + (month - 1).div_euclid(12);
    let nm = (month - 1).rem_euclid(12) + 1;
    let serial = ymd_to_serial(ny, nm, 1) + (day - 1);
    if serial < 0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(serial as f64)
}

#[derive(Clone, Copy)]
pub(crate) enum DatePart {
    Year,
    Month,
    Day,
}

pub(crate) fn eval_date_part(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    part: DatePart,
) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let serial = match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    if serial < 0 {
        return Value::Error(ErrorValue::Num);
    }
    let (y, m, d) = serial_to_ymd(serial);
    let out = match part {
        DatePart::Year => y,
        DatePart::Month => m,
        DatePart::Day => d,
    };
    Value::Number(out as f64)
}

/// `WEEKDAY(serial, [type])`. Type 1 (default) Sun=1..Sat=7, type 2
/// Mon=1..Sun=7, type 3 Mon=0..Sun=6.
pub(crate) fn eval_weekday(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let serial = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    let kind = match args.get(1) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n as i64,
            Err(e) => return Value::Error(e),
        },
        None => 1,
    };
    if serial < 0 {
        return Value::Error(ErrorValue::Num);
    }
    // Days since the Unix epoch; 1970-01-01 was a Thursday.
    let unix = serial - 25_569;
    let dow_sun0 = (unix + 4).rem_euclid(7); // 0 = Sunday .. 6 = Saturday
    let out = match kind {
        1 => dow_sun0 + 1,
        2 => (dow_sun0 + 6).rem_euclid(7) + 1,
        3 => (dow_sun0 + 6).rem_euclid(7),
        _ => return Value::Error(ErrorValue::Num),
    };
    Value::Number(out as f64)
}

/// `EDATE` (`eomonth` false) advances by whole months keeping the day (clamped
/// to the month length). `EOMONTH` (true) returns the last day of that month.
pub(crate) fn eval_edate(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    eomonth: bool,
) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let serial = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    let months = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    if serial < 0 {
        return Value::Error(ErrorValue::Num);
    }
    let (y, m, d) = serial_to_ymd(serial);
    let total = m - 1 + months;
    let ny = y + total.div_euclid(12);
    let nm = total.rem_euclid(12) + 1;
    let last = days_in_month(ny, nm);
    let day = if eomonth { last } else { d.min(last) };
    let out = ymd_to_serial(ny, nm, day);
    if out < 0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(out as f64)
}

// --- M6-2 built-ins --------------------------------------------------------

/// `TIME(h, m, s)` — a fraction of a day.
///
/// The components roll over rather than erroring: `TIME(25,0,0)` is 1:00, which
/// is what makes the function usable for arithmetic.
pub(crate) fn eval_time(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let mut parts = [0.0f64; 3];
    for (i, slot) in parts.iter_mut().enumerate() {
        match ev.eval_expr(sheet, &args[i]).as_number() {
            Ok(v) => *slot = v.trunc(),
            Err(e) => return Value::Error(e),
        }
    }
    let seconds = parts[0] * 3600.0 + parts[1] * 60.0 + parts[2];
    if seconds < 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((seconds % 86_400.0) / 86_400.0)
}

/// `HOUR`/`MINUTE`/`SECOND` — the component of a serial's time-of-day.
pub(crate) fn eval_time_part(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    unit: f64,
) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let serial = match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    if serial < 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    // Round to the nearest second before splitting: a time stored as a binary
    // fraction is very often a hair under, so truncating raw gives 59 minutes
    // where the sheet plainly shows 60.
    let seconds = ((serial - serial.floor()) * 86_400.0).round() as i64;
    // `seconds` is within one day, so the hour needs no wrap; minutes and
    // seconds take the remainder within the next larger unit.
    let value = match unit as i64 {
        3600 => seconds / 3600,
        60 => (seconds / 60) % 60,
        _ => seconds % 60,
    };
    Value::Number(value as f64)
}

/// `DAYS360(start, end, [european])` — the 360-day year used in bond maths.
pub(crate) fn eval_days360(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let (start, end) = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok([a, b]) => (a, b),
        Err(e) => return e,
    };
    let european = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_bool() {
            Ok(b) => b,
            Err(e) => return Value::Error(e),
        },
        None => false,
    };
    let (y1, m1, mut d1) = serial_to_ymd(start.trunc() as i64);
    let (y2, m2, mut d2) = serial_to_ymd(end.trunc() as i64);
    if european {
        d1 = d1.min(30);
        d2 = d2.min(30);
    } else {
        // The US convention: only after clamping the start does a 31st end date
        // move, which is why these two cannot be written symmetrically.
        if d1 == 31 {
            d1 = 30;
        }
        if d2 == 31 && d1 == 30 {
            d2 = 30;
        }
    }
    Value::Number(((y2 - y1) * 360 + (m2 - m1) * 30 + (d2 - d1)) as f64)
}

/// `DATEDIF(start, end, unit)` — whole years, months or days between dates.
pub(crate) fn eval_datedif(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let (start, end) = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok([a, b]) => (a.trunc() as i64, b.trunc() as i64),
        Err(e) => return e,
    };
    let unit = match ev.eval_expr(sheet, &args[2]) {
        Value::Text(t) => t.to_ascii_uppercase(),
        Value::Error(e) => return Value::Error(e),
        _ => return Value::Error(ErrorValue::Value),
    };
    if end < start {
        // Excel reports #NUM! rather than a negative span.
        return Value::Error(ErrorValue::Num);
    }
    let (y1, m1, d1) = serial_to_ymd(start);
    let (y2, m2, d2) = serial_to_ymd(end);
    let mut months = (y2 - y1) * 12 + (m2 - m1);
    if d2 < d1 {
        months -= 1;
    }
    Value::Number(match unit.as_str() {
        "D" => (end - start) as f64,
        "M" => months as f64,
        "Y" => (months / 12) as f64,
        // Months ignoring years, days ignoring months, days ignoring years.
        "YM" => (months % 12) as f64,
        "MD" => {
            let anchor = ymd_to_serial(y2, m2 - i64::from(d2 < d1), d1);
            (end - anchor) as f64
        }
        "YD" => {
            let anchor = ymd_to_serial(y2 - i64::from((m2, d2) < (m1, d1)), m1, d1);
            (end - anchor) as f64
        }
        _ => return Value::Error(ErrorValue::Num),
    })
}

/// The weekday of a serial, 0 = Sunday.
pub(crate) fn weekday_of(serial: i64) -> i64 {
    // Serial 1 is 1900-01-01, a Monday under Excel's calendar.
    (serial + 6).rem_euclid(7)
}

/// `WEEKNUM(serial, [type])` — the week of the year, counting from the week
/// containing 1 January.
pub(crate) fn eval_weeknum(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let serial = match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    let start_day = match args.get(1) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n as i64,
            Err(e) => return Value::Error(e),
        },
        None => 1,
    };
    // Types 1 and 17 start on Sunday, 2 and 11 on Monday, 12..=17 on the day
    // (type - 10). ISO week numbering is type 21 and is ISOWEEKNUM's job.
    let first_weekday = match start_day {
        1 | 17 => 0,
        2 | 11 => 1,
        12..=16 => (start_day - 10) % 7,
        21 => return eval_isoweeknum(ev, sheet, &args[..1]),
        _ => return Value::Error(ErrorValue::Num),
    };
    let (year, _, _) = serial_to_ymd(serial);
    let jan1 = ymd_to_serial(year, 1, 1);
    let offset = (weekday_of(jan1) - first_weekday).rem_euclid(7);
    Value::Number(((serial - jan1 + offset) / 7 + 1) as f64)
}

/// `ISOWEEKNUM` — ISO 8601 weeks, which start on Monday and belong to the year
/// containing their Thursday.
pub(crate) fn eval_isoweeknum(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let serial = match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    // Shift to the Thursday of this week; its year is the ISO week-year, which
    // is what makes 1 January sometimes belong to week 52 of the year before.
    let iso_weekday = (weekday_of(serial) + 6).rem_euclid(7); // 0 = Monday
    let thursday = serial - iso_weekday + 3;
    let (year, _, _) = serial_to_ymd(thursday);
    let jan1 = ymd_to_serial(year, 1, 1);
    Value::Number(((thursday - jan1) / 7 + 1) as f64)
}

/// `YEARFRAC(start, end, [basis])` — the fraction of a year between two dates.
pub(crate) fn eval_yearfrac(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let (start, end) = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok([a, b]) => (a.trunc() as i64, b.trunc() as i64),
        Err(e) => return e,
    };
    let basis = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n as i64,
            Err(e) => return Value::Error(e),
        },
        None => 0,
    };
    let (lo, hi) = (start.min(end), start.max(end));
    let days = (hi - lo) as f64;
    let frac = match basis {
        // The day-count conventions. Getting one wrong gives an answer that is
        // close enough to look right and wrong enough to matter in interest.
        0 => {
            let d = match eval_days360_serials(lo, hi, false) {
                Some(d) => d,
                None => return Value::Error(ErrorValue::Num),
            };
            d as f64 / 360.0
        }
        1 => days / average_year_length(lo, hi),
        2 => days / 360.0,
        3 => days / 365.0,
        4 => {
            let d = match eval_days360_serials(lo, hi, true) {
                Some(d) => d,
                None => return Value::Error(ErrorValue::Num),
            };
            d as f64 / 360.0
        }
        _ => return Value::Error(ErrorValue::Num),
    };
    Value::Number(frac)
}

/// The 360-day span between two serials, shared by DAYS360 and YEARFRAC.
pub(crate) fn eval_days360_serials(start: i64, end: i64, european: bool) -> Option<i64> {
    let (y1, m1, mut d1) = serial_to_ymd(start);
    let (y2, m2, mut d2) = serial_to_ymd(end);
    if european {
        d1 = d1.min(30);
        d2 = d2.min(30);
    } else {
        if d1 == 31 {
            d1 = 30;
        }
        if d2 == 31 && d1 == 30 {
            d2 = 30;
        }
    }
    Some((y2 - y1) * 360 + (m2 - m1) * 30 + (d2 - d1))
}

/// The actual/actual basis divisor: the mean length of the years spanned.
pub(crate) fn average_year_length(start: i64, end: i64) -> f64 {
    let (y1, _, _) = serial_to_ymd(start);
    let (y2, _, _) = serial_to_ymd(end);
    let mut total = 0.0;
    for year in y1..=y2 {
        total += if is_leap(year) { 366.0 } else { 365.0 };
    }
    total / ((y2 - y1 + 1) as f64)
}

pub(crate) fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// `NETWORKDAYS` (count) and `WORKDAY` (advance), which share their weekend and
/// holiday handling.
pub(crate) fn eval_workdays(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    advance: bool,
) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let (start, second) = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok([a, b]) => (a.trunc() as i64, b.trunc()),
        Err(e) => return e,
    };
    let holidays: Vec<i64> = match args.get(2) {
        Some(_) => match flatten_numbers(ev, sheet, &args[2..]) {
            Ok(ns) => ns.into_iter().map(|n| n.trunc() as i64).collect(),
            Err(e) => return Value::Error(e),
        },
        None => Vec::new(),
    };
    let is_workday = |serial: i64| {
        let day = weekday_of(serial);
        day != 0 && day != 6 && !holidays.contains(&serial)
    };

    if advance {
        let mut remaining = second as i64;
        let step = if remaining >= 0 { 1 } else { -1 };
        let mut at = start;
        while remaining != 0 {
            at += step;
            if is_workday(at) {
                remaining -= step;
            }
        }
        return Value::Number(at as f64);
    }
    // NETWORKDAYS counts inclusively at both ends and is symmetric: a reversed
    // pair returns the same magnitude, negated.
    let end = second as i64;
    let (lo, hi) = (start.min(end), start.max(end));
    let count = (lo..=hi).filter(|d| is_workday(*d)).count() as f64;
    Value::Number(if end < start { -count } else { count })
}

// --- Lookup and reference helpers ------------------------------------------

/// The coupon period bracketing `settlement`, as `(previous, next)` serials.
///
/// Coupons are counted **backwards from maturity**, not forwards from issue —
/// a bond's last payment lands on its maturity date, and stepping forwards from
/// an assumed start puts every date a few days out whenever the month lengths
/// differ. Excel counts back, and so does this.
///
/// The day-of-month is taken from maturity and clamped to each month's length,
/// so a bond maturing on the 31st pays on the 30th in a 30-day month and comes
/// back to the 31st afterwards, rather than drifting earlier every period.
pub(crate) fn coupon_period(settlement: i64, maturity: i64, frequency: i64) -> Option<(i64, i64)> {
    if !matches!(frequency, 1 | 2 | 4) || settlement >= maturity {
        return None;
    }
    let months = 12 / frequency;
    let (my, mm, md) = serial_to_ymd(maturity);
    // Step back a period at a time until the date is at or before settlement.
    // Bounded by the periods in a century, so a nonsensical pair cannot spin.
    let step = |k: i64| -> i64 {
        let total = my * 12 + (mm - 1) - k * months;
        let (y, m) = (total.div_euclid(12), total.rem_euclid(12) + 1);
        let last = days_in_month(y, m);
        ymd_to_serial(y, m, md.min(last))
    };
    let mut k = 0;
    while k < 1200 {
        let date = step(k);
        if date <= settlement {
            return Some((date, step(k - 1)));
        }
        k += 1;
    }
    None
}

/// The six `COUP*` functions, which all answer questions about the coupon
/// schedule and therefore all derive from the same one.
///
/// On bases 0 and 4 (the 30/360 conventions) a coupon period is 360/frequency
/// days *by definition* — the whole point of a 30/360 basis is that every
/// period is the same length — so the period length is not measured from the
/// calendar. Measuring it would make COUPDAYS disagree with COUPDAYBS +
/// COUPDAYSNC, which must sum to it.
pub(crate) fn eval_coupon(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    name: &str,
    args: &[Expr],
) -> Value {
    if args.len() < 3 || args.len() > 4 {
        return Value::Error(ErrorValue::Value);
    }
    let [settle, mature, frequency, basis] =
        match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    let basis = basis as i64;
    if !(0..=4).contains(&basis) {
        return Value::Error(ErrorValue::Num);
    }
    let (settle, mature) = (settle.trunc() as i64, mature.trunc() as i64);
    let Some((prev, next)) = coupon_period(settle, mature, frequency as i64) else {
        return Value::Error(ErrorValue::Num);
    };
    let freq = frequency as i64;

    // 30/360 bases define the period; the others measure it.
    let period_days = |a: i64, b: i64| -> f64 {
        match basis {
            0 => eval_days360_serials(a, b, false).unwrap_or(0) as f64,
            4 => eval_days360_serials(a, b, true).unwrap_or(0) as f64,
            _ => (b - a) as f64,
        }
    };

    match name {
        "COUPPCD" => Value::Number(prev as f64),
        "COUPNCD" => Value::Number(next as f64),
        "COUPDAYBS" => Value::Number(period_days(prev, settle)),
        "COUPDAYSNC" => Value::Number(period_days(settle, next)),
        "COUPDAYS" => Value::Number(match basis {
            0 | 4 => 360.0 / freq as f64,
            // Basis 1 measures the actual period; 2 and 3 use their fixed year
            // divided by the frequency, which is what Excel reports.
            1 => (next - prev) as f64,
            2 => 360.0 / freq as f64,
            _ => 365.0 / freq as f64,
        }),
        "COUPNUM" => {
            // Whole periods from settlement to maturity, counting the one that
            // ends at `next`.
            let (my, mm, _) = serial_to_ymd(mature);
            let (ny, nm, _) = serial_to_ymd(next);
            let months = (my * 12 + mm) - (ny * 12 + nm);
            Value::Number((months / (12 / freq)) as f64 + 1.0)
        }
        _ => Value::Error(ErrorValue::Name),
    }
}

/// `DATEVALUE` / `TIMEVALUE` — text to a serial.
///
/// Only the unambiguous forms are accepted. `03/04/2024` is 3 April in most of
/// the world and 4 March in the United States, and there is no locale here to
/// decide; guessing would silently produce the wrong date a third of the time,
/// so it is `#VALUE!` instead.
pub(crate) fn eval_datevalue(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    time: bool,
) -> Value {
    if args.len() != 1 {
        return Value::Error(ErrorValue::Value);
    }
    let text = match ev.eval_expr(sheet, &args[0]) {
        Value::Error(e) => return Value::Error(e),
        v => v.as_text().unwrap_or_default(),
    };
    let text = text.trim();
    if time {
        return match parse_time_text(text) {
            Some(f) => Value::Number(f),
            None => Value::Error(ErrorValue::Value),
        };
    }
    match parse_date_text(text) {
        // A date carries no time of day, so the serial is whole — DATEVALUE
        // discards any time in the text, as Excel does.
        Some(serial) => Value::Number(serial as f64),
        None => Value::Error(ErrorValue::Value),
    }
}

/// `YYYY-MM-DD` (ISO) or `D-MMM-YYYY` / `MMM D, YYYY`, which name their month.
pub(crate) fn parse_date_text(text: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let head = text.split_whitespace().next().unwrap_or(text);
    let iso: Vec<&str> = head.split('-').collect();
    if iso.len() == 3
        && iso[0].len() == 4
        && let (Ok(y), Ok(m), Ok(d)) = (
            iso[0].parse::<i64>(),
            iso[1].parse::<i64>(),
            iso[2].parse::<i64>(),
        )
    {
        return valid_ymd(y, m, d);
    }
    // Named-month forms, in either order, with any of space, `-` or `,`.
    let parts: Vec<String> = text
        .split(|c: char| c.is_whitespace() || c == '-' || c == ',')
        .filter(|p| !p.is_empty())
        .map(|p| p.to_ascii_lowercase())
        .collect();
    if parts.len() != 3 {
        return None;
    }
    let month = parts
        .iter()
        .position(|p| MONTHS.iter().any(|m| p.starts_with(m)))?;
    let m = MONTHS.iter().position(|m| parts[month].starts_with(m))? as i64 + 1;
    let others: Vec<i64> = parts
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != month)
        .map(|(_, p)| p.parse::<i64>().unwrap_or(-1))
        .collect();
    if others.iter().any(|n| *n < 0) {
        return None;
    }
    // Whichever number cannot be a day is the year; otherwise the year is the
    // one after the month, as in "May 17, 2024".
    let (y, d) = if others[0] > 31 {
        (others[0], others[1])
    } else {
        (others[1], others[0])
    };
    valid_ymd(if y < 100 { y + 2000 } else { y }, m, d)
}

/// A serial, or `None` when the date does not exist — 31 February included.
pub(crate) fn valid_ymd(y: i64, m: i64, d: i64) -> Option<i64> {
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) || !(1900..=9999).contains(&y) {
        return None;
    }
    Some(ymd_to_serial(y, m, d))
}

/// `h:mm[:ss] [AM|PM]` as a fraction of a day.
pub(crate) fn parse_time_text(text: &str) -> Option<f64> {
    let lower = text.to_ascii_lowercase();
    let pm = lower.contains("pm");
    let am = lower.contains("am");
    let body = lower.replace("am", "").replace("pm", "");
    let parts: Vec<&str> = body.trim().split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let h: f64 = parts[0].trim().parse().ok()?;
    let m: f64 = parts.get(1).map_or(Ok(0.0), |p| p.trim().parse()).ok()?;
    let s: f64 = parts.get(2).map_or(Ok(0.0), |p| p.trim().parse()).ok()?;
    if !(0.0..60.0).contains(&m) || !(0.0..60.0).contains(&s) {
        return None;
    }
    // 12 AM is midnight and 12 PM is noon — the one case where adding 12 hours
    // for PM gives the wrong answer.
    let hour = if am || pm {
        if !(1.0..=12.0).contains(&h) {
            return None;
        }
        match (pm, h) {
            (true, 12.0) => 12.0,
            (true, _) => h + 12.0,
            (false, 12.0) => 0.0,
            (false, _) => h,
        }
    } else {
        if !(0.0..=24.0).contains(&h) {
            return None;
        }
        h
    };
    Some((hour * 3600.0 + m * 60.0 + s) / 86400.0)
}

/// The volatile functions: the clock and the random generator.
///
/// Both read state the host supplies rather than the machine's, which is what
/// keeps a recalculation reproducible — see `Workbook::volatile_now`.
pub(crate) fn eval_volatile(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    name: &str,
    args: &[Expr],
) -> Value {
    match name {
        "TODAY" | "NOW" => {
            if !args.is_empty() {
                return Value::Error(ErrorValue::Value);
            }
            let now = ev.now_serial();
            Value::Number(if name == "TODAY" { now.floor() } else { now })
        }
        "RAND" => {
            if !args.is_empty() {
                return Value::Error(ErrorValue::Value);
            }
            Value::Number(ev.next_random())
        }
        "RANDBETWEEN" => {
            let [lo, hi] = match pair_of_numbers(ev, sheet, args) {
                Ok(v) => v,
                Err(e) => return e,
            };
            let (lo, hi) = (lo.ceil(), hi.floor());
            if hi < lo {
                return Value::Error(ErrorValue::Num);
            }
            let span = hi - lo + 1.0;
            // Both ends inclusive, so the draw is scaled across the whole span
            // and floored — clamping guards the one-in-2^53 case where the
            // draw rounds up to exactly 1.
            Value::Number((lo + (ev.next_random() * span).floor()).min(hi))
        }
        _ => Value::Error(ErrorValue::Name),
    }
}
