//! Number-format interpretation: a cached numeric value plus a format code →
//! the displayed string. See `docs/42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md`.
//!
//! Supported subset: `General`; fixed decimals (`0`, `0.00`); thousands grouping
//! (`#,##0`); percent (`0%`, `0.00%`); **literal runs** around the number —
//! currency symbols (`$#,##0.00`), quoted text (`0" kg"`), escaped characters
//! (`0\ x`), and `[$SYM-locale]` currency tokens; and date/time formats (rendered
//! as `YYYY-MM-DD`, `HH:MM:SS`, or both). Deferred: negative/zero/text sections,
//! colors, fractions, scientific notation, and token-exact date layout.

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

fn format_date_time(value: f64, section: &str) -> String {
    let lower = section.to_ascii_lowercase();
    let has_date = lower.contains('y') || lower.contains('d');
    let has_time = lower.contains('h') || lower.contains('s');

    let serial_days = value.trunc() as i64;
    let (year, month, day) = serial_to_ymd(serial_days);
    let date = format!("{year:04}-{month:02}-{day:02}");

    let fraction = value - value.trunc();
    let (hour, minute, second) = fraction_to_hms(fraction);
    let time = format!("{hour:02}:{minute:02}:{second:02}");

    match (has_date, has_time) {
        (true, true) => format!("{date} {time}"),
        (false, true) => time,
        _ => date,
    }
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

fn fraction_to_hms(fraction: f64) -> (i64, i64, i64) {
    let total = (fraction.abs() * 86_400.0).round() as i64;
    ((total / 3600) % 24, (total / 60) % 60, total % 60)
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
    fn dates_and_times() {
        // Serial 45000 == 2023-03-15.
        assert_eq!(format_number(45000.0, "yyyy-mm-dd"), "2023-03-15");
        assert_eq!(format_number(45000.0, "mm/dd/yy"), "2023-03-15");
        // Half a day == noon.
        assert_eq!(format_number(0.5, "h:mm:ss"), "12:00:00");
    }
}
