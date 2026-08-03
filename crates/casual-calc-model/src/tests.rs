//! Model + snapshot tests. The empty-workbook byte-stable round-trip is the
//! Phase 0 exit-gate condition (`docs/06-ROADMAP-AND-DELIVERY.md`).

use crate::{
    Cell, CellRef, CellValue, Id, IdGenerator, SCHEMA_VERSION, Sheet, SheetId, StringId, Workbook,
};

fn wb_id() -> Id {
    Id::from_parts(1, 1)
}

#[test]
fn id_is_nonzero_and_hex_roundtrips() {
    assert!(Id::new(0).is_none());
    let id = Id::from_parts(0xABCD, 0x1234);
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"000000000000abcd0000000000001234\"");
    let back: Id = serde_json::from_str(&json).unwrap();
    assert_eq!(id, back);
}

#[test]
fn id_generator_produces_unique_nonzero_ids() {
    let mut generator = IdGenerator::new(7);
    let a = generator.next_id();
    let b = generator.next_id();
    assert_ne!(a, b);
    assert_ne!(a.get(), 0);
}

#[test]
fn empty_workbook_snapshot_is_byte_stable() {
    let wb = Workbook::new(wb_id());
    let first = wb.to_snapshot().unwrap();
    let reopened = Workbook::from_snapshot(&first).unwrap();
    assert_eq!(wb, reopened);
    let second = reopened.to_snapshot().unwrap();
    assert_eq!(
        first, second,
        "snapshot must be byte-identical across a round-trip"
    );
    // The empty workbook omits its empty `sheets` vec.
    assert_eq!(
        String::from_utf8(first).unwrap(),
        r#"{"schemaVersion":0,"workbookId":"00000000000000010000000000000001"}"#
    );
}

#[test]
fn populated_workbook_roundtrips_byte_stably() {
    let mut wb = Workbook::new(wb_id());
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "Sheet1");
    sheet
        .cells
        .set(CellRef::new(0, 0), Cell::value(CellValue::Number(42.0)));
    sheet.cells.set(
        CellRef::new(3, 1),
        Cell::value(CellValue::SharedString(StringId(Id::from_parts(9, 5)))),
    );
    wb.sheets.push(sheet);

    let first = wb.to_snapshot().unwrap();
    let reopened = Workbook::from_snapshot(&first).unwrap();
    assert_eq!(wb, reopened);
    let second = reopened.to_snapshot().unwrap();
    assert_eq!(first, second);
    assert_eq!(reopened.schema_version, SCHEMA_VERSION);
}

#[test]
fn blank_cells_are_not_stored() {
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    sheet
        .cells
        .set(CellRef::new(0, 0), Cell::value(CellValue::Number(1.0)));
    assert_eq!(sheet.cells.len(), 1);
    // Overwriting with a blank cell evicts it.
    sheet.cells.set(CellRef::new(0, 0), Cell::default());
    assert_eq!(sheet.cells.len(), 0);
    assert!(sheet.cells.is_empty());
}

#[test]
fn cells_iterate_in_row_major_order() {
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    sheet
        .cells
        .set(CellRef::new(5, 0), Cell::value(CellValue::Number(1.0)));
    sheet
        .cells
        .set(CellRef::new(0, 9), Cell::value(CellValue::Number(2.0)));
    sheet
        .cells
        .set(CellRef::new(0, 1), Cell::value(CellValue::Number(3.0)));
    let order: Vec<CellRef> = sheet.cells.iter().map(|(r, _)| r).collect();
    assert_eq!(
        order,
        vec![CellRef::new(0, 1), CellRef::new(0, 9), CellRef::new(5, 0)]
    );
}

#[test]
fn duplicate_sheet_ids_are_rejected() {
    let mut wb = Workbook::new(wb_id());
    let dup = SheetId(Id::from_parts(2, 1));
    wb.sheets.push(Sheet::new(dup, "A"));
    wb.sheets.push(Sheet::new(dup, "B"));
    let err = wb.validate().unwrap_err();
    assert_eq!(err.code(), "OC-MDL-0001");
}

#[test]
fn unknown_snapshot_fields_are_rejected() {
    let bytes = br#"{"schemaVersion":0,"workbookId":"00000000000000010000000000000001","bogus":1}"#;
    assert!(Workbook::from_snapshot(bytes).is_err());
}
