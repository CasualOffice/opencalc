//! Format detection, and the delimited-text adapters.
//!
//! The first adapter is **delimited text** (CSV / TSV / PSV): a deterministic,
//! RFC 4180-style reader and writer over the normalized [`Workbook`]. Fields are
//! typed on read (number / boolean / ISO date / text) and a parse → write →
//! parse round-trip is a model fixed point — which is what rules out Excel's
//! habit of turning `007` into `7`, since that round-trip does not settle.
//! XLSX and ODS live in their own crates, and this one does **not** depend on
//! them. [`detect`] identifies their bytes anyway — a zip's first local file
//! header names its first entry, which is all it takes — so a caller can learn
//! what a file is here and open it a layer up, where the format crates are.
//!
//! That split is [ADR-022](../../../docs/08-ADR-REGISTER.md), which amends
//! `docs/19`: this crate was once described as the adapter registry and the
//! single entry point for opening a spreadsheet. It never was. Dispatch grew in
//! `casual-calc-sdk`, which is the entry point hosts actually call; detection
//! belongs down here, where it costs no dependencies. Giving this crate the
//! whole OOXML stack so the sentence came true was considered and rejected.
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
    // Every field parsed out of the text is the document's own, not this
    // session's — the distinction a writer uses to decide which strings it may
    // reclaim (`FID-36`, `StringTable::preserve_all`).
    workbook.strings.preserve_all();
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
///
/// # What a sparse sheet exports
///
/// A delimited field's *position* is its address: the third field of the fifth
/// line is `C5`, and nothing else in the file says so. So a sheet's cells are
/// written where they are — never compacted towards `A1`, which would move
/// every one of them and is the one transformation that would make a
/// parse → write → parse round trip stop being a fixed point.
///
/// A row index is an address in the same way, so an empty row still costs its
/// line break. What an empty *column* position costs is a delimiter, and only
/// while a further field on that line still has to be placed: trailing empty
/// fields place nothing, and `a,,,` and `a` read back as the same row holding
/// the same one cell. They are padding, not data.
///
/// This is the whole of `IO-02`. Walking the bounding box — every row of the
/// extent by every column of it — pads every line out to the widest, so three
/// cells at `A1`, `(0, n-1)` and `(n, 0)` cost `n²` cell lookups and `n²`
/// bytes: 16 KiB of input exhausted 2 GiB. Walking the populated cells instead,
/// in the row-major order [`casual_calc_model::CellStore`] already iterates in,
/// costs one delimiter per column position actually crossed and one line break
/// per row. For a sheet that came from delimited text that is bounded by the
/// input, since a field at column `c` needed `c` delimiters to say so; for one
/// that came from a `.xlsx` it is bounded by the widest populated row rather
/// than by the product of the extents.
pub fn write_delimited(workbook: &Workbook, sheet_index: usize, delimiter: u8) -> String {
    let Some(sheet) = workbook.sheets.get(sheet_index) else {
        return String::new();
    };

    let delim = delimiter as char;
    let mut out = String::new();
    // The row the line in progress belongs to, and how many delimiters it
    // carries so far — which is exactly the column index of the next field
    // position that has not been passed.
    let (mut row, mut delims) = (0u32, 0u32);
    let mut started = false;

    for (at, cell) in sheet.cells.iter() {
        let field = field_text(workbook, &cell.value, cell.style);
        // A cell whose value renders as nothing is indistinguishable from an
        // empty position on the way back in, so it neither opens a line nor
        // holds a column open behind it.
        if field.is_empty() {
            continue;
        }
        started = true;
        for _ in row..at.row {
            out.push_str("\r\n");
        }
        if at.row != row {
            row = at.row;
            delims = 0;
        }
        for _ in delims..at.col {
            out.push(delim);
        }
        delims = at.col;
        push_quoted(&mut out, &field, delimiter);
    }

    if started {
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
///
/// # Significant digits, not decimal places
///
/// The rounding is done in **exponential** form — `{n:.14e}` is one digit before
/// the point and fourteen after it, which is fifteen significant digits at any
/// magnitude — and the result is reparsed, so what is written back is the
/// nearest `f64` to those fifteen digits. Rust's `{}` and `{:e}` both print the
/// shortest decimal that reads back as the same `f64`, so nothing beyond the
/// deliberate rounding is lost.
///
/// Deriving decimal *places* from the magnitude instead is `IO-01`. It cannot
/// express the digits a small value needs — `(14 - magnitude).clamp(0, 15)`
/// asked for sixteen or more and was given fifteen — so `format!("{n:.15}")`
/// rendered `1e-16` as `0.000000000000000` and the cell was written **`0`**.
/// Not rounded, erased; and `-1e-300` became `-0`, which the `n == 0.0` branch
/// above then wrote as `0` on the *next* save, so the sign left one save after
/// the magnitude did. At the other end the same clamp asked for **zero**
/// decimal places above `1e15`, which is rounding to the integer rather than to
/// fifteen digits, so the binary tail it exists to hide was written out in full.
///
/// # Which notation
///
/// Positional up to `1e21` and down to `1e-6`, exponential outside that — the
/// rule `f64`'s own shortest-round-trip printers in other languages use, and
/// close to where Excel's General switches. Both forms read back as the same
/// number here and in Excel, so the choice is only about which is not mostly
/// padding: `1e300` positional is 301 digits of which 300 say nothing, and
/// `5e-324` is 324. Every value that had a reasonable positional form before
/// still gets one.
fn format_number(n: f64) -> String {
    if n == 0.0 {
        return "0".to_owned();
    }
    if !n.is_finite() {
        return format!("{n}");
    }
    // Fifteen significant digits, and the decimal exponent that goes with them.
    let significant = format!("{n:.14e}");
    let exponent: i32 = significant
        .rsplit('e')
        .next()
        .and_then(|e| e.parse().ok())
        .unwrap_or(0);
    let rounded = match significant.parse::<f64>() {
        // Rounding up at the top of the range leaves `f64`: `f64::MAX` rounds
        // to `1.79769313486232e308`, which parses back as infinity, and `inf`
        // is a number leaving the file as text — `read_delimited` types only
        // finite fields. That half is reached, by `seeds/delimited/
        // magnitudes.psv`. The other half cannot be, since a mantissa of at
        // least 1 cannot round to zero; it is stated because turning a value
        // into zero is the whole of `IO-01`, and a guard against it does not
        // belong in the reader alone.
        Ok(rounded) if rounded.is_finite() && rounded != 0.0 => rounded,
        _ => n,
    };
    if (-6..21).contains(&exponent) {
        format!("{rounded}")
    } else {
        format!("{rounded:e}")
    }
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

/// What a run of bytes looks like, by its own content (`ODS-01`).
///
/// Deliberately **not** the same type as the SDK's `SessionFormat`: this crate
/// knows what a file *is*, and the SDK knows what this engine will do about it.
/// Keeping them apart is what lets detection live down here, where it needs no
/// format crates at all, while dispatch stays where the format crates are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detected {
    /// An OOXML package — a zip whose first entry is `[Content_Types].xml`.
    Xlsx,
    /// An OpenDocument spreadsheet.
    Ods,
    /// Delimited text, with the separator the first line is built from.
    Delimited(u8),
}

/// The format `bytes` are, read from the bytes themselves.
///
/// # Why this is not the filename
///
/// A filename extension is the file's own claim about itself, and so is a
/// content type. This engine believed that claim for whole documents while
/// already refusing to for the pictures inside them — `foreign_format` in
/// `casual-calc-render` sniffs magic numbers for exactly this reason. A `.xlsx`
/// that is really an `.ods` opened wrong, and an upload with no filename could
/// not be opened at all.
///
/// # What it will not do
///
/// Guess. `None` when the bytes do not clearly say, because "probably a CSV"
/// applied to a binary file produces a sheet full of mojibake and a person
/// wondering what happened to their document. A caller that knows the format
/// should still say so; this is for the caller that does not.
#[must_use]
pub fn detect(bytes: &[u8]) -> Option<Detected> {
    if bytes.starts_with(b"PK\x03\x04") {
        return detect_zip(bytes);
    }
    detect_delimited(bytes)
}

/// An OPC package, told apart by the name of its **first** entry.
///
/// Read from the local file header rather than by unzipping: the name sits at a
/// fixed offset after a 30-byte header, so this needs no zip reader and no
/// inflation — which is what keeps this crate free of the format stack.
///
/// ODF requires `mimetype` to be first *and stored uncompressed*, precisely so
/// that a reader can identify the document without decompressing it; OOXML
/// conventionally puts `[Content_Types].xml` first. A zip that is neither is
/// not something this engine opens, and says so rather than guessing.
fn detect_zip(bytes: &[u8]) -> Option<Detected> {
    const HEADER: usize = 30;
    let name_len = usize::from(u16::from_le_bytes([*bytes.get(26)?, *bytes.get(27)?]));
    let extra_len = usize::from(u16::from_le_bytes([*bytes.get(28)?, *bytes.get(29)?]));
    let name = bytes.get(HEADER..HEADER.checked_add(name_len)?)?;

    if name.eq_ignore_ascii_case(b"[Content_Types].xml") {
        return Some(Detected::Xlsx);
    }
    if name == b"mimetype" {
        // Stored, so themedia type is the next bytes verbatim. Bounded to the
        // declared size rather than scanned for, so a `mimetype` entry that
        // lies about its length cannot walk this off the end of the buffer.
        let at = HEADER.checked_add(name_len)?.checked_add(extra_len)?;
        let size = u32::from_le_bytes([
            *bytes.get(18)?,
            *bytes.get(19)?,
            *bytes.get(20)?,
            *bytes.get(21)?,
        ]) as usize;
        let media = bytes.get(at..at.checked_add(size)?)?;
        if media == b"application/vnd.oasis.opendocument.spreadsheet" {
            return Some(Detected::Ods);
        }
        // An ODF document that is not a spreadsheet — a text document, a
        // presentation. Named as not-ours rather than opened as one.
        return None;
    }
    None
}

/// Delimited text, by which separator the first line is actually built from.
///
/// Counted **outside quotes**, because a comma inside `"Smith, J"` is data and
/// counting it would call a tab-separated file a CSV. Ties go to the earlier
/// entry in the list, which is the conventional order.
fn detect_delimited(bytes: &[u8]) -> Option<Detected> {
    // Binary is not text. A NUL in the first kilobyte is the cheapest reliable
    // signal, and it is what stops a `.bin` being read as a one-column sheet.
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0) {
        return None;
    }
    let text = core::str::from_utf8(head).ok()?;
    let line = text.lines().next()?;
    if line.is_empty() {
        return None;
    }

    let mut best: Option<(u8, usize)> = None;
    for sep in [b',', b'\t', b'|', b';'] {
        let mut count = 0usize;
        let mut quoted = false;
        for b in line.bytes() {
            match b {
                b'"' => quoted = !quoted,
                _ if b == sep && !quoted => count += 1,
                _ => {}
            }
        }
        if count > 0 && best.is_none_or(|(_, seen)| count > seen) {
            best = Some((sep, count));
        }
    }
    // `None` when nothing separated anything: one column of text is a
    // legitimate CSV and is also every plain-text file ever written. Refused,
    // because the caller who really meant it can still say so.
    best.map(|(sep, _)| Detected::Delimited(sep))
}
