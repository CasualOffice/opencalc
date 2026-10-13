//! What typed text means, the way Excel (en-US) reads it (`XL-FEAT-001`).
//!
//! [`WorkbookSession::input_edit`](crate::WorkbookSession::input_edit) used to
//! recognise only plain numbers and ISO dates, so `50%`, `$5.00`, `1,234.5`,
//! `TRUE`, `(5)` and `12/31/2026` were stored as text: right on screen, and
//! zero to every formula that summed them.
//!
//! This module is a pure function of the text and the session clock. It reads
//! no locale and no wall clock: the locale is fixed at en-US, the product's
//! only locale today, and "this year" for a year-less date (`1/2`, `31-Dec`)
//! comes from [`Environment::now`](crate::Environment::now). With no clock set
//! a year-less date is left as text rather than guessed into 1900.
//!
//! The plain decimal (`42`, `-0.5`) is deliberately **not** handled here; it
//! stays on the original path in `input_edit`, which owns the leading-zero and
//! finiteness rules. ISO `yyyy-mm-dd` is also left to
//! [`casual_calc_io::parse_iso_datetime`] so typing and importing agree.
//!
//! The number format each entry implies is the one Excel applies on entry.
//! Where Excel's exact choice is not certain the choice is noted at the
//! constant.

/// What an entry is, which decides whether an existing cell format survives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A number that implies no format: `(5)`.
    Plain,
    /// `TRUE` / `FALSE`.
    Bool,
    /// `1,234.5` — thousands separators.
    Grouped,
    /// `50%`.
    Percent,
    /// `$5.00`.
    Currency,
    /// `1e3`.
    Scientific,
    /// `0 1/2`.
    Fraction,
    /// A date with no time of day.
    Date,
    /// A time of day with no date.
    Time,
    /// A date and a time.
    DateTime,
}

/// A recognised entry: its value, the format it implies, and its kind.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    /// The number stored; for [`Kind::Bool`] this is 1 or 0 and `boolean` is set.
    pub number: f64,
    pub boolean: Option<bool>,
    /// The number format Excel applies on entry; `None` leaves the cell's own.
    pub format: Option<String>,
    pub kind: Kind,
}

impl Entry {
    fn number(number: f64, format: Option<String>, kind: Kind) -> Option<Self> {
        number.is_finite().then_some(Self {
            number,
            boolean: None,
            format,
            kind,
        })
    }
}

/// Recognise `text` (already trimmed) as Excel en-US would on entry.
///
/// `now` is the session clock as a date serial; below 1 there is no clock.
pub(crate) fn parse(text: &str, now: f64) -> Option<Entry> {
    if text.eq_ignore_ascii_case("TRUE") || text.eq_ignore_ascii_case("FALSE") {
        let b = text.eq_ignore_ascii_case("TRUE");
        return Some(Entry {
            number: f64::from(u8::from(b)),
            boolean: Some(b),
            format: None,
            kind: Kind::Bool,
        });
    }
    if let Some(entry) = numeric(text) {
        return Some(entry);
    }
    if let Some(entry) = fraction(text) {
        return Some(entry);
    }
    date_time(text, now)
}

// ---------------------------------------------------------------- numbers

/// The unsigned body of a number: digits, optional thousands groups, an
/// optional decimal part and an optional exponent.
struct Core {
    value: f64,
    grouped: bool,
    decimals: bool,
    exponent: bool,
}

fn core(text: &str) -> Option<Core> {
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    };
    let (int, frac) = match mantissa.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (mantissa, None),
    };
    if !frac.is_none_or(|f| f.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let grouped = int.contains(',');
    let digits: String = if grouped {
        // Excel en-US: a first group of one to three digits, then groups of
        // exactly three. `1,23` and `1,2345` are text, not numbers.
        let mut groups = int.split(',');
        let first = groups.next()?;
        if first.is_empty() || first.len() > 3 || !first.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let mut all = first.to_owned();
        for g in groups {
            if g.len() != 3 || !g.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            all.push_str(g);
        }
        all
    } else {
        if !int.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        int.to_owned()
    };
    if digits.is_empty() && frac.is_none_or(str::is_empty) {
        return None;
    }
    if let Some(e) = exponent {
        let body = e.strip_prefix(['+', '-']).unwrap_or(e);
        if grouped || body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    let mut normal = digits;
    if let Some(f) = frac {
        normal.push('.');
        normal.push_str(f);
    }
    if let Some(e) = exponent {
        normal.push('e');
        normal.push_str(e);
    }
    let value: f64 = normal.parse().ok()?;
    Some(Core {
        value,
        grouped,
        decimals: frac.is_some(),
        exponent: exponent.is_some(),
    })
}

/// Formats Excel applies on entry. Excel chooses between a whole-number and a
/// two-decimal variant by whether a decimal point was typed, not by how many
/// digits followed it (`12.5%` shows `12.50%`).
const GROUPED: [&str; 2] = ["#,##0", "#,##0.00"];
const PERCENT: [&str; 2] = ["0%", "0.00%"];
/// Excel stores `"$"#,##0` / `"$"#,##0.00` for an entered dollar amount. A
/// negative shows with a minus sign (`-$5`), which is how it was typed.
const CURRENCY: [&str; 2] = ["\"$\"#,##0", "\"$\"#,##0.00"];
const SCIENTIFIC: &str = "0.00E+00";

/// Signed, parenthesised, currency and percent numbers.
fn numeric(text: &str) -> Option<Entry> {
    let mut rest = text;
    let mut negative = false;
    let mut paren = false;
    if let Some(inner) = rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        paren = true;
        negative = true;
        rest = inner;
    }
    let sign = |rest: &mut &str| -> bool {
        if let Some(r) = rest.strip_prefix('-') {
            *rest = r;
            true
        } else if let Some(r) = rest.strip_prefix('+') {
            *rest = r;
            true
        } else {
            false
        }
    };
    // A sign before the dollar (`-$5`) or after it (`$-5`), never both, and
    // never inside parentheses: `(-5)` is text in Excel.
    let before = rest.starts_with('-');
    let signed_before = sign(&mut rest);
    let currency = if let Some(r) = rest.strip_prefix('$') {
        rest = r;
        true
    } else {
        false
    };
    let mut signed_after = false;
    let mut after_negative = false;
    if currency && !signed_before {
        after_negative = rest.starts_with('-');
        signed_after = sign(&mut rest);
    }
    if paren && (signed_before || signed_after) {
        return None;
    }
    if before || after_negative {
        negative = true;
    }
    let percent = if let Some(r) = rest.strip_suffix('%') {
        rest = r;
        true
    } else {
        false
    };
    if currency && percent {
        return None;
    }
    let core = core(rest)?;
    if core.exponent && (currency || percent || paren) {
        return None;
    }
    let mut value = if negative { -core.value } else { core.value };
    let pick = |pair: [&str; 2]| Some(pair[usize::from(core.decimals)].to_owned());
    let (format, kind) = if currency {
        (pick(CURRENCY), Kind::Currency)
    } else if percent {
        value /= 100.0;
        (pick(PERCENT), Kind::Percent)
    } else if core.exponent {
        (Some(SCIENTIFIC.to_owned()), Kind::Scientific)
    } else if core.grouped {
        (pick(GROUPED), Kind::Grouped)
    } else if paren {
        (None, Kind::Plain)
    } else {
        // A plain decimal: the caller's own path, with its leading-zero rule.
        return None;
    };
    Entry::number(value, format, kind)
}

/// `0 1/2`, `-1 3/4`: a whole part, one space, and a fraction. Without the
/// whole part `1/2` is a date in Excel en-US, not a half.
fn fraction(text: &str) -> Option<Entry> {
    let (negative, body) = match text.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (whole, frac) = body.split_once(' ')?;
    let (num, den) = frac.split_once('/')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || !digits(num) || !digits(den) {
        return None;
    }
    let den_value: f64 = den.parse().ok()?;
    if den_value == 0.0 {
        return None;
    }
    let width = den.trim_start_matches('0').len();
    if width == 0 || width > 3 {
        return None;
    }
    let value = whole.parse::<f64>().ok()? + num.parse::<f64>().ok()? / den_value;
    // **The value, without the format Excel would add.** Excel applies `# ?/?`
    // for a one-digit denominator and `# ??/??` for two, but the renderer does
    // not lay out fraction codes yet (`casual-calc-layout` numfmt: "Fraction
    // layout is still unsupported") and draws 0.5 under `# ?/?` as `1  /` — a
    // wrong number on screen, which is worse than `0.5`. Applying it is a
    // one-line change once the renderer can draw it.
    Entry::number(if negative { -value } else { value }, None, Kind::Fraction)
}

// ---------------------------------------------------------------- dates

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

fn month_name(token: &str) -> Option<u32> {
    let lower = token.to_ascii_lowercase();
    MONTHS
        .iter()
        .position(|m| lower == *m || (lower.len() == 3 && m.starts_with(lower.as_str())))
        .map(|i| i as u32 + 1)
}

fn all_digits(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_digit())
}

/// A typed year: four digits as written, or two digits through Excel's window
/// (00–29 is 2000–2029, 30–99 is 1930–1999). Three digits are not a year.
fn year(s: &str) -> Option<i32> {
    let n: i32 = s.parse().ok()?;
    match s.len() {
        1 | 2 => Some(if n < 30 { 2000 + n } else { 1900 + n }),
        4 => Some(n),
        _ => None,
    }
}

/// The Excel serial of a calendar date, or `None` when it does not exist or
/// is outside Excel's range. Validated by the importer's own ISO reader, so
/// the two cannot disagree about a date or about the 1900 leap-year bug.
fn serial(year: i32, month: u32, day: u32) -> Option<f64> {
    if !(1900..=9999).contains(&year) {
        return None;
    }
    let iso = format!("{year:04}-{month:02}-{day:02}");
    casual_calc_io::parse_iso_datetime(&iso).map(|(s, _)| s)
}

/// The calendar year the session clock falls in, or `None` with no clock.
fn current_year(now: f64) -> Option<i32> {
    if !now.is_finite() || now < 1.0 {
        return None;
    }
    let day = now.floor();
    let mut y = 1900 + (day / 365.2425) as i32;
    while y > 1900 && serial(y, 1, 1).is_none_or(|s| s > day) {
        y -= 1;
    }
    while serial(y + 1, 1, 1).is_some_and(|s| s <= day) {
        y += 1;
    }
    (1900..=9999).contains(&y).then_some(y)
}

fn days_in(year: i32, month: u32) -> u32 {
    (28..=31)
        .rev()
        .find(|&d| serial(year, month, d).is_some())
        .unwrap_or(0)
}

/// A month followed by one number (`1/2`, `Dec-26`, `Dec-2026`): a day of this
/// year when it fits the month, otherwise the first of that month in that
/// year — Excel's reading. With no clock a number that could be a day is not
/// guessed at.
fn month_and_number(month: u32, n: &str, now: f64) -> Option<(f64, &'static str)> {
    if !all_digits(n, 4) || !(1..=12).contains(&month) {
        return None;
    }
    let value: u32 = n.parse().ok()?;
    if n.len() <= 2 && (1..=31).contains(&value) {
        if let Some(y) = current_year(now) {
            if value <= days_in(y, month) {
                return Some((serial(y, month, value)?, "d-mmm"));
            }
        } else {
            return None;
        }
    }
    Some((serial(year(n)?, month, 1)?, "mmm-yy"))
}

/// A date alone, as `(serial, format)`.
fn date(text: &str, now: f64) -> Option<(f64, &'static str)> {
    // Numeric: one kind of separator throughout.
    for sep in ['/', '-'] {
        if !text.contains(sep) {
            continue;
        }
        let parts: Vec<&str> = text.split(sep).collect();
        if !parts.iter().all(|p| all_digits(p, 4)) {
            break;
        }
        return match parts.as_slice() {
            // Year first: `2026/12/31`. With dashes it is ISO, which the
            // importer's reader owns.
            [y, m, d] if y.len() == 4 => {
                if sep == '-' || m.len() > 2 || d.len() > 2 {
                    return None;
                }
                Some((
                    serial(year(y)?, m.parse().ok()?, d.parse().ok()?)?,
                    "m/d/yyyy",
                ))
            }
            [m, d, y] if m.len() <= 2 && d.len() <= 2 => Some((
                serial(year(y)?, m.parse().ok()?, d.parse().ok()?)?,
                "m/d/yyyy",
            )),
            [m, n] if m.len() <= 2 => month_and_number(m.parse().ok()?, n, now),
            _ => None,
        };
    }

    // A named month, separated by spaces, dashes, or the comma of
    // `Dec 31, 2026`.
    if !text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | ','))
    {
        return None;
    }
    let tokens: Vec<&str> = text
        .split([' ', '-', ','])
        .filter(|t| !t.is_empty())
        .collect();
    let at = tokens.iter().position(|t| month_name(t).is_some())?;
    let month = month_name(tokens[at])?;
    let day = |s: &str| all_digits(s, 2).then(|| s.parse::<u32>().ok()).flatten();
    match (at, tokens.len()) {
        // `31-Dec-2026`, `31 Dec 2026`
        (1, 3) => Some((
            serial(year(tokens[2])?, month, day(tokens[0])?)?,
            "d-mmm-yy",
        )),
        // `Dec 31, 2026`
        (0, 3) => Some((
            serial(year(tokens[2])?, month, day(tokens[1])?)?,
            "d-mmm-yy",
        )),
        // `31-Dec`
        (1, 2) => {
            let y = current_year(now)?;
            Some((serial(y, month, day(tokens[0])?)?, "d-mmm"))
        }
        // `Dec-26`, `Dec 31`, `Dec-2026`
        (0, 2) => month_and_number(month, tokens[1], now),
        _ => None,
    }
}

/// A time of day: `(fraction, with_seconds, twelve_hour, elapsed)`.
fn time(text: &str) -> Option<(f64, bool, bool, bool)> {
    let upper = text.to_ascii_uppercase();
    let (body, meridiem) = if let Some(b) = upper.strip_suffix("AM") {
        (b.trim_end(), Some(false))
    } else if let Some(b) = upper.strip_suffix("PM") {
        (b.trim_end(), Some(true))
    } else {
        (upper.as_str(), None)
    };
    let mut parts = body.split(':');
    let hours = parts.next()?;
    let minutes = parts.next();
    let seconds = parts.next();
    if parts.next().is_some() {
        return None;
    }
    // `1 PM` is a time; a bare `1` is not.
    if minutes.is_none() && meridiem.is_none() {
        return None;
    }
    let field = |s: Option<&str>| -> Option<u32> {
        match s {
            None => Some(0),
            Some(s) if all_digits(s, 2) => s.parse().ok().filter(|&v| v < 60),
            Some(_) => None,
        }
    };
    if !all_digits(hours, 4) {
        return None;
    }
    let mut h: u32 = hours.parse().ok()?;
    let m = field(minutes)?;
    let s = field(seconds)?;
    match meridiem {
        Some(pm) => {
            if h > 12 {
                return None;
            }
            h %= 12;
            if pm {
                h += 12;
            }
        }
        None if hours.len() > 2 && h < 24 => return None,
        None => {}
    }
    let frac = (f64::from(h) * 3600.0 + f64::from(m) * 60.0 + f64::from(s)) / 86400.0;
    Some((frac, seconds.is_some(), meridiem.is_some(), h >= 24))
}

fn time_format(seconds: bool, twelve_hour: bool) -> &'static str {
    match (seconds, twelve_hour) {
        (false, false) => "h:mm",
        (true, false) => "h:mm:ss",
        (false, true) => "h:mm AM/PM",
        (true, true) => "h:mm:ss AM/PM",
    }
}

fn date_time(text: &str, now: f64) -> Option<Entry> {
    if let Some((frac, seconds, twelve, elapsed)) = time(text) {
        // Past 24 hours Excel keeps the whole duration, shown as `[h]:mm:ss`.
        let format = if elapsed {
            "[h]:mm:ss"
        } else {
            time_format(seconds, twelve)
        };
        return Entry::number(frac, Some(format.to_owned()), Kind::Time);
    }
    if let Some((serial, format)) = date(text, now) {
        return Entry::number(serial, Some(format.to_owned()), Kind::Date);
    }
    // A date, a space, and a time — tried at each space, since the date part
    // may contain spaces of its own (`Dec 31, 2026 1:45 PM`).
    for (at, _) in text.match_indices(' ') {
        let (d, t) = (&text[..at], text[at + 1..].trim_start());
        let Some((frac, seconds, twelve, elapsed)) = time(t) else {
            continue;
        };
        if elapsed {
            continue;
        }
        let Some((day, _)) = date(d.trim_end(), now) else {
            continue;
        };
        let format = format!("m/d/yyyy {}", time_format(seconds, twelve));
        return Entry::number(day + frac, Some(format), Kind::DateTime);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_year_inverts_the_serial() {
        assert_eq!(current_year(46309.0), Some(2026)); // 2026-10-14
        assert_eq!(current_year(46023.0), Some(2026)); // 2026-01-01
        assert_eq!(current_year(46022.0), Some(2025)); // 2025-12-31
        assert_eq!(current_year(1.0), Some(1900));
        assert_eq!(current_year(0.0), None);
    }

    #[test]
    fn a_plain_decimal_is_left_to_the_caller() {
        for text in ["42", "-0.5", "+3", ".5", "007"] {
            assert_eq!(parse(text, 46309.0), None, "{text:?}");
        }
    }
}
