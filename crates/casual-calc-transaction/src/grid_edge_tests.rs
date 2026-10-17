//! `XL-FEAT-126`: an insert that would push content off the grid is refused.
//!
//! Excel refuses such an insert with "Microsoft Excel can't insert new cells
//! because it would push non-empty cells off the end of the worksheet. These
//! non-empty cells might appear empty but have blank values, some formatting,
//! or a formula." — so a cell counts when it holds a value, a formula **or a
//! style**. What was refused here before is that nothing was: the insert
//! succeeded and the last row's content landed on row 1,048,577.

use casual_calc_formula::parse;
use casual_calc_formula::stored::Origin;
use casual_calc_model::{
    Cell, CellComment, CellRange, CellRef, CellValue, ConditionalFormat, DataValidation,
    GRID_MAX_COL, GRID_MAX_ROW, Id, Sheet, SheetId, StyleId, Workbook,
};

use crate::{Operation, TxnError, apply};

fn workbook() -> Workbook {
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "S"));
    wb
}

fn num(wb: &mut Workbook, row: u32, col: u32, n: f64) {
    wb.sheets[0]
        .cells
        .set(CellRef::new(row, col), Cell::value(CellValue::Number(n)));
}

fn formula(wb: &mut Workbook, row: u32, col: u32, text: &str) {
    let handle = wb.store_formula_at(
        parse(text.trim_start_matches('=')).unwrap(),
        Origin::at(row, col),
    );
    let mut cell = Cell::value(CellValue::Empty);
    cell.formula = Some(handle);
    wb.sheets[0].cells.set(CellRef::new(row, col), cell);
}

fn formula_text(wb: &Workbook, row: u32, col: u32) -> String {
    let handle = wb.sheets[0]
        .cells
        .get(CellRef::new(row, col))
        .and_then(|c| c.formula)
        .expect("a formula cell");
    let expr = wb.formula(handle).unwrap();
    casual_calc_formula::restore_at(
        expr,
        Origin::at(row, col),
        casual_calc_formula::stored::ABSOLUTE,
    )
    .to_string()
}

fn rows(at: u32, count: u32) -> Operation {
    Operation::InsertRows {
        sheet: 0,
        at,
        count,
    }
}

fn cols(at: u32, count: u32) -> Operation {
    Operation::InsertColumns {
        sheet: 0,
        at,
        count,
    }
}

fn range(r0: u32, c0: u32, r1: u32, c1: u32) -> CellRange {
    CellRange::new(CellRef::new(r0, c0), CellRef::new(r1, c1))
}

fn assert_off_grid(wb: &mut Workbook, op: Operation) {
    let before = wb.clone();
    let err = apply(wb, op.clone()).expect_err("an insert pushing content off the grid");
    assert!(
        matches!(err, TxnError::OffGrid { .. }),
        "refused for the wrong reason: {err}"
    );
    assert_eq!(err.code(), "OC-TXN-0003");
    assert_eq!(*wb, before, "a refused insert changed the workbook: {op:?}");
}

#[test]
fn a_value_in_the_last_row_refuses_a_row_insert_above_it() {
    let mut wb = workbook();
    num(&mut wb, GRID_MAX_ROW, 0, 7.0);
    assert_off_grid(&mut wb, rows(0, 1));
    // At the last row itself, too: the row on `at` is pushed down.
    assert_off_grid(&mut wb, rows(GRID_MAX_ROW, 1));
}

#[test]
fn a_value_in_the_last_column_refuses_a_column_insert_left_of_it() {
    let mut wb = workbook();
    num(&mut wb, 4, GRID_MAX_COL, 7.0);
    assert_off_grid(&mut wb, cols(2, 3));
}

#[test]
fn a_formula_or_a_style_alone_counts_as_nonblank() {
    let mut wb = workbook();
    formula(&mut wb, GRID_MAX_ROW, 1, "=1+1");
    assert_off_grid(&mut wb, rows(10, 1));

    let mut wb = workbook();
    let mut styled = Cell::value(CellValue::Empty);
    styled.style = Some(StyleId(core::num::NonZeroU32::new(1).unwrap()));
    wb.sheets[0]
        .cells
        .set(CellRef::new(GRID_MAX_ROW, 3), styled);
    assert_off_grid(&mut wb, rows(10, 1));
}

#[test]
fn the_count_decides_how_far_from_the_edge_content_is_at_risk() {
    let mut wb = workbook();
    num(&mut wb, GRID_MAX_ROW - 2, 0, 1.0);
    // Two empty rows below it: two fit, three do not.
    assert_off_grid(&mut wb, rows(0, 3));
    apply(&mut wb, rows(0, 2)).expect("two rows fit");
    assert_eq!(
        wb.sheets[0]
            .cells
            .get(CellRef::new(GRID_MAX_ROW, 0))
            .map(|c| c.value.clone()),
        Some(CellValue::Number(1.0))
    );
    // Content above the insertion point never moves, however large the count.
    let mut wb = workbook();
    num(&mut wb, 5, 0, 1.0);
    apply(&mut wb, rows(10, GRID_MAX_ROW - 10)).expect("nothing below row 10 to push");
}

#[test]
fn a_merge_a_note_or_a_validation_in_the_last_row_refuses_it() {
    let mut wb = workbook();
    wb.sheets[0]
        .merges
        .push(range(GRID_MAX_ROW - 1, 0, GRID_MAX_ROW, 1));
    assert_off_grid(&mut wb, rows(0, 1));

    let mut wb = workbook();
    wb.sheets[0].comments.push(CellComment::note(
        CellRef::new(GRID_MAX_ROW, 2),
        "edge",
        None,
    ));
    assert_off_grid(&mut wb, rows(0, 1));

    let mut wb = workbook();
    let validation: DataValidation = serde_json::from_value(serde_json::json!({
        "range": range(GRID_MAX_ROW, 0, GRID_MAX_ROW, 0),
    }))
    .unwrap();
    wb.sheets[0].validations.push(validation);
    assert_off_grid(&mut wb, rows(0, 1));
}

#[test]
fn a_rule_reaching_the_edge_is_clipped_and_undo_restores_it() {
    // A conditional format over the whole of column A written as a range: Excel
    // keeps it reaching the last row rather than refusing the insert.
    let mut wb = workbook();
    let format: ConditionalFormat = serde_json::from_value(serde_json::json!({
        "range": range(0, 0, GRID_MAX_ROW, 0),
        "rule": {"greaterThan": 1.0},
        "fill": "FF0000",
    }))
    .unwrap();
    wb.sheets[0].conditional_formats.push(format);
    let before = wb.clone();
    let inverse = apply(&mut wb, rows(0, 1)).expect("a rule is not content");
    assert_eq!(
        wb.sheets[0].conditional_formats[0].range,
        range(1, 0, GRID_MAX_ROW, 0),
        "the rule ran off the grid"
    );
    apply(&mut wb, inverse).unwrap();
    assert_eq!(
        wb.sheets[0].conditional_formats[0].range, before.sheets[0].conditional_formats[0].range,
        "undo did not restore the rule's reach"
    );
}

#[test]
fn a_reference_pushed_off_the_grid_is_ref_and_a_range_is_clamped() {
    let mut wb = workbook();
    formula(&mut wb, 0, 1, "=A1048576");
    formula(&mut wb, 0, 2, "=SUM(A2:A1048576)");
    let inverse = apply(&mut wb, rows(0, 1)).unwrap();
    assert_eq!(formula_text(&wb, 1, 1), "#REF!");
    assert_eq!(formula_text(&wb, 1, 2), "SUM(A3:A1048576)");
    apply(&mut wb, inverse).unwrap();
    assert_eq!(
        formula_text(&wb, 0, 1),
        "A1048576",
        "undo lost the reference"
    );
    assert_eq!(
        formula_text(&wb, 0, 2),
        "SUM(A2:A1048576)",
        "undo lost the range"
    );
}
