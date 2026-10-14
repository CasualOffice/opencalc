//! A1 addresses as an agent writes them: `B7`, `$B$7`, `A1:C10`, `A:C`, `2:5`,
//! each optionally sheet-qualified (`Sheet2!A1:B3`, `'Q1 Sales'!A:A`).

use casual_calc_sdk::CellRef;

/// The last row index (0-based) a sheet has — Excel's 1,048,576 rows.
pub const MAX_ROW: u32 = 1_048_575;
/// The last column index (0-based) — Excel's `XFD`.
pub const MAX_COL: u32 = 16_383;

/// A rectangle, both corners inclusive and 0-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Top-left.
    pub start: CellRef,
    /// Bottom-right.
    pub end: CellRef,
}

impl Rect {
    /// Rows in the rectangle.
    pub fn rows(&self) -> u64 {
        u64::from(self.end.row - self.start.row) + 1
    }
    /// Columns in the rectangle.
    pub fn cols(&self) -> u64 {
        u64::from(self.end.col - self.start.col) + 1
    }
    /// Cells in the rectangle.
    pub fn cells(&self) -> u64 {
        self.rows() * self.cols()
    }
}

impl std::fmt::Display for Rect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.start == self.end {
            write!(f, "{}", cell_name(self.start))
        } else {
            write!(f, "{}:{}", cell_name(self.start), cell_name(self.end))
        }
    }
}

/// What a range reference names before it is tied to a sheet's contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    /// An ordinary rectangle (`A1`, `A1:C3`).
    Cells(Rect),
    /// Whole columns (`A:C`), first and last 0-based.
    Cols(u32, u32),
    /// Whole rows (`2:5`), first and last 0-based.
    Rows(u32, u32),
}

/// `A`, `XFD` → 0-based column, or `None` past `XFD`.
pub fn parse_col(text: &str) -> Option<u32> {
    let text = text.strip_prefix('$').unwrap_or(text);
    if text.is_empty() || text.len() > 3 || !text.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let mut n: u32 = 0;
    for b in text.bytes() {
        n = n * 26 + u32::from(b.to_ascii_uppercase() - b'A') + 1;
    }
    (n >= 1 && n - 1 <= MAX_COL).then(|| n - 1)
}

/// `1` → row 0, or `None` outside 1..=1,048,576.
pub fn parse_row(text: &str) -> Option<u32> {
    let text = text.strip_prefix('$').unwrap_or(text);
    if text.is_empty() || text.len() > 7 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = text.parse().ok()?;
    (n >= 1 && n - 1 <= MAX_ROW).then(|| n - 1)
}

/// `B7`, `$B$7` → a cell.
pub fn parse_cell(text: &str) -> Option<CellRef> {
    let text = text.trim();
    let body = text.strip_prefix('$').unwrap_or(text);
    let split = body.find(|c: char| !c.is_ascii_alphabetic())?;
    let (col, row) = body.split_at(split);
    Some(CellRef::new(parse_row(row)?, parse_col(col)?))
}

/// The column letters for a 0-based column.
pub fn col_name(col: u32) -> String {
    let mut n = col + 1;
    let mut out = Vec::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        out.push(b'A' + rem as u8);
        n = (n - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// `B7` for a cell.
pub fn cell_name(at: CellRef) -> String {
    format!("{}{}", col_name(at.col), at.row + 1)
}

/// Split `Sheet!A1:B2` into its sheet (unquoted) and the rest.
///
/// A quoted name doubles an embedded apostrophe (`'Bob''s'!A1`), as Excel
/// writes it.
pub fn split_sheet(text: &str) -> Result<(Option<String>, &str), String> {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix('\'') {
        let mut name = String::new();
        let mut chars = rest.char_indices();
        while let Some((i, c)) = chars.next() {
            if c == '\'' {
                if rest[i + 1..].starts_with('\'') {
                    name.push('\'');
                    chars.next();
                    continue;
                }
                let after = &rest[i + 1..];
                let range = after
                    .strip_prefix('!')
                    .ok_or_else(|| format!("expected ! after the sheet name in {text}"))?;
                return Ok((Some(name), range));
            }
            name.push(c);
        }
        return Err(format!("unterminated quoted sheet name in {text}"));
    }
    match text.rsplit_once('!') {
        Some((sheet, range)) => Ok((Some(sheet.to_owned()), range)),
        None => Ok((None, text)),
    }
}

/// Parse a range body (no sheet) into a [`Span`].
pub fn parse_span(text: &str) -> Result<Span, String> {
    let bad = || format!("{text:?} is not an A1 reference (expected e.g. B7, A1:C10, A:C or 2:5)");
    let text = text.trim();
    let (a, b) = match text.split_once(':') {
        Some((a, b)) => (a.trim(), b.trim()),
        None => (text, text),
    };
    if let (Some(a), Some(b)) = (parse_cell(a), parse_cell(b)) {
        return Ok(Span::Cells(Rect {
            start: CellRef::new(a.row.min(b.row), a.col.min(b.col)),
            end: CellRef::new(a.row.max(b.row), a.col.max(b.col)),
        }));
    }
    if text.contains(':') {
        if let (Some(a), Some(b)) = (parse_col(a), parse_col(b)) {
            return Ok(Span::Cols(a.min(b), a.max(b)));
        }
        if let (Some(a), Some(b)) = (parse_row(a), parse_row(b)) {
            return Ok(Span::Rows(a.min(b), a.max(b)));
        }
    }
    Err(bad())
}

/// Quote a sheet name for an A1 reference when it needs it.
pub fn quote_sheet(name: &str) -> String {
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        && !name.starts_with(|c: char| c.is_ascii_digit());
    if plain {
        name.to_owned()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_and_columns_round_trip() {
        assert_eq!(parse_cell("A1"), Some(CellRef::new(0, 0)));
        assert_eq!(parse_cell("$B$7"), Some(CellRef::new(6, 1)));
        assert_eq!(
            parse_cell("xfd1048576"),
            Some(CellRef::new(MAX_ROW, MAX_COL))
        );
        assert_eq!(parse_cell("XFE1"), None);
        assert_eq!(parse_cell("A0"), None);
        assert_eq!(parse_cell("A1048577"), None);
        assert_eq!(parse_cell("1A"), None);
        for c in [0, 25, 26, 701, 702, MAX_COL] {
            assert_eq!(parse_col(&col_name(c)), Some(c));
        }
        assert_eq!(col_name(27), "AB");
    }

    #[test]
    fn spans_normalise_and_name_whole_lines() {
        assert_eq!(
            parse_span("C3:A1").unwrap(),
            Span::Cells(Rect {
                start: CellRef::new(0, 0),
                end: CellRef::new(2, 2)
            })
        );
        assert_eq!(parse_span("B:D").unwrap(), Span::Cols(1, 3));
        assert_eq!(parse_span("5:2").unwrap(), Span::Rows(1, 4));
        assert!(parse_span("A1:").is_err());
        assert!(parse_span("hello").is_err());
    }

    #[test]
    fn sheet_names_split_quoted_and_plain() {
        assert_eq!(split_sheet("A1").unwrap(), (None, "A1"));
        assert_eq!(
            split_sheet("Data!A1:B2").unwrap(),
            (Some("Data".to_owned()), "A1:B2")
        );
        assert_eq!(
            split_sheet("'Bob''s Q1'!C3").unwrap(),
            (Some("Bob's Q1".to_owned()), "C3")
        );
        assert!(split_sheet("'open!A1").is_err());
        assert_eq!(quote_sheet("Sheet1"), "Sheet1");
        assert_eq!(quote_sheet("Bob's Q1"), "'Bob''s Q1'");
    }
}
