//! Number-format interpretation: a cached numeric value plus a format code →
//! the displayed string. See `docs/42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md`.
//!
//! Supported subset: `General`; fixed decimals (`0`, `0.00`); thousands grouping
//! (`#,##0`); percent (`0%`, `0.00%`); **literal runs** around the number —
//! currency symbols (`$#,##0.00`), quoted text (`0" kg"`), escaped characters
//! (`0\ x`), and `[$SYM-locale]` currency tokens; and date/time formats
//! rendered by their exact token layout (`mm-dd-yy`, `d-mmm-yyyy`, `h:mm AM/PM`)
//! on the 1900 serial-date system. Deferred: negative/zero/text sections,
//! colors, fractions, scientific notation, and elapsed-time (`[h]`) layout.

/// Format `value` for display using the SpreadsheetML format `code`.
pub fn format_number(value: f64, code: &str) -> String {
    let section = code.split(';').next().unwrap_or(code).trim();
    if section.is_empty() || section.eq_ignore_ascii_case("General") {
        return format_general(value);
    }
    if has_digit_placeholder(section) {
        return format_numeric(value, section);
    }
    if is_date_or_time(section) {
        return format_date_time(value, section);
    }
    format_general(value)
}

/// Default (`General`) formatting for a number.
///
/// Rounds to 15 significant digits (Excel's precision) so floating-point tails
/// like `43.480000000000004` display as `43.48`, then prints the shortest form.
pub fn format_general(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    if !value.is_finite() {
        return format!("{value}");
    }
    let magnitude = value.abs().log10().floor() as i32;
    // Keep 15 significant digits; clamp the decimal count to a sane range.
    let decimals = (14 - magnitude).clamp(0, 30) as usize;
    let rounded: f64 = format!("{value:.decimals$}").parse().unwrap_or(value);
    format!("{rounded}")
}

fn has_digit_placeholder(section: &str) -> bool {
    section.contains('0') || section.contains('#')
}

fn is_date_or_time(section: &str) -> bool {
    section
        .chars()
        .any(|c| matches!(c.to_ascii_lowercase(), 'y' | 'm' | 'd' | 'h' | 's'))
}

fn decimal_places(section: &str) -> usize {
    match section.split_once('.') {
        Some((_, frac)) => frac.chars().take_while(|c| matches!(c, '0' | '#')).count(),
        None => 0,
    }
}

fn format_numeric(value: f64, section: &str) -> String {
    let percent = section.contains('%');
    let scaled = if percent { value * 100.0 } else { value };

    let (prefix, pattern, suffix) = split_literal_runs(section);
    let decimals = decimal_places(&pattern);
    let mut digits = format!("{:.*}", decimals, scaled.abs());
    if pattern.contains(',') {
        digits = group_thousands(&digits);
    }

    let sign = if scaled < 0.0 { "-" } else { "" };
    format!("{sign}{prefix}{digits}{suffix}")
}

/// Split a numeric format section into `(prefix literal, digit pattern, suffix
/// literal)`. Literal runs honor quotes (`"…"`), escapes (`\x`), and `[$SYM-…]`
/// currency tokens; `_x` (spacing) and `*x` (fill) are skipped.
fn split_literal_runs(section: &str) -> (String, String, String) {
    let mut prefix = String::new();
    let mut pattern = String::new();
    let mut suffix = String::new();
    let mut chars = section.chars().peekable();
    // Phase 0 = before the number, 1 = inside it, 2 = after it.
    let mut phase = 0u8;

    while let Some(ch) = chars.next() {
        // The digit pattern itself.
        if matches!(ch, '0' | '#' | '.' | ',') {
            if phase == 0 {
                phase = 1;
            }
            if phase == 1 {
                pattern.push(ch);
                continue;
            }
            // A stray placeholder after the number: treat as a literal.
        }
        if phase == 1 {
            phase = 2; // first non-pattern char after the number starts the suffix.
        }
        let out = if phase == 0 { &mut prefix } else { &mut suffix };
        match ch {
            '%' => out.push('%'),
            '"' => {
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                    out.push(q);
                }
            }
            '\\' => {
                if let Some(esc) = chars.next() {
                    out.push(esc);
                }
            }
            '[' => {
                // `[$SYM-locale]` → emit SYM; other bracket tokens ([Red], …) skip.
                let mut token = String::new();
                for b in chars.by_ref() {
                    if b == ']' {
                        break;
                    }
                    token.push(b);
                }
                if let Some(rest) = token.strip_prefix('$') {
                    out.push_str(rest.split('-').next().unwrap_or(""));
                }
            }
            '_' => {
                chars.next(); // spacing: consume the next char, emit a space.
                out.push(' ');
            }
            '*' => {
                chars.next(); // fill: consume the next char, ignore.
            }
            ' ' => out.push(' '),
            c if !matches!(c, '0' | '#' | '.' | ',') => out.push(c),
            _ => {}
        }
    }
    (prefix, pattern, suffix)
}

fn group_thousands(number: &str) -> String {
    let (int_part, frac_part) = match number.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (number, None),
    };
    let bytes = int_part.as_bytes();
    let mut grouped = String::with_capacity(int_part.len() + int_part.len() / 3);
    let len = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(*b as char);
    }
    match frac_part {
        Some(f) => format!("{grouped}.{f}"),
        None => grouped,
    }
}

/// A single element of a parsed date/time format section.
enum DateToken {
    /// A run of a date/time field letter (`y`, `m`, `d`, `h`, `s`), lower-cased,
    /// with its repeat count (`yyyy` → `('y', 4)`).
    Field { kind: char, count: usize },
    /// An `AM/PM` (or `A/P`) marker, carrying the exact source text of each half
    /// so the rendered output preserves the author's casing.
    AmPm { am: String, pm: String },
    /// A literal run — separators, quoted text, escaped characters, spaces.
    Literal(String),
}

/// English month names, indexed by `month - 1`. Locale-independent by design.
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// English weekday names, indexed with `0 = Sunday` to match [`weekday_sun0`].
const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// Render a numeric cell value through a date/time format `section`, honoring the
/// exact token layout (`mm-dd-yy`, `d-mmm-yyyy`, `h:mm AM/PM`, …).
///
/// The value is interpreted on the Excel 1900 serial-date system shared with the
/// calc engine (serial `25569` == 1970-01-01). The integer part selects the civil
/// date; the fractional part is the time of day. Rendering never consults a locale
/// or the wall clock, so it is fully deterministic.
fn format_date_time(value: f64, section: &str) -> String {
    let tokens = parse_date_tokens(section);
    let has_ampm = tokens.iter().any(|t| matches!(t, DateToken::AmPm { .. }));

    // Split the serial into whole days and a rounded whole-second time of day.
    // Rounding seconds can carry into the next day (e.g. 23:59:59.7 → 00:00:00),
    // so fold any overflow back into the day count before deriving the date.
    let mut serial_days = value.trunc() as i64;
    let fraction = value - value.trunc();
    let mut total_seconds = (fraction * 86_400.0).round() as i64;
    if total_seconds >= 86_400 {
        serial_days += 1;
        total_seconds -= 86_400;
    } else if total_seconds < 0 {
        serial_days -= 1;
        total_seconds += 86_400;
    }
    let hour24 = total_seconds / 3600;
    let minute = (total_seconds / 60) % 60;
    let second = total_seconds % 60;

    let (year, month, day) = serial_to_ymd(serial_days);
    let weekday = weekday_sun0(serial_days);
    let is_pm = hour24 >= 12;
    let hour = if has_ampm {
        let h = hour24 % 12;
        if h == 0 { 12 } else { h }
    } else {
        hour24
    };

    let mut out = String::new();
    for (idx, token) in tokens.iter().enumerate() {
        match token {
            DateToken::Literal(text) => out.push_str(text),
            DateToken::AmPm { am, pm } => out.push_str(if is_pm { pm } else { am }),
            DateToken::Field { kind, count } => match kind {
                'y' => {
                    if *count <= 2 {
                        out.push_str(&format!("{:02}", year.rem_euclid(100)));
                    } else {
                        out.push_str(&format!("{year:04}"));
                    }
                }
                'd' => match count {
                    1 => out.push_str(&day.to_string()),
                    2 => out.push_str(&format!("{day:02}")),
                    3 => out.push_str(&WEEKDAYS[weekday as usize][..3]),
                    _ => out.push_str(WEEKDAYS[weekday as usize]),
                },
                'h' => {
                    if *count >= 2 {
                        out.push_str(&format!("{hour:02}"));
                    } else {
                        out.push_str(&hour.to_string());
                    }
                }
                's' => {
                    if *count >= 2 {
                        out.push_str(&format!("{second:02}"));
                    } else {
                        out.push_str(&second.to_string());
                    }
                }
                'm' => {
                    if is_minute_context(&tokens, idx) {
                        if *count >= 2 {
                            out.push_str(&format!("{minute:02}"));
                        } else {
                            out.push_str(&minute.to_string());
                        }
                    } else {
                        let name = MONTHS[(month - 1) as usize];
                        match count {
                            1 => out.push_str(&month.to_string()),
                            2 => out.push_str(&format!("{month:02}")),
                            3 => out.push_str(&name[..3]),
                            4 => out.push_str(name),
                            _ => out.push_str(&name[..1]),
                        }
                    }
                }
                _ => {}
            },
        }
    }
    out
}

/// Decide whether the `m`/`mm` field at `idx` means minutes rather than a month.
///
/// Following Excel: an `m` run is minutes when its nearest neighboring field
/// (skipping literals) is an hour before it or a seconds field after it.
fn is_minute_context(tokens: &[DateToken], idx: usize) -> bool {
    if adjacent_field_kind(tokens, idx, false) == Some('h') {
        return true;
    }
    adjacent_field_kind(tokens, idx, true) == Some('s')
}

/// The `kind` of the nearest [`DateToken::Field`] before (`forward == false`) or
/// after (`forward == true`) `idx`, skipping literals and `AM/PM` markers.
fn adjacent_field_kind(tokens: &[DateToken], idx: usize, forward: bool) -> Option<char> {
    let mut i = idx;
    loop {
        if forward {
            i += 1;
            if i >= tokens.len() {
                return None;
            }
        } else {
            if i == 0 {
                return None;
            }
            i -= 1;
        }
        match &tokens[i] {
            DateToken::Field { kind, .. } => return Some(*kind),
            DateToken::AmPm { .. } | DateToken::Literal(_) => {}
        }
    }
}

/// Tokenize a date/time format section into fields, `AM/PM` markers, and literal
/// runs. Honors the same literal conventions as the numeric renderer: quoted text
/// (`"…"`), escaped characters (`\x`), and bracket tokens (`[$-409]`, `[Red]`)
/// which are dropped. Any character that is not a field letter is literal.
fn parse_date_tokens(section: &str) -> Vec<DateToken> {
    let chars: Vec<char> = section.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        let lower = ch.to_ascii_lowercase();
        match lower {
            'y' | 'm' | 'd' | 'h' | 's' => {
                let mut count = 1;
                while i + count < chars.len() && chars[i + count].to_ascii_lowercase() == lower {
                    count += 1;
                }
                tokens.push(DateToken::Field { kind: lower, count });
                i += count;
            }
            'a' => {
                if let Some(len) = match_ampm(&chars[i..]) {
                    let raw: String = chars[i..i + len].iter().collect();
                    let (am, pm) = raw.split_once('/').unwrap_or((raw.as_str(), ""));
                    tokens.push(DateToken::AmPm {
                        am: am.to_owned(),
                        pm: pm.to_owned(),
                    });
                    i += len;
                } else {
                    push_literal(&mut tokens, ch);
                    i += 1;
                }
            }
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    push_literal(&mut tokens, chars[i]);
                    i += 1;
                }
                i += 1; // skip the closing quote (or run off the end harmlessly)
            }
            '\\' => {
                if i + 1 < chars.len() {
                    push_literal(&mut tokens, chars[i + 1]);
                }
                i += 2;
            }
            '[' => {
                // Locale / color / elapsed brackets: consume through `]`, emit nothing.
                i += 1;
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
                i += 1;
            }
            _ => {
                push_literal(&mut tokens, ch);
                i += 1;
            }
        }
    }
    tokens
}

/// If `chars` starts with an `AM/PM` (5-char) or `A/P` (3-char) marker,
/// case-insensitively, return its length. Otherwise `None`.
fn match_ampm(chars: &[char]) -> Option<usize> {
    let eq = |slice: &[char], pat: &str| -> bool {
        slice.len() == pat.len()
            && slice
                .iter()
                .zip(pat.chars())
                .all(|(c, p)| c.to_ascii_lowercase() == p)
    };
    if chars.len() >= 5 && eq(&chars[..5], "am/pm") {
        Some(5)
    } else if chars.len() >= 3 && eq(&chars[..3], "a/p") {
        Some(3)
    } else {
        None
    }
}

/// Append `ch` to the trailing literal token, starting a new one if needed.
fn push_literal(tokens: &mut Vec<DateToken>, ch: char) {
    if let Some(DateToken::Literal(text)) = tokens.last_mut() {
        text.push(ch);
    } else {
        tokens.push(DateToken::Literal(ch.to_string()));
    }
}

/// Weekday of an Excel serial day, `0 = Sunday .. 6 = Saturday`. Matches the
/// `WEEKDAY` function in the calc engine (1970-01-01 / serial 25569 was Thursday).
fn weekday_sun0(serial_days: i64) -> i64 {
    (serial_days - 25_569 + 4).rem_euclid(7)
}

/// Convert an Excel (1900-system) serial day number to a civil `(y, m, d)`.
fn serial_to_ymd(serial_days: i64) -> (i64, i64, i64) {
    // Excel serial 25569 == 1970-01-01; shift to the civil-from-days epoch.
    let mut z = serial_days - 25569 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    z -= era * 146_097;
    let doe = z; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::format_number;

    #[test]
    fn general_and_fixed_decimals() {
        assert_eq!(format_number(42.0, "General"), "42");
        assert_eq!(format_number(2.345, "0.00"), "2.35");
        assert_eq!(format_number(2.345, "0"), "2");
        assert_eq!(format_number(-2.5, "0.0"), "-2.5");
    }

    #[test]
    fn thousands_grouping() {
        assert_eq!(format_number(1234567.0, "#,##0"), "1,234,567");
        assert_eq!(format_number(1234.5, "#,##0.00"), "1,234.50");
    }

    #[test]
    fn percent() {
        assert_eq!(format_number(0.5, "0%"), "50%");
        assert_eq!(format_number(0.1234, "0.0%"), "12.3%");
    }

    #[test]
    fn currency_and_literals() {
        assert_eq!(format_number(4.5, "$#,##0.00"), "$4.50");
        assert_eq!(format_number(1234.5, "$#,##0.00"), "$1,234.50");
        assert_eq!(format_number(-9.99, "$#,##0.00"), "-$9.99");
        assert_eq!(format_number(3.0, "0\" kg\""), "3 kg");
        assert_eq!(format_number(5.0, "[$€-407]#,##0.00"), "€5.00");
    }

    #[test]
    fn date_layout_honors_tokens() {
        // Serial 45000 == 2023-03-15 (a Wednesday).
        assert_eq!(format_number(45000.0, "yyyy-mm-dd"), "2023-03-15");
        assert_eq!(format_number(45000.0, "mm/dd/yy"), "03/15/23");
        assert_eq!(format_number(45000.0, "m/d/yyyy"), "3/15/2023");
        assert_eq!(format_number(45000.0, "d-mmm-yyyy"), "15-Mar-2023");
        assert_eq!(format_number(45000.0, "mmmm d, yyyy"), "March 15, 2023");
        assert_eq!(format_number(45000.0, "mmm"), "Mar");
        assert_eq!(format_number(45000.0, "mmmmm"), "M");
    }

    #[test]
    fn weekday_names() {
        // 2023-03-15 is a Wednesday.
        assert_eq!(format_number(45000.0, "dddd"), "Wednesday");
        assert_eq!(format_number(45000.0, "ddd"), "Wed");
        assert_eq!(format_number(45000.0, "ddd, mmm d"), "Wed, Mar 15");
    }

    #[test]
    fn time_layout_and_ampm() {
        // Half a day == noon; a quarter == 06:00.
        assert_eq!(format_number(0.5, "h:mm:ss"), "12:00:00");
        assert_eq!(format_number(0.5, "hh:mm"), "12:00");
        assert_eq!(format_number(0.5, "h:mm AM/PM"), "12:00 PM");
        assert_eq!(format_number(0.25, "h:mm AM/PM"), "6:00 AM");
        assert_eq!(format_number(0.75, "h:mm AM/PM"), "6:00 PM");
        // Midnight (integer serial, no fraction) reads as 12 AM on a 12h clock.
        assert_eq!(format_number(45000.0, "h:mm AM/PM"), "12:00 AM");
        assert_eq!(format_number(45000.0, "h:mm A/P"), "12:00 A");
        // Lower-case markers preserve their casing.
        assert_eq!(format_number(0.5, "h:mm am/pm"), "12:00 pm");
    }

    #[test]
    fn minute_vs_month_disambiguation() {
        // 45000.5 == 2023-03-15 12:00:00. `m` after `h` is minutes; standalone
        // `mm` between dashes is the month.
        assert_eq!(
            format_number(45000.5, "yyyy-mm-dd hh:mm"),
            "2023-03-15 12:00"
        );
        // `mm:ss` — the `mm` before seconds is minutes.
        // Serial 45000 + 1/24/60*5 + 1/86400*9 → 00:05:09.
        let v = 45000.0 + (5.0 * 60.0 + 9.0) / 86_400.0;
        assert_eq!(format_number(v, "mm:ss"), "05:09");
        // Combined date+time: the leading `mm` is month, the trailing `mm` minutes.
        assert_eq!(format_number(45000.5, "mm/dd hh:mm"), "03/15 12:00");
    }

    #[test]
    fn seconds_rounding_carries_into_the_next_day() {
        // 23:59:59.7 rounds up to 00:00:00 of the following day.
        let v = 45000.0 + (23.0 * 3600.0 + 59.0 * 60.0 + 59.7) / 86_400.0;
        assert_eq!(
            format_number(v, "yyyy-mm-dd hh:mm:ss"),
            "2023-03-16 00:00:00"
        );
    }

    #[test]
    fn quoted_literals_in_date_codes() {
        assert_eq!(
            format_number(45000.0, "yyyy\" year \"mm\" month\""),
            "2023 year 03 month"
        );
    }
}
