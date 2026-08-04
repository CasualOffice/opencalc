//! A1 cell-reference parsing (e.g. `B7`, `$B$7`).

use casual_calc_model::{CellRange, CellRef};

/// Parse an A1 range (`A1:B2`, or a single cell `A1`) into a [`CellRange`].
pub fn parse_range(text: &str) -> Option<CellRange> {
    match text.split_once(':') {
        Some((a, b)) => Some(CellRange::new(parse_a1(a)?, parse_a1(b)?)),
        None => {
            let cell = parse_a1(text)?;
            Some(CellRange::new(cell, cell))
        }
    }
}

/// Parse an A1 reference into a zero-based [`CellRef`]. Accepts `$` anchors;
/// returns `None` if the reference is malformed.
pub fn parse_a1(reference: &str) -> Option<CellRef> {
    let bytes = reference.trim().as_bytes();
    let mut i = 0;

    let mut column: u32 = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'$' => {}
            c if c.is_ascii_alphabetic() => {
                let value = (c.to_ascii_uppercase() - b'A' + 1) as u32;
                column = column.checked_mul(26)?.checked_add(value)?;
            }
            _ => break,
        }
        i += 1;
    }
    if column == 0 {
        return None;
    }

    let mut row: u32 = 0;
    let mut saw_digit = false;
    while i < bytes.len() {
        match bytes[i] {
            b'$' => {}
            c if c.is_ascii_digit() => {
                row = row.checked_mul(10)?.checked_add((c - b'0') as u32)?;
                saw_digit = true;
            }
            _ => return None,
        }
        i += 1;
    }
    if !saw_digit || row == 0 {
        return None;
    }

    Some(CellRef::new(row - 1, column - 1))
}

#[cfg(test)]
mod tests {
    use super::parse_a1;
    use casual_calc_model::CellRef;

    #[test]
    fn parses_references() {
        assert_eq!(parse_a1("A1"), Some(CellRef::new(0, 0)));
        assert_eq!(parse_a1("B7"), Some(CellRef::new(6, 1)));
        assert_eq!(parse_a1("$B$7"), Some(CellRef::new(6, 1)));
        assert_eq!(parse_a1("Z1"), Some(CellRef::new(0, 25)));
        assert_eq!(parse_a1("AA1"), Some(CellRef::new(0, 26)));
        assert_eq!(parse_a1("AB10"), Some(CellRef::new(9, 27)));
    }

    #[test]
    fn rejects_malformed() {
        assert_eq!(parse_a1(""), None);
        assert_eq!(parse_a1("1"), None);
        assert_eq!(parse_a1("A"), None);
        assert_eq!(parse_a1("A0"), None);
        assert_eq!(parse_a1("1A"), None);
    }
}
