//! A1 cell references with `$` anchors and optional sheet qualification.

use core::fmt;

use serde::{Deserialize, Serialize};

/// A single cell reference, e.g. `A1`, `$B$7`, `Sheet2!C3`. Coordinates are
/// zero-based.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CellReference {
    /// The qualifying sheet name, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    /// Zero-based column.
    pub col: u32,
    /// Zero-based row.
    pub row: u32,
    /// Whether the column is `$`-anchored (absolute).
    #[serde(default, skip_serializing_if = "is_false")]
    pub col_absolute: bool,
    /// Whether the row is `$`-anchored (absolute).
    #[serde(default, skip_serializing_if = "is_false")]
    pub row_absolute: bool,
    /// The reference named no row: it is a whole column, as in `A:A`.
    ///
    /// `row` still holds a usable bound — 0 at the start of a range, the last
    /// row at the end — so every consumer that reads `row` without knowing
    /// about this keeps working and simply sees the widest possible span. Only
    /// code that cares (printing, and clamping a range to the data) looks at
    /// the flag.
    #[serde(default, skip_serializing_if = "is_false")]
    pub row_implicit: bool,
    /// The reference named no column: it is a whole row, as in `$1:$2`. `col`
    /// carries the equivalent bound, exactly as [`Self::row_implicit`] does.
    #[serde(default, skip_serializing_if = "is_false")]
    pub col_implicit: bool,
}

/// The last row and column a sheet can have, which is what an unnamed axis
/// spans. OOXML's limits; the evaluator clamps to the used region before
/// iterating, so these bounds are never walked in full.
pub const MAX_ROW: u32 = 1_048_575;
/// See [`MAX_ROW`].
pub const MAX_COL: u32 = 16_383;

fn is_false(value: &bool) -> bool {
    !*value
}

/// Convert a zero-based column index to letters (`0` → `A`, `26` → `AA`).
pub fn column_to_letters(mut index: u32) -> String {
    let mut letters = Vec::new();
    loop {
        let remainder = (index % 26) as u8;
        letters.push(b'A' + remainder);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    letters.reverse();
    String::from_utf8(letters).expect("ascii")
}

/// Parse a bare A1 reference (no sheet), tracking `$` anchors. Returns `None`
/// if the text is not a valid reference.
pub fn parse_a1(text: &str) -> Option<CellReference> {
    let bytes = text.as_bytes();
    let mut i = 0;

    let col_absolute = bytes.first() == Some(&b'$');
    if col_absolute {
        i += 1;
    }

    let mut col: u32 = 0;
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        let value = (bytes[i].to_ascii_uppercase() - b'A' + 1) as u32;
        col = col.checked_mul(26)?.checked_add(value)?;
        i += 1;
    }
    if i == start {
        return None;
    }

    let row_absolute = bytes.get(i) == Some(&b'$');
    if row_absolute {
        i += 1;
    }

    let mut row: u32 = 0;
    let mut saw_digit = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        row = row.checked_mul(10)?.checked_add((bytes[i] - b'0') as u32)?;
        saw_digit = true;
        i += 1;
    }
    if !saw_digit || row == 0 || i != bytes.len() {
        return None;
    }

    Some(CellReference {
        sheet: None,
        col: col - 1,
        row: row - 1,
        col_absolute,
        row_absolute,
        row_implicit: false,
        col_implicit: false,
    })
}

/// Parse one side of a whole-row or whole-column reference: `A`, `$A`, `1` or
/// `$1`.
///
/// Separate from [`parse_a1`] on purpose. `parse_a1` is also the test for
/// "does this name look like a cell reference", and teaching it that a bare
/// `A` is a reference would make every one-letter defined name illegal.
///
/// `at_end` picks the bound the missing component takes: a range's start wants
/// the first row/column and its end wants the last, so `A:A` covers the whole
/// column without any consumer needing to know that.
#[must_use]
pub fn parse_a1_axis(text: &str, at_end: bool) -> Option<CellReference> {
    let bytes = text.as_bytes();
    let absolute = bytes.first() == Some(&b'$');
    let rest = if absolute { &text[1..] } else { text };
    if rest.is_empty() {
        return None;
    }

    if rest.bytes().all(|b| b.is_ascii_alphabetic()) {
        let mut col: u32 = 0;
        for b in rest.bytes() {
            let value = (b.to_ascii_uppercase() - b'A' + 1) as u32;
            col = col.checked_mul(26)?.checked_add(value)?;
        }
        let col = col.checked_sub(1)?;
        if col > MAX_COL {
            return None;
        }
        return Some(CellReference {
            sheet: None,
            col,
            row: if at_end { MAX_ROW } else { 0 },
            col_absolute: absolute,
            row_absolute: false,
            row_implicit: true,
            col_implicit: false,
        });
    }

    if rest.bytes().all(|b| b.is_ascii_digit()) {
        let row: u32 = rest.parse().ok()?;
        let row = row.checked_sub(1)?;
        if row > MAX_ROW {
            return None;
        }
        return Some(CellReference {
            sheet: None,
            col: if at_end { MAX_COL } else { 0 },
            row,
            col_absolute: false,
            row_absolute: absolute,
            row_implicit: false,
            col_implicit: true,
        });
    }

    None
}

/// Whether a sheet name has to be written in single quotes to be read back.
///
/// The old rule was "quote unless every character is alphanumeric or `_`",
/// which asks the wrong question. A sheet called `2024` clears that test and is
/// emitted bare as `2024!A1` — text that neither this engine's parser nor
/// Excel's will read as a reference, which is exactly why Excel always writes
/// `'2024'!A1`. Round-tripping such a workbook dropped the formula: the
/// re-import failed to parse it, kept the stale cached value, and left a
/// hard-coded constant that never recalculates again.
///
/// The right question is whether the name is a plain identifier that cannot be
/// mistaken for something else. Three ways it can fail:
///
/// - it contains something outside `[A-Za-z0-9_]`, or is empty;
/// - it starts with a digit, so `2024!A1` reads as a number followed by junk;
/// - it *is* a cell reference — a sheet named `A1` written bare gives `A1!B2`,
///   and a sheet named `R1C1` collides with the other reference style.
///
/// Quoting more often than strictly required is free: a quoted name is always
/// valid. Quoting less often is silent data loss, so this errs toward quoting.
fn sheet_name_needs_quoting(sheet: &str) -> bool {
    let mut chars = sheet.chars();
    let Some(first) = chars.next() else {
        return true; // Empty. Not a name, and certainly not a bare one.
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return true;
    }
    if !sheet.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return true;
    }
    looks_like_a_reference(sheet)
}

/// Whether `name` would parse as a cell reference in either notation.
///
/// `A1` style is one to three letters then one to seven digits. `R1C1` style is
/// `R`/`C` with optional digits — including the bare `R` and `C` Excel reserves
/// for it.
fn looks_like_a_reference(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let letters = upper.chars().take_while(char::is_ascii_alphabetic).count();
    let digits = upper.len() - letters;
    if (1..=3).contains(&letters) && (1..=7).contains(&digits) {
        // The tail after the letters must be all digits for this to be a
        // reference rather than a name that merely starts like one.
        if upper[letters..].chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    if upper == "R" || upper == "C" {
        return true;
    }
    // R1C1: `R`, digits, `C`, digits — any of the digit runs may be empty.
    if let Some(rest) = upper.strip_prefix('R') {
        let after_row = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        if let Some(cols) = after_row.strip_prefix('C') {
            let row_digits = rest.len() - after_row.len();
            if cols.chars().all(|c| c.is_ascii_digit()) && (row_digits > 0 || !cols.is_empty()) {
                return true;
            }
        }
    }
    false
}

impl fmt::Display for CellReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(sheet) = &self.sheet {
            if sheet_name_needs_quoting(sheet) {
                let escaped = sheet.replace('\'', "''");
                write!(f, "'{escaped}'!")?;
            } else {
                write!(f, "{sheet}!")?;
            }
        }
        // A component the source did not name is not printed, or `A:A` would
        // come back as `A1:A1048576` and no longer mean the same thing.
        if !self.col_implicit {
            if self.col_absolute {
                f.write_str("$")?;
            }
            f.write_str(&column_to_letters(self.col))?;
        }
        if !self.row_implicit {
            if self.row_absolute {
                f.write_str("$")?;
            }
            write!(f, "{}", self.row + 1)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{column_to_letters, parse_a1};

    #[test]
    fn column_letters_roundtrip() {
        assert_eq!(column_to_letters(0), "A");
        assert_eq!(column_to_letters(25), "Z");
        assert_eq!(column_to_letters(26), "AA");
        assert_eq!(column_to_letters(701), "ZZ");
        assert_eq!(column_to_letters(702), "AAA");
    }

    #[test]
    fn parses_anchors() {
        let r = parse_a1("$B$7").unwrap();
        assert_eq!((r.col, r.row), (1, 6));
        assert!(r.col_absolute && r.row_absolute);
        assert_eq!(r.to_string(), "$B$7");

        let r = parse_a1("A1").unwrap();
        assert_eq!((r.col, r.row), (0, 0));
        assert!(!r.col_absolute && !r.row_absolute);

        assert!(parse_a1("A").is_none());
        assert!(parse_a1("1").is_none());
        assert!(parse_a1("A0").is_none());
        assert!(parse_a1("A1B").is_none());
    }
}
