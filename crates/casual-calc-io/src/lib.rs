//! Format-neutral identities, detection/dispatch, and built-in adapters.
//!
//! The first adapter is **delimited text** (CSV / TSV / PSV): a deterministic,
//! RFC 4180-style reader and writer over the normalized [`Workbook`]. Fields are
//! typed on read (number / boolean / text) and a parse → write → parse round-trip
//! is a model fixed point. XLSX/ODS live in their own crates; this crate is where
//! the lightweight, dependency-free text formats and the format registry belong.
//!
//! See `docs/19-WORKSPACE-SCAFFOLD-DESIGN.md`.

use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

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
/// (case-insensitive) become [`CellValue::Bool`], the empty string is a blank
/// cell, and anything else is interned as text.
pub fn read_delimited(bytes: &[u8], delimiter: u8) -> Result<Workbook, IoError> {
    let text = std::str::from_utf8(bytes).map_err(|_| IoError::InvalidUtf8)?;
    let records = parse_records(text, delimiter as char);

    let mut workbook = Workbook::new(Id::from_parts(0x4353_5600_0000_0001, 1)); // "CSV"
    let mut sheet = Sheet::new(SheetId(Id::from_parts(0x4353_5600_0000_0002, 1)), "Sheet1");

    for (r, record) in records.iter().enumerate() {
        for (c, field) in record.iter().enumerate() {
            let value = type_field(field, &mut workbook);
            if value.is_empty() {
                continue;
            }
            sheet
                .cells
                .set(CellRef::new(r as u32, c as u32), Cell::value(value));
        }
    }
    workbook.sheets.push(sheet);
    Ok(workbook)
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
                .map(|cell| field_text(workbook, &cell.value))
                .unwrap_or_default();
            push_quoted(&mut out, &field, delimiter);
        }
        out.push_str("\r\n");
    }
    out
}

/// The typed value for a raw field.
fn type_field(field: &str, workbook: &mut Workbook) -> CellValue {
    if field.is_empty() {
        return CellValue::Empty;
    }
    if field.eq_ignore_ascii_case("true") {
        return CellValue::Bool(true);
    }
    if field.eq_ignore_ascii_case("false") {
        return CellValue::Bool(false);
    }
    if let Ok(number) = field.parse::<f64>()
        && number.is_finite()
    {
        return CellValue::Number(number);
    }
    CellValue::SharedString(workbook.intern_string(field))
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

/// The text form of a cell value for export.
fn field_text(workbook: &Workbook, value: &CellValue) -> String {
    match value {
        CellValue::Empty => String::new(),
        CellValue::Number(n) => format_number(*n),
        CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        CellValue::SharedString(id) | CellValue::InlineString(id) => {
            workbook.strings.get(*id).unwrap_or_default().to_owned()
        }
        CellValue::Error(e) => e.to_string(),
    }
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
