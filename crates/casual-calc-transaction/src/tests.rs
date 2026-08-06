//! Transaction tests: inverses, atomic batches, undo/redo, and edit→recalc.

use casual_calc_formula::parse;
use casual_calc_model::{
    Cell, CellRange, CellRef, CellValue, Id, Sheet, SheetId, Style, StyleId, Workbook,
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
    let handle = wb.store_formula(casual_calc_formula::parse("A1*2").unwrap());
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
    let handle = wb.store_formula(parse(text).unwrap());
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
    Some(wb.formula(handle).unwrap().to_string())
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
        Some("(A7+$A$7)"),
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
    assert_eq!(formula_text(&wb, 0, 0, 0).as_deref(), Some("(#REF!+B1)"));
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
        Operation::SetSheetMetadata {
            sheet: 0,
            merges: vec![merge(3, 3, 4, 4)],
            columns: Default::default(),
            rows: Default::default(),
            hidden_rows: Default::default(),
            hidden_cols: Default::default(),
            view: Default::default(),
        },
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
    assert_eq!(formula_text(&wb, 1, 0, 0).as_deref(), Some("(Data!A1+1)"));

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
    assert_eq!(formula_text(&wb, 1, 0, 0).as_deref(), Some("(Facts!A1+1)"));

    // Undo: both the name and the reference revert.
    apply(&mut wb, inverse).unwrap();
    assert_eq!(wb.sheets[0].name, "Data");
    assert_eq!(formula_text(&wb, 1, 0, 0).as_deref(), Some("(Data!A1+1)"));
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
