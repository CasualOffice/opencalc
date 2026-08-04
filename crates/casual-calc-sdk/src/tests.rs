//! SDK facade tests: the full open/edit/recalc/render/save lifecycle.

use casual_calc_model::{CellRef, CellValue, Id, Sheet, SheetId};

use crate::{EditOperation, GridViewport, WorkbookSession};

/// Build a session with A1 = 10 and A2 = A1*2 (a formula cell).
fn session_with_formula() -> WorkbookSession {
    let mut session = WorkbookSession::blank();
    let wb = session.workbook_mut();
    let mut sheet = Sheet::new(SheetId(Id::from_parts(9, 1)), "Sheet1");
    sheet.cells.set(
        CellRef::new(0, 0),
        casual_calc_model::Cell::value(CellValue::Number(10.0)),
    );
    let handle = wb.store_formula(casual_calc_formula::parse("A1*2").unwrap());
    let mut a2 = casual_calc_model::Cell::value(CellValue::Empty);
    a2.formula = Some(handle);
    sheet.cells.set(CellRef::new(1, 0), a2);
    wb.sheets.push(sheet);
    session.recalculate();
    session
}

fn value(session: &WorkbookSession, at: CellRef) -> CellValue {
    session.workbook().sheets[0]
        .cells
        .get(at)
        .map(|c| c.value.clone())
        .unwrap_or(CellValue::Empty)
}

#[test]
fn edit_recalculates_and_undo_redo_works() {
    let mut session = session_with_formula();
    assert_eq!(value(&session, CellRef::new(1, 0)), CellValue::Number(20.0));

    // Edit A1 to 30 → A2 recomputes to 60.
    session
        .edit(EditOperation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(30.0),
        })
        .unwrap();
    assert_eq!(value(&session, CellRef::new(1, 0)), CellValue::Number(60.0));
    assert!(session.can_undo());

    session.undo().unwrap();
    assert_eq!(value(&session, CellRef::new(0, 0)), CellValue::Number(10.0));
    assert_eq!(value(&session, CellRef::new(1, 0)), CellValue::Number(20.0));
    assert!(session.can_redo());

    session.redo().unwrap();
    assert_eq!(value(&session, CellRef::new(1, 0)), CellValue::Number(60.0));
}

#[test]
fn save_then_open_round_trips_and_recalculates() {
    let session = session_with_formula();
    let bytes = session.save().unwrap();

    let reopened = WorkbookSession::open(bytes).unwrap();
    // The formula recomputes on open.
    assert_eq!(
        value(&reopened, CellRef::new(1, 0)),
        CellValue::Number(20.0)
    );
    assert_eq!(reopened.workbook().sheets[0].name, "Sheet1");
}

#[test]
fn render_produces_a_png() {
    let session = session_with_formula();
    let viewport = GridViewport {
        x: 0,
        y: 0,
        width: 4 * 960,
        height: 4 * 300,
    };
    let png = session.render_png(0, &viewport, 96).unwrap();
    assert_eq!(&png[0..4], &[0x89, b'P', b'N', b'G']);
}

#[test]
fn blank_session_saves_and_reopens_empty() {
    let session = WorkbookSession::blank();
    let bytes = session.save().unwrap();
    let reopened = WorkbookSession::open(bytes).unwrap();
    assert!(reopened.workbook().sheets.is_empty());
}
