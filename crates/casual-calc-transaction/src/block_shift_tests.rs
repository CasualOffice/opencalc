//! `XL-UX-173`: Insert Cells and Delete Cells with a shift adjust references.
//!
//! Excel's Insert ▸ Shift cells down (right) and Delete ▸ Shift cells up
//! (left) move only the cells in the selection's columns (rows), and every
//! reference to a moved cell follows it — no question asked. OpenCalc did the
//! move as a batch of cell writes, so references kept naming the old
//! addresses, and the editor warned that they "will not be adjusted".
//!
//! The rule for a range, after LibreOffice's `ScRefUpdate` and checked against
//! the cases Microsoft documents (see the row's notes): a range whose columns
//! (rows) lie **inside** the shifted block's moves exactly as it would for a
//! whole-row (column) insert or delete; one reaching outside the block is left
//! alone, since no single rectangle names the cells it held after a partial
//! shift.

use casual_calc_formula::parse;
use casual_calc_formula::stored::{ABSOLUTE, Origin};
use casual_calc_model::{
    Cell, CellComment, CellRange, CellRef, CellValue, ConditionalFormat, DefinedName, Id, Sheet,
    SheetId, Workbook,
};

use crate::{CellShift, Operation, TxnError, apply};

fn workbook() -> Workbook {
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "S"));
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(2, 2)), "T"));
    wb
}

fn num(wb: &mut Workbook, sheet: usize, row: u32, col: u32, n: f64) {
    wb.sheets[sheet]
        .cells
        .set(CellRef::new(row, col), Cell::value(CellValue::Number(n)));
}

fn formula(wb: &mut Workbook, sheet: usize, row: u32, col: u32, text: &str) {
    let handle = wb.store_formula_at(
        parse(text.trim_start_matches('=')).unwrap(),
        Origin::at(row, col),
    );
    let mut cell = Cell::value(CellValue::Empty);
    cell.formula = Some(handle);
    wb.sheets[sheet].cells.set(CellRef::new(row, col), cell);
}

fn text_at(wb: &Workbook, sheet: usize, row: u32, col: u32) -> String {
    let handle = wb.sheets[sheet]
        .cells
        .get(CellRef::new(row, col))
        .and_then(|c| c.formula)
        .unwrap_or_else(|| panic!("no formula at {row},{col}"));
    casual_calc_formula::restore_at(wb.formula(handle).unwrap(), Origin::at(row, col), ABSOLUTE)
        .to_string()
}

fn value(wb: &Workbook, sheet: usize, row: u32, col: u32) -> CellValue {
    wb.sheets[sheet]
        .cells
        .get(CellRef::new(row, col))
        .map_or(CellValue::Empty, |c| c.value.clone())
}

fn range(r0: u32, c0: u32, r1: u32, c1: u32) -> CellRange {
    CellRange::new(CellRef::new(r0, c0), CellRef::new(r1, c1))
}

fn shift(r: CellRange, direction: CellShift) -> Operation {
    Operation::ShiftCells {
        sheet: 0,
        range: r,
        shift: direction,
    }
}

/// The tracker row's acceptance case: Insert Cells, Shift cells down on B2,
/// and `=B2` in D2 reads `=B3`.
#[test]
fn insert_shift_down_moves_a_reference_into_the_block() {
    let mut wb = workbook();
    num(&mut wb, 0, 1, 1, 5.0); // B2
    formula(&mut wb, 0, 1, 3, "=B2"); // D2
    apply(&mut wb, shift(range(1, 1, 1, 1), CellShift::Down)).unwrap();
    assert_eq!(
        value(&wb, 0, 2, 1),
        CellValue::Number(5.0),
        "B2 moved to B3"
    );
    assert_eq!(text_at(&wb, 0, 1, 3), "B3");
}

#[test]
fn only_the_blocks_columns_move_and_only_their_references_follow() {
    let mut wb = workbook();
    num(&mut wb, 0, 4, 0, 1.0); // A5 — outside the block's column
    num(&mut wb, 0, 4, 1, 2.0); // B5 — inside
    formula(&mut wb, 0, 0, 5, "=A5+B5+$B$5"); // F1
    apply(&mut wb, shift(range(1, 1, 2, 1), CellShift::Down)).unwrap(); // B2:B3
    assert_eq!(value(&wb, 0, 4, 0), CellValue::Number(1.0), "A5 stayed");
    assert_eq!(
        value(&wb, 0, 6, 1),
        CellValue::Number(2.0),
        "B5 moved two down"
    );
    assert_eq!(text_at(&wb, 0, 0, 5), "A5+B7+$B$7");
}

#[test]
fn a_range_inside_the_block_columns_grows_and_one_reaching_outside_stays() {
    let mut wb = workbook();
    formula(&mut wb, 0, 0, 6, "=SUM(B1:C10)"); // inside B:C
    formula(&mut wb, 0, 1, 6, "=SUM(A1:C10)"); // reaches column A
    formula(&mut wb, 0, 2, 6, "=SUM(B4:C6)"); // wholly below the insertion
    formula(&mut wb, 0, 3, 6, "=SUM(B:B)"); // whole column
    apply(&mut wb, shift(range(2, 1, 2, 2), CellShift::Down)).unwrap(); // B3:C3
    assert_eq!(text_at(&wb, 0, 0, 6), "SUM(B1:C11)");
    assert_eq!(text_at(&wb, 0, 1, 6), "SUM(A1:C10)");
    assert_eq!(text_at(&wb, 0, 2, 6), "SUM(B5:C7)");
    assert_eq!(text_at(&wb, 0, 3, 6), "SUM(B:B)");
}

#[test]
fn delete_shift_up_turns_a_reference_into_the_block_to_ref_and_pulls_the_rest_up() {
    let mut wb = workbook();
    num(&mut wb, 0, 5, 1, 9.0); // B6
    formula(&mut wb, 0, 0, 4, "=B2"); // E1 — B2 is deleted
    formula(&mut wb, 0, 1, 4, "=B6"); // E2 — moves up
    formula(&mut wb, 0, 2, 4, "=SUM(B1:B10)"); // shrinks
    formula(&mut wb, 0, 3, 4, "=A6"); // column A does not move
    apply(&mut wb, shift(range(1, 1, 2, 1), CellShift::Up)).unwrap(); // B2:B3
    assert_eq!(text_at(&wb, 0, 0, 4), "#REF!");
    assert_eq!(text_at(&wb, 0, 1, 4), "B4");
    assert_eq!(text_at(&wb, 0, 2, 4), "SUM(B1:B8)");
    assert_eq!(text_at(&wb, 0, 3, 4), "A6");
    assert_eq!(
        value(&wb, 0, 3, 1),
        CellValue::Number(9.0),
        "B6 moved to B4"
    );
}

#[test]
fn shift_right_and_left_work_on_the_other_axis() {
    let mut wb = workbook();
    num(&mut wb, 0, 1, 3, 4.0); // D2
    formula(&mut wb, 0, 5, 0, "=D2+D3"); // A6 — D3 is outside the block's row
    apply(&mut wb, shift(range(1, 1, 1, 2), CellShift::Right)).unwrap(); // B2:C2
    assert_eq!(
        value(&wb, 0, 1, 5),
        CellValue::Number(4.0),
        "D2 moved to F2"
    );
    assert_eq!(text_at(&wb, 0, 5, 0), "F2+D3");
    apply(&mut wb, shift(range(1, 1, 1, 2), CellShift::Left)).unwrap();
    assert_eq!(text_at(&wb, 0, 5, 0), "D2+D3");
    assert_eq!(value(&wb, 0, 1, 3), CellValue::Number(4.0));
}

#[test]
fn references_from_other_sheets_and_names_follow() {
    let mut wb = workbook();
    formula(&mut wb, 1, 0, 0, "=S!B2+S!A2"); // on T
    wb.defined_names.push(DefinedName {
        name: "Pick".to_owned(),
        sheet: None,
        formula: parse("S!$B$2").unwrap(),
        comment: None,
        attributes: Default::default(),
    });
    apply(&mut wb, shift(range(1, 1, 1, 1), CellShift::Down)).unwrap();
    assert_eq!(text_at(&wb, 1, 0, 0), "S!B3+S!A2");
    assert_eq!(wb.defined_names[0].formula.to_string(), "S!$B$3");
}

#[test]
fn a_formula_inside_the_moved_block_keeps_naming_the_same_cells() {
    let mut wb = workbook();
    num(&mut wb, 0, 0, 0, 1.0); // A1
    formula(&mut wb, 0, 3, 1, "=A1+B5"); // B4 moves; A1 does not, B5 does
    apply(&mut wb, shift(range(1, 1, 1, 1), CellShift::Down)).unwrap();
    assert_eq!(text_at(&wb, 0, 4, 1), "A1+B6");
}

#[test]
fn notes_and_rules_inside_the_columns_move_and_outside_stay() {
    let mut wb = workbook();
    wb.sheets[0]
        .comments
        .push(CellComment::note(CellRef::new(4, 1), "in", None));
    wb.sheets[0]
        .comments
        .push(CellComment::note(CellRef::new(4, 0), "out", None));
    let rule = |r: CellRange| -> ConditionalFormat {
        serde_json::from_value(serde_json::json!({
            "range": r, "rule": {"greaterThan": 1.0}, "fill": "FF0000",
        }))
        .unwrap()
    };
    wb.sheets[0]
        .conditional_formats
        .push(rule(range(3, 1, 6, 1)));
    wb.sheets[0]
        .conditional_formats
        .push(rule(range(3, 0, 6, 1)));
    apply(&mut wb, shift(range(1, 1, 1, 1), CellShift::Down)).unwrap();
    assert_eq!(wb.sheets[0].comments[0].at, CellRef::new(5, 1));
    assert_eq!(wb.sheets[0].comments[1].at, CellRef::new(4, 0));
    assert_eq!(wb.sheets[0].conditional_formats[0].range, range(4, 1, 7, 1));
    assert_eq!(wb.sheets[0].conditional_formats[1].range, range(3, 0, 6, 1));
}

#[test]
fn a_shift_that_would_split_a_merge_is_refused() {
    let mut wb = workbook();
    wb.sheets[0].merges.push(range(4, 0, 4, 2)); // A5:C5
    let before = wb.clone();
    let err = apply(&mut wb, shift(range(1, 1, 1, 1), CellShift::Down)).unwrap_err();
    assert!(matches!(err, TxnError::WouldSplit { .. }), "{err}");
    assert_eq!(wb, before);
    // A merge wholly inside the columns moves with them.
    let mut wb = workbook();
    wb.sheets[0].merges.push(range(4, 1, 5, 1));
    apply(&mut wb, shift(range(1, 1, 1, 1), CellShift::Down)).unwrap();
    assert_eq!(wb.sheets[0].merges[0], range(5, 1, 6, 1));
}

#[test]
fn undo_puts_every_reference_back() {
    let mut wb = workbook();
    num(&mut wb, 0, 1, 1, 5.0);
    num(&mut wb, 0, 5, 1, 6.0);
    formula(&mut wb, 0, 0, 4, "=B2");
    formula(&mut wb, 0, 1, 4, "=SUM(B1:B10)");
    formula(&mut wb, 1, 0, 0, "=S!B6");
    for direction in [CellShift::Down, CellShift::Up] {
        let before = wb.clone();
        let inverse = apply(&mut wb, shift(range(1, 1, 2, 1), direction)).unwrap();
        apply(&mut wb, inverse).unwrap();
        for (sheet, row, col) in [(0, 0, 4), (0, 1, 4), (1, 0, 0)] {
            assert_eq!(
                text_at(&wb, sheet, row, col),
                text_at(&before, sheet, row, col),
                "{direction:?} undo"
            );
        }
        assert_eq!(value(&wb, 0, 1, 1), CellValue::Number(5.0));
        assert_eq!(value(&wb, 0, 5, 1), CellValue::Number(6.0));
    }
}

#[test]
fn shifting_down_into_the_edge_of_the_grid_is_refused() {
    let mut wb = workbook();
    num(&mut wb, 0, casual_calc_model::GRID_MAX_ROW, 1, 1.0);
    let err = apply(&mut wb, shift(range(0, 1, 0, 1), CellShift::Down)).unwrap_err();
    assert!(matches!(err, TxnError::OffGrid { .. }), "{err}");
    // Another column's edge is not in the way.
    apply(&mut wb, shift(range(0, 2, 0, 2), CellShift::Down)).unwrap();
}
