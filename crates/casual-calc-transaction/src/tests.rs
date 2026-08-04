//! Transaction tests: inverses, atomic batches, undo/redo, and edit→recalc.

use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Style, Workbook};

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
