//! Format-neutral identities, detection/dispatch, and built-in adapters.
//!
//! The first adapter is **delimited text** (CSV / TSV / PSV): a deterministic,
//! RFC 4180-style reader and writer over the normalized [`Workbook`]. Fields are
//! typed on read (number / boolean / ISO date / text) and a parse → write →
//! parse round-trip is a model fixed point — which is what rules out Excel's
//! habit of turning `007` into `7`, since that round-trip does not settle.
//! XLSX/ODS live in their own crates; this crate is where the lightweight text
//! formats and the format registry belong.
//!
//! See `docs/19-WORKSPACE-SCAFFOLD-DESIGN.md`.

use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Style, StyleId, Workbook};

/// The comma delimiter (`.csv`).
pub const COMMA: u8 = b',';
/// The tab delimiter (`.tsv`).
pub const TAB: u8 = b'\t';
/// The pipe delimiter (`.psv`).
pub const PIPE: u8 = b'|';

/// An error reading a delimited file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IoError {
    /// The input was not valid UTF-8.
    InvalidUtf8,
}

impl core::fmt::Display for IoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            IoError::InvalidUtf8 => write!(f, "[OC-IO-0001] input is not valid UTF-8"),
        }
    }
}

impl std::error::Error for IoError {}

/// Map a filename extension (without the dot, case-insensitive) to its delimiter.
pub fn delimiter_for_extension(ext: &str) -> Option<u8> {
    match ext.to_ascii_lowercase().as_str() {
        "csv" => Some(COMMA),
        "tsv" | "tab" => Some(TAB),
        "psv" => Some(PIPE),
        _ => None,
    }
}

/// Parse delimited text into a single-sheet workbook. Each field is typed:
/// a finite number becomes [`CellValue::Number`], `TRUE`/`FALSE`
/// (case-insensitive) become [`CellValue::Bool`], an ISO 8601 date or time
/// becomes a serial number carrying a matching number format, the empty string
/// is a blank cell, and anything else is interned as text.
///
/// Typing is deliberately conservative in two places, both so that reading is
/// lossless rather than merely convenient:
///
/// * A field whose integer part carries a **leading zero** (`007`, `0042`)
///   stays text. Excel turns these into numbers and the zeros are gone for
///   good; since zip codes, part numbers and account IDs are exactly the fields
///   that look like this, that conversion is silent data loss and a round-trip
///   through it is not a fixed point.
/// * Only **unambiguous ISO 8601** dates are recognised. `3/5/2024` is the
///   fifth of March or the third of May depending on where the file was
///   written, and nothing in the file says which; guessing would make the same
///   bytes import differently on two machines, so it stays text.
pub fn read_delimited(bytes: &[u8], delimiter: u8) -> Result<Workbook, IoError> {
    let text = std::str::from_utf8(bytes).map_err(|_| IoError::InvalidUtf8)?;
    let records = parse_records(text, delimiter as char);

    let mut workbook = Workbook::new(Id::from_parts(0x4353_5600_0000_0001, 1)); // "CSV"
    let mut sheet = Sheet::new(SheetId(Id::from_parts(0x4353_5600_0000_0002, 1)), "Sheet1");

    for (r, record) in records.iter().enumerate() {
        for (c, field) in record.iter().enumerate() {
            let (value, format) = type_field(field, &mut workbook);
            if value.is_empty() {
                continue;
            }
            let mut cell = Cell::value(value);
            if let Some(code) = format {
                cell.style = Some(date_style(&mut workbook, code));
            }
            sheet.cells.set(CellRef::new(r as u32, c as u32), cell);
        }
    }
    workbook.sheets.push(sheet);
    Ok(workbook)
}

/// Intern the style that carries a detected date/time format.
fn date_style(workbook: &mut Workbook, code: &str) -> StyleId {
    workbook.intern_style(Style {
        number_format: Some(code.to_owned()),
        ..Style::default()
    })
}

/// Serialize a sheet's populated grid to delimited text (RFC 4180 quoting,
/// CRLF line endings). Cells use their cached value, so formulas export as their
/// computed result.
pub fn write_delimited(workbook: &Workbook, sheet_index: usize, delimiter: u8) -> String {
    let Some(sheet) = workbook.sheets.get(sheet_index) else {
        return String::new();
    };
    let (mut max_row, mut max_col) = (None, None);
    for (at, _) in sheet.cells.iter() {
        max_row = Some(max_row.map_or(at.row, |m: u32| m.max(at.row)));
        max_col = Some(max_col.map_or(at.col, |m: u32| m.max(at.col)));
    }
    let (Some(max_row), Some(max_col)) = (max_row, max_col) else {
        return String::new();
    };

    let delim = delimiter as char;
    let mut out = String::new();
    for r in 0..=max_row {
        for c in 0..=max_col {
            if c > 0 {
                out.push(delim);
            }
            let field = sheet
                .cells
                .get(CellRef::new(r, c))
                .map(|cell| field_text(workbook, &cell.value, cell.style))
                .unwrap_or_default();
            push_quoted(&mut out, &field, delimiter);
        }
        out.push_str("\r\n");
    }
    out
}

/// The typed value for a raw field, with the number-format code to attach when
/// the field was recognised as a date or time.
fn type_field(field: &str, workbook: &mut Workbook) -> (CellValue, Option<&'static str>) {
    if field.is_empty() {
        return (CellValue::Empty, None);
    }
    if field.eq_ignore_ascii_case("true") {
        return (CellValue::Bool(true), None);
    }
    if field.eq_ignore_ascii_case("false") {
        return (CellValue::Bool(false), None);
    }
    if let Some((serial, code)) = parse_iso_datetime(field) {
        return (CellValue::Number(serial), Some(code));
    }
    if !has_leading_zero(field)
        && let Ok(number) = field.parse::<f64>()
        && number.is_finite()
    {
        return (CellValue::Number(number), None);
    }
    (CellValue::SharedString(workbook.intern_string(field)), None)
}

/// Whether the integer part of a numeric-looking field is padded with a zero,
/// as in `007` or `-0042`. A bare `0`, and any `0.5`/`0e3`, are not padded.
fn has_leading_zero(field: &str) -> bool {
    let digits = field.strip_prefix(['+', '-']).unwrap_or(field);
    let mut chars = digits.chars();
    chars.next() == Some('0') && chars.next().is_some_and(|c| c.is_ascii_digit())
}

/// Recognise an ISO 8601 date, date-time or time-of-day, returning its Excel
/// serial and the number-format code that displays it as written. Anything with
/// trailing characters, an out-of-range component, or a non-ISO layout is
/// rejected so it falls through to text.
///
/// Public because typing `2024-03-05` into a cell has to mean the same thing as
/// importing a file that contains it; two parsers would drift.
pub fn parse_iso_datetime(field: &str) -> Option<(f64, &'static str)> {
    let field = field.trim();
    // Time of day alone: `13:45` or `13:45:30`.
    if let Some((frac, with_seconds)) = parse_time(field) {
        return Some((frac, if with_seconds { "hh:mm:ss" } else { "hh:mm" }));
    }

    let (date_part, time_part) = match field.split_once(['T', ' ']) {
        Some((d, t)) => (d, Some(t)),
        None => (field, None),
    };
    let days = parse_date(date_part)?;
    let Some(time_part) = time_part else {
        return Some((days, "yyyy-mm-dd"));
    };
    let (frac, with_seconds) = parse_time(time_part)?;
    Some((
        days + frac,
        if with_seconds {
            "yyyy-mm-dd hh:mm:ss"
        } else {
            "yyyy-mm-dd hh:mm"
        },
    ))
}

/// `YYYY-MM-DD` to an Excel day serial.
fn parse_date(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i32 = text.get(0..4)?.parse().ok()?;
    let month: u32 = text.get(5..7)?.parse().ok()?;
    let day: u32 = text.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    // Excel's epoch has no year 0 and its serials start at 1899-12-31 = 1.
    if !(1900..=9999).contains(&year) {
        return None;
    }
    let serial = days_from_civil(year, month, day) - days_from_civil(1899, 12, 30);
    // Excel keeps Lotus's phantom 1900-02-29, so every real date up to
    // 1900-02-28 sits one day later in a plain day count than in Excel's.
    Some(if serial <= 60 { serial - 1 } else { serial } as f64)
}

/// `HH:MM` or `HH:MM:SS` to a day fraction, with whether seconds were present.
fn parse_time(text: &str) -> Option<(f64, bool)> {
    let mut parts = text.split(':');
    let hours: u32 = parts.next()?.parse().ok()?;
    let minutes: u32 = parts.next().filter(|p| p.len() == 2)?.parse().ok()?;
    let (seconds, with_seconds) = match parts.next() {
        Some(s) if s.len() == 2 => (s.parse::<u32>().ok()?, true),
        Some(_) => return None,
        None => (0, false),
    };
    if parts.next().is_some() || hours > 23 || minutes > 59 || seconds > 59 {
        return None;
    }
    let frac =
        (f64::from(hours) * 3600.0 + f64::from(minutes) * 60.0 + f64::from(seconds)) / 86400.0;
    Some((frac, with_seconds))
}

/// Days from the civil epoch (1970-01-01), by Howard Hinnant's algorithm.
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The length of a month in the proleptic Gregorian calendar.
fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Format a number the way a spreadsheet's "General" format does: round to 15
/// significant digits (hiding binary-float tails like `43.480000000000004`) then
/// use the shortest exact representation.
fn format_number(n: f64) -> String {
    if n == 0.0 {
        return "0".to_owned();
    }
    if !n.is_finite() {
        return format!("{n}");
    }
    let magnitude = n.abs().log10().floor() as i32;
    let decimals = (14 - magnitude).clamp(0, 15) as usize;
    let rounded: f64 = format!("{n:.decimals$}").parse().unwrap_or(n);
    format!("{rounded}")
}

/// The text form of a cell value for export. A number under a date or time
/// format is written the way it reads on the sheet, not as its serial — a CSV
/// full of `45356` is what "export lost my dates" means to the person opening
/// it, and it is also what stops a read → write → read round-trip settling.
fn field_text(workbook: &Workbook, value: &CellValue, style: Option<StyleId>) -> String {
    match value {
        CellValue::Empty => String::new(),
        CellValue::Number(n) => match date_format_of(workbook, style) {
            Some(code) => casual_calc_layout::format_number(*n, code),
            None => format_number(*n),
        },
        CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        CellValue::SharedString(id) | CellValue::InlineString(id) => {
            workbook.strings.get(*id).unwrap_or_default().to_owned()
        }
        CellValue::Error(e) => e.to_string(),
    }
}

/// The number-format code on a cell's style, when that format is a date or time
/// one. Detected by the date/time placeholders rather than by matching the codes
/// this module writes, so a format the user later chose is honored too.
fn date_format_of(workbook: &Workbook, style: Option<StyleId>) -> Option<&str> {
    let code = workbook.styles.get(style?)?.number_format.as_deref()?;
    is_date_format(code).then_some(code)
}

/// Whether a number-format code renders its value as a date or a time.
pub fn is_date_format(code: &str) -> bool {
    let mut in_literal = false;
    let mut chars = code.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => in_literal = !in_literal,
            '\\' | '*' | '_' => {
                chars.next();
            }
            _ if in_literal => {}
            // `m` is minutes or months depending on its neighbours, but either
            // way the cell is a date/time.
            'y' | 'd' | 'h' | 's' | 'm' | 'Y' | 'D' | 'H' | 'S' | 'M' => return true,
            _ => {}
        }
    }
    false
}

/// Append a field, quoting per RFC 4180 if it holds the delimiter, a quote, or a
/// line break.
fn push_quoted(out: &mut String, field: &str, delimiter: u8) {
    let needs_quotes = field
        .bytes()
        .any(|b| b == delimiter || b == b'"' || b == b'\n' || b == b'\r');
    if !needs_quotes {
        out.push_str(field);
        return;
    }
    out.push('"');
    for ch in field.chars() {
        if ch == '"' {
            out.push('"');
        }
        out.push(ch);
    }
    out.push('"');
}

/// Split delimited text into records of fields, honoring RFC 4180 quoting and
/// both `\n` and `\r\n` line endings. A trailing newline does not add an empty
/// record.
fn parse_records(text: &str, delim: char) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        match ch {
            '"' => in_quotes = true,
            c if c == delim => record.push(std::mem::take(&mut field)),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            c => field.push(c),
        }
    }
    // Flush a final record that had no trailing newline.
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}

#[cfg(test)]
mod tests;
