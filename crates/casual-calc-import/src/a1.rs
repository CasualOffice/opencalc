//! A1 cell-reference parsing (e.g. `B7`, `$B$7`), bounded by the addressable
//! grid.
//!
//! # What an out-of-range reference becomes, and why (FID-18)
//!
//! This module used to accept any reference that fitted in a `u32`, which is
//! four thousand times more rows than a sheet has. A worksheet carrying
//!
//! ```text
//! <row r="4294967295"><c r="ZZZZ4294967295"><v>7</v></c></row>
//! <mergeCell ref="A1:ZZZZ4294967295"/>
//! ```
//!
//! imported cleanly to `row=4294967294 col=475253` with an **empty**
//! compatibility report, handed layout a 475,254-column by 4-billion-row merge
//! to walk, and was re-emitted verbatim on save. The saved package is outside
//! ECMA-376's addressable grid, so the engine took a bad input and produced a
//! corrupt output — a file Excel and LibreOffice both refuse — while telling the
//! host nothing had happened. That last part is the actual defect: a reader is
//! allowed to meet input it cannot represent, and is not allowed to be quiet
//! about it.
//!
//! Three dispositions were available, and only one of them is honest:
//!
//! - **Refuse the package.** Correct about the input and useless about the file:
//!   one bad `r` attribute in a hundred-sheet workbook would leave the user with
//!   nothing, which is precisely the outcome the preservation architecture
//!   (`docs/34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md`) exists to avoid. Import
//!   is loss-*aware*, not all-or-nothing. `docs/21-PARSER-LIMITS.md`'s "fail
//!   closed" governs *resource* bounds — a file that would exhaust memory —
//!   and a single unrepresentable address threatens no resource.
//! - **Clamp into range.** The worst of the three, and the tempting one, because
//!   it yields a valid file. `ZZZZ4294967295` would become `XFD1048576` and the
//!   7 would now sit in a cell nobody put it in, reported as `Mapped`. That is
//!   silent data *corruption* wearing the costume of a successful import, and it
//!   breaks the first engineering priority (never produce wrong cell values)
//!   with no diagnostic anywhere.
//! - **Drop it and report it.** What this module does. docs/34 names exactly one
//!   way for data to leave the system — `Omitted` + `NotRetained` — and requires
//!   it to be counted and named in the `CompatibilityReport`. An address that
//!   does not exist in the grid cannot be preserved verbatim either, since
//!   re-emitting it is what corrupts the package, so `NotRetained` is not a
//!   choice so much as a fact.
//!
//! The rule is applied here rather than at each of the dozen call sites because
//! here is the single point where file text becomes a [`CellRef`]. A cell, a
//! merged range, a defined name, an `sqref` area, a hyperlink and a `<col>` span
//! therefore all get the same answer, which is the other half of the bug: the
//! old code was not inconsistent, it was uniformly unbounded, and a fix that
//! only taught `<c r>` about the grid would have left `<mergeCell>` writing
//! packages just as unopenable.
//!
//! The invariant this holds up: **what this engine writes is inside the
//! addressable grid**, and whatever it declined to keep is visible in the
//! report rather than absent.

use casual_calc_model::{CellRange, CellRef, GRID_MAX_COL, GRID_MAX_ROW};

/// What a reference off the wire turned into.
///
/// Three outcomes rather than an `Option`, because "outside the grid" and "not
/// a reference at all" are different failures and the report has to be able to
/// say which one it saw. Collapsing them was how the missing bound stayed
/// invisible for so long: every call site already handled `None`, so adding the
/// check without adding this enum would have produced a fix that dropped the
/// cell and still said nothing about why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Parsed<T> {
    /// A reference, inside the grid.
    Ok(T),
    /// Well-formed A1 naming a row or column past the ECMA-376 maximum.
    OutOfGrid,
    /// Not an A1 reference.
    Malformed,
}

impl<T> Parsed<T> {
    /// The reference, if there is one.
    pub(crate) fn ok(self) -> Option<T> {
        match self {
            Parsed::Ok(value) => Some(value),
            _ => None,
        }
    }

    /// Whether this was a real address the grid cannot hold — the case that has
    /// to reach the compatibility report.
    pub(crate) fn is_out_of_grid(&self) -> bool {
        matches!(self, Parsed::OutOfGrid)
    }
}

/// Parse an A1 range (`A1:B2`, or a single cell `A1`) into a [`CellRange`].
///
/// A range is refused **whole** when either corner is out of range. Keeping the
/// half that fits would be clamping by another name: `A1:ZZZZ4294967295` reduced
/// to its in-grid corner is `A1:XFD1048576`, a merge over the entire sheet,
/// which is a far louder lie than the one it replaced.
pub fn parse_range(text: &str) -> Option<CellRange> {
    parse_range_classified(text).ok()
}

/// [`parse_range`], keeping why it failed.
pub(crate) fn parse_range_classified(text: &str) -> Parsed<CellRange> {
    match text.split_once(':') {
        Some((a, b)) => match (parse_a1_classified(a), parse_a1_classified(b)) {
            (Parsed::Ok(a), Parsed::Ok(b)) => Parsed::Ok(CellRange::new(a, b)),
            (x, y) if x.is_out_of_grid() || y.is_out_of_grid() => Parsed::OutOfGrid,
            _ => Parsed::Malformed,
        },
        None => match parse_a1_classified(text) {
            Parsed::Ok(cell) => Parsed::Ok(CellRange::new(cell, cell)),
            Parsed::OutOfGrid => Parsed::OutOfGrid,
            Parsed::Malformed => Parsed::Malformed,
        },
    }
}

/// Parse an A1 reference into a zero-based [`CellRef`]. Accepts `$` anchors;
/// returns `None` if the reference is malformed **or outside the grid**.
pub fn parse_a1(reference: &str) -> Option<CellRef> {
    parse_a1_classified(reference).ok()
}

/// [`parse_a1`], keeping why it failed.
///
/// The accumulators are `u64` and saturate rather than `u32` and overflow.
/// `checked_mul` on a `u32` answered `None` for `ZZZZZZZZZZ1`, which the caller
/// then reported as a malformed reference — it is nothing of the kind, it is a
/// perfectly well-formed address for a column that does not exist, and the
/// report would have named the wrong problem.
pub(crate) fn parse_a1_classified(reference: &str) -> Parsed<CellRef> {
    let bytes = reference.trim().as_bytes();
    let mut i = 0;

    let mut column: u64 = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'$' => {}
            c if c.is_ascii_alphabetic() => {
                let value = u64::from(c.to_ascii_uppercase() - b'A' + 1);
                column = column.saturating_mul(26).saturating_add(value);
            }
            _ => break,
        }
        i += 1;
    }
    if column == 0 {
        return Parsed::Malformed;
    }

    let mut row: u64 = 0;
    let mut saw_digit = false;
    while i < bytes.len() {
        match bytes[i] {
            b'$' => {}
            c if c.is_ascii_digit() => {
                row = row.saturating_mul(10).saturating_add(u64::from(c - b'0'));
                saw_digit = true;
            }
            _ => return Parsed::Malformed,
        }
        i += 1;
    }
    if !saw_digit || row == 0 {
        return Parsed::Malformed;
    }

    // One-based on the wire, zero-based in the model, and the comparison is done
    // in `u64` before the narrowing cast — checking after the cast is what an
    // `as u32` would have made of `4294967297`, which is row 1.
    let (row, column) = (row - 1, column - 1);
    if row > u64::from(GRID_MAX_ROW) || column > u64::from(GRID_MAX_COL) {
        return Parsed::OutOfGrid;
    }
    Parsed::Ok(CellRef::new(row as u32, column as u32))
}

#[cfg(test)]
mod tests {
    use super::{Parsed, parse_a1, parse_a1_classified, parse_range};
    use casual_calc_model::{CellRef, GRID_MAX_COL, GRID_MAX_ROW};

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

    /// The last cell of the grid is a cell; the first one past it is not.
    #[test]
    fn the_grid_is_bounded_at_its_last_cell() {
        assert_eq!(
            parse_a1("XFD1048576"),
            Some(CellRef::new(GRID_MAX_ROW, GRID_MAX_COL))
        );
        assert_eq!(parse_a1("XFD1048577"), None);
        assert_eq!(parse_a1("XFE1048576"), None);
        assert_eq!(parse_a1("ZZZZ4294967295"), None);
        assert_eq!(parse_range("A1:ZZZZ4294967295"), None);
    }

    /// An address the grid cannot hold is not a typo, and the report must not
    /// call it one.
    #[test]
    fn out_of_grid_is_told_apart_from_malformed() {
        assert_eq!(parse_a1_classified("XFD1048577"), Parsed::OutOfGrid);
        assert_eq!(parse_a1_classified("ZZZZ4294967295"), Parsed::OutOfGrid);
        // Past `u32` entirely, in both axes: still an address, still not a typo.
        assert_eq!(parse_a1_classified("ZZZZZZZZZZ1"), Parsed::OutOfGrid);
        assert_eq!(
            parse_a1_classified("A99999999999999999999"),
            Parsed::OutOfGrid
        );
        assert_eq!(parse_a1_classified("A1:B2"), Parsed::Malformed);
        assert_eq!(parse_a1_classified("A0"), Parsed::Malformed);
        // `Sheet1` reads as column `SHEET`, row 1 — a well-formed address for a
        // column 300 million past the end of the grid. Out of grid, not
        // malformed: the distinction is about shape, and this has the shape.
        assert_eq!(parse_a1_classified("Sheet1"), Parsed::OutOfGrid);
    }
}
