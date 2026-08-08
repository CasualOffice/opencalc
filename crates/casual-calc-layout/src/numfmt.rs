//! Number-format interpretation: a cached numeric value plus a format code →
//! the displayed string. See `docs/42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md`.
//!
//! Supported: `General`; fixed decimals (`0`, `0.00`); thousands grouping
//! (`#,##0`); percent (`0%`, `0.00%`); scientific (`0.00E+00`); **literal runs**
//! around the number — currency symbols (`$#,##0.00`), quoted text (`0" kg"`),
//! escaped characters (`0\ x`), and `[$SYM-locale]` currency tokens; date/time
//! formats rendered by their exact token layout (`mm-dd-yy`, `d-mmm-yyyy`,
//! `h:mm AM/PM`) on the 1900 serial-date system; the positive/negative/zero/text
//! sections of a multi-section code; the text section applied to string values
//! ([`format_text`]); and the eight named section colours
//! ([`format_number_colored`]).
//!
//! Deferred: fractions (`# ??/??`), elapsed-time (`[h]`) layout, conditional
//! sections (`[>100]`), and the legacy `[Color n]` palette index.

/// Split a SpreadsheetML format code into sections separated by unquoted `;`.
pub fn split_sections(code: &str) -> Vec<&str> {
    let mut sections = Vec::new();
    let mut start = 0;
    let mut in_quotes = false;
    let mut in_brackets = false;
    let mut chars = code.char_indices().peekable();

    while let Some((i, ch)) = chars.next() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '\\' => {
                let _ = chars.next(); // Skip escaped character
            }
            '[' if !in_quotes => in_brackets = true,
            ']' if !in_quotes => in_brackets = false,
            ';' if !in_quotes && !in_brackets => {
                sections.push(&code[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    sections.push(&code[start..]);
    sections
}

/// Format `value` for display using the SpreadsheetML format `code`.
///
/// Colour-blind: a `[Red]` in the chosen section is applied by
/// [`format_number_colored`], which this delegates to.
pub fn format_number(value: f64, code: &str) -> String {
    format_number_colored(value, code).0
}

/// The colour a format section names, as `RRGGBB`.
///
/// A number format may state the colour of its own output — `#,##0;[Red]-#,##0`
/// is how a negative total turns red — and dropping it silently loses the one
/// piece of formatting the author explicitly asked for. Excel's eight named
/// colours are recognised; the legacy `[Color n]` palette index is not, and is
/// treated as no colour rather than guessed at.
fn section_color(section: &str) -> Option<&'static str> {
    let mut chars = section.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                }
            }
            '\\' => {
                chars.next();
            }
            '[' => {
                let mut token = String::new();
                for b in chars.by_ref() {
                    if b == ']' {
                        break;
                    }
                    token.push(b);
                }
                let named = match token.trim().to_ascii_lowercase().as_str() {
                    "black" => "000000",
                    "blue" => "0000FF",
                    "cyan" => "00FFFF",
                    "green" => "00FF00",
                    "magenta" => "FF00FF",
                    "red" => "FF0000",
                    "white" => "FFFFFF",
                    "yellow" => "FFFF00",
                    _ => continue,
                };
                return Some(named);
            }
            _ => {}
        }
    }
    None
}

/// Format `value`, also reporting the colour its section names (if any).
pub fn format_number_colored(value: f64, code: &str) -> (String, Option<&'static str>) {
    let sections = split_sections(code);
    if sections.is_empty()
        || (sections.len() == 1
            && (sections[0].trim().is_empty() || sections[0].eq_ignore_ascii_case("General")))
    {
        return (format_general(value), None);
    }

    let (section, is_custom_negative) = match sections.len() {
        1 => (sections[0].trim(), false),
        2 => {
            if value < 0.0 {
                (sections[1].trim(), true)
            } else {
                (sections[0].trim(), false)
            }
        }
        _ => {
            // 3 or 4 sections
            if value > 0.0 {
                (sections[0].trim(), false)
            } else if value < 0.0 {
                (sections[1].trim(), true)
            } else {
                (sections[2].trim(), false)
            }
        }
    };

    let color = section_color(section);
    if section.is_empty() || section.eq_ignore_ascii_case("General") {
        return (format_general(value), color);
    }
    if has_digit_placeholder(section) {
        return (
            format_numeric_section(value, section, is_custom_negative),
            color,
        );
    }
    if is_date_or_time(section) {
        return (format_date_time(value, section), color);
    }

    // Literal-only section (e.g. `"-"` or `"Zero"`)
    let (prefix, pattern, suffix) = split_literal_runs(section);
    (format!("{prefix}{pattern}{suffix}"), color)
}

/// Apply a format code's **text section** to a string value.
///
/// A four-section code ends with the section for text (`…;…;…;"pre "@`), and
/// the stock `@` ("Text") format is a single section that applies to text too.
/// `@` stands for the value; everything else is literal. `None` when the code
/// has nothing to say about text, in which case the value is shown as-is.
///
/// Without this a cell formatted as Text was indistinguishable from an
/// unformatted one, and a code like `@" kg"` printed nothing at all.
#[must_use]
pub fn format_text(text: &str, code: &str) -> Option<String> {
    let sections = split_sections(code);
    let section = match sections.len() {
        4 => sections[3].trim(),
        1 if sections[0].contains('@') => sections[0].trim(),
        _ => return None,
    };
    if section.is_empty() || section.eq_ignore_ascii_case("General") {
        return None;
    }
    let mut out = String::new();
    let mut chars = section.chars().peekable();
    let mut saw_placeholder = false;
    while let Some(ch) = chars.next() {
        match ch {
            '@' => {
                out.push_str(text);
                saw_placeholder = true;
            }
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
                // Colour / condition tokens are not text.
                for b in chars.by_ref() {
                    if b == ']' {
                        break;
                    }
                }
            }
            '_' => {
                chars.next();
                out.push(' ');
            }
            '*' => {
                chars.next();
            }
            c => out.push(c),
        }
    }
    // A text section with no `@` (e.g. `;;;"n/a"`) replaces the value outright,
    // which is legitimate — but a section that never mentioned text and yielded
    // nothing is more likely a code we misread, so leave the value alone.
    if !saw_placeholder && out.is_empty() {
        return None;
    }
    Some(out)
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

/// A scientific section (`0.00E+00`): the mantissa pattern, how many digits the
/// exponent is padded to, and whether a positive exponent shows its `+`.
///
/// `None` for any other section. The scan skips quoted literals and escapes so
/// an `E` inside `"SIZE"` is not mistaken for the exponent marker.
fn scientific_spec(section: &str) -> Option<(String, usize, bool)> {
    let chars: Vec<char> = section.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    i += 1;
                }
            }
            '\\' => i += 1,
            '[' => {
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
            }
            'E' | 'e' if matches!(chars.get(i + 1), Some('+') | Some('-')) => {
                let plus = chars[i + 1] == '+';
                let digits = chars[i + 2..]
                    .iter()
                    .take_while(|c| matches!(c, '0' | '#' | '?'))
                    .count();
                if digits == 0 {
                    return None;
                }
                let mantissa: String = chars[..i].iter().collect();
                return Some((mantissa, digits, plus));
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Format a value in scientific notation for a `0.00E+00`-style section.
fn format_scientific(
    value: f64,
    mantissa_pat: &str,
    exp_digits: usize,
    plus: bool,
    is_custom_negative: bool,
) -> String {
    let target = if is_custom_negative { value.abs() } else { value };
    let decimals = decimal_places(mantissa_pat);
    let (prefix, _, suffix) = split_literal_runs(mantissa_pat);

    let mut exp = 0i32;
    let mut mantissa = target.abs();
    if mantissa != 0.0 && mantissa.is_finite() {
        exp = mantissa.log10().floor() as i32;
        mantissa /= 10f64.powi(exp);
        // Rounding the mantissa can carry it to 10.0 (9.99 at 1 decimal), which
        // must become 1.0 one exponent up rather than printing "10.0E+03".
        let rounded: f64 = format!("{mantissa:.decimals$}").parse().unwrap_or(mantissa);
        if rounded >= 10.0 {
            mantissa /= 10.0;
            exp += 1;
        }
    }
    let sign = if !is_custom_negative && target < 0.0 {
        "-"
    } else {
        ""
    };
    let exp_sign = if exp < 0 {
        "-"
    } else if plus {
        "+"
    } else {
        ""
    };
    format!(
        "{sign}{prefix}{:.*}{suffix}E{exp_sign}{:0width$}",
        decimals,
        mantissa,
        exp.unsigned_abs(),
        width = exp_digits
    )
}

fn format_numeric_section(value: f64, section: &str, is_custom_negative: bool) -> String {
    if let Some((mantissa, exp_digits, plus)) = scientific_spec(section) {
        return format_scientific(value, &mantissa, exp_digits, plus, is_custom_negative);
    }
    let percent = section.contains('%');
    let target_val = if is_custom_negative {
        value.abs()
    } else {
        value
    };
    let scaled = if percent {
        target_val * 100.0
    } else {
        target_val
    };

    let (prefix, pattern, suffix) = split_literal_runs(section);
    let decimals = decimal_places(&pattern);
    let mut digits = format!("{:.*}", decimals, scaled.abs());
    if pattern.contains(',') {
        digits = group_thousands(&digits);
    }

    let sign = if !is_custom_negative && scaled < 0.0 {
        "-"
    } else {
        ""
    };
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
            // `?` is a digit-alignment placeholder (pads with a space instead of
            // a digit when there's nothing to show — used e.g. in the Accounting
            // zero-section `"-"??` to align with the decimal digits of the
            // positive/negative sections). Fraction layout (`# ??/??`) is still
            // unsupported; this covers the common alignment-padding usage.
            '?' => out.push(' '),
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

/// Adjust the number of decimal places in format code `code` by `delta`.
///
/// Positive `delta` adds decimal places (`0`), negative `delta` reduces them.
/// Works across multi-section formats, adjusting decimal precision in numeric sections.
pub fn adjust_format_decimals(code: &str, delta: i32) -> String {
    if delta == 0 {
        return code.to_owned();
    }
    let sections = split_sections(code);
    if sections.is_empty() {
        return adjust_section_decimals("General", delta);
    }
    let adjusted_sections: Vec<String> = sections
        .iter()
        .map(|sec| adjust_section_decimals(sec, delta))
        .collect();
    adjusted_sections.join(";")
}

fn adjust_section_decimals(section: &str, delta: i32) -> String {
    let trimmed = section.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("General") {
        if delta > 0 {
            return format!("0.{}", "0".repeat(delta as usize));
        } else {
            return "0".to_owned();
        }
    }
    if !has_digit_placeholder(trimmed) {
        return trimmed.to_owned();
    }

    let (prefix, pattern, suffix) = split_literal_runs(trimmed);
    let new_pattern = if let Some((int_part, frac_part)) = pattern.split_once('.') {
        let current_dec = frac_part
            .chars()
            .take_while(|c| matches!(c, '0' | '#'))
            .count();
        let target_dec = (current_dec as i32 + delta).max(0) as usize;
        let int_str = if int_part.is_empty() { "0" } else { int_part };
        if target_dec > 0 {
            format!("{int_str}.{}", "0".repeat(target_dec))
        } else {
            int_str.to_owned()
        }
    } else {
        if delta > 0 {
            let int_str = if pattern.is_empty() { "0" } else { &pattern };
            format!("{int_str}.{}", "0".repeat(delta as usize))
        } else {
            pattern
        }
    };

    format!("{prefix}{new_pattern}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::{
        adjust_format_decimals, format_number, format_number_colored, format_text, split_sections,
    };

    #[test]
    fn text_sections_apply_to_text() {
        // The stock Text format shows the value unchanged…
        assert_eq!(format_text("007", "@").as_deref(), Some("007"));
        // …and a text section can decorate it.
        assert_eq!(format_text("12", "@\" kg\"").as_deref(), Some("12 kg"));
        assert_eq!(format_text("x", "\"[\"@\"]\"").as_deref(), Some("[x]"));
        // A four-section code's last section is the text one.
        assert_eq!(
            format_text("hi", "#,##0;[Red]-#,##0;\"-\";\">> \"@").as_deref(),
            Some(">> hi")
        );
        // Codes that say nothing about text leave the value alone.
        assert_eq!(format_text("hi", "#,##0.00"), None);
        assert_eq!(format_text("hi", "General"), None);
        assert_eq!(format_text("hi", "#,##0;[Red]-#,##0"), None);
    }

    #[test]
    fn section_colors_are_reported() {
        // The negative section paints red; the positive one says nothing.
        let code = "#,##0;[Red]-#,##0";
        assert_eq!(format_number_colored(1234.0, code), ("1,234".to_owned(), None));
        assert_eq!(
            format_number_colored(-1234.0, code),
            ("-1,234".to_owned(), Some("FF0000"))
        );
        // The colour is stripped from the output text, not printed.
        assert_eq!(format_number(-5.0, "[Blue]0.00"), "-5.00");
        assert_eq!(format_number_colored(-5.0, "[Blue]0.00").1, Some("0000FF"));
        // A currency token is not a colour, and an unknown bracket token is
        // ignored rather than guessed at.
        assert_eq!(format_number_colored(5.0, "[$€-407]0.00").1, None);
        assert_eq!(format_number_colored(5.0, "[Color 7]0.00").1, None);
        // "Red" inside a literal is text, not a colour instruction.
        assert_eq!(format_number_colored(5.0, "0\"[Red]\"").1, None);
    }

    #[test]
    fn scientific_sections() {
        // The reported garbage: "12345.00E+" from the unimplemented branch.
        assert_eq!(format_number(12345.0, "0.00E+00"), "1.23E+04");
        assert_eq!(format_number(-12345.0, "0.00E+00"), "-1.23E+04");
        assert_eq!(format_number(0.00012, "0.00E+00"), "1.20E-04");
        assert_eq!(format_number(0.0, "0.00E+00"), "0.00E+00");
        // No `+` in the pattern means no sign on a positive exponent.
        assert_eq!(format_number(12345.0, "0.0E-0"), "1.2E4");
        // Rounding that carries the mantissa to 10 steps the exponent instead.
        assert_eq!(format_number(9_999.0, "0.0E+00"), "1.0E+04");
        // A literal `E` is not an exponent marker.
        assert_eq!(format_number(12.0, "0\"EACH\""), "12EACH");
    }

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
    fn multi_section_formats() {
        let code = "$#,##0.00;($#,##0.00);\"-\"";
        assert_eq!(format_number(1234.5, code), "$1,234.50");
        assert_eq!(format_number(-1234.5, code), "($1,234.50)");
        assert_eq!(format_number(0.0, code), "-");

        let code4 = "0.00;[Red]-0.00;\"Zero\";\"Text\"";
        assert_eq!(format_number(5.25, code4), "5.25");
        assert_eq!(format_number(-5.25, code4), "-5.25");
        assert_eq!(format_number(0.0, code4), "Zero");
    }

    #[test]
    fn accounting_format_pads_question_marks_as_spaces() {
        // The standard Accounting preset: pos;neg;zero;text, with `_(`/`_)`
        // spacing tokens, a `*` fill token, and a `"-"??` zero-section that
        // pads with spaces (not literal question marks) to align with the
        // decimal digits of the other sections.
        let code = "_($* #,##0.00_);_($* (#,##0.00);_($* \"-\"??_);_(@_)";
        assert_eq!(format_number(1234.5, code), " $1,234.50 ");
        assert_eq!(format_number(-1234.5, code), " $(1,234.50)");
        // The zero-section's `??` pads with spaces, not literal `?` characters.
        let zero = format_number(0.0, code);
        assert!(
            !zero.contains('?'),
            "zero-section rendered a literal '?': {zero:?}"
        );
        assert_eq!(zero, " $-   ");
    }

    #[test]
    fn split_sections_honors_quotes_and_brackets() {
        let sections = split_sections("$#,##0.00;($#,##0.00);\"-\";[Red]@");
        assert_eq!(sections.len(), 4);
        assert_eq!(sections[0], "$#,##0.00");
        assert_eq!(sections[1], "($#,##0.00)");
        assert_eq!(sections[2], "\"-\"");
        assert_eq!(sections[3], "[Red]@");

        let quoted = split_sections("\"a;b\";\"c;d\"");
        assert_eq!(quoted.len(), 2);
        assert_eq!(quoted[0], "\"a;b\"");
        assert_eq!(quoted[1], "\"c;d\"");
    }

    #[test]
    fn decimal_adjustments() {
        assert_eq!(adjust_format_decimals("General", 1), "0.0");
        assert_eq!(adjust_format_decimals("General", 2), "0.00");
        assert_eq!(adjust_format_decimals("General", -1), "0");
        assert_eq!(adjust_format_decimals("0.00", 1), "0.000");
        assert_eq!(adjust_format_decimals("0.00", -1), "0.0");
        assert_eq!(adjust_format_decimals("0.00", -2), "0");
        assert_eq!(adjust_format_decimals("$#,##0.00", 1), "$#,##0.000");
        assert_eq!(adjust_format_decimals("$#,##0.00", -1), "$#,##0.0");
        assert_eq!(
            adjust_format_decimals("$#,##0.00;($#,##0.00);\"-\"", 1),
            "$#,##0.000;($#,##0.000);\"-\""
        );
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
