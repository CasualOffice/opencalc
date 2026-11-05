//! What typed text means, the way Excel (en-US) reads it (`XL-FEAT-001`).
//!
//! **One reader for typing and for formulas** (`XL-FEAT-019`). The SDK's
//! `input_edit` uses [`parse`] to decide what a typed entry is; the calc
//! engine uses [`value_of`] for `VALUE`, `DATEVALUE`, `TIMEVALUE` and the
//! coercion an operator applies to text (`="1,234"*1`). It lives here, below
//! both, because a second copy is how `1,234` came to be a number when typed
//! and `#VALUE!` inside `VALUE` — the two had drifted before either was
//! wrong on its own. It moved down from `casual-calc-sdk`, which the engine
//! cannot depend on.
//!
//! `WorkbookSession::input_edit` used to
//! recognise only plain numbers and ISO dates, so `50%`, `$5.00`, `1,234.5`,
//! `TRUE`, `(5)` and `12/31/2026` were stored as text: right on screen, and
//! zero to every formula that summed them.
//!
//! This module is a pure function of the text and the session clock. It reads
//! no locale and no wall clock: the locale is fixed at en-US, the product's
//! only locale today, and "this year" for a year-less date (`1/2`, `31-Dec`)
//! comes from the session clock (the SDK's `Environment::now`). With no clock set
//! a year-less date is left as text rather than guessed into 1900.
//!
//! The plain decimal (`42`, `-0.5`) is deliberately **not** handled here; it
//! stays on the original path in `input_edit`, which owns the leading-zero and
//! finiteness rules. ISO `yyyy-mm-dd` is also left to
//! [`crate::parse_iso_datetime`] so typing and importing agree. [`value_of`]
//! folds both back in for a formula, which has no leading-zero rule to keep.
//!
//! The number format each entry implies is the one Excel applies on entry.
//! Where Excel's exact choice is not certain the choice is noted at the
//! constant.

/// What an entry is, which decides whether an existing cell format survives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
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
pub struct Entry {
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
pub fn parse(text: &str, now: f64) -> Option<Entry> {
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
    crate::parse_iso_datetime(&iso).map(|(s, _)| s)
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
            // Year first: `2026/12/31`. With dashes and both fields padded
            // it is ISO, which the importer's reader owns; `2024-5-7` is not
            // ISO but is still a date to Excel en-US, so it is read here.
            [y, m, d] if y.len() == 4 => {
                let iso = sep == '-' && m.len() == 2 && d.len() == 2;
                if iso || m.len() > 2 || d.len() > 2 {
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

/// A time of day as typed.
struct Clock {
    /// The fraction of a day; past 1 for an elapsed `25:00`.
    fraction: f64,
    seconds: bool,
    /// The seconds carried a decimal part: `13:30:15.5`.
    tenths: bool,
    twelve_hour: bool,
    elapsed: bool,
}

/// `h:mm`, `h:mm:ss`, `h:mm:ss.0`, each with an optional `AM`/`PM`.
fn time(text: &str) -> Option<Clock> {
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
    // Only the seconds may carry a decimal part, and it needs a digit after
    // the point: `13:30:15.` and `13:30.5` are text.
    let (whole, tenths) = match seconds.map(|s| s.split_once('.')) {
        Some(Some((w, f))) if !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()) => {
            (Some(w), Some(f))
        }
        Some(Some(_)) => return None,
        Some(None) => (seconds, None),
        None => (None, None),
    };
    let s = f64::from(field(whole)?)
        + match tenths {
            Some(f) => format!("0.{f}").parse::<f64>().ok()?,
            None => 0.0,
        };
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
    let fraction = (f64::from(h) * 3600.0 + f64::from(m) * 60.0 + s) / 86400.0;
    Some(Clock {
        fraction,
        seconds: seconds.is_some(),
        tenths: tenths.is_some(),
        twelve_hour: meridiem.is_some(),
        elapsed: h >= 24,
    })
}

/// The format Excel applies to a typed time.
///
/// **Fractional seconds: `h:mm:ss.0` is this product's choice, not a verified
/// Excel one.** Excel appears to apply `mm:ss.0` (built-in format 47), which
/// hides the hour; until that is checked against Excel the hour stays on
/// screen. The value is exact either way.
fn time_format(clock: &Clock) -> &'static str {
    match (clock.seconds, clock.tenths, clock.twelve_hour) {
        (false, _, false) => "h:mm",
        (true, false, false) => "h:mm:ss",
        (true, true, false) => "h:mm:ss.0",
        (false, _, true) => "h:mm AM/PM",
        (true, false, true) => "h:mm:ss AM/PM",
        (true, true, true) => "h:mm:ss.0 AM/PM",
    }
}

/// A date, a space, and a time — tried at each space, since the date part may
/// contain spaces of its own (`Dec 31, 2026 1:45 PM`). The two halves are
/// returned apart: their sum has already rounded the time away to the date's
/// magnitude, which `TIMEVALUE` must not inherit.
///
/// A padded ISO date is taken here only when the whole text is not ISO
/// (`2024-05-07 1:30 PM`): `2024-05-07 13:30` stays the importer's, with the
/// format it gives.
fn date_then_time(text: &str, now: f64) -> Option<(f64, Clock)> {
    let iso_date = |d: &str| {
        crate::parse_iso_datetime(text)
            .is_none()
            .then(|| crate::parse_iso_datetime(d))
            .flatten()
            .filter(|(_, code)| *code == "yyyy-mm-dd")
            .map(|(day, _)| day)
    };
    for (at, _) in text.match_indices(' ') {
        let (d, t) = (&text[..at], text[at + 1..].trim_start());
        let Some(clock) = time(t) else {
            continue;
        };
        if clock.elapsed {
            continue;
        }
        let d = d.trim_end();
        let Some(day) = date(d, now).map(|(day, _)| day).or_else(|| iso_date(d)) else {
            continue;
        };
        return Some((day, clock));
    }
    None
}

fn date_time(text: &str, now: f64) -> Option<Entry> {
    if let Some(clock) = time(text) {
        // Past 24 hours Excel keeps the whole duration, shown as `[h]:mm:ss`.
        let format = if clock.elapsed {
            "[h]:mm:ss"
        } else {
            time_format(&clock)
        };
        return Entry::number(clock.fraction, Some(format.to_owned()), Kind::Time);
    }
    if let Some((serial, format)) = date(text, now) {
        return Entry::number(serial, Some(format.to_owned()), Kind::Date);
    }
    let (day, clock) = date_then_time(text, now)?;
    let format = format!("m/d/yyyy {}", time_format(&clock));
    Entry::number(day + clock.fraction, Some(format), Kind::DateTime)
}

// ---------------------------------------------------------------- formulas

/// What `text` is worth as a number inside a formula: `VALUE`, `DATEVALUE`,
/// `TIMEVALUE`, and the coercion `="1,234"*1` applies.
///
/// Everything [`parse`] reads, plus the two forms it leaves to its caller —
/// the plain decimal and ISO `yyyy-mm-dd` — so a formula reads exactly what
/// typing would have stored. Two differences from entry, both Excel's:
///
/// - **Text that says `TRUE` is not a number.** Typing `TRUE` stores a
///   logical; `="TRUE"*1` and `=VALUE("TRUE")` are `#VALUE!`.
/// - **A leading zero is just a digit.** Entry keeps `007` apart for its own
///   reasons (`input_edit`); `=VALUE("007")` is 7.
///
/// The kind says which of a date and a time the text named, which is what
/// `DATEVALUE` and `TIMEVALUE` need to refuse a plain number. `now` is the
/// clock for a year-less date, as in [`parse`]; below 1 there is none.
pub fn value_of(text: &str, now: f64) -> Option<(f64, Kind)> {
    let text = text.trim();
    if let Some(entry) = parse(text, now) {
        return entry
            .boolean
            .is_none()
            .then_some((entry.number, entry.kind));
    }
    if let Some((serial, code)) = crate::parse_iso_datetime(text) {
        let kind = match (code.starts_with("yyyy"), code.contains("hh")) {
            (true, true) => Kind::DateTime,
            (true, false) => Kind::Date,
            (false, _) => Kind::Time,
        };
        return Some((serial, kind));
    }
    plain_decimal(text).map(|n| (n, Kind::Plain))
}

/// The date and the time of day that formula text names, each read on its
/// own: `(day serial, time fraction)`, with `None` for a half the text does not
/// have. `None` overall when the text names neither (`5`, `1,234`).
///
/// `DATEVALUE` and `TIMEVALUE` keep one half each. They cannot take it from
/// [`value_of`]'s sum: near serial 45 000 a double keeps only about eleven
/// significant digits of the time, so `fract()` of the sum disagrees with the
/// time read alone from the eleventh digit — enough for `=` to tell them apart.
pub fn date_and_time_of(text: &str, now: f64) -> Option<(Option<f64>, Option<f64>)> {
    let text = text.trim();
    let (serial, kind) = value_of(text, now)?;
    match kind {
        Kind::Date => Some((Some(serial), None)),
        Kind::Time => Some((None, Some(serial))),
        Kind::DateTime => {
            if let Some((day, clock)) = date_then_time(text, now) {
                return Some((Some(day), Some(clock.fraction)));
            }
            // ISO: `yyyy-mm-dd hh:mm` or `yyyy-mm-ddThh:mm`, each half
            // through the importer's own reader.
            let (d, t) = text.split_once(['T', ' '])?;
            let day = crate::parse_iso_datetime(d)?.0;
            let fraction = crate::parse_iso_datetime(t)?.0;
            Some((Some(day), Some(fraction)))
        }
        _ => None,
    }
}

/// `42`, `-0.5`, `+3`, `.5`, `007`, `5.`: a sign, digits and one point.
///
/// Spelled out rather than left to `f64::from_str`, which also reads `inf`,
/// `infinity` and `NaN` — none of which is a number to Excel, and the first
/// two are values no workbook can store.
fn plain_decimal(text: &str) -> Option<f64> {
    let body = text.strip_prefix(['+', '-']).unwrap_or(text);
    let mut points = 0;
    let mut digits = 0;
    for b in body.bytes() {
        match b {
            b'0'..=b'9' => digits += 1,
            b'.' => points += 1,
            _ => return None,
        }
    }
    if digits == 0 || points > 1 {
        return None;
    }
    text.parse::<f64>().ok().filter(|n| n.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_formula_reads_what_typing_reads() {
        assert_eq!(value_of("1,234", 0.0), Some((1234.0, Kind::Grouped)));
        assert_eq!(value_of(" 42 ", 0.0), Some((42.0, Kind::Plain)));
        assert_eq!(value_of("007", 0.0), Some((7.0, Kind::Plain)));
        assert_eq!(value_of("2026-12-31", 0.0), Some((46387.0, Kind::Date)));
        assert_eq!(value_of("12/31/2026", 0.0), Some((46387.0, Kind::Date)));
        assert_eq!(value_of("13:30", 0.0), Some((13.5 / 24.0, Kind::Time)));
        for text in [
            "TRUE",
            "inf",
            "-infinity",
            "NaN",
            "1e400",
            "",
            ".",
            "+",
            "1.2.3",
        ] {
            assert_eq!(value_of(text, 0.0), None, "{text:?}");
        }
    }

    #[test]
    fn typing_reads_an_unpadded_iso_date() {
        let e = parse("2024-5-7", 0.0).expect("2024-5-7 is a date");
        assert_eq!((e.number, e.kind), (45419.0, Kind::Date));
        // The padded form stays with the importer's ISO reader, with a time
        // it reads too; a time it does not read makes it ours.
        assert_eq!(parse("2024-05-07", 0.0), None);
        assert_eq!(parse("2024-05-07 13:30", 0.0), None);
        let e = parse("2024-05-07 1:30 PM", 0.0).expect("a date and a time");
        assert_eq!((e.number, e.kind), (45419.0 + 13.5 / 24.0, Kind::DateTime));
        let e = parse("2024-5-7 1:30 PM", 0.0).expect("a date and a time");
        assert_eq!((e.number, e.kind), (45419.0 + 13.5 / 24.0, Kind::DateTime));
    }

    #[test]
    fn typing_reads_fractional_seconds() {
        let e = parse("13:30:15.5", 0.0).expect("13:30:15.5 is a time");
        assert_eq!(e.number, (13.0 * 3600.0 + 30.0 * 60.0 + 15.5) / 86400.0);
        assert_eq!(e.format.as_deref(), Some("h:mm:ss.0"));
        for text in ["13:30:15.", "13:30:15.5.5", "13:30.5", "13:30:59.99999x"] {
            assert_eq!(parse(text, 0.0), None, "{text:?}");
        }
    }

    #[test]
    fn date_and_time_halves_are_read_apart() {
        let alone = date_and_time_of("6:01", 0.0).unwrap();
        assert_eq!(alone.0, None);
        let both = date_and_time_of("5/17/2024 6:01", 0.0).unwrap();
        assert_eq!(both, (Some(45429.0), alone.1));
        let iso = date_and_time_of("2024-05-17 06:01", 0.0).unwrap();
        assert_eq!(iso, both);
        assert_eq!(date_and_time_of("5", 0.0), None);
        assert_eq!(date_and_time_of("1,234", 0.0), None);
    }

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
