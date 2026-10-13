//! Go To Special (`XL-UX-020`): which cells of a range are constants, formulas
//! or blanks, answered in one call rather than one `session_cell_input` per cell.

use super::*;

/// Go To Special's value-type checkboxes, as `types` bits: numbers.
pub const SPECIAL_NUMBERS: u32 = 1;
/// Text, including a formula's `""`.
pub const SPECIAL_TEXT: u32 = 2;
/// TRUE / FALSE.
pub const SPECIAL_LOGICALS: u32 = 4;
/// `#DIV/0!` and the rest.
pub const SPECIAL_ERRORS: u32 = 8;

/// Which of the value-type bits a value answers to. An empty cached value on a
/// formula is the `""` case, which Excel counts as text.
fn special_type_bit(value: &CellValue) -> u32 {
    match value {
        CellValue::Number(_) => SPECIAL_NUMBERS,
        CellValue::Bool(_) => SPECIAL_LOGICALS,
        CellValue::Error(_) => SPECIAL_ERRORS,
        CellValue::SharedString(_) | CellValue::InlineString(_) | CellValue::Empty => SPECIAL_TEXT,
    }
}

/// Append `col` on `row` to the per-row runs, extending the last run when it
/// is adjacent. Rows arrive in order, so only the last row is ever open.
fn push_cell(runs: &mut Vec<(u32, Vec<(u32, u32)>)>, row: u32, col: u32) {
    if runs.last().is_none_or(|(r, _)| *r != row) {
        runs.push((row, Vec::new()));
    }
    if let Some((_, v)) = runs.last_mut() {
        match v.last_mut() {
            Some(run) if run.1 + 1 == col => run.1 = col,
            _ => v.push((col, col)),
        }
    }
}

/// The cells of `[r0..=r1] × [c0..=c1]` that Go To Special's `kind` selects,
/// as rectangles `(r0, c0, r1, c1)`: runs along each row, merged down while
/// consecutive rows have the same run. `kind` is `"constants"`, `"formulas"`
/// or `"blanks"`; `types` filters the first two by value type (the
/// `SPECIAL_*` bits). Anything else selects nothing.
///
/// A spilled cell counts as a formula, not a constant: its value is the
/// anchor's formula's, and Excel's Constants never picks one. Blanks are
/// bounded by the sheet's used extent, as in Excel, so a whole-column
/// selection asks about the rows that exist rather than a million.
pub(crate) fn special_cells(
    sheet: &Sheet,
    (r0, c0, r1, c1): (u32, u32, u32, u32),
    kind: &str,
    types: u32,
) -> Vec<(u32, u32, u32, u32)> {
    let (mut r1, mut c1) = (r1, c1);
    let blanks = kind == "blanks";
    let want_formulas = kind == "formulas";
    if !blanks && !want_formulas && kind != "constants" {
        return Vec::new();
    }
    if blanks {
        let (mut rows, mut cols) = (0u32, 0u32);
        for (at, _) in sheet.cells.iter() {
            rows = rows.max(at.row + 1);
            cols = cols.max(at.col + 1);
        }
        if rows == 0 {
            return Vec::new();
        }
        r1 = r1.min(rows - 1);
        c1 = c1.min(cols - 1);
    }
    if r0 > r1 || c0 > c1 {
        return Vec::new();
    }
    let is_formula = |cell: &Cell| {
        cell.formula.is_some()
            || cell
                .flags
                .contains(casual_calc_model::CellFlags::SPILL_CHILD)
    };

    let mut row_runs: Vec<(u32, Vec<(u32, u32)>)> = Vec::new();
    if blanks {
        let mut filled = sheet.cells.row_band(r0, r1).peekable();
        for row in r0..=r1 {
            let mut taken = BTreeSet::new();
            while let Some((at, cell)) = filled.next_if(|(at, _)| at.row == row) {
                if is_formula(cell) || !cell.value.is_empty() {
                    taken.insert(at.col);
                }
            }
            for col in c0..=c1 {
                if !taken.contains(&col) {
                    push_cell(&mut row_runs, row, col);
                }
            }
        }
    } else {
        for (at, cell) in sheet.cells.row_band(r0, r1) {
            if at.col < c0 || at.col > c1 {
                continue;
            }
            let formula = is_formula(cell);
            if formula != want_formulas {
                continue;
            }
            // A styled, empty cell is no constant.
            if !formula && cell.value.is_empty() {
                continue;
            }
            if types & special_type_bit(&cell.value) == 0 {
                continue;
            }
            push_cell(&mut row_runs, at.row, at.col);
        }
    }

    // Merge identical runs on consecutive rows into one rectangle.
    let mut done: Vec<(u32, u32, u32, u32)> = Vec::new();
    let mut open: Vec<(u32, u32, u32, u32)> = Vec::new();
    for (row, runs) in row_runs {
        let mut next = Vec::with_capacity(runs.len());
        for (a, b) in runs {
            if let Some(i) = open
                .iter()
                .position(|&(_, oc0, or1, oc1)| oc0 == a && oc1 == b && or1 + 1 == row)
            {
                let mut rect = open.swap_remove(i);
                rect.2 = row;
                next.push(rect);
            } else {
                next.push((row, a, row, b));
            }
        }
        done.append(&mut open);
        open = next;
    }
    done.append(&mut open);
    done.sort_unstable();
    done
}

/// Go To Special over a range: JSON `[{r0,c0,r1,c1}, …]`, the matching cells
/// as rectangles. See `special_cells` for `kind` and `types`.
#[wasm_bindgen]
pub fn session_special_cells(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    kind: &str,
    types: u32,
) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let items: Vec<String> = special_cells(sh, (r0, c0, r1, c1), kind, types)
            .into_iter()
            .map(|(a, b, c, d)| format!("{{\"r0\":{a},\"c0\":{b},\"r1\":{c},\"c1\":{d}}}"))
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{
        SPECIAL_ERRORS, SPECIAL_LOGICALS, SPECIAL_NUMBERS, SPECIAL_TEXT, session_special_cells,
    };
    use crate::{session_new, session_set_cell};

    const ALL: u32 = SPECIAL_NUMBERS | SPECIAL_TEXT | SPECIAL_LOGICALS | SPECIAL_ERRORS;

    /// ```text
    ///        A      B        C
    ///   1    1      =A1*2    x
    ///   2    2      =A2*2
    ///   3    TRUE   =1/0     y
    /// ```
    fn seed() {
        session_new();
        for (r, c, v) in [
            (0, 0, "1"),
            (1, 0, "2"),
            (2, 0, "TRUE"),
            (0, 1, "=A1*2"),
            (1, 1, "=A2*2"),
            (2, 1, "=1/0"),
            (0, 2, "x"),
            (2, 2, "y"),
        ] {
            session_set_cell(0, r, c, v).unwrap();
        }
    }

    #[test]
    fn constants_and_formulas_split_and_filter_by_type() {
        seed();
        // Constants: A1:A3 is one rectangle, C1 and C3 two more.
        assert_eq!(
            session_special_cells(0, 0, 0, 2, 2, "constants", ALL),
            r#"[{"r0":0,"c0":0,"r1":2,"c1":0},{"r0":0,"c0":2,"r1":0,"c1":2},{"r0":2,"c0":2,"r1":2,"c1":2}]"#
        );
        // Numbers only: the logical in A3 drops out.
        assert_eq!(
            session_special_cells(0, 0, 0, 2, 2, "constants", SPECIAL_NUMBERS),
            r#"[{"r0":0,"c0":0,"r1":1,"c1":0}]"#
        );
        assert_eq!(
            session_special_cells(0, 0, 0, 2, 2, "formulas", ALL),
            r#"[{"r0":0,"c0":1,"r1":2,"c1":1}]"#
        );
        // Errors only: the =1/0.
        assert_eq!(
            session_special_cells(0, 0, 0, 2, 2, "formulas", SPECIAL_ERRORS),
            r#"[{"r0":2,"c0":1,"r1":2,"c1":1}]"#
        );
        assert_eq!(
            session_special_cells(0, 0, 0, 2, 2, "formulas", SPECIAL_TEXT),
            "[]"
        );
        // Restricted to the range asked about.
        assert_eq!(
            session_special_cells(0, 1, 0, 1, 2, "constants", ALL),
            r#"[{"r0":1,"c0":0,"r1":1,"c1":0}]"#
        );
    }

    #[test]
    fn blanks_are_the_holes_within_the_used_extent() {
        seed();
        // C2 is the only hole; asking about whole columns stops at row 3.
        assert_eq!(
            session_special_cells(0, 0, 0, 1_048_575, 2, "blanks", 0),
            r#"[{"r0":1,"c0":2,"r1":1,"c1":2}]"#
        );
        session_new();
        assert_eq!(session_special_cells(0, 0, 0, 9, 9, "blanks", 0), "[]");
    }

    /// A cell that holds only a fill is stored, but it is not a constant —
    /// and it is still a blank.
    #[test]
    fn a_styled_empty_cell_is_blank_not_constant() {
        seed();
        crate::session_set_fill(0, 1, 2, 1, 2, "FFFF00", -1, 0.0).unwrap(); // C2
        assert_eq!(session_special_cells(0, 1, 2, 1, 2, "constants", ALL), "[]");
        assert_eq!(
            session_special_cells(0, 1, 2, 1, 2, "blanks", 0),
            r#"[{"r0":1,"c0":2,"r1":1,"c1":2}]"#
        );
    }

    #[test]
    fn an_unknown_kind_selects_nothing() {
        seed();
        assert_eq!(session_special_cells(0, 0, 0, 2, 2, "comments", ALL), "[]");
    }
}
