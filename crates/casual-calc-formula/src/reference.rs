//! A1 cell references with `$` anchors and optional sheet qualification.

use core::fmt;

use serde::{Deserialize, Serialize};

/// A single cell reference, e.g. `A1`, `$B$7`, `Sheet2!C3`. Coordinates are
/// zero-based.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
}

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
    })
}

impl fmt::Display for CellReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(sheet) = &self.sheet {
            if sheet.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                write!(f, "{sheet}!")?;
            } else {
                let escaped = sheet.replace('\'', "''");
                write!(f, "'{escaped}'!")?;
            }
        }
        if self.col_absolute {
            f.write_str("$")?;
        }
        f.write_str(&column_to_letters(self.col))?;
        if self.row_absolute {
            f.write_str("$")?;
        }
        write!(f, "{}", self.row + 1)
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
