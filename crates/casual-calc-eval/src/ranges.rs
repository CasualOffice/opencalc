//! Turning a range expression into the bounds worth walking.
//!
//! A whole-column reference (`A:A`) or whole-row reference (`$1:$2`) names no
//! row or no column, so the parser fills the missing side with the sheet's
//! limit — 1,048,575 rows, 16,383 columns. Iterating that literally is a
//! million-cell loop for `=SUM(A:A)`, which is one of the commonest formulas
//! there is.
//!
//! Every consumer therefore asks for bounds through here, where an unnamed
//! axis is narrowed to the data that actually exists. Excel behaves the same
//! way: `SUM(A:A)` over three populated cells costs three cells.

use casual_calc_formula::stored::{Origin, StoredRef};
use casual_calc_model::Sheet;

/// The rows and columns a range covers on `sheet`, as `(r0, c0, r1, c1)`.
///
/// Bounds are normalised (start ≤ end) and an axis the reference did not name
/// is clamped to the populated extent. An empty sheet collapses to a single
/// cell rather than to nothing, so callers keep a valid — if empty — range.
#[must_use]
pub fn range_bounds(
    sheet: &Sheet,
    a: &StoredRef,
    b: &StoredRef,
    origin: Origin,
) -> (u32, u32, u32, u32) {
    // A bound that resolves off the sheet leaves nothing to walk. Refusing the
    // whole range is the honest answer: an aggregate over `#REF!` is `#REF!`.
    let (a, b) = match (a.resolve(origin), b.resolve(origin)) {
        (Some(a), Some(b)) => (a, b),
        _ => return (0, 0, 0, 0),
    };
    let (mut r0, mut r1) = (a.row.min(b.row), a.row.max(b.row));
    let (mut c0, mut c1) = (a.col.min(b.col), a.col.max(b.col));

    if a.row_implicit || b.row_implicit {
        // Whole columns: the rows are whatever the sheet uses.
        r0 = 0;
        r1 = sheet.cells.last_row().unwrap_or(0);
    }
    if a.col_implicit || b.col_implicit {
        // Whole rows: only these rows' columns matter, which is far cheaper
        // than asking the sheet for its widest column.
        c0 = 0;
        c1 = sheet.cells.last_col_in_rows(r0, r1).unwrap_or(0);
    }
    (r0, c0, r1, c1)
}

/// Whether either side left an axis unnamed — i.e. the bounds came from the
/// data rather than from the text.
#[must_use]
pub fn is_open(a: &StoredRef, b: &StoredRef) -> bool {
    a.row_implicit || b.row_implicit || a.col_implicit || b.col_implicit
}
