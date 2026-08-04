//! Delimited-format tests: field typing, RFC 4180 quoting, and the
//! parse → write → parse round-trip fixed point.

use casual_calc_model::{CellRef, CellValue, Workbook};

use crate::{COMMA, PIPE, TAB, read_delimited, write_delimited};

fn value(wb: &Workbook, r: u32, c: u32) -> CellValue {
    wb.sheets[0]
        .cells
        .get(CellRef::new(r, c))
        .map(|cell| cell.value.clone())
        .unwrap_or(CellValue::Empty)
}

/// The resolved text of a string cell (panics if it isn't a string).
fn text(wb: &Workbook, r: u32, c: u32) -> String {
    match value(wb, r, c) {
        CellValue::SharedString(id) | CellValue::InlineString(id) => {
            wb.strings.get(id).unwrap().to_owned()
        }
        other => panic!("expected a string cell, got {other:?}"),
    }
}

#[test]
fn fields_are_typed() {
    let wb = read_delimited(b"Item,Qty,Flag\nWidget,3,TRUE\nGadget,4.5,false", COMMA).unwrap();
    assert_eq!(text(&wb, 0, 0), "Item");
    assert_eq!(value(&wb, 1, 1), CellValue::Number(3.0));
    assert_eq!(value(&wb, 1, 2), CellValue::Bool(true));
    assert_eq!(value(&wb, 2, 1), CellValue::Number(4.5));
    assert_eq!(value(&wb, 2, 2), CellValue::Bool(false));
}

#[test]
fn quoted_fields_round_trip() {
    // Fields with the delimiter, a quote, and a newline must survive.
    let src = "a,\"b,c\",\"quote\"\"d\"\r\n\"line\nbreak\",e,f\r\n";
    let wb = read_delimited(src.as_bytes(), COMMA).unwrap();
    assert_eq!(text(&wb, 0, 1), "b,c");
    assert_eq!(text(&wb, 0, 2), "quote\"d");
    assert_eq!(text(&wb, 1, 0), "line\nbreak");

    let written = write_delimited(&wb, 0, COMMA);
    let reparsed = read_delimited(written.as_bytes(), COMMA).unwrap();
    assert_eq!(
        wb, reparsed,
        "parse -> write -> parse must be a fixed point"
    );
}

#[test]
fn tsv_and_psv_delimiters() {
    let tsv = read_delimited(b"x\ty\n1\t2", TAB).unwrap();
    assert_eq!(value(&tsv, 1, 0), CellValue::Number(1.0));
    assert_eq!(value(&tsv, 1, 1), CellValue::Number(2.0));

    let psv = read_delimited(b"x|y\n1|2", PIPE).unwrap();
    assert_eq!(value(&psv, 1, 1), CellValue::Number(2.0));
    // A comma inside a pipe-delimited field is ordinary text, not a separator.
    let psv2 = read_delimited(b"a,b|c", PIPE).unwrap();
    assert_eq!(text(&psv2, 0, 0), "a,b");
}

#[test]
fn round_trip_fixed_point_mixed() {
    let src = "Item,Qty,Price\r\nWidget,3,4.5\r\nGadget,5,2\r\n,,\r\n";
    let wb = read_delimited(src.as_bytes(), COMMA).unwrap();
    let written = write_delimited(&wb, 0, COMMA);
    let reparsed = read_delimited(written.as_bytes(), COMMA).unwrap();
    assert_eq!(wb, reparsed);
}

#[test]
fn empty_input_yields_empty_sheet() {
    let wb = read_delimited(b"", COMMA).unwrap();
    assert!(wb.sheets[0].cells.is_empty());
    assert_eq!(write_delimited(&wb, 0, COMMA), "");
}
