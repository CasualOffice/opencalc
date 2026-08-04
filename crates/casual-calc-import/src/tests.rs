//! Import tests over in-memory `.xlsx` packages.

use std::io::{Cursor, Write};

use casual_calc_model::{CellRef, CellValue, ErrorValue};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::{ModelOutcome, import_package};

fn zip_parts(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in parts {
        writer.start_file(*name, opts).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

const CONTENT_TYPES: &[u8] =
    b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>";
const ROOT_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
const WORKBOOK: &[u8] = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
const WORKBOOK_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
const SHARED: &[u8] = br#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>Alpha</t></si><si><t>Beta</t></si></sst>"#;

fn sheet_with(rows: &str) -> Vec<u8> {
    format!(
        "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>{rows}</sheetData></worksheet>"
    )
    .into_bytes()
}

fn package_with_sheet(sheet_xml: Vec<u8>, shared: Option<&[u8]>) -> Vec<u8> {
    let mut parts: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
    ];
    if let Some(s) = shared {
        parts.push(("xl/sharedStrings.xml", s));
    }
    parts.push(("xl/worksheets/sheet1.xml", &sheet_xml));
    zip_parts(&parts)
}

#[test]
fn imports_values_and_shared_strings() {
    let rows = r#"<row r="1">
        <c r="A1" t="n"><v>42</v></c>
        <c r="B1"><v>3.5</v></c>
        <c r="C1" t="b"><v>1</v></c>
        <c r="D1" t="s"><v>0</v></c>
        <c r="E1" t="s"><v>1</v></c>
        <c r="F1" t="inlineStr"><is><t>Inline</t></is></c>
        <c r="G1" t="e"><v>#DIV/0!</v></c>
    </row>"#;
    let bytes = package_with_sheet(sheet_with(rows), Some(SHARED));
    let import = import_package(bytes).unwrap();
    let wb = &import.workbook;

    assert_eq!(wb.sheets.len(), 1);
    let sheet = &wb.sheets[0];
    assert_eq!(sheet.name, "Data");

    let num = sheet.cells.get(CellRef::new(0, 0)).unwrap();
    assert_eq!(num.value, CellValue::Number(42.0));
    assert_eq!(
        sheet.cells.get(CellRef::new(0, 1)).unwrap().value,
        CellValue::Number(3.5)
    );
    assert_eq!(
        sheet.cells.get(CellRef::new(0, 2)).unwrap().value,
        CellValue::Bool(true)
    );

    // Shared strings resolve to interned text.
    let d1 = sheet.cells.get(CellRef::new(0, 3)).unwrap();
    let CellValue::SharedString(id) = d1.value else {
        panic!("expected shared string");
    };
    assert_eq!(wb.strings.get(id), Some("Alpha"));
    let e1 = sheet.cells.get(CellRef::new(0, 4)).unwrap();
    let CellValue::SharedString(id) = e1.value else {
        panic!("expected shared string");
    };
    assert_eq!(wb.strings.get(id), Some("Beta"));

    // Inline string.
    let f1 = sheet.cells.get(CellRef::new(0, 5)).unwrap();
    let CellValue::InlineString(id) = f1.value else {
        panic!("expected inline string");
    };
    assert_eq!(wb.strings.get(id), Some("Inline"));

    // Error value.
    assert_eq!(
        sheet.cells.get(CellRef::new(0, 6)).unwrap().value,
        CellValue::Error(ErrorValue::Div0)
    );
}

#[test]
fn empty_cells_are_not_stored() {
    let rows = r#"<row r="1"><c r="A1"/><c r="B1" t="n"><v>7</v></c></row>"#;
    let bytes = package_with_sheet(sheet_with(rows), None);
    let import = import_package(bytes).unwrap();
    let sheet = &import.workbook.sheets[0];
    assert!(sheet.cells.get(CellRef::new(0, 0)).is_none());
    assert_eq!(sheet.cells.len(), 1);
}

#[test]
fn formula_cells_are_parsed_to_ast_with_cached_value() {
    let rows = r#"<row r="1"><c r="A1"><v>10</v></c><c r="A2"><f>A1*2</f><v>20</v></c></row>"#;
    let bytes = package_with_sheet(sheet_with(rows), None);
    let import = import_package(bytes).unwrap();
    let wb = &import.workbook;
    let cell = wb.sheets[0].cells.get(CellRef::new(1, 0)).unwrap();

    // Cached value preserved.
    assert_eq!(cell.value, CellValue::Number(20.0));

    // Formula parsed into the arena and referenced by the cell.
    let handle = cell.formula.expect("cell has a formula handle");
    let expr = wb.formula(handle).expect("handle resolves in the arena");
    assert_eq!(expr.to_string(), "(A1*2)");

    // Reported as mapped.
    let formula_entry = import
        .report
        .entries()
        .into_iter()
        .find(|e| e.feature == "f")
        .expect("formula disposition recorded");
    assert_eq!(formula_entry.model, ModelOutcome::Mapped);
    assert_eq!(formula_entry.count, 1);
}

#[test]
fn unparseable_formula_is_degraded_but_keeps_value() {
    let rows = r#"<row r="1"><c r="A1"><f>1+</f><v>5</v></c></row>"#;
    let bytes = package_with_sheet(sheet_with(rows), None);
    let import = import_package(bytes).unwrap();
    let cell = import.workbook.sheets[0]
        .cells
        .get(CellRef::new(0, 0))
        .unwrap();
    assert_eq!(cell.value, CellValue::Number(5.0));
    assert!(cell.formula.is_none());
    let entry = import
        .report
        .entries()
        .into_iter()
        .find(|e| e.feature == "f")
        .unwrap();
    assert_eq!(entry.model, ModelOutcome::Degraded);
}

#[test]
fn imports_number_formats_from_styles() {
    const STYLES: &[u8] =
        br#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
        <numFmts count="1"><numFmt numFmtId="164" formatCode="0.00"/></numFmts>
        <cellXfs count="3"><xf numFmtId="0"/><xf numFmtId="164"/><xf numFmtId="9"/></cellXfs>
    </styleSheet>"#;
    let sheet_xml = sheet_with(
        r#"<row r="1"><c r="A1" s="0"><v>1</v></c><c r="B1" s="1"><v>3.14</v></c><c r="C1" s="2"><v>0.5</v></c></row>"#,
    );
    let bytes = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/styles.xml", STYLES),
        ("xl/worksheets/sheet1.xml", &sheet_xml),
    ]);
    let import = import_package(bytes).unwrap();
    let wb = &import.workbook;
    let sheet = &wb.sheets[0];

    // s=0 → General → no style.
    assert!(sheet.cells.get(CellRef::new(0, 0)).unwrap().style.is_none());
    // s=1 → custom 164 → "0.00".
    let b1_style = sheet.cells.get(CellRef::new(0, 1)).unwrap().style.unwrap();
    assert_eq!(
        wb.styles.get(b1_style).unwrap().number_format.as_deref(),
        Some("0.00")
    );
    // s=2 → built-in 9 → "0%".
    let c1_style = sheet.cells.get(CellRef::new(0, 2)).unwrap().style.unwrap();
    assert_eq!(
        wb.styles.get(c1_style).unwrap().number_format.as_deref(),
        Some("0%")
    );
}

#[test]
fn import_is_deterministic() {
    let rows = r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>"#;
    let bytes = package_with_sheet(sheet_with(rows), Some(SHARED));
    let first = import_package(bytes.clone())
        .unwrap()
        .workbook
        .to_snapshot()
        .unwrap();
    let second = import_package(bytes)
        .unwrap()
        .workbook
        .to_snapshot()
        .unwrap();
    assert_eq!(first, second);
}
