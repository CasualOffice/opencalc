//! Transaction tests: inverses, atomic batches, undo/redo, and edit→recalc.

use casual_calc_formula::parse;
use casual_calc_formula::stored::Origin;
use casual_calc_model::{
    AutoFilter, Cell, CellComment, CellRange, CellRef, CellValue, Id, Sheet, SheetId, Style,
    StyleId, Table, TableColumn, Workbook,
};

use crate::{History, Operation, apply};

fn workbook() -> Workbook {
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "S"));
    wb
}

fn value_at(wb: &Workbook, at: CellRef) -> CellValue {
    wb.sheets[0]
        .cells
        .get(at)
        .map(|c| c.value.clone())
        .unwrap_or(CellValue::Empty)
}

#[test]
fn set_value_and_inverse_restores() {
    let mut wb = workbook();
    let at = CellRef::new(0, 0);

    let inverse = apply(
        &mut wb,
        Operation::SetValue {
            sheet: 0,
            at,
            value: CellValue::Number(42.0),
        },
    )
    .unwrap();
    assert_eq!(value_at(&wb, at), CellValue::Number(42.0));

    apply(&mut wb, inverse).unwrap();
    assert_eq!(value_at(&wb, at), CellValue::Empty);
    assert!(wb.sheets[0].cells.get(at).is_none());
}

#[test]
fn resize_ops_are_invertible() {
    let mut wb = workbook();

    // Set an explicit width; the inverse should clear it back to the default.
    let inverse = apply(
        &mut wb,
        Operation::SetColumnWidth {
            sheet: 0,
            col: 2,
            width: Some(2400),
        },
    )
    .unwrap();
    assert_eq!(wb.sheets[0].columns.sizes.get(&2), Some(&2400));
    assert_eq!(
        inverse,
        Operation::SetColumnWidth {
            sheet: 0,
            col: 2,
            width: None
        }
    );
    apply(&mut wb, inverse).unwrap();
    assert!(!wb.sheets[0].columns.sizes.contains_key(&2));

    // Overwriting an existing height: the inverse restores the prior value.
    apply(
        &mut wb,
        Operation::SetRowHeight {
            sheet: 0,
            row: 1,
            height: Some(300),
        },
    )
    .unwrap();
    let inverse = apply(
        &mut wb,
        Operation::SetRowHeight {
            sheet: 0,
            row: 1,
            height: Some(900),
        },
    )
    .unwrap();
    assert_eq!(wb.sheets[0].rows.sizes.get(&1), Some(&900));
    apply(&mut wb, inverse).unwrap();
    assert_eq!(wb.sheets[0].rows.sizes.get(&1), Some(&300));
}

#[test]
fn set_style_preserves_value() {
    let mut wb = workbook();
    let at = CellRef::new(0, 0);
    let style = wb.intern_style(Style {
        number_format: Some("0.00".to_owned()),
        ..Style::default()
    });
    apply(
        &mut wb,
        Operation::SetValue {
            sheet: 0,
            at,
            value: CellValue::Number(3.0),
        },
    )
    .unwrap();

    apply(
        &mut wb,
        Operation::SetStyle {
            sheet: 0,
            at,
            style: Some(style),
        },
    )
    .unwrap();

    let cell = wb.sheets[0].cells.get(at).unwrap();
    assert_eq!(cell.value, CellValue::Number(3.0));
    assert_eq!(cell.style, Some(style));
}

#[test]
fn setting_a_value_clears_a_formula() {
    let mut wb = workbook();
    let at = CellRef::new(0, 0);
    // A cell that carries a formula handle.
    let handle = wb.store_formula(casual_calc_formula::parse("1+1").unwrap());
    let mut formula_cell = Cell::value(CellValue::Number(2.0));
    formula_cell.formula = Some(handle);
    apply(
        &mut wb,
        Operation::SetCell {
            sheet: 0,
            at,
            cell: Some(formula_cell),
        },
    )
    .unwrap();
    assert!(wb.sheets[0].cells.get(at).unwrap().formula.is_some());

    apply(
        &mut wb,
        Operation::SetValue {
            sheet: 0,
            at,
            value: CellValue::Number(9.0),
        },
    )
    .unwrap();
    let cell = wb.sheets[0].cells.get(at).unwrap();
    assert_eq!(cell.value, CellValue::Number(9.0));
    assert!(cell.formula.is_none());
}

#[test]
fn batch_is_atomic_and_invertible() {
    let mut wb = workbook();
    let batch = Operation::Batch(vec![
        Operation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(1.0),
        },
        Operation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 1),
            value: CellValue::Number(2.0),
        },
    ]);
    let inverse = apply(&mut wb, batch).unwrap();
    assert_eq!(value_at(&wb, CellRef::new(0, 0)), CellValue::Number(1.0));
    assert_eq!(value_at(&wb, CellRef::new(0, 1)), CellValue::Number(2.0));

    apply(&mut wb, inverse).unwrap();
    assert!(wb.sheets[0].cells.is_empty());
}

#[test]
fn batch_rolls_back_on_failure() {
    let mut wb = workbook();
    let batch = Operation::Batch(vec![
        Operation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(1.0),
        },
        // This targets a non-existent sheet and must fail the whole batch.
        Operation::SetValue {
            sheet: 9,
            at: CellRef::new(0, 0),
            value: CellValue::Number(2.0),
        },
    ]);
    assert!(apply(&mut wb, batch).is_err());
    // The first member was rolled back.
    assert!(wb.sheets[0].cells.is_empty());
}

#[test]
fn history_undo_redo_roundtrips() {
    let mut wb = workbook();
    let mut history = History::new();
    let at = CellRef::new(0, 0);

    history
        .apply(
            &mut wb,
            Operation::SetValue {
                sheet: 0,
                at,
                value: CellValue::Number(7.0),
            },
        )
        .unwrap();
    assert!(history.can_undo());
    assert_eq!(value_at(&wb, at), CellValue::Number(7.0));

    history.undo(&mut wb).unwrap();
    assert_eq!(value_at(&wb, at), CellValue::Empty);
    assert!(history.can_redo());

    history.redo(&mut wb).unwrap();
    assert_eq!(value_at(&wb, at), CellValue::Number(7.0));
}

#[test]
fn edit_then_recalc_updates_dependents() {
    let mut wb = workbook();
    // A1 = 10, A2 = A1*2 (cached 20).
    wb.sheets[0]
        .cells
        .set(CellRef::new(0, 0), Cell::value(CellValue::Number(10.0)));
    let handle = wb.store_formula_at(
        casual_calc_formula::parse("A1*2").unwrap(),
        Origin::at(1, 0),
    );
    let mut a2 = Cell::value(CellValue::Number(20.0));
    a2.formula = Some(handle);
    wb.sheets[0].cells.set(CellRef::new(1, 0), a2);

    // Edit A1 to 30, then recalc.
    apply(
        &mut wb,
        Operation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(30.0),
        },
    )
    .unwrap();
    casual_calc_eval::recalculate(&mut wb);

    assert_eq!(value_at(&wb, CellRef::new(1, 0)), CellValue::Number(60.0));
}

// ---------------------------------------------------------------------------
// Structural operations: insert/delete rows & columns with ref rewriting.
// ---------------------------------------------------------------------------

/// A two-sheet workbook, tabs `S` then `T`.
fn workbook_two() -> Workbook {
    let mut wb = workbook();
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(2, 2)), "T"));
    wb
}

/// Put a literal number cell at `(row, col)` on sheet `sheet`.
fn set_num(wb: &mut Workbook, sheet: usize, row: u32, col: u32, n: f64) {
    wb.sheets[sheet]
        .cells
        .set(CellRef::new(row, col), Cell::value(CellValue::Number(n)));
}

/// Put a formula cell (parsed from `text`) at `(row, col)` on sheet `sheet`.
fn set_formula(wb: &mut Workbook, sheet: usize, row: u32, col: u32, text: &str) {
    // Relative to the cell that holds it (`PERF-11`).
    let handle = wb.store_formula_at(parse(text).unwrap(), Origin::at(row, col));
    let mut cell = Cell::value(CellValue::Empty);
    cell.formula = Some(handle);
    wb.sheets[sheet].cells.set(CellRef::new(row, col), cell);
}

/// The canonical text of the formula at `(row, col)` on sheet `sheet`, if any.
fn formula_text(wb: &Workbook, sheet: usize, row: u32, col: u32) -> Option<String> {
    let handle = wb.sheets[sheet]
        .cells
        .get(CellRef::new(row, col))?
        .formula?;
    // Printed **at the cell**, because a stored tree's references are offsets
    // and `Display` prints the absolute form.
    Some(casual_calc_formula::print_at(
        wb.formula(handle).unwrap(),
        Origin::at(row, col),
    ))
}

/// The observable, arena-independent state of a workbook: per sheet, the name
/// and every populated cell as (address, value, style, formula-text). Two
/// workbooks with this equal are indistinguishable to any reader; only the
/// (append-only, harmless) formula arena may differ, which round-trips must
/// ignore.
#[derive(Debug, PartialEq)]
struct CellSnap {
    value: CellValue,
    style: Option<StyleId>,
    formula: Option<String>,
}

fn observable(wb: &Workbook) -> Vec<(String, Vec<(CellRef, CellSnap)>)> {
    wb.sheets
        .iter()
        .map(|s| {
            let cells = s
                .cells
                .iter()
                .map(|(addr, cell)| {
                    (
                        addr,
                        CellSnap {
                            value: cell.value.clone(),
                            style: cell.style,
                            formula: cell.formula.map(|h| wb.formula(h).unwrap().to_string()),
                        },
                    )
                })
                .collect();
            (s.name.clone(), cells)
        })
        .collect()
}

/// Round trip via observable equivalence (tolerates harmless arena growth).
fn assert_round_trip(mut wb: Workbook, op: Operation) {
    let before = observable(&wb);
    let inverse = apply(&mut wb, op).unwrap();
    apply(&mut wb, inverse).unwrap();
    assert_eq!(observable(&wb), before);
}

/// Round trip via strict `Workbook` equality (valid only when no formula is
/// rewritten, so the arena never grows).
fn assert_round_trip_strict(mut wb: Workbook, op: Operation) {
    let before = wb.clone();
    let inverse = apply(&mut wb, op).unwrap();
    apply(&mut wb, inverse).unwrap();
    assert_eq!(wb, before);
}

#[test]
fn insert_rows_shifts_cells_down() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 0, 1.0); // A1
    set_num(&mut wb, 0, 2, 0, 3.0); // A3

    let inverse = apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 1,
            count: 2,
        },
    )
    .unwrap();

    assert_eq!(value_at(&wb, CellRef::new(0, 0)), CellValue::Number(1.0)); // A1 stays
    assert!(wb.sheets[0].cells.get(CellRef::new(2, 0)).is_none()); // old A3 empty
    assert_eq!(value_at(&wb, CellRef::new(4, 0)), CellValue::Number(3.0)); // A3 -> A5
    assert_eq!(
        inverse,
        Operation::DeleteRows {
            sheet: 0,
            at: 1,
            count: 2
        }
    );
}

#[test]
fn delete_rows_shifts_cells_up_and_drops_band() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 0, 1.0); // A1
    set_num(&mut wb, 0, 1, 0, 2.0); // A2 (deleted)
    set_num(&mut wb, 0, 3, 0, 4.0); // A4

    let inverse = apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 1,
            count: 1,
        },
    )
    .unwrap();

    assert_eq!(value_at(&wb, CellRef::new(0, 0)), CellValue::Number(1.0)); // A1 stays
    assert_eq!(value_at(&wb, CellRef::new(1, 0)), CellValue::Empty); // A2 gone
    assert_eq!(value_at(&wb, CellRef::new(2, 0)), CellValue::Number(4.0)); // A4 -> A3
    // Delete's inverse is a Batch that begins with the re-insert.
    match inverse {
        Operation::Batch(ops) => assert_eq!(
            ops[0],
            Operation::InsertRows {
                sheet: 0,
                at: 1,
                count: 1
            }
        ),
        other => panic!("expected Batch, got {other:?}"),
    }
}

#[test]
fn insert_columns_shifts_cells_right() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 0, 1.0); // A1
    set_num(&mut wb, 0, 0, 2, 3.0); // C1

    apply(
        &mut wb,
        Operation::InsertColumns {
            sheet: 0,
            at: 1,
            count: 2,
        },
    )
    .unwrap();

    assert_eq!(value_at(&wb, CellRef::new(0, 0)), CellValue::Number(1.0)); // A1 stays
    assert_eq!(value_at(&wb, CellRef::new(0, 4)), CellValue::Number(3.0)); // C1 -> E1
}

#[test]
fn delete_columns_shifts_cells_left_and_drops_band() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 0, 1.0); // A1
    set_num(&mut wb, 0, 0, 1, 2.0); // B1 (deleted)
    set_num(&mut wb, 0, 0, 3, 4.0); // D1

    apply(
        &mut wb,
        Operation::DeleteColumns {
            sheet: 0,
            at: 1,
            count: 1,
        },
    )
    .unwrap();

    assert_eq!(value_at(&wb, CellRef::new(0, 0)), CellValue::Number(1.0)); // A1 stays
    assert_eq!(value_at(&wb, CellRef::new(0, 1)), CellValue::Empty); // B1 gone
    assert_eq!(value_at(&wb, CellRef::new(0, 2)), CellValue::Number(4.0)); // D1 -> C1
}

#[test]
fn relative_and_absolute_refs_both_shift() {
    let mut wb = workbook();
    set_formula(&mut wb, 0, 0, 0, "A5+$A$5");

    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 0,
            count: 2,
        },
    )
    .unwrap();

    // The formula cell itself moved A1 -> A3; both refs shifted A5 -> A7.
    assert_eq!(
        formula_text(&wb, 0, 2, 0).as_deref(),
        Some("A7+$A$7"),
        "relative and absolute row refs both shift on insert"
    );
}

#[test]
fn ref_onto_deleted_row_becomes_ref_error() {
    let mut wb = workbook();
    set_formula(&mut wb, 0, 0, 0, "A5+B1");

    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 4,
            count: 1,
        },
    )
    .unwrap();

    // A5 sat on the deleted row -> #REF!; B1 (row 0) is untouched.
    assert_eq!(formula_text(&wb, 0, 0, 0).as_deref(), Some("#REF!+B1"));
}

#[test]
fn range_partial_overlap_shrinks() {
    let mut wb = workbook();
    set_formula(&mut wb, 0, 0, 5, "A1:A5"); // rows 0..=4

    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 3,
            count: 1,
        },
    )
    .unwrap();

    // Row 3 removed from the tail: A1:A5 -> A1:A4.
    assert_eq!(formula_text(&wb, 0, 0, 5).as_deref(), Some("A1:A4"));
}

#[test]
fn range_deleted_low_endpoint_clamps() {
    let mut wb = workbook();
    set_formula(&mut wb, 0, 0, 5, "A3:A6"); // rows 2..=5

    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 2,
            count: 2,
        },
    )
    .unwrap();

    // Rows 2,3 deleted; surviving rows 4,5 collapse to rows 2,3: A3:A4.
    assert_eq!(formula_text(&wb, 0, 0, 5).as_deref(), Some("A3:A4"));
}

#[test]
fn range_full_overlap_becomes_ref_error() {
    let mut wb = workbook();
    set_formula(&mut wb, 0, 0, 5, "A3:A4"); // rows 2..=3

    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 2,
            count: 2,
        },
    )
    .unwrap();

    // The whole range lay inside the deleted band -> #REF!.
    assert_eq!(formula_text(&wb, 0, 0, 5).as_deref(), Some("#REF!"));
}

#[test]
fn cross_sheet_refs_rewrite_and_others_are_left_alone() {
    let mut wb = workbook_two();
    // On S (sheet 0): an unqualified ref targeting S's own grid.
    set_formula(&mut wb, 0, 0, 0, "A5");
    // On T (sheet 1): a qualified ref into S, plus an unqualified local ref.
    set_formula(&mut wb, 1, 0, 0, "S!A5");
    set_formula(&mut wb, 1, 0, 1, "A5");

    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 0,
            count: 1,
        },
    )
    .unwrap();

    // S's own formula moved A1 -> A2 and its ref shifted A5 -> A6.
    assert_eq!(formula_text(&wb, 0, 1, 0).as_deref(), Some("A6"));
    // T's cross-sheet ref into S shifted; T is not moved geometrically.
    assert_eq!(formula_text(&wb, 1, 0, 0).as_deref(), Some("S!A6"));
    // T's local unqualified ref targets T, not S -> untouched.
    assert_eq!(formula_text(&wb, 1, 0, 1).as_deref(), Some("A5"));
}

#[test]
fn insert_rows_round_trips() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 0, 1.0);
    set_num(&mut wb, 0, 3, 1, 4.0);
    set_num(&mut wb, 0, 7, 2, 8.0);
    assert_round_trip_strict(
        wb.clone(),
        Operation::InsertRows {
            sheet: 0,
            at: 2,
            count: 3,
        },
    );
    assert_round_trip(
        wb,
        Operation::InsertRows {
            sheet: 0,
            at: 2,
            count: 3,
        },
    );
}

#[test]
fn delete_rows_round_trips() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 0, 1.0);
    set_num(&mut wb, 0, 3, 1, 4.0); // inside the deleted band
    set_num(&mut wb, 0, 7, 2, 8.0);
    let op = Operation::DeleteRows {
        sheet: 0,
        at: 2,
        count: 3,
    };
    assert_round_trip_strict(wb.clone(), op.clone());
    assert_round_trip(wb, op);
}

#[test]
fn insert_columns_round_trips() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 0, 1.0);
    set_num(&mut wb, 0, 1, 3, 4.0);
    set_num(&mut wb, 0, 2, 7, 8.0);
    let op = Operation::InsertColumns {
        sheet: 0,
        at: 2,
        count: 3,
    };
    assert_round_trip_strict(wb.clone(), op.clone());
    assert_round_trip(wb, op);
}

#[test]
fn delete_columns_round_trips() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 0, 1.0);
    set_num(&mut wb, 0, 1, 3, 4.0); // inside the deleted band
    set_num(&mut wb, 0, 2, 7, 8.0);
    let op = Operation::DeleteColumns {
        sheet: 0,
        at: 2,
        count: 3,
    };
    assert_round_trip_strict(wb.clone(), op.clone());
    assert_round_trip(wb, op);
}

#[test]
fn insert_rows_round_trips_with_formulas() {
    let mut wb = workbook_two();
    set_num(&mut wb, 0, 0, 0, 10.0);
    set_formula(&mut wb, 0, 6, 0, "A1+A5"); // A1 stays, A5 shifts
    set_formula(&mut wb, 0, 6, 1, "A3:A8"); // range straddling the insert
    set_formula(&mut wb, 1, 0, 0, "S!A5"); // cross-sheet ref into S
    assert_round_trip(
        wb,
        Operation::InsertRows {
            sheet: 0,
            at: 2,
            count: 2,
        },
    );
}

#[test]
fn delete_rows_round_trips_with_formulas_even_through_ref_errors() {
    let mut wb = workbook_two();
    set_num(&mut wb, 0, 0, 0, 10.0); // A1 (kept, no formula)
    set_num(&mut wb, 0, 4, 0, 50.0); // A5 (deleted)
    set_formula(&mut wb, 0, 0, 1, "A5"); // B1 -> #REF! after delete
    set_formula(&mut wb, 0, 6, 0, "A1:A9"); // range shrinks
    set_formula(&mut wb, 1, 0, 0, "S!A5"); // cross-sheet -> #REF! after delete
    // The delete makes references collapse to #REF!, yet the snapshot-based
    // inverse restores every original formula exactly.
    assert_round_trip(
        wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 4,
            count: 1,
        },
    );
}

#[test]
fn insert_columns_round_trips_with_formulas() {
    let mut wb = workbook_two();
    set_num(&mut wb, 0, 0, 0, 10.0);
    set_formula(&mut wb, 0, 0, 6, "A1+E1"); // A1 stays, E1 shifts
    set_formula(&mut wb, 1, 0, 0, "S!E1"); // cross-sheet ref into S
    assert_round_trip(
        wb,
        Operation::InsertColumns {
            sheet: 0,
            at: 2,
            count: 2,
        },
    );
}

#[test]
fn delete_columns_round_trips_with_formulas() {
    let mut wb = workbook_two();
    set_num(&mut wb, 0, 0, 0, 10.0);
    set_num(&mut wb, 0, 0, 4, 50.0); // E1 (deleted)
    set_formula(&mut wb, 0, 1, 0, "E1"); // -> #REF! after delete
    set_formula(&mut wb, 0, 2, 0, "A1:I1"); // range shrinks
    set_formula(&mut wb, 1, 0, 0, "S!E1"); // cross-sheet -> #REF!
    assert_round_trip(
        wb,
        Operation::DeleteColumns {
            sheet: 0,
            at: 4,
            count: 1,
        },
    );
}

// ---------------------------------------------------------------------------
// Structural operations: position-indexed metadata (merges, sizing, hidden,
// frozen panes) must shift in lock-step with the cells.
// ---------------------------------------------------------------------------

/// A merge spanning `[(r0,c0)..=(r1,c1)]`.
fn merge(r0: u32, c0: u32, r1: u32, c1: u32) -> CellRange {
    CellRange::new(CellRef::new(r0, c0), CellRef::new(r1, c1))
}

#[test]
fn insert_rows_shifts_merges_below_and_grows_a_straddling_one() {
    let mut wb = workbook();
    wb.sheets[0].merges.push(merge(4, 0, 5, 1)); // wholly below the insert
    wb.sheets[0].merges.push(merge(0, 0, 3, 1)); // straddles the insert point

    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 2,
            count: 2,
        },
    )
    .unwrap();

    // Below-merge moved down by 2; straddling merge kept its top, grew its bottom.
    assert_eq!(wb.sheets[0].merges[0], merge(6, 0, 7, 1));
    assert_eq!(wb.sheets[0].merges[1], merge(0, 0, 5, 1));
}

#[test]
fn delete_rows_removes_inner_merge_and_clamps_straddling() {
    let mut wb = workbook();
    wb.sheets[0].merges.push(merge(2, 0, 3, 1)); // wholly inside the deleted band
    wb.sheets[0].merges.push(merge(1, 0, 4, 1)); // straddles the band
    wb.sheets[0].merges.push(merge(6, 0, 7, 1)); // wholly below the band

    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 2,
            count: 2,
        },
    )
    .unwrap();

    // The inner merge is gone; the straddling one loses its two deleted rows
    // (1..=4 -> 1..=2); the below one shifts up by 2 (6..=7 -> 4..=5).
    assert_eq!(wb.sheets[0].merges.len(), 2);
    assert_eq!(wb.sheets[0].merges[0], merge(1, 0, 2, 1));
    assert_eq!(wb.sheets[0].merges[1], merge(4, 0, 5, 1));
}

/// A table is a range, and a range that does not move when rows move stops
/// describing the data it names.
///
/// `structural.rs` shifted merges, the autofilter, filter-hidden rows, sizing,
/// hidden lines, the freeze boundary and the outline — and never `sheet.tables`.
/// Insert one row above a table and its range, banding, filter buttons and
/// column list stayed on the old row numbers, so `SUM(Table1[Amount])` read the
/// header text row and dropped the last record: a wrong number in a saved file,
/// with no error and no entry in the compatibility report.
fn table(name: &str, r0: u32, c0: u32, r1: u32, c1: u32, cols: &[&str]) -> Table {
    Table {
        id: 1,
        name: name.to_owned(),
        display_name: name.to_owned(),
        range: merge(r0, c0, r1, c1),
        header_row_count: 1,
        totals_row_count: 0,
        columns: cols
            .iter()
            .enumerate()
            .map(|(i, n)| TableColumn {
                id: i as u32 + 1,
                name: (*n).to_owned(),
                totals_row_function: None,
                totals_row_label: None,
                calculated_column_formula: None,
                totals_row_formula: None,
            })
            .collect(),
        auto_filter: Some(AutoFilter {
            range: merge(r0, c0, r1, c1),
            rules: Default::default(),
        }),
        style: Default::default(),
        attrs: Default::default(),
    }
}

#[test]
fn inserting_rows_moves_a_table_rather_than_leaving_it_on_the_old_rows() {
    let mut wb = workbook();
    // Header on row 3, records on 4..=6.
    wb.sheets[0]
        .tables
        .push(table("Sales", 3, 0, 6, 1, &["Item", "Amount"]));

    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 1,
            count: 2,
        },
    )
    .unwrap();

    let t = &wb.sheets[0].tables[0];
    assert_eq!(
        t.range,
        merge(5, 0, 8, 1),
        "the whole table moved down by 2"
    );
    assert_eq!(
        t.auto_filter.as_ref().unwrap().range,
        merge(5, 0, 8, 1),
        "and so did the filter buttons, which are what the header row draws"
    );
    assert_eq!(t.columns.len(), 2, "a row insert changes no columns");
}

#[test]
fn inserting_rows_inside_a_table_grows_it() {
    let mut wb = workbook();
    wb.sheets[0]
        .tables
        .push(table("Sales", 0, 0, 4, 1, &["Item", "Amount"]));

    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 2,
            count: 3,
        },
    )
    .unwrap();

    // The header stays put and the last record moves down: the new rows are
    // records, which is what makes them part of the table.
    assert_eq!(wb.sheets[0].tables[0].range, merge(0, 0, 7, 1));
}

#[test]
fn inserting_a_column_inside_a_table_gives_it_a_column_to_match() {
    let mut wb = workbook();
    wb.sheets[0]
        .tables
        .push(table("Sales", 0, 0, 4, 2, &["Item", "Qty", "Amount"]));

    apply(
        &mut wb,
        Operation::InsertColumns {
            sheet: 0,
            at: 1,
            count: 1,
        },
    )
    .unwrap();

    let t = &wb.sheets[0].tables[0];
    assert_eq!(t.range, merge(0, 0, 4, 3), "one column wider");
    // Width and column count must agree, or every structured reference past the
    // insert resolves to the wrong column.
    assert_eq!(t.columns.len(), 4);
    let names: Vec<&str> = t.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Item", "Column4", "Qty", "Amount"]);
    let ids: std::collections::BTreeSet<u32> = t.columns.iter().map(|c| c.id).collect();
    assert_eq!(
        ids.len(),
        4,
        "ids stay unique: a filter refers to a column by id"
    );
}

#[test]
fn deleting_columns_takes_the_table_columns_with_them() {
    let mut wb = workbook();
    wb.sheets[0].tables.push(table(
        "Sales",
        0,
        1,
        4,
        4,
        &["Item", "Qty", "Price", "Amount"],
    ));

    // Deletes sheet columns 2..=3, which are the table's "Qty" and "Price".
    apply(
        &mut wb,
        Operation::DeleteColumns {
            sheet: 0,
            at: 2,
            count: 2,
        },
    )
    .unwrap();

    let t = &wb.sheets[0].tables[0];
    assert_eq!(t.range, merge(0, 1, 4, 2));
    let names: Vec<&str> = t.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        ["Item", "Amount"],
        "the survivors are the ones outside the band"
    );
}

#[test]
fn deleting_every_row_of_a_table_removes_the_table() {
    let mut wb = workbook();
    wb.sheets[0]
        .tables
        .push(table("Sales", 2, 0, 5, 1, &["Item", "Amount"]));

    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 2,
            count: 4,
        },
    )
    .unwrap();

    assert!(
        wb.sheets[0].tables.is_empty(),
        "a table whose every row is gone is gone: {:?}",
        wb.sheets[0].tables
    );
}

#[test]
fn insert_and_delete_shift_a_hidden_row() {
    let mut wb = workbook();
    wb.sheets[0].hidden_rows.insert(5);

    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 2,
            count: 3,
        },
    )
    .unwrap();
    assert!(wb.sheets[0].hidden_rows.contains(&8));

    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 2,
            count: 3,
        },
    )
    .unwrap();
    assert!(wb.sheets[0].hidden_rows.contains(&5));
}

#[test]
fn delete_rows_drops_a_hidden_row_inside_the_band() {
    let mut wb = workbook();
    wb.sheets[0].hidden_rows.insert(3);

    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 2,
            count: 2,
        },
    )
    .unwrap();
    // Row 3 was deleted; nothing survives to hide.
    assert!(wb.sheets[0].hidden_rows.is_empty());
}

#[test]
fn insert_columns_shifts_a_custom_width() {
    let mut wb = workbook();
    wb.sheets[0].columns.sizes.insert(2, 4200); // C is 4200 twips wide

    apply(
        &mut wb,
        Operation::InsertColumns {
            sheet: 0,
            at: 1,
            count: 1,
        },
    )
    .unwrap();
    // The custom width followed its column: C(2) -> D(3).
    assert_eq!(wb.sheets[0].columns.sizes.get(&3), Some(&4200));
    assert!(!wb.sheets[0].columns.sizes.contains_key(&2));
}

#[test]
fn delete_columns_drops_a_custom_width_in_the_band() {
    let mut wb = workbook();
    wb.sheets[0].columns.sizes.insert(2, 4200);

    apply(
        &mut wb,
        Operation::DeleteColumns {
            sheet: 0,
            at: 2,
            count: 1,
        },
    )
    .unwrap();
    assert!(wb.sheets[0].columns.sizes.is_empty());
}

#[test]
fn insert_before_freeze_boundary_grows_the_freeze() {
    let mut wb = workbook();
    wb.sheets[0].view.frozen_rows = 3;

    // Insert inside the frozen band: the freeze extends to keep the same lines
    // pinned plus the new blanks.
    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 1,
            count: 2,
        },
    )
    .unwrap();
    assert_eq!(wb.sheets[0].view.frozen_rows, 5);
}

#[test]
fn insert_at_or_after_freeze_boundary_leaves_it_alone() {
    let mut wb = workbook();
    wb.sheets[0].view.frozen_rows = 3;

    // Insert exactly at the boundary (index 3): the new rows fall below the
    // freeze, so the count is unchanged.
    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 3,
            count: 2,
        },
    )
    .unwrap();
    assert_eq!(wb.sheets[0].view.frozen_rows, 3);
}

#[test]
fn delete_across_freeze_boundary_shrinks_the_freeze_by_the_overlap() {
    let mut wb = workbook();
    wb.sheets[0].view.frozen_rows = 3;

    // Delete rows 2,3,4: only rows 2 (of 0,1,2 frozen) lie in the freeze, so it
    // drops by exactly one to 2.
    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 2,
            count: 3,
        },
    )
    .unwrap();
    assert_eq!(wb.sheets[0].view.frozen_rows, 2);
}

#[test]
fn insert_rows_metadata_round_trips_strictly() {
    let mut wb = workbook();
    wb.sheets[0].merges.push(merge(4, 0, 6, 2));
    wb.sheets[0].merges.push(merge(0, 0, 3, 1));
    wb.sheets[0].rows.sizes.insert(5, 900);
    wb.sheets[0].hidden_rows.insert(6);
    wb.sheets[0].view.frozen_rows = 2;
    set_num(&mut wb, 0, 7, 0, 1.0);

    // Insert is exactly invertible by its matching delete, metadata and all.
    assert_round_trip_strict(
        wb,
        Operation::InsertRows {
            sheet: 0,
            at: 1,
            count: 3,
        },
    );
}

#[test]
fn delete_rows_metadata_round_trips_via_snapshot() {
    let mut wb = workbook();
    wb.sheets[0].merges.push(merge(2, 0, 3, 1)); // dropped by the delete
    wb.sheets[0].merges.push(merge(1, 0, 4, 1)); // clamped by the delete
    wb.sheets[0].merges.push(merge(6, 0, 7, 2)); // shifted up
    wb.sheets[0].rows.sizes.insert(2, 300); // dropped
    wb.sheets[0].rows.sizes.insert(6, 900); // shifted
    wb.sheets[0].hidden_rows.insert(3); // dropped
    wb.sheets[0].hidden_rows.insert(7); // shifted
    wb.sheets[0].view.frozen_rows = 3; // partially overlaps the band
    set_num(&mut wb, 0, 8, 0, 1.0);

    // The delete drops and clamps metadata, but its inverse carries the
    // pre-delete snapshot, so undo restores every merge, size, and freeze exactly.
    assert_round_trip_strict(
        wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 2,
            count: 2,
        },
    );
}

#[test]
fn set_sheet_metadata_is_self_inverse() {
    let mut wb = workbook();
    wb.sheets[0].merges.push(merge(0, 0, 1, 1));
    wb.sheets[0].view.frozen_cols = 2;
    assert_round_trip_strict(
        wb,
        Operation::set_sheet_metadata(
            0,
            crate::SheetMetadata {
                merges: vec![merge(3, 3, 4, 4)],
                ..Default::default()
            },
        ),
    );
}

#[test]
fn structural_op_on_missing_sheet_errors() {
    let mut wb = workbook();
    assert!(
        apply(
            &mut wb,
            Operation::InsertRows {
                sheet: 9,
                at: 0,
                count: 1
            }
        )
        .is_err()
    );
    assert!(
        apply(
            &mut wb,
            Operation::DeleteColumns {
                sheet: 9,
                at: 0,
                count: 1
            }
        )
        .is_err()
    );
}

// ---- Sheet-collection ops (M1-1): insert / remove / rename / move / tab color

fn named_sheet(ns: u64, name: &str) -> Sheet {
    Sheet::new(SheetId(Id::from_parts(ns, 1)), name)
}

#[test]
fn insert_and_remove_sheet_are_inverse() {
    let mut wb = workbook(); // one sheet: "S"
    let inverse = apply(
        &mut wb,
        Operation::InsertSheet {
            index: 1,
            sheet: Box::new(named_sheet(3, "Two")),
        },
    )
    .unwrap();
    assert_eq!(wb.sheets.len(), 2);
    assert_eq!(wb.sheets[1].name, "Two");
    assert_eq!(inverse, Operation::RemoveSheet { index: 1 });

    apply(&mut wb, inverse).unwrap();
    assert_eq!(wb.sheets.len(), 1);
    assert_eq!(wb.sheets[0].name, "S");
}

#[test]
fn insert_index_clamps_to_end() {
    let mut wb = workbook();
    let inverse = apply(
        &mut wb,
        Operation::InsertSheet {
            index: 99,
            sheet: Box::new(named_sheet(3, "End")),
        },
    )
    .unwrap();
    assert_eq!(wb.sheets[1].name, "End");
    // The inverse targets the clamped position, not the requested 99.
    assert_eq!(inverse, Operation::RemoveSheet { index: 1 });
}

#[test]
fn remove_sheet_restores_full_contents_on_undo() {
    let mut wb = workbook();
    // Give the sheet a cell so we prove the whole sheet round-trips, not a stub.
    let at = CellRef::new(2, 1);
    wb.sheets[0]
        .cells
        .set(at, Cell::value(CellValue::Number(7.0)));

    let inverse = apply(&mut wb, Operation::RemoveSheet { index: 0 }).unwrap();
    assert!(wb.sheets.is_empty());

    apply(&mut wb, inverse).unwrap();
    assert_eq!(wb.sheets.len(), 1);
    assert_eq!(
        wb.sheets[0].cells.get(at).map(|c| c.value.clone()),
        Some(CellValue::Number(7.0))
    );
}

#[test]
fn remove_missing_sheet_errors() {
    let mut wb = workbook();
    assert!(apply(&mut wb, Operation::RemoveSheet { index: 5 }).is_err());
}

#[test]
fn rename_sheet_inverse_restores_name() {
    let mut wb = workbook();
    let inverse = apply(
        &mut wb,
        Operation::RenameSheet {
            index: 0,
            name: "Renamed".to_owned(),
        },
    )
    .unwrap();
    assert_eq!(wb.sheets[0].name, "Renamed");
    assert_eq!(
        inverse,
        Operation::RenameSheet {
            index: 0,
            name: "S".to_owned()
        }
    );
    apply(&mut wb, inverse).unwrap();
    assert_eq!(wb.sheets[0].name, "S");
}

#[test]
fn move_sheet_is_invertible() {
    let mut wb = workbook(); // "S"
    apply(
        &mut wb,
        Operation::InsertSheet {
            index: 1,
            sheet: Box::new(named_sheet(3, "B")),
        },
    )
    .unwrap();
    apply(
        &mut wb,
        Operation::InsertSheet {
            index: 2,
            sheet: Box::new(named_sheet(4, "C")),
        },
    )
    .unwrap();
    // Order: S, B, C. Move index 0 to the end.
    let inverse = apply(&mut wb, Operation::MoveSheet { from: 0, to: 2 }).unwrap();
    assert_eq!(
        wb.sheets
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["B", "C", "S"]
    );
    assert_eq!(inverse, Operation::MoveSheet { from: 2, to: 0 });
    apply(&mut wb, inverse).unwrap();
    assert_eq!(
        wb.sheets
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["S", "B", "C"]
    );
}

#[test]
fn move_sheet_out_of_range_errors() {
    let mut wb = workbook();
    assert!(apply(&mut wb, Operation::MoveSheet { from: 0, to: 9 }).is_err());
    assert!(apply(&mut wb, Operation::MoveSheet { from: 9, to: 0 }).is_err());
}

#[test]
fn set_tab_color_inverse_restores_prior() {
    let mut wb = workbook();
    // First set: prior was None.
    let inverse = apply(
        &mut wb,
        Operation::SetTabColor {
            sheet: 0,
            color: Some("FF0000".to_owned()),
        },
    )
    .unwrap();
    assert_eq!(wb.sheets[0].tab_color.as_deref(), Some("FF0000"));
    assert_eq!(
        inverse,
        Operation::SetTabColor {
            sheet: 0,
            color: None
        }
    );
    // Overwrite, then undo restores the red.
    apply(
        &mut wb,
        Operation::SetTabColor {
            sheet: 0,
            color: Some("00FF00".to_owned()),
        },
    )
    .unwrap();
    apply(&mut wb, inverse).unwrap();
    assert_eq!(wb.sheets[0].tab_color, None);
}

#[test]
fn sheet_ops_undo_redo_through_history() {
    let mut wb = workbook();
    let mut history = History::new();

    history
        .apply(
            &mut wb,
            Operation::InsertSheet {
                index: 1,
                sheet: Box::new(named_sheet(3, "Extra")),
            },
        )
        .unwrap();
    history
        .apply(
            &mut wb,
            Operation::RenameSheet {
                index: 1,
                name: "Data".to_owned(),
            },
        )
        .unwrap();
    assert_eq!(wb.sheets[1].name, "Data");

    history.undo(&mut wb).unwrap(); // undo rename
    assert_eq!(wb.sheets[1].name, "Extra");
    history.undo(&mut wb).unwrap(); // undo insert
    assert_eq!(wb.sheets.len(), 1);

    history.redo(&mut wb).unwrap(); // redo insert
    assert_eq!(wb.sheets[1].name, "Extra");
    history.redo(&mut wb).unwrap(); // redo rename
    assert_eq!(wb.sheets[1].name, "Data");
}

#[test]
fn rename_sheet_rewrites_cross_sheet_refs_and_undoes() {
    // Two sheets; a formula on "Report" reads from "Data".
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "Data"));
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(2, 2)), "Report"));
    set_formula(&mut wb, 1, 0, 0, "Data!A1+1");
    assert_eq!(formula_text(&wb, 1, 0, 0).as_deref(), Some("Data!A1+1"));

    // Rename Data -> Facts: the cross-sheet reference must follow.
    let inverse = apply(
        &mut wb,
        Operation::RenameSheet {
            index: 0,
            name: "Facts".to_owned(),
        },
    )
    .unwrap();
    assert_eq!(wb.sheets[0].name, "Facts");
    assert_eq!(formula_text(&wb, 1, 0, 0).as_deref(), Some("Facts!A1+1"));

    // Undo: both the name and the reference revert.
    apply(&mut wb, inverse).unwrap();
    assert_eq!(wb.sheets[0].name, "Data");
    assert_eq!(formula_text(&wb, 1, 0, 0).as_deref(), Some("Data!A1+1"));
}

// ---- Defined names (M5-1): undoable define / rename / delete

#[test]
fn set_defined_names_is_self_inverse() {
    let mut wb = workbook();
    let named = |n: &str, f: &str| casual_calc_model::DefinedName {
        name: n.to_owned(),
        sheet: None,
        formula: parse(f).unwrap(),
    };

    // Define "Total" over A1:A3.
    let inverse = apply(
        &mut wb,
        Operation::SetDefinedNames(vec![named("Total", "A1:A3")]),
    )
    .unwrap();
    assert_eq!(wb.defined_names.len(), 1);
    assert_eq!(wb.defined_names[0].name, "Total");
    assert_eq!(inverse, Operation::SetDefinedNames(vec![]));

    // Undo: the name is gone again.
    apply(&mut wb, inverse).unwrap();
    assert!(wb.defined_names.is_empty());
}

#[test]
fn defined_names_undo_redo_through_history() {
    let mut wb = workbook();
    let mut history = History::new();
    let named = |n: &str, f: &str| casual_calc_model::DefinedName {
        name: n.to_owned(),
        sheet: None,
        formula: parse(f).unwrap(),
    };

    history
        .apply(
            &mut wb,
            Operation::SetDefinedNames(vec![named("Total", "A1:A3")]),
        )
        .unwrap();
    assert_eq!(wb.defined_names[0].name, "Total");

    // "Rename" by replacing the whole list (as the WASM layer does).
    history
        .apply(
            &mut wb,
            Operation::SetDefinedNames(vec![named("Grand", "A1:A3")]),
        )
        .unwrap();
    assert_eq!(wb.defined_names[0].name, "Grand");

    history.undo(&mut wb).unwrap(); // back to "Total"
    assert_eq!(wb.defined_names[0].name, "Total");
    history.undo(&mut wb).unwrap(); // back to no names at all
    assert!(wb.defined_names.is_empty());

    history.redo(&mut wb).unwrap(); // "Total" again
    assert_eq!(wb.defined_names[0].name, "Total");
    history.redo(&mut wb).unwrap(); // "Grand" again
    assert_eq!(wb.defined_names[0].name, "Grand");
}

// --- Autofilter shifting --------------------------------------------------

fn sheet_with_filter(r0: u32, r1: u32) -> Workbook {
    use casual_calc_model::AutoFilter;
    let mut wb = workbook();
    wb.sheets[0].auto_filter = Some(AutoFilter::new(CellRange::new(
        CellRef::new(r0, 0),
        CellRef::new(r1, 2),
    )));
    wb
}

#[test]
fn inserting_rows_moves_the_filter_range_and_its_hidden_rows() {
    let mut wb = sheet_with_filter(0, 9);
    wb.sheets[0].filter_hidden.insert(5);
    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 2,
            count: 3,
        },
    )
    .unwrap();
    let f = wb.sheets[0].auto_filter.as_ref().unwrap();
    // The header stays at row 0; the insert falls inside, so the range grows.
    assert_eq!(f.range.start.row, 0);
    assert_eq!(f.range.end.row, 12);
    // The hidden row rode along with the data it belonged to.
    assert!(wb.sheets[0].filter_hidden.contains(&8));
    assert!(!wb.sheets[0].filter_hidden.contains(&5));
}

#[test]
fn deleting_rows_drops_filtered_rows_in_the_band_and_shifts_the_rest() {
    let mut wb = sheet_with_filter(0, 9);
    wb.sheets[0].filter_hidden.extend([3, 7]);
    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 2,
            count: 3,
        },
    )
    .unwrap();
    // Row 3 was inside [2,5) and is gone; row 7 shifts down by 3.
    assert_eq!(
        wb.sheets[0]
            .filter_hidden
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![4]
    );
    assert_eq!(wb.sheets[0].auto_filter.as_ref().unwrap().range.end.row, 6);
}

#[test]
fn deleting_the_whole_filter_range_removes_the_filter() {
    let mut wb = sheet_with_filter(2, 5);
    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 0,
            count: 8,
        },
    )
    .unwrap();
    assert!(wb.sheets[0].auto_filter.is_none());
}

#[test]
fn set_sheet_metadata_restores_the_filter_on_undo() {
    use casual_calc_model::{AutoFilter, FilterRule};

    let mut wb = sheet_with_filter(0, 9);
    let mut with_rule = AutoFilter::new(CellRange::new(CellRef::new(0, 0), CellRef::new(9, 2)));
    with_rule
        .rules
        .insert(1, FilterRule::Values(vec!["Apple".into()]));

    let inverse = apply(
        &mut wb,
        Operation::set_sheet_metadata(
            0,
            crate::SheetMetadata {
                auto_filter: Some(with_rule),
                filter_hidden: [4u32].into_iter().collect(),
                ..Default::default()
            },
        ),
    )
    .unwrap();
    assert!(wb.sheets[0].is_row_hidden(4));

    // Undo takes the rules *and* the rows they hid back together.
    apply(&mut wb, inverse).unwrap();
    assert!(wb.sheets[0].filter_hidden.is_empty());
    assert!(!wb.sheets[0].is_row_hidden(4));
    assert!(
        wb.sheets[0]
            .auto_filter
            .as_ref()
            .is_some_and(|f| f.rules.is_empty())
    );
}

#[test]
fn the_operation_enum_stays_small_enough_for_the_undo_stack() {
    // `SetSheetMetadata` carries a dozen collections. Unboxed it padded every
    // other variant — including `SetCell`, which a bulk edit pushes thousands of
    // — up to its size. Boxing keeps the enum near its second-largest variant.
    assert!(
        std::mem::size_of::<Operation>() <= 128,
        "Operation grew to {} bytes; box the payload of whatever was added",
        std::mem::size_of::<Operation>()
    );
}

#[test]
fn sheet_metadata_install_returns_the_exact_prior_bundle() {
    let mut wb = workbook();
    wb.sheets[0].row_outline_levels.insert(3, 2);
    wb.sheets[0].collapsed_rows.insert(5);
    wb.sheets[0].hidden_rows.insert(3);

    let replacement = crate::SheetMetadata::default();
    let inverse = apply(&mut wb, Operation::set_sheet_metadata(0, replacement)).unwrap();
    assert!(wb.sheets[0].row_outline_levels.is_empty());
    assert!(wb.sheets[0].collapsed_rows.is_empty());

    // Undo restores the outline, the collapse flag and the rows it hid together.
    apply(&mut wb, inverse).unwrap();
    assert_eq!(wb.sheets[0].row_level(3), 2);
    assert!(wb.sheets[0].collapsed_rows.contains(&5));
    assert!(wb.sheets[0].is_row_hidden(3));
}

// ---------------------------------------------------------------------------
// The change mask (COL-02). A metadata op declares the whole bundle; `apply`
// narrows it to what actually differs, which is what stops undo over-reaching
// and what lets two concurrent edits to different fields merge instead of one
// silently discarding the other.
// ---------------------------------------------------------------------------

#[test]
fn apply_narrows_a_whole_bundle_to_the_fields_that_actually_changed() {
    let mut wb = workbook();
    let mut data = crate::SheetMetadata::capture(&wb.sheets[0]);
    data.view.hide_gridlines = !data.view.hide_gridlines;

    let inverse = apply(&mut wb, Operation::set_sheet_metadata(0, data)).unwrap();

    // Declared ALL, but only the view differs — so that is all the op is.
    assert_eq!(inverse.sheet_fields(), crate::SheetFields::VIEW);
}

#[test]
fn an_op_that_changes_nothing_narrows_to_nothing() {
    let mut wb = workbook();
    let unchanged = crate::SheetMetadata::capture(&wb.sheets[0]);

    let inverse = apply(&mut wb, Operation::set_sheet_metadata(0, unchanged)).unwrap();

    assert!(inverse.sheet_fields().is_empty());
}

#[test]
fn the_inverse_restores_only_the_field_the_op_touched() {
    let mut wb = workbook();
    wb.sheets[0].comments.clear();

    // One edit changes the view.
    let mut data = crate::SheetMetadata::capture(&wb.sheets[0]);
    data.view.hide_gridlines = true;
    let inverse = apply(&mut wb, Operation::set_sheet_metadata(0, data)).unwrap();

    // Something else then changes a *different* field — as a second user would.
    wb.sheets[0].hidden_rows.insert(7);

    // Undoing the first edit must not reach across and undo the second. With a
    // whole-bundle inverse it would: the snapshot predates the hidden row.
    apply(&mut wb, inverse).unwrap();
    assert!(!wb.sheets[0].view.hide_gridlines, "the view is restored");
    assert!(
        wb.sheets[0].hidden_rows.contains(&7),
        "the unrelated field survives the undo"
    );
}

#[test]
fn concurrent_edits_to_different_fields_are_distinguishable() {
    // The property the collaboration transform is built on: two ops generated
    // from the same base that touch disjoint fields report disjoint masks, so
    // they can be merged rather than ordered.
    let mut wb = workbook();
    let base = crate::SheetMetadata::capture(&wb.sheets[0]);

    let mut resize = base.clone();
    resize.columns.sizes.insert(2, 140);

    let mut comment = base.clone();
    comment
        .comments
        .push(CellComment::note(CellRef::new(0, 0), "second user", None));

    let a = apply(&mut wb.clone(), Operation::set_sheet_metadata(0, resize))
        .unwrap()
        .sheet_fields();
    let b = apply(&mut wb, Operation::set_sheet_metadata(0, comment))
        .unwrap()
        .sheet_fields();

    assert!(
        !a.intersects(b),
        "a column resize and a comment do not collide"
    );
    assert_eq!(a, crate::SheetFields::COLUMNS);
    assert_eq!(b, crate::SheetFields::COMMENTS);
}

#[test]
fn concurrent_edits_to_the_same_field_do_collide() {
    // The other half: same field, so the transform must order them rather than
    // merge. A mask that reported these as disjoint would lose one silently.
    let mut wb = workbook();
    let base = crate::SheetMetadata::capture(&wb.sheets[0]);

    let mut one = base.clone();
    one.columns.sizes.insert(2, 140);
    let mut two = base.clone();
    two.columns.sizes.insert(5, 200);

    let a = apply(&mut wb.clone(), Operation::set_sheet_metadata(0, one))
        .unwrap()
        .sheet_fields();
    let b = apply(&mut wb, Operation::set_sheet_metadata(0, two))
        .unwrap()
        .sheet_fields();

    assert!(a.intersects(b), "two column resizes are the same field");
}

#[test]
fn a_batch_reports_the_union_of_its_metadata_fields() {
    let mut wb = workbook();
    let base = crate::SheetMetadata::capture(&wb.sheets[0]);

    let mut view = base.clone();
    view.view.hide_gridlines = true;
    let mut cols = base.clone();
    cols.columns.sizes.insert(1, 99);

    let batch = Operation::Batch(vec![
        Operation::set_sheet_metadata(0, view),
        Operation::set_sheet_metadata(0, cols),
    ]);
    let inverse = apply(&mut wb, batch).unwrap();

    assert_eq!(
        inverse.sheet_fields(),
        crate::SheetFields::VIEW.union(crate::SheetFields::COLUMNS)
    );
}
#[test]
fn a_structural_insert_carries_the_outline_with_it() {
    // The outline is position-indexed like the sizing and the hidden sets, and
    // was the one thing the shift did not touch: inserting above a group left
    // its levels and collapse flags on the rows the group used to occupy, so
    // the group silently detached from its own rows.
    let mut wb = workbook();
    wb.sheets[0].row_outline_levels.insert(5, 2);
    wb.sheets[0].collapsed_rows.insert(5);

    let inverse = apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 0,
            count: 3,
        },
    )
    .unwrap();

    assert_eq!(wb.sheets[0].row_outline_levels.get(&8), Some(&2));
    assert!(wb.sheets[0].collapsed_rows.contains(&8));

    apply(&mut wb, inverse).unwrap();
    assert_eq!(wb.sheets[0].row_outline_levels.get(&5), Some(&2));
    assert!(wb.sheets[0].collapsed_rows.contains(&5));
}

#[test]
fn a_structural_delete_drops_the_outline_it_removes_and_moves_the_rest() {
    let mut wb = workbook();
    wb.sheets[0].row_outline_levels.insert(2, 1);
    wb.sheets[0].row_outline_levels.insert(7, 3);
    wb.sheets[0].collapsed_cols.insert(4);

    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 1,
            count: 3,
        },
    )
    .unwrap();

    assert!(
        !wb.sheets[0].row_outline_levels.contains_key(&2),
        "the level inside the deleted band goes with it"
    );
    assert_eq!(
        wb.sheets[0].row_outline_levels.get(&4),
        Some(&3),
        "the one below moves up by the band width"
    );
    assert!(
        wb.sheets[0].collapsed_cols.contains(&4),
        "a row delete leaves the column outline alone"
    );
}

/// An edit that changes nothing must leave no trace on the history.
///
/// Found by the `browser-smoke` gate, in the editor, as a user would find it:
/// type a value, press Ctrl+Z, watch nothing happen, press it again and watch
/// the value come back. The editor calls `session_table_autoexpand` after every
/// cell commit — a whole-sheet metadata bundle that, with no table anywhere
/// near the cell, differs in nothing — and each one was landing on the undo
/// stack. Undo was one press behind for every user of the editor.
mod no_op_edits {
    use super::*;
    use crate::{SheetFields, SheetMetadata};

    /// The bundle the editor submits after every commit: captured from the
    /// sheet and handed back unchanged.
    fn unchanged_bundle(wb: &Workbook) -> Operation {
        Operation::set_sheet_metadata(0, SheetMetadata::capture(&wb.sheets[0]))
    }

    #[test]
    fn a_metadata_bundle_that_differs_in_nothing_is_not_undoable() {
        let mut wb = workbook();
        let mut history = History::default();

        let current = wb.clone();
        history.apply(&mut wb, unchanged_bundle(&current)).unwrap();

        assert!(
            !history.can_undo(),
            "nothing changed, so there is nothing to undo — an entry here is \
             one the user presses Ctrl+Z for and sees no effect from"
        );
    }

    #[test]
    fn the_first_undo_after_a_real_edit_reverses_that_edit() {
        // The browser symptom, in miniature: an edit, then the no-op bundle the
        // editor sends after it. One Ctrl+Z must put the value back.
        let mut wb = workbook();
        let mut history = History::default();

        history
            .apply(
                &mut wb,
                Operation::SetCell {
                    sheet: 0,
                    at: CellRef::new(0, 0),
                    cell: Some(Cell::value(CellValue::Number(7.0))),
                },
            )
            .unwrap();
        let after = wb.clone();
        history.apply(&mut wb, unchanged_bundle(&after)).unwrap();

        history.undo(&mut wb).unwrap();
        assert_eq!(
            value_at(&wb, CellRef::new(0, 0)),
            CellValue::Empty,
            "one undo, one edit reversed"
        );
    }

    #[test]
    fn a_no_op_does_not_discard_the_redo_stack() {
        // Clearing redo is how the history says "a new edit happened". Nothing
        // happened, so a redo the user is one keystroke away from must survive.
        let mut wb = workbook();
        let mut history = History::default();

        history
            .apply(
                &mut wb,
                Operation::SetCell {
                    sheet: 0,
                    at: CellRef::new(0, 0),
                    cell: Some(Cell::value(CellValue::Number(7.0))),
                },
            )
            .unwrap();
        history.undo(&mut wb).unwrap();
        assert!(history.can_redo());

        let current = wb.clone();
        history.apply(&mut wb, unchanged_bundle(&current)).unwrap();
        assert!(history.can_redo(), "the redo was not thrown away");

        history.redo(&mut wb).unwrap();
        assert_eq!(value_at(&wb, CellRef::new(0, 0)), CellValue::Number(7.0));
    }

    #[test]
    fn a_metadata_bundle_that_does_differ_is_still_undoable() {
        // The guard must not swallow real work. Freezing a row is exactly the
        // same shape of operation as the no-op above.
        let mut wb = workbook();
        let mut history = History::default();

        let mut data = SheetMetadata::capture(&wb.sheets[0]);
        data.view.frozen_rows = 2;
        history
            .apply(&mut wb, Operation::set_sheet_metadata(0, data))
            .unwrap();

        assert!(history.can_undo(), "a real change is on the stack");
        history.undo(&mut wb).unwrap();
        assert_eq!(wb.sheets[0].view.frozen_rows, 0, "and it reverses");
    }

    #[test]
    fn a_batch_of_nothing_is_nothing() {
        let mut wb = workbook();
        let mut history = History::default();
        let current = wb.clone();

        history
            .apply(
                &mut wb,
                Operation::Batch(vec![unchanged_bundle(&current), unchanged_bundle(&current)]),
            )
            .unwrap();
        assert!(!history.can_undo());

        // But a batch with one real member in it is a real edit.
        let mut data = SheetMetadata::capture(&wb.sheets[0]);
        data.view.frozen_cols = 1;
        history
            .apply(
                &mut wb,
                Operation::Batch(vec![
                    unchanged_bundle(&current),
                    Operation::set_sheet_metadata(0, data),
                ]),
            )
            .unwrap();
        assert!(history.can_undo(), "one member did something");
    }

    #[test]
    fn the_mask_is_what_makes_it_provable() {
        // The guard rests on `apply` handing back an inverse narrowed to the
        // fields that actually differed. If that stopped happening, the guard
        // would silently stop working, so it is asserted directly.
        let mut wb = workbook();
        let current = wb.clone();
        let inverse = apply(&mut wb, unchanged_bundle(&current)).unwrap();
        assert_eq!(inverse.sheet_fields(), SheetFields::NONE);
    }
}

/// Clearing the history: the moment a document becomes the document.
mod clearing_history {
    use super::*;

    #[test]
    fn a_populated_workbook_starts_with_nothing_to_undo() {
        // What a host does to seed a template. Each write is an edit to the
        // engine, so without a way to say "this is the starting point" a user
        // can Ctrl+Z their way out of the document they were handed.
        let mut wb = workbook();
        let mut history = History::default();
        for row in 0..5u32 {
            history
                .apply(
                    &mut wb,
                    Operation::SetCell {
                        sheet: 0,
                        at: CellRef::new(row, 0),
                        cell: Some(Cell::value(CellValue::Number(f64::from(row)))),
                    },
                )
                .unwrap();
        }
        assert!(
            history.can_undo(),
            "the seed is on the stack, as it must be"
        );

        history.clear();
        assert!(!history.can_undo(), "and then it is the starting point");
        assert!(!history.can_redo());
        assert_eq!(
            value_at(&wb, CellRef::new(4, 0)),
            CellValue::Number(4.0),
            "clearing the history keeps the document — it is not an undo"
        );
    }

    #[test]
    fn editing_after_a_clear_is_undoable_back_to_that_point_and_no_further() {
        let mut wb = workbook();
        let mut history = History::default();
        history
            .apply(
                &mut wb,
                Operation::SetCell {
                    sheet: 0,
                    at: CellRef::new(0, 0),
                    cell: Some(Cell::value(CellValue::Number(1.0))),
                },
            )
            .unwrap();
        history.clear();

        history
            .apply(
                &mut wb,
                Operation::SetCell {
                    sheet: 0,
                    at: CellRef::new(0, 0),
                    cell: Some(Cell::value(CellValue::Number(2.0))),
                },
            )
            .unwrap();
        history.undo(&mut wb).unwrap();

        assert_eq!(
            value_at(&wb, CellRef::new(0, 0)),
            CellValue::Number(1.0),
            "back to the starting point"
        );
        assert!(!history.can_undo(), "and no further back than that");
    }
}

/// **A version-5 peer and this build still read each other's operations.**
///
/// The fix for COL-30 needed the inverse of a chart removal to carry bytes, and
/// a new `Operation` *variant* would have been a wire break — an older client
/// cannot parse a variant it has never heard of, which is exactly the failure
/// COL-38 describes. A new **field** is not, on three conditions, and this
/// pins all three:
///
/// 1. the forward operation serialises byte-identically to before, so ordinary
///    editing traffic is unchanged;
/// 2. a message written without the field still parses here;
/// 3. `Operation` does not deny unknown fields, so an older peer given a
///    message that has it ignores it rather than refusing the batch.
///
/// If any of these stops holding, `PROTOCOL_VERSION` has to move — and this
/// test is what will say so.
#[test]
fn the_retained_bytes_field_is_not_a_wire_break() {
    use crate::SheetMetadata;
    use casual_calc_model::{Id, Sheet, SheetId};

    let sheet = Sheet::new(SheetId(Id::from_parts(0x5348, 1)), "Sheet1");
    let forward = Operation::set_sheet_metadata(0, SheetMetadata::capture(&sheet));

    // 1. Nothing new on the wire for an ordinary edit.
    let json = serde_json::to_string(&forward).expect("serialises");
    assert!(
        !json.contains("restore"),
        "a forward edit grew a field: {json}"
    );

    // 2. A message from a peer that has never heard of it.
    let from_v5 = json.clone();
    let parsed: Operation = serde_json::from_str(&from_v5).expect("an older message still parses");
    assert_eq!(parsed, forward);

    // 3. And a message carrying it is readable by anything that ignores unknown
    //    fields — which is what `Operation` does, having no `deny_unknown_fields`.
    let with_field = json.replace(
        r#""changed":"#,
        r#""restore":{"parts":[],"rels":[]},"changed":"#,
    );
    assert_ne!(with_field, json, "the substitution found nothing to do");
    let parsed: Operation = serde_json::from_str(&with_field).expect("parses with the field");
    assert_eq!(parsed, forward);
}

// --- Defined names move with what they point at (FID-24) ---------------------

mod defined_names_shift {
    use super::*;
    use casual_calc_model::DefinedName;

    fn named(refers_to: &str, scoped: bool) -> Workbook {
        let mut wb = workbook();
        let sheet = wb.sheets[0].id;
        wb.defined_names.push(DefinedName {
            name: "Total".to_owned(),
            sheet: scoped.then_some(sheet),
            formula: parse(refers_to).expect("parses"),
        });
        wb
    }

    /// The name's target, compared as a parsed expression rather than as text:
    /// the formula crate exposes no printer, and comparing trees is the
    /// stronger check anyway.
    fn refers_to(wb: &Workbook, expected: &str) -> bool {
        wb.defined_names[0].formula == parse(expected).expect("parses")
    }

    /// **A name below an insert moves with its target.**
    ///
    /// `structural.rs` shifted merges, sizing, hidden sets, the freeze
    /// boundary, outline levels, tables and autofilters — everything
    /// position-indexed except this. A name pointing at `S!$A$10` still pointed
    /// at `S!$A$10` after five rows were inserted above it, so every formula
    /// using the name silently read different cells.
    ///
    /// Silent is the whole of it: nothing is marked `#REF!`, nothing is
    /// refused, the number simply changes.
    #[test]
    fn a_name_below_an_insert_follows_its_target() {
        let mut wb = named("S!$A$10", false);
        apply(
            &mut wb,
            Operation::InsertRows {
                sheet: 0,
                at: 2,
                count: 5,
            },
        )
        .expect("inserted");

        assert!(
            refers_to(&wb, "S!$A$15"),
            "the name stayed where the data no longer is: {:?}",
            wb.defined_names[0].formula
        );
    }

    /// **And back up on a delete.**
    #[test]
    fn a_name_below_a_delete_follows_its_target() {
        let mut wb = named("S!$A$10", false);
        apply(
            &mut wb,
            Operation::DeleteRows {
                sheet: 0,
                at: 2,
                count: 3,
            },
        )
        .expect("deleted");

        assert!(
            refers_to(&wb, "S!$A$7"),
            "{:?}",
            wb.defined_names[0].formula
        );
    }

    /// **A name above the insert does not move.**
    ///
    /// The control. A rewrite that shifted everything would be as wrong as one
    /// that shifted nothing, and harder to notice.
    #[test]
    fn a_name_above_the_insert_is_left_alone() {
        let mut wb = named("S!$A$1", false);
        apply(
            &mut wb,
            Operation::InsertRows {
                sheet: 0,
                at: 5,
                count: 3,
            },
        )
        .expect("inserted");

        assert!(
            refers_to(&wb, "S!$A$1"),
            "{:?}",
            wb.defined_names[0].formula
        );
    }

    /// **A sheet-scoped name is rewritten the same way.**
    #[test]
    fn a_sheet_scoped_name_follows_too() {
        let mut wb = named("S!$B$8:$B$12", true);
        apply(
            &mut wb,
            Operation::InsertRows {
                sheet: 0,
                at: 0,
                count: 2,
            },
        )
        .expect("inserted");

        assert!(
            refers_to(&wb, "S!$B$10:$B$14"),
            "{:?}",
            wb.defined_names[0].formula
        );
    }
}

// --- The rest of the position-indexed state (MNT/structural) ---------------
//
// `sheet.tables` was "the one position-indexed thing this function never
// touched". It was not the only one. `shift_metadata_insert` moves merges, the
// autofilter, filter-hidden rows, sizing, hidden lines, the freeze boundary,
// the outline and now tables — and the sheet also holds `validations`,
// `conditional_formats`, `charts` and `pivots`, every one of them a *range*
// that stops describing its data the moment rows move under it.
//
// The failure is the same shape as the table one and just as quiet: no error,
// no entry in the compatibility report, and a saved file that looks right.

/// A validation over rows `r0..=r1` in column A.
fn validation_over(r0: u32, r1: u32) -> casual_calc_model::DataValidation {
    casual_calc_model::DataValidation {
        range: merge(r0, 0, r1, 0),
        values: Vec::new(),
        kind: Default::default(),
        operator: Default::default(),
        formula1: String::new(),
        formula2: String::new(),
        allow_blank: true,
        error_style: None,
        hide_dropdown: false,
        ime_mode: None,
        error_title: String::new(),
        error_text: String::new(),
        prompt_title: String::new(),
        prompt_text: String::new(),
    }
}

/// **A validation follows its rows.**
///
/// Otherwise a list restricted to `A5:A8` goes on policing `A5:A8` after a row
/// is inserted above it — so the row that moved *out* keeps the rule and the
/// row that moved *in* is unguarded. A person typing into the cell they just
/// created is refused, and the cell they meant is not.
#[test]
fn inserting_rows_moves_a_data_validation() {
    let mut wb = workbook();
    wb.sheets[0]
        .validations
        .push(casual_calc_model::DataValidation {
            range: merge(4, 0, 7, 0),
            values: Vec::new(),
            kind: Default::default(),
            operator: Default::default(),
            formula1: String::new(),
            formula2: String::new(),
            allow_blank: true,
            error_style: None,
            hide_dropdown: false,
            ime_mode: None,
            error_title: String::new(),
            error_text: String::new(),
            prompt_title: String::new(),
            prompt_text: String::new(),
        });

    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 2,
            count: 1,
        },
    )
    .unwrap();

    let moved = &wb.sheets[0].validations[0].range;
    assert_eq!(
        (moved.start.row, moved.end.row),
        (5, 8),
        "the validation stayed on the rows its data left"
    );
}

/// **A conditional format follows its rows**, for the same reason: the
/// highlight is a property of the data, and data that moves takes it along.
#[test]
fn inserting_rows_moves_a_conditional_format() {
    let mut wb = workbook();
    wb.sheets[0]
        .conditional_formats
        .push(casual_calc_model::ConditionalFormat {
            range: merge(4, 0, 7, 0),
            rule: casual_calc_model::CfRule::GreaterThan(1.0),
            fill: String::new(),
            font_color: None,
            bold: false,
            priority: 1,
            stop_if_true: false,
        });

    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 2,
            count: 1,
        },
    )
    .unwrap();

    let moved = &wb.sheets[0].conditional_formats[0].range;
    assert_eq!(
        (moved.start.row, moved.end.row),
        (5, 8),
        "the highlight stayed on the rows its data left"
    );
}

/// And a delete brings them back, so the pair is symmetric rather than
/// one-directional — an insert that moves and a delete that does not is a
/// range that drifts every time somebody edits above it.
#[test]
fn deleting_rows_moves_them_back() {
    let mut wb = workbook();
    wb.sheets[0]
        .validations
        .push(casual_calc_model::DataValidation {
            range: merge(4, 0, 7, 0),
            values: Vec::new(),
            kind: Default::default(),
            operator: Default::default(),
            formula1: String::new(),
            formula2: String::new(),
            allow_blank: true,
            error_style: None,
            hide_dropdown: false,
            ime_mode: None,
            error_title: String::new(),
            error_text: String::new(),
            prompt_title: String::new(),
            prompt_text: String::new(),
        });
    wb.sheets[0]
        .conditional_formats
        .push(casual_calc_model::ConditionalFormat {
            range: merge(4, 0, 7, 0),
            rule: casual_calc_model::CfRule::GreaterThan(1.0),
            fill: String::new(),
            font_color: None,
            bold: false,
            priority: 1,
            stop_if_true: false,
        });

    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 2,
            count: 1,
        },
    )
    .unwrap();
    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 2,
            count: 1,
        },
    )
    .unwrap();

    assert_eq!(
        (
            wb.sheets[0].validations[0].range.start.row,
            wb.sheets[0].conditional_formats[0].range.start.row
        ),
        (4, 4),
        "a round trip left them somewhere else"
    );
}

/// **A comment follows its cell.**
///
/// A note is attached to the cell, not to the address — leave it behind and it
/// annotates whatever moved into that row instead, which is worse than losing
/// it because it reads as though it belongs.
#[test]
fn inserting_rows_moves_a_comment_and_deleting_takes_it_with_the_row() {
    let mut wb = workbook();
    wb.sheets[0].comments.push(casual_calc_model::CellComment {
        at: CellRef::new(5, 0),
        text: "check this".to_owned(),
        author: None,
        created: None,
        resolved: false,
        replies: Vec::new(),
    });

    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 2,
            count: 1,
        },
    )
    .unwrap();
    assert_eq!(
        wb.sheets[0].comments[0].at.row, 6,
        "the note stayed on the row its cell left"
    );

    // Deleting the row the comment is on takes the comment with it.
    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 6,
            count: 1,
        },
    )
    .unwrap();
    assert!(
        wb.sheets[0].comments.is_empty(),
        "the note outlived the cell it annotates"
    );
}

/// **A hyperlink follows its range**, and a delete that swallows the range
/// takes the link with it rather than leaving one pointed at other data.
#[test]
fn inserting_rows_moves_a_hyperlink_and_a_delete_can_remove_it() {
    let mut wb = workbook();
    wb.sheets[0].hyperlinks.push(casual_calc_model::Hyperlink {
        range: merge(4, 0, 4, 0),
        target: Some("https://example.invalid/".to_owned()),
        location: None,
        tooltip: None,
        display: None,
    });

    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 2,
            count: 1,
        },
    )
    .unwrap();
    assert_eq!(
        wb.sheets[0].hyperlinks[0].range.start.row, 5,
        "the link stayed on the row its cell left"
    );

    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 5,
            count: 1,
        },
    )
    .unwrap();
    assert!(
        wb.sheets[0].hyperlinks.is_empty(),
        "a link survived the cells it covered"
    );
}

/// A delete that swallows a validated block drops the rule — and **undo puts
/// it back**, because the delete's inverse carries the whole metadata
/// snapshot. That is why this shift belongs in the bundle rather than beside
/// it: the restore was already there, only the movement was missing.
#[test]
fn a_delete_that_drops_a_validation_is_undone_by_its_inverse() {
    let mut wb = workbook();
    wb.sheets[0].validations.push(validation_over(4, 7));

    let inverse = apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 4,
            count: 4,
        },
    )
    .unwrap();
    assert!(wb.sheets[0].validations.is_empty(), "the delete kept it");

    apply(&mut wb, inverse).unwrap();
    assert_eq!(
        wb.sheets[0]
            .validations
            .first()
            .map(|v| (v.range.start.row, v.range.end.row)),
        Some((4, 7)),
        "undo did not bring the validation back"
    );
}

/// `top..=bottom` of one column, as a range — the shape most of the positional
/// assertions below need.
fn rows(top: u32, bottom: u32, col: u32) -> CellRange {
    CellRange::new(CellRef::new(top, col), CellRef::new(bottom, col))
}

/// Charts, images and pivots are position-indexed too, and were the last three
/// collections `structural.rs` never touched (FID-26).
///
/// A chart's frame is a range and moves like a merge; its series are reference
/// **strings**, so they have to be parsed, shifted and re-emitted. An image's
/// frame is the same range. A pivot is a third shape again: its `source` lives
/// on `source_sheet` while its `anchor` and `output` live on the sheet holding
/// it, so one insert shifts different fields depending on which sheet it landed
/// on.
#[test]
fn insert_rows_moves_charts_images_and_pivots() {
    use casual_calc_model::{ChartKind, ChartSeries, ChartView, Emu, ImageView};

    let mut wb = workbook();
    let sheet = &mut wb.sheets[0];
    let mut chart = ChartView::new(rows(4, 9, 5), ChartKind::Column); // F5:F10
    chart.series.push(ChartSeries {
        name: "Amount".into(),
        categories: Some("S!$A$2:$A$11".into()),
        values: "S!$D$2:$D$11".into(),
    });
    sheet.charts.push(chart);
    sheet.images.push(ImageView {
        anchor: rows(4, 9, 8), // I5:I10
        from_offset: Emu::default(),
        to_offset: Emu::default(),
        part: "xl/media/image1.png".into(),
        extent: None,
    });

    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 1,
            count: 2,
        },
    )
    .unwrap();

    let sheet = &wb.sheets[0];
    assert_eq!(
        sheet.charts[0].anchor,
        rows(6, 11, 5),
        "a chart's frame must follow the rows it sits on"
    );
    assert_eq!(
        sheet.charts[0].series[0].values, "S!$D$4:$D$13",
        "a series must follow the data it plots"
    );
    assert_eq!(
        sheet.charts[0].series[0].categories.as_deref(),
        Some("S!$A$4:$A$13"),
        "a series' categories must follow their data too"
    );
    assert_eq!(
        sheet.images[0].anchor,
        rows(6, 11, 8),
        "an image's frame must follow the rows it sits on"
    );
}

/// A pivot reads one sheet and writes another, so an insert on the source sheet
/// and an insert on the output sheet move different fields. Getting this the
/// wrong way round is silent: the pivot still refreshes, over the wrong records.
#[test]
fn insert_rows_moves_a_pivot_source_and_output_independently() {
    use casual_calc_model::PivotTable;

    let mut wb = workbook();
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(3, 1)), "Report"));
    let data_sheet = wb.sheets[0].id;
    let mut pivot = PivotTable::new(
        1,
        "P".into(),
        data_sheet,
        CellRange::new(CellRef::new(4, 0), CellRef::new(9, 3)), // A5:D10 on S
        CellRef::new(4, 0),                                     // A5 on Report
    );
    pivot.output = Some(CellRange::new(CellRef::new(4, 0), CellRef::new(8, 2)));
    wb.sheets[1].pivots.push(pivot);

    // An insert on the *source* sheet moves the source and leaves the report
    // block where it is.
    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 1,
            count: 2,
        },
    )
    .unwrap();
    let pivot = &wb.sheets[1].pivots[0];
    assert_eq!(
        pivot.source,
        CellRange::new(CellRef::new(6, 0), CellRef::new(11, 3)),
        "an insert on the source sheet must move the source rectangle"
    );
    assert_eq!(
        pivot.anchor,
        CellRef::new(4, 0),
        "it must not move the report, which is on another sheet"
    );

    // An insert on the sheet holding the pivot moves the report and leaves the
    // source alone.
    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 1,
            at: 0,
            count: 1,
        },
    )
    .unwrap();
    let pivot = &wb.sheets[1].pivots[0];
    assert_eq!(
        pivot.anchor,
        CellRef::new(5, 0),
        "an insert above the report must move the report"
    );
    assert_eq!(
        pivot.output.unwrap(),
        CellRange::new(CellRef::new(5, 0), CellRef::new(9, 2)),
        "and the extent the last refresh wrote, so the next one clears it"
    );
    assert_eq!(
        pivot.source,
        CellRange::new(CellRef::new(6, 0), CellRef::new(11, 3)),
        "it must not move the source, which is on another sheet"
    );
}

/// The delete counterpart. A frame wholly inside the band goes with it, one
/// straddling it shrinks, and a series whose data is deleted out from under it
/// becomes `#REF!` rather than quietly plotting the rows that moved up into
/// its old address.
#[test]
fn delete_rows_clamps_charts_and_images_and_breaks_lost_series() {
    use casual_calc_model::{ChartKind, ChartSeries, ChartView, Emu, ImageView};

    let mut wb = workbook();
    let sheet = &mut wb.sheets[0];
    // F1:F10 straddles the band [4, 7) and must shrink by three rows.
    let mut straddling = ChartView::new(rows(0, 9, 5), ChartKind::Column);
    straddling.series.push(ChartSeries {
        name: "kept".into(),
        categories: None,
        values: "S!$D$1:$D$20".into(),
    });
    sheet.charts.push(straddling);
    // J5:J6 is wholly inside the band and goes with it.
    let mut doomed = ChartView::new(rows(4, 5, 9), ChartKind::Pie);
    doomed.series.push(ChartSeries {
        name: "gone".into(),
        categories: None,
        values: "S!$E$5:$E$6".into(),
    });
    sheet.charts.push(doomed);
    sheet.images.push(ImageView {
        anchor: rows(4, 5, 12), // M5:M6, wholly inside
        from_offset: Emu::default(),
        to_offset: Emu::default(),
        part: "xl/media/image1.png".into(),
        extent: None,
    });
    sheet.images.push(ImageView {
        anchor: rows(8, 12, 13), // N9:N13, below the band
        from_offset: Emu::default(),
        to_offset: Emu::default(),
        part: "xl/media/image2.png".into(),
        extent: None,
    });

    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 0,
            at: 4,
            count: 3,
        },
    )
    .unwrap();

    let sheet = &wb.sheets[0];
    assert_eq!(
        sheet.charts.len(),
        1,
        "a chart whose whole frame is deleted goes with it"
    );
    assert_eq!(
        sheet.charts[0].anchor,
        rows(0, 6, 5),
        "a chart straddling the band shrinks by the rows it lost"
    );
    assert_eq!(
        sheet.charts[0].series[0].values, "S!$D$1:$D$17",
        "a series spanning the band shrinks by the rows it lost"
    );
    assert_eq!(
        sheet.images.len(),
        1,
        "an image whose whole frame is deleted goes with it"
    );
    assert_eq!(
        sheet.images[0].anchor,
        rows(5, 9, 13),
        "an image below the band moves up"
    );
}

/// A pivot survives losing its output extent — the definition is still valid
/// and a refresh will write a new one. Dropping the pivot instead would delete
/// a report the user can still see the definition of.
#[test]
fn deleting_a_pivot_output_band_keeps_the_pivot() {
    use casual_calc_model::PivotTable;

    let mut wb = workbook();
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(3, 1)), "Report"));
    let data_sheet = wb.sheets[0].id;
    let mut pivot = PivotTable::new(
        1,
        "P".into(),
        data_sheet,
        CellRange::new(CellRef::new(0, 0), CellRef::new(9, 3)),
        CellRef::new(4, 0),
    );
    pivot.output = Some(CellRange::new(CellRef::new(4, 0), CellRef::new(6, 2)));
    wb.sheets[1].pivots.push(pivot);

    apply(
        &mut wb,
        Operation::DeleteRows {
            sheet: 1,
            at: 4,
            count: 3,
        },
    )
    .unwrap();

    let pivot = &wb.sheets[1].pivots[0];
    assert_eq!(pivot.output, None, "the extent it wrote is gone");
    assert_eq!(
        pivot.anchor,
        CellRef::new(4, 0),
        "but the pivot itself survives, ready to refresh again"
    );
    assert_eq!(
        pivot.source,
        CellRange::new(CellRef::new(0, 0), CellRef::new(9, 3)),
        "and its source, on another sheet, is untouched"
    );
}

/// Re-emitting a series means printing it in *our* spelling, so a sheet name
/// that needs quoting has to come back quoted. Excel writes `'My Data'!$D$2`
/// and reads nothing else; losing the quotes would produce a chart that is
/// shifted correctly and refuses to load.
#[test]
fn a_shifted_series_keeps_the_quoting_its_sheet_name_needs() {
    use casual_calc_model::{ChartKind, ChartSeries, ChartView};

    let mut wb = Workbook::new(Id::from_parts(1, 1));
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "My Data"));
    let mut chart = ChartView::new(rows(0, 4, 5), ChartKind::Column);
    chart.series.push(ChartSeries {
        name: "Amount".into(),
        categories: None,
        values: "'My Data'!$D$2:$D$11".into(),
    });
    wb.sheets[0].charts.push(chart);

    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 1,
            count: 2,
        },
    )
    .unwrap();

    assert_eq!(
        wb.sheets[0].charts[0].series[0].values,
        "'My Data'!$D$4:$D$13"
    );
}

/// A series naming a sheet the insert did not touch must be left exactly as it
/// was — including its original spelling, because re-emitting an unchanged
/// reference through our printer is a silent rewrite of somebody's file.
#[test]
fn a_series_on_another_sheet_is_not_touched_or_reformatted() {
    use casual_calc_model::{ChartKind, ChartSeries, ChartView};

    let mut wb = workbook();
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(3, 1)), "Other"));
    let mut chart = ChartView::new(rows(0, 4, 5), ChartKind::Column);
    chart.series.push(ChartSeries {
        name: "Amount".into(),
        categories: None,
        values: "Other!$D$2:$D$11".into(),
    });
    wb.sheets[0].charts.push(chart);

    apply(
        &mut wb,
        Operation::InsertRows {
            sheet: 0,
            at: 1,
            count: 2,
        },
    )
    .unwrap();

    assert_eq!(
        wb.sheets[0].charts[0].series[0].values, "Other!$D$2:$D$11",
        "an insert on S must not move a series reading Other"
    );
}

// ---------------------------------------------------------------------------
// Moving columns, rows and ranges.
// ---------------------------------------------------------------------------

/// A sheet whose columns A..E hold 1..5 in row 1, so a reorder is readable as a
/// sequence.
fn lettered_columns() -> Workbook {
    let mut wb = workbook();
    for c in 0..5u32 {
        set_num(&mut wb, 0, 0, c, f64::from(c) + 1.0);
    }
    wb
}

/// Row 1's values left to right, with a gap for an empty column.
fn row_one(wb: &Workbook, width: u32) -> Vec<Option<f64>> {
    (0..width)
        .map(|c| match value_at(wb, CellRef::new(0, c)) {
            CellValue::Number(n) => Some(n),
            _ => None,
        })
        .collect()
}

#[test]
fn moving_a_column_relocates_its_data() {
    let mut wb = lettered_columns();
    // Drag B (value 2) and drop it before E: A C D B E.
    apply(
        &mut wb,
        Operation::MoveColumns {
            sheet: 0,
            at: 1,
            count: 1,
            before: 4,
        },
    )
    .unwrap();
    assert_eq!(
        row_one(&wb, 5),
        vec![Some(1.0), Some(3.0), Some(4.0), Some(2.0), Some(5.0)],
        "cut-and-insert, not a swap"
    );
}

#[test]
fn moving_a_band_of_columns_left_relocates_its_data() {
    let mut wb = lettered_columns();
    // Drag D:E and drop them before B: A D E B C.
    apply(
        &mut wb,
        Operation::MoveColumns {
            sheet: 0,
            at: 3,
            count: 2,
            before: 1,
        },
    )
    .unwrap();
    assert_eq!(
        row_one(&wb, 5),
        vec![Some(1.0), Some(4.0), Some(5.0), Some(2.0), Some(3.0)]
    );
}

#[test]
fn a_drop_onto_the_band_itself_changes_nothing() {
    for before in [1u32, 2, 3] {
        let mut wb = lettered_columns();
        let before_state = observable(&wb);
        let inverse = apply(
            &mut wb,
            Operation::MoveColumns {
                sheet: 0,
                at: 1,
                count: 2,
                before,
            },
        )
        .unwrap();
        assert_eq!(observable(&wb), before_state, "drop before {before}");
        // Provably nothing, so `History` keeps it off the undo stack.
        assert_eq!(inverse, Operation::Batch(Vec::new()));
        assert!(crate::changes_nothing(&inverse));
    }
}

#[test]
fn a_formula_outside_the_moved_columns_follows_them() {
    let mut wb = lettered_columns();
    for r in 0..3u32 {
        set_num(&mut wb, 0, r, 1, f64::from(r) + 10.0); // B1:B3
    }
    set_formula(&mut wb, 0, 0, 5, "SUM(B1:B3)"); // F1, outside the move

    apply(
        &mut wb,
        Operation::MoveColumns {
            sheet: 0,
            at: 1,
            count: 1,
            before: 4,
        },
    )
    .unwrap();

    // B landed where D was, so the reference has to land there too.
    assert_eq!(
        formula_text(&wb, 0, 0, 5).as_deref(),
        Some("SUM(D1:D3)"),
        "a reference to a moved column must follow it"
    );
}

#[test]
fn an_anchored_reference_follows_a_moved_column_too() {
    let mut wb = lettered_columns();
    set_formula(&mut wb, 0, 0, 5, "$B$1*2");
    apply(
        &mut wb,
        Operation::MoveColumns {
            sheet: 0,
            at: 1,
            count: 1,
            before: 4,
        },
    )
    .unwrap();
    // `$` is about what a *copy* does to a reference, not about whether the
    // cell it names may move. Excel moves both.
    assert_eq!(formula_text(&wb, 0, 0, 5).as_deref(), Some("$D$1*2"));
}

#[test]
fn a_formula_inside_a_moved_column_keeps_its_meaning() {
    let mut wb = lettered_columns();
    // B1 reads one column left (A, which does not move) and one right (C, which
    // is renumbered by the move).
    set_formula(&mut wb, 0, 0, 1, "C1+A1");

    apply(
        &mut wb,
        Operation::MoveColumns {
            sheet: 0,
            at: 1,
            count: 2,
            before: 4,
        },
    )
    .unwrap();

    // B:C landed where D:E began, so the formula is now in C1. It still names
    // the same two cells: A never moved, and old C is now D.
    assert_eq!(
        formula_text(&wb, 0, 0, 2).as_deref(),
        Some("D1+A1"),
        "a formula that moves means what it always meant"
    );
    assert!(formula_text(&wb, 0, 0, 1).is_none(), "B1 is empty now");
}

#[test]
fn a_moved_formulas_relative_reference_does_not_drift() {
    let mut wb = lettered_columns();
    set_formula(&mut wb, 0, 0, 1, "A1*2"); // B1, reading its left neighbour

    apply(
        &mut wb,
        Operation::MoveColumns {
            sheet: 0,
            at: 1,
            count: 1,
            before: 4,
        },
    )
    .unwrap();

    // The tree is stored as an offset from its own cell (`PERF-11`), so the
    // move has to re-store it at the new origin. Left un-restored, the same
    // "one column left" read as C1 from its new home in D1.
    assert_eq!(formula_text(&wb, 0, 0, 3).as_deref(), Some("A1*2"));
}

#[test]
fn a_range_the_moved_column_is_taken_out_of_shrinks() {
    let mut wb = lettered_columns();
    set_formula(&mut wb, 0, 0, 5, "SUM(A1:C1)");
    apply(
        &mut wb,
        Operation::MoveColumns {
            sheet: 0,
            at: 1,
            count: 1,
            before: 4,
        },
    )
    .unwrap();
    // {A, B, C} minus B is {A, C}, which is not contiguous; the delete half of
    // Excel's cut-and-insert leaves the surviving span, so old A and old C.
    assert_eq!(formula_text(&wb, 0, 0, 5).as_deref(), Some("SUM(A1:B1)"));
}

#[test]
fn a_range_the_moved_column_is_dropped_into_grows() {
    let mut wb = lettered_columns();
    set_formula(&mut wb, 0, 0, 5, "SUM(B1:E1)");
    // Drag A and drop it before D, i.e. into the middle of the range.
    apply(
        &mut wb,
        Operation::MoveColumns {
            sheet: 0,
            at: 0,
            count: 1,
            before: 3,
        },
    )
    .unwrap();
    assert_eq!(formula_text(&wb, 0, 0, 5).as_deref(), Some("SUM(A1:E1)"));
}

#[test]
fn a_move_never_produces_a_ref_error() {
    // Every span position against every move, on a small grid: a move destroys
    // nothing, so nothing may read as destroyed.
    for at in 0..5u32 {
        for count in 1..3u32 {
            for before in 0..6u32 {
                for lo in 0..5u32 {
                    for hi in lo..5u32 {
                        let mut wb = lettered_columns();
                        let text = format!(
                            "SUM({}1:{}1)",
                            casual_calc_formula::column_to_letters(lo),
                            casual_calc_formula::column_to_letters(hi)
                        );
                        set_formula(&mut wb, 0, 0, 8, &text);
                        apply(
                            &mut wb,
                            Operation::MoveColumns {
                                sheet: 0,
                                at,
                                count,
                                before,
                            },
                        )
                        .unwrap();
                        let after = formula_text(&wb, 0, 0, 8).unwrap_or_default();
                        assert!(
                            !after.contains("#REF!"),
                            "{text} became {after} moving {count} at {at} before {before}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_defined_name_follows_a_moved_column() {
    let mut wb = lettered_columns();
    wb.defined_names.push(casual_calc_model::DefinedName {
        name: "Rate".to_owned(),
        sheet: None,
        formula: parse("S!$B$1").unwrap(),
    });
    apply(
        &mut wb,
        Operation::MoveColumns {
            sheet: 0,
            at: 1,
            count: 1,
            before: 4,
        },
    )
    .unwrap();
    assert_eq!(wb.defined_names[0].formula.to_string(), "S!$D$1");
}

#[test]
fn a_moved_column_takes_its_width_and_hidden_flag_with_it() {
    let mut wb = lettered_columns();
    wb.sheets[0].columns.sizes.insert(1, 2400);
    wb.sheets[0].hidden_cols.insert(1);
    apply(
        &mut wb,
        Operation::MoveColumns {
            sheet: 0,
            at: 1,
            count: 1,
            before: 4,
        },
    )
    .unwrap();
    assert_eq!(wb.sheets[0].columns.sizes.get(&3), Some(&2400));
    assert!(!wb.sheets[0].columns.sizes.contains_key(&1));
    assert!(wb.sheets[0].hidden_cols.contains(&3));
    assert!(!wb.sheets[0].hidden_cols.contains(&1));
}

#[test]
fn a_merge_inside_a_moved_column_travels_with_it() {
    let mut wb = lettered_columns();
    wb.sheets[0]
        .merges
        .push(CellRange::new(CellRef::new(0, 1), CellRef::new(2, 1)));
    apply(
        &mut wb,
        Operation::MoveColumns {
            sheet: 0,
            at: 1,
            count: 1,
            before: 4,
        },
    )
    .unwrap();
    assert_eq!(
        wb.sheets[0].merges,
        vec![CellRange::new(CellRef::new(0, 3), CellRef::new(2, 3))]
    );
}

#[test]
fn moving_columns_undoes_exactly() {
    let mut wb = lettered_columns();
    set_formula(&mut wb, 0, 0, 5, "SUM(A1:C1)"); // the band is dragged out of it
    // **The endpoint the band swallows is the case a reverse move cannot
    // recover.** `B1:C1` loses its low endpoint to the band, so the forward
    // rewrite clamps it to what is left — `B1:B1`, meaning old C — and the
    // reverse move, seeing a span that is merely *next to* the returning band,
    // shifts it rather than re-growing it. Only the snapshot restores it, and
    // dropping the snapshot leaves every other assertion here green.
    set_formula(&mut wb, 0, 0, 7, "SUM(B1:C1)");
    set_formula(&mut wb, 0, 0, 1, "A1*2"); // travels
    set_formula(&mut wb, 0, 1, 6, "B1+$C$1"); // follows
    wb.sheets[0].columns.sizes.insert(1, 2400);
    wb.sheets[0]
        .merges
        .push(CellRange::new(CellRef::new(0, 1), CellRef::new(2, 1)));
    wb.sheets[0]
        .merges
        .push(CellRange::new(CellRef::new(4, 1), CellRef::new(4, 2)));
    wb.defined_names.push(casual_calc_model::DefinedName {
        name: "Rate".to_owned(),
        sheet: None,
        formula: parse("S!$B$1:$C$1").unwrap(),
    });
    let before = wb.clone();

    let inverse = apply(
        &mut wb,
        Operation::MoveColumns {
            sheet: 0,
            at: 1,
            count: 1,
            before: 4,
        },
    )
    .unwrap();
    assert_ne!(observable(&wb), observable(&before));
    apply(&mut wb, inverse).unwrap();

    assert_eq!(observable(&wb), observable(&before));
    // Spelled out rather than left to `observable`, which compares a stored
    // tree's *absolute* rendering: under `PERF-11` a tree is a set of offsets,
    // and two different offset sets can render alike off their own origin.
    assert_eq!(formula_text(&wb, 0, 0, 5).as_deref(), Some("SUM(A1:C1)"));
    assert_eq!(formula_text(&wb, 0, 0, 7).as_deref(), Some("SUM(B1:C1)"));
    assert_eq!(formula_text(&wb, 0, 0, 1).as_deref(), Some("A1*2"));
    assert_eq!(formula_text(&wb, 0, 1, 6).as_deref(), Some("B1+$C$1"));
    assert_eq!(wb.sheets[0].columns.sizes, before.sheets[0].columns.sizes);
    assert_eq!(wb.sheets[0].merges, before.sheets[0].merges);
    assert_eq!(wb.defined_names, before.defined_names);
}

#[test]
fn moving_columns_redoes_what_it_undid() {
    let mut wb = lettered_columns();
    set_formula(&mut wb, 0, 0, 5, "SUM(B1:B1)");
    let mut history = History::new();
    let op = Operation::MoveColumns {
        sheet: 0,
        at: 1,
        count: 1,
        before: 4,
    };
    let original = observable(&wb);
    history.apply(&mut wb, op).unwrap();
    let moved = observable(&wb);
    assert_eq!(formula_text(&wb, 0, 0, 5).as_deref(), Some("SUM(D1:D1)"));

    history.undo(&mut wb).unwrap();
    assert_eq!(observable(&wb), original);
    history.redo(&mut wb).unwrap();
    assert_eq!(observable(&wb), moved);
}

#[test]
fn moving_rows_relocates_data_and_follows_references() {
    let mut wb = workbook();
    for r in 0..5u32 {
        set_num(&mut wb, 0, r, 0, f64::from(r) + 1.0); // A1:A5 = 1..5
    }
    set_formula(&mut wb, 0, 0, 3, "SUM(A2:A2)"); // D1, outside the move
    set_formula(&mut wb, 0, 1, 1, "A2*2"); // B2, travels with row 2

    apply(
        &mut wb,
        Operation::MoveRows {
            sheet: 0,
            at: 1,
            count: 1,
            before: 4,
        },
    )
    .unwrap();

    // Row 2 (value 2) lands where row 4 was: 1 3 4 2 5.
    let column: Vec<Option<f64>> = (0..5)
        .map(|r| match value_at(&wb, CellRef::new(r, 0)) {
            CellValue::Number(n) => Some(n),
            _ => None,
        })
        .collect();
    assert_eq!(
        column,
        vec![Some(1.0), Some(3.0), Some(4.0), Some(2.0), Some(5.0)]
    );
    assert_eq!(formula_text(&wb, 0, 0, 3).as_deref(), Some("SUM(A4:A4)"));
    // The travelling formula is now in B4 and still names the cell it named.
    assert_eq!(formula_text(&wb, 0, 3, 1).as_deref(), Some("A4*2"));
}

#[test]
fn moving_rows_undoes_exactly() {
    let mut wb = workbook();
    for r in 0..5u32 {
        set_num(&mut wb, 0, r, 0, f64::from(r) + 1.0);
    }
    set_formula(&mut wb, 0, 0, 3, "SUM(A1:A3)");
    wb.sheets[0].rows.sizes.insert(1, 400);
    assert_round_trip(
        wb,
        Operation::MoveRows {
            sheet: 0,
            at: 1,
            count: 1,
            before: 4,
        },
    );
}

#[test]
fn moving_a_range_leaves_the_source_empty() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 0, 1.0); // A1
    set_num(&mut wb, 0, 1, 0, 2.0); // A2
    set_num(&mut wb, 0, 1, 1, 4.0); // B2 — B1 deliberately empty
    set_num(&mut wb, 0, 2, 2, 99.0); // C3, about to be overwritten
    set_num(&mut wb, 0, 2, 3, 77.0); // D3, under the block's *blank* corner

    apply(
        &mut wb,
        Operation::MoveRange {
            sheet: 0,
            from: CellRange::new(CellRef::new(0, 0), CellRef::new(1, 1)),
            to: CellRef::new(2, 2),
        },
    )
    .unwrap();

    for at in [CellRef::new(0, 0), CellRef::new(1, 0), CellRef::new(1, 1)] {
        assert!(
            wb.sheets[0].cells.get(at).is_none(),
            "source {at:?} must be empty"
        );
    }
    assert_eq!(value_at(&wb, CellRef::new(2, 2)), CellValue::Number(1.0));
    assert_eq!(value_at(&wb, CellRef::new(3, 2)), CellValue::Number(2.0));
    assert_eq!(value_at(&wb, CellRef::new(3, 3)), CellValue::Number(4.0));
    // A move carries its blanks: D3 held 77 and the block's empty B1 landed on
    // it, so it is empty now rather than left behind.
    assert!(wb.sheets[0].cells.get(CellRef::new(2, 3)).is_none());
}

#[test]
fn a_formula_outside_a_moved_range_follows_it() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 0, 1.0); // A1
    set_num(&mut wb, 0, 1, 1, 4.0); // B2
    set_formula(&mut wb, 0, 0, 5, "A1+$B$2"); // F1, outside

    apply(
        &mut wb,
        Operation::MoveRange {
            sheet: 0,
            from: CellRange::new(CellRef::new(0, 0), CellRef::new(1, 1)),
            to: CellRef::new(2, 2),
        },
    )
    .unwrap();

    assert_eq!(formula_text(&wb, 0, 0, 5).as_deref(), Some("C3+$D$4"));
}

#[test]
fn a_formula_inside_a_moved_range_keeps_its_meaning() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 5, 7.0); // F1, outside the block and staying put
    set_formula(&mut wb, 0, 0, 0, "F1*2"); // A1, inside the block

    apply(
        &mut wb,
        Operation::MoveRange {
            sheet: 0,
            from: CellRange::new(CellRef::new(0, 0), CellRef::new(1, 1)),
            to: CellRef::new(2, 2),
        },
    )
    .unwrap();

    // The cell moved to C3 and still reads F1 — a cut travels verbatim.
    assert_eq!(formula_text(&wb, 0, 2, 2).as_deref(), Some("F1*2"));
}

#[test]
fn moving_a_range_undoes_exactly() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 0, 1.0);
    set_num(&mut wb, 0, 1, 1, 4.0);
    set_num(&mut wb, 0, 2, 2, 99.0); // destroyed by the move; only a snapshot brings it back
    set_formula(&mut wb, 0, 0, 5, "A1+$B$2");
    wb.defined_names.push(casual_calc_model::DefinedName {
        name: "Rate".to_owned(),
        sheet: None,
        formula: parse("S!$A$1").unwrap(),
    });
    let before = wb.clone();

    let inverse = apply(
        &mut wb,
        Operation::MoveRange {
            sheet: 0,
            from: CellRange::new(CellRef::new(0, 0), CellRef::new(1, 1)),
            to: CellRef::new(2, 2),
        },
    )
    .unwrap();
    assert_eq!(wb.defined_names[0].formula.to_string(), "S!$C$3");
    apply(&mut wb, inverse).unwrap();

    assert_eq!(observable(&wb), observable(&before));
    assert_eq!(wb.defined_names, before.defined_names);
    assert_eq!(value_at(&wb, CellRef::new(2, 2)), CellValue::Number(99.0));
}

#[test]
fn a_range_move_to_where_it_already_is_changes_nothing() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 0, 1.0);
    let inverse = apply(
        &mut wb,
        Operation::MoveRange {
            sheet: 0,
            from: CellRange::new(CellRef::new(0, 0), CellRef::new(1, 1)),
            to: CellRef::new(0, 0),
        },
    )
    .unwrap();
    assert_eq!(inverse, Operation::Batch(Vec::new()));
    assert_eq!(value_at(&wb, CellRef::new(0, 0)), CellValue::Number(1.0));
}

#[test]
fn a_merge_inside_a_moved_range_travels_and_one_it_lands_on_is_destroyed() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 0, 1.0);
    wb.sheets[0]
        .merges
        .push(CellRange::new(CellRef::new(0, 0), CellRef::new(0, 1)));
    wb.sheets[0]
        .merges
        .push(CellRange::new(CellRef::new(2, 2), CellRef::new(2, 3)));

    apply(
        &mut wb,
        Operation::MoveRange {
            sheet: 0,
            from: CellRange::new(CellRef::new(0, 0), CellRef::new(1, 1)),
            to: CellRef::new(2, 2),
        },
    )
    .unwrap();

    assert_eq!(
        wb.sheets[0].merges,
        vec![CellRange::new(CellRef::new(2, 2), CellRef::new(2, 3))],
        "the source merge travels onto exactly where the destination one was"
    );
}

#[test]
fn a_concurrent_move_is_refused_by_the_transform() {
    // Not designed yet, and a wrong answer would be silent divergence.
    let subject = Operation::MoveColumns {
        sheet: 0,
        at: 1,
        count: 1,
        before: 4,
    };
    let against = Operation::SetValue {
        sheet: 0,
        at: CellRef::new(0, 0),
        value: CellValue::Number(1.0),
    };
    assert!(
        crate::transform::transform(&subject, &against, crate::transform::Side::Later, &[])
            .is_err()
    );
    assert!(
        crate::transform::transform(&against, &subject, crate::transform::Side::Later, &[])
            .is_err()
    );
}

#[test]
fn a_cross_sheet_reference_follows_a_moved_column() {
    let mut wb = lettered_columns();
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(3, 1)), "T"));
    set_formula(&mut wb, 1, 0, 0, "S!B1*2");
    // An unqualified reference on the *other* sheet names that sheet, so it
    // must not move.
    set_formula(&mut wb, 1, 1, 0, "B1*2");

    apply(
        &mut wb,
        Operation::MoveColumns {
            sheet: 0,
            at: 1,
            count: 1,
            before: 4,
        },
    )
    .unwrap();

    assert_eq!(formula_text(&wb, 1, 0, 0).as_deref(), Some("S!D1*2"));
    assert_eq!(formula_text(&wb, 1, 1, 0).as_deref(), Some("B1*2"));
}

#[test]
fn dragging_a_column_inside_a_table_reorders_its_column_list() {
    let mut wb = lettered_columns();
    // A table over B..D with three named columns.
    wb.sheets[0]
        .tables
        .push(table("Sales", 0, 1, 4, 3, &["Region", "Amount", "Date"]));

    // Drag "Amount" (column C) to sit before "Region" (column B).
    apply(
        &mut wb,
        Operation::MoveColumns {
            sheet: 0,
            at: 2,
            count: 1,
            before: 1,
        },
    )
    .unwrap();

    let names: Vec<&str> = wb.sheets[0].tables[0]
        .columns
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    // A structured reference resolves through this list by position, so leaving
    // it alone would make `Sales[Amount]` name its neighbour.
    assert_eq!(names, vec!["Amount", "Region", "Date"]);
    assert_eq!(wb.sheets[0].tables[0].range, merge(0, 1, 4, 3));
}

#[test]
fn a_range_move_that_overlaps_its_own_source_keeps_every_cell() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 0, 1.0); // A1
    set_num(&mut wb, 0, 0, 1, 2.0); // B1
    set_num(&mut wb, 0, 1, 0, 3.0); // A2
    set_num(&mut wb, 0, 1, 1, 4.0); // B2

    // A1:B2 dragged one cell down-right, so the two rectangles share B2.
    apply(
        &mut wb,
        Operation::MoveRange {
            sheet: 0,
            from: CellRange::new(CellRef::new(0, 0), CellRef::new(1, 1)),
            to: CellRef::new(1, 1),
        },
    )
    .unwrap();

    assert_eq!(value_at(&wb, CellRef::new(1, 1)), CellValue::Number(1.0));
    assert_eq!(value_at(&wb, CellRef::new(1, 2)), CellValue::Number(2.0));
    assert_eq!(value_at(&wb, CellRef::new(2, 1)), CellValue::Number(3.0));
    assert_eq!(value_at(&wb, CellRef::new(2, 2)), CellValue::Number(4.0));
    // The parts of the source the destination does not cover are empty.
    assert!(wb.sheets[0].cells.get(CellRef::new(0, 0)).is_none());
    assert!(wb.sheets[0].cells.get(CellRef::new(0, 1)).is_none());
    assert!(wb.sheets[0].cells.get(CellRef::new(1, 0)).is_none());
}

#[test]
fn an_overlapping_range_move_undoes_exactly() {
    let mut wb = workbook();
    for r in 0..2u32 {
        for c in 0..2u32 {
            set_num(&mut wb, 0, r, c, f64::from(r * 2 + c) + 1.0);
        }
    }
    assert_round_trip(
        wb,
        Operation::MoveRange {
            sheet: 0,
            from: CellRange::new(CellRef::new(0, 0), CellRef::new(1, 1)),
            to: CellRef::new(1, 1),
        },
    );
}

#[test]
fn a_range_move_off_the_end_of_the_grid_does_nothing() {
    let mut wb = workbook();
    set_num(&mut wb, 0, 0, 0, 1.0);
    let inverse = apply(
        &mut wb,
        Operation::MoveRange {
            sheet: 0,
            from: CellRange::new(CellRef::new(0, 0), CellRef::new(1, 1)),
            to: CellRef::new(u32::MAX, 0),
        },
    )
    .unwrap();
    assert_eq!(inverse, Operation::Batch(Vec::new()));
    assert_eq!(value_at(&wb, CellRef::new(0, 0)), CellValue::Number(1.0));
}

#[test]
fn a_whole_column_reference_follows_a_column_move_and_ignores_a_row_move() {
    let mut wb = lettered_columns();
    set_formula(&mut wb, 0, 4, 7, "SUM(B:B)");
    apply(
        &mut wb,
        Operation::MoveColumns {
            sheet: 0,
            at: 1,
            count: 1,
            before: 4,
        },
    )
    .unwrap();
    // A whole-column range still names a column, and that column moved.
    assert_eq!(formula_text(&wb, 0, 4, 7).as_deref(), Some("SUM(D:D)"));

    // A *row* move cannot change it: it already covers every row, and shifting
    // its open bound would turn `D:D` into a range that no longer starts at
    // row 1.
    apply(
        &mut wb,
        Operation::MoveRows {
            sheet: 0,
            at: 0,
            count: 1,
            before: 3,
        },
    )
    .unwrap();
    assert_eq!(formula_text(&wb, 0, 4, 7).as_deref(), Some("SUM(D:D)"));
}
