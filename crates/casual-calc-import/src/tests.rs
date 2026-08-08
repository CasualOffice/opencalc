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

fn sheet_with_extra(cells: &str, merges: &str, views: &str) -> Vec<u8> {
    format!(
        "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">{views}<sheetData><row r=\"1\">{cells}</row></sheetData>{merges}</worksheet>"
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
fn imports_merges_frozen_panes_and_defined_names() {
    const WORKBOOK_DN: &[u8] = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets><definedNames><definedName name="Total">Data!$A$1:$A$3</definedName><definedName name="Local" localSheetId="0">Data!$B$1</definedName></definedNames></workbook>"#;
    let sheet_xml = sheet_with_extra(
        r#"<c r="A1"><v>1</v></c>"#,
        r#"<mergeCells count="1"><mergeCell ref="A1:B2"/></mergeCells>"#,
        r#"<sheetViews><sheetView><pane xSplit="1" ySplit="2" topLeftCell="B3" state="frozen"/></sheetView></sheetViews>"#,
    );
    let bytes = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK_DN),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/worksheets/sheet1.xml", &sheet_xml),
    ]);
    let import = import_package(bytes).unwrap();
    let wb = &import.workbook;
    let sheet = &wb.sheets[0];

    // Merged range A1:B2.
    assert_eq!(sheet.merges.len(), 1);
    assert_eq!(sheet.merges[0].start, CellRef::new(0, 0));
    assert_eq!(sheet.merges[0].end, CellRef::new(1, 1));

    // Frozen: ySplit=2 rows, xSplit=1 col.
    assert_eq!(sheet.view.frozen_rows, 2);
    assert_eq!(sheet.view.frozen_cols, 1);

    // Defined names: one workbook-scoped, one sheet-scoped.
    assert_eq!(wb.defined_names.len(), 2);
    let total = wb.defined_names.iter().find(|d| d.name == "Total").unwrap();
    assert!(total.sheet.is_none());
    let local = wb.defined_names.iter().find(|d| d.name == "Local").unwrap();
    assert_eq!(local.sheet, Some(sheet.id));
}

#[test]
fn imports_outline_levels_collapsed_and_zoom() {
    let sheet_xml = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
        <sheetPr><outlinePr summaryBelow="0"/></sheetPr>
        <sheetViews><sheetView zoomScale="150"><pane xSplit="0" ySplit="0" state="split"/></sheetView></sheetViews>
        <cols><col min="3" max="4" outlineLevel="1"/><col min="5" max="5" outlineLevel="2" collapsed="1"/></cols>
        <sheetData>
          <row r="1" outlineLevel="1"><c r="A1"><v>1</v></c></row>
          <row r="2" outlineLevel="2" collapsed="1"><c r="A2"><v>2</v></c></row>
        </sheetData>
    </worksheet>"#
        .to_vec();
    let bytes = package_with_sheet(sheet_xml, None);
    let import = import_package(bytes).unwrap();
    let sheet = &import.workbook.sheets[0];

    // Rows: outline levels and the collapsed flag (zero-based).
    assert_eq!(sheet.row_outline_levels.get(&0), Some(&1));
    assert_eq!(sheet.row_outline_levels.get(&1), Some(&2));
    assert!(sheet.collapsed_rows.contains(&1));
    assert!(!sheet.collapsed_rows.contains(&0));

    // Columns: the span 3..4 (zero-based 2..3) is level 1; column 5 (index 4) is
    // level 2 and collapsed.
    assert_eq!(sheet.col_outline_levels.get(&2), Some(&1));
    assert_eq!(sheet.col_outline_levels.get(&3), Some(&1));
    assert_eq!(sheet.col_outline_levels.get(&4), Some(&2));
    assert!(sheet.collapsed_cols.contains(&4));

    // outlinePr summary flag and the view zoom.
    assert!(!sheet.outline.summary_below);
    assert!(sheet.outline.summary_right);
    assert_eq!(sheet.view.zoom, 150);
}

#[test]
fn theme_and_indexed_colors_resolve_to_rgb() {
    // Excel's built-in cell styles state colours as a theme slot plus a tint,
    // never as literal rgb — reading only `rgb` dropped all of them.
    const THEME: &[u8] = br#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:clrScheme name="Office">
        <a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
        <a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
        <a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2>
        <a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2>
        <a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4>
        <a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6>
        <a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink>
    </a:clrScheme></a:themeElements></a:theme>"#;
    const STYLES: &[u8] = br#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
        <fonts count="2"><font><sz val="11"/><name val="Calibri"/></font>
                         <font><color theme="4"/><name val="Calibri"/></font></fonts>
        <fills count="3"><fill><patternFill patternType="none"/></fill>
                         <fill><patternFill patternType="gray125"/></fill>
                         <fill><patternFill patternType="solid"><fgColor theme="0" tint="-0.15"/></patternFill></fill></fills>
        <borders count="1"><border/></borders>
        <cellXfs count="3">
          <xf numFmtId="0" fontId="0" fillId="0"/>
          <xf numFmtId="0" fontId="1" fillId="0" applyFont="1"/>
          <xf numFmtId="0" fontId="0" fillId="2" applyFill="1"/>
        </cellXfs>
    </styleSheet>"#;
    let sheet_xml = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>
        <row r="1"><c r="A1" s="1" t="inlineStr"><is><t>accent1 text</t></is></c>
                   <c r="B1" s="2" t="inlineStr"><is><t>shaded fill</t></is></c></row>
    </sheetData></worksheet>"#
        .to_vec();
    let bytes = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/theme/theme1.xml", THEME),
        ("xl/styles.xml", STYLES),
        ("xl/worksheets/sheet1.xml", &sheet_xml),
    ]);
    let import = import_package(bytes).unwrap();
    let wb = &import.workbook;
    let sheet = &wb.sheets[0];
    let style_of = |col: u32| {
        let id = sheet.cells.get(CellRef::new(0, col)).unwrap().style.unwrap();
        wb.styles.get(id).unwrap().clone()
    };
    // theme="4" is accent1 …
    assert_eq!(style_of(0).font_color.as_deref(), Some("4472C4"));
    // … and theme="0" tint="-0.15" is white darkened 15%.
    assert_eq!(style_of(1).fill_color.as_deref(), Some("D9D9D9"));
}

#[test]
fn shared_formula_followers_are_expanded_from_their_master() {
    // Excel's fill-down writes the expression once (on the master, which also
    // carries ref+si) and leaves each follower's <f> empty. Followers used to
    // import as a cached constant with no formula at all.
    let sheet_xml = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>
        <row r="1"><c r="A1"><v>10</v></c><c r="B1"><f t="shared" ref="B1:B3" si="0">A1*$D$1+$A2</f><v>10</v></c></row>
        <row r="2"><c r="A2"><v>20</v></c><c r="B2"><f t="shared" si="0"/><v>20</v></c></row>
        <row r="3"><c r="A3"><v>30</v></c><c r="B3"><f t="shared" si="0"/><v>30</v></c></row>
    </sheetData></worksheet>"#
        .to_vec();
    let import = import_package(package_with_sheet(sheet_xml, None)).unwrap();
    let wb = &import.workbook;
    let sheet = &wb.sheets[0];

    let formula_at = |row: u32| {
        let cell = sheet.cells.get(CellRef::new(row, 1)).unwrap();
        let handle = cell.formula.expect("follower kept its formula");
        wb.formula(handle).unwrap().to_string()
    };
    // The master is unchanged; each follower is shifted by its row delta, and
    // the `$`-anchored parts stay put ($D$1 entirely, $A2's column only).
    assert_eq!(formula_at(0), "((A1*$D$1)+$A2)");
    assert_eq!(formula_at(1), "((A2*$D$1)+$A3)");
    assert_eq!(formula_at(2), "((A3*$D$1)+$A4)");
}

#[test]
fn multi_area_sqref_applies_to_every_area() {
    let sheet_xml = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
        <sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData>
        <dataValidations>
          <dataValidation type="list" sqref="A1:A3 C1:C3"><formula1>"yes,no"</formula1></dataValidation>
        </dataValidations>
    </worksheet>"#
        .to_vec();
    let import = import_package(package_with_sheet(sheet_xml, None)).unwrap();
    let sheet = &import.workbook.sheets[0];
    // One validation per area, not just the first.
    assert_eq!(sheet.validations.len(), 2);
    assert_eq!(sheet.validations[0].range.start.col, 0);
    assert_eq!(sheet.validations[1].range.start.col, 2);
}

#[test]
fn column_widths_survive_the_true_spelling_of_ooxml_booleans() {
    // LibreOffice, Apache POI and ExcelJS all write `customWidth="true"` rather
    // than `="1"`. Both are valid `xsd:boolean`; matching only "1" silently
    // dropped every column width, row height and hidden flag in those files.
    let sheet_xml = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
        <cols>
          <col min="1" max="1" width="30" customWidth="true"/>
          <col min="2" max="2" width="12" customWidth="1"/>
          <col min="3" max="3" width="9" hidden="true"/>
          <col min="4" max="4" width="18"/>
        </cols>
        <sheetData>
          <row r="1" ht="42" customHeight="true"><c r="A1"><v>1</v></c></row>
          <row r="2" hidden="true"><c r="A2"><v>2</v></c></row>
        </sheetData>
    </worksheet>"#
        .to_vec();
    let import = import_package(package_with_sheet(sheet_xml, None)).unwrap();
    let sheet = &import.workbook.sheets[0];

    // Every width is kept, whichever spelling declared it — and a width with no
    // customWidth attribute at all is still authoritative for its column.
    let w = |c: u32| sheet.columns.sizes.get(&c).copied();
    assert_eq!(w(0), Some(super::read::col_width_to_twips(30.0)));
    assert_eq!(w(1), Some(super::read::col_width_to_twips(12.0)));
    assert_eq!(w(3), Some(super::read::col_width_to_twips(18.0)));
    assert!(sheet.hidden_cols.contains(&2));
    assert!(sheet.hidden_rows.contains(&1));
    assert!(sheet.rows.sizes.contains_key(&0));
}

#[test]
fn imports_cell_indent_from_alignment() {
    const STYLES: &[u8] = br#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
        <cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="0" applyAlignment="1"><alignment horizontal="left" indent="2"/></xf></cellXfs>
    </styleSheet>"#;
    let sheet_xml = sheet_with(r#"<row r="1"><c r="A1" s="1"><v>7</v></c></row>"#);
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
    let style_id = wb.sheets[0]
        .cells
        .get(CellRef::new(0, 0))
        .unwrap()
        .style
        .unwrap();
    let style = wb.styles.get(style_id).unwrap();
    assert_eq!(style.indent, 2);
}

#[test]
fn imports_fonts_and_fills_from_styles() {
    const STYLES: &[u8] = br#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
        <fonts count="2"><font/><font><b/><color rgb="FFFF0000"/></font></fonts>
        <fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/></patternFill></fill></fills>
        <cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="0" fontId="1" fillId="2"/></cellXfs>
    </styleSheet>"#;
    let sheet_xml = sheet_with(r#"<row r="1"><c r="A1" s="1"><v>7</v></c></row>"#);
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
    let style_id = wb.sheets[0]
        .cells
        .get(CellRef::new(0, 0))
        .unwrap()
        .style
        .unwrap();
    let style = wb.styles.get(style_id).unwrap();
    assert!(style.bold);
    assert_eq!(style.font_color.as_deref(), Some("FF0000"));
    assert_eq!(style.fill_color.as_deref(), Some("FFFF00"));
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
