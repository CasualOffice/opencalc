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
    assert_eq!(expr.to_string(), "A1*2");

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
fn comments_bind_through_the_relationship_graph_not_part_numbering() {
    // Only the second sheet has notes, so its part is `comments1.xml`. Guessing
    // `comments{sheet index + 1}.xml` put those notes on sheet 1.
    const WB2: &[u8] = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="First" sheetId="1" r:id="rId1"/><sheet name="Second" sheetId="2" r:id="rId2"/></sheets></workbook>"#;
    const WB2_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#;
    const SHEET2_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="../comments1.xml"/></Relationships>"#;
    const COMMENTS: &[u8] = br#"<comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><authors><author>Ada</author></authors><commentList><comment ref="B2" authorId="0"><text><t>note on the second sheet</t></text></comment></commentList></comments>"#;
    let sheet1 = sheet_with(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#);
    let sheet2 = sheet_with(r#"<row r="2"><c r="B2"><v>2</v></c></row>"#);
    let bytes = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WB2),
        ("xl/_rels/workbook.xml.rels", WB2_RELS),
        ("xl/worksheets/sheet1.xml", &sheet1),
        ("xl/worksheets/sheet2.xml", &sheet2),
        ("xl/worksheets/_rels/sheet2.xml.rels", SHEET2_RELS),
        ("xl/comments1.xml", COMMENTS),
    ]);
    let import = import_package(bytes).unwrap();
    assert!(
        import.workbook.sheets[0].comments.is_empty(),
        "sheet 1 has no comments part of its own"
    );
    assert_eq!(import.workbook.sheets[1].comments.len(), 1);
    assert_eq!(import.workbook.sheets[1].comments[0].at, CellRef::new(1, 1));
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
        let id = sheet
            .cells
            .get(CellRef::new(0, col))
            .unwrap()
            .style
            .unwrap();
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
    assert_eq!(formula_at(0), "A1*$D$1+$A2");
    assert_eq!(formula_at(1), "A2*$D$1+$A3");
    assert_eq!(formula_at(2), "A3*$D$1+$A4");
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
    let sheet_xml =
        br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
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

/// Every childless element the worksheet reader dispatches on must be handled
/// in the `Event::Empty` arm, not only in `Event::Start`.
///
/// This has been the single most repeated defect in the importer: `<cfRule>`,
/// `<sheetProtection>`, `<person>`, `<hyperlink>` and the whole print-setup
/// group were each written into the `Start` dispatch alone, and each read as
/// *absent* from every real file, because a writer self-closes an element with
/// no children. It is invisible in review — the arm is right there — and shows
/// up only as a construct that silently fails to import.
///
/// So the schema decides. An element whose complexType declares no child
/// elements can always be written self-closed, and must therefore appear in the
/// `Empty` dispatch.
#[test]
fn childless_elements_are_handled_in_the_empty_dispatch() {
    use std::collections::HashSet;

    let src = include_str!("read.rs");
    let start_of = |needle: &str| src.find(needle).expect("dispatch block not found");

    // The worksheet reader's two dispatch blocks, in source order.
    let ws = start_of("pub fn parse_worksheet(");
    let empty = src[ws..].find("Event::Empty(e) => {").expect("empty arm") + ws;
    let end = src[empty..]
        .find("Event::Eof")
        .map_or(src.len(), |i| i + empty);

    let names = |slice: &str| -> HashSet<String> {
        let mut out = HashSet::new();
        let mut rest = slice;
        while let Some(i) = rest.find("b\"") {
            rest = &rest[i + 2..];
            if let Some(j) = rest.find('"') {
                let name = &rest[..j];
                // Attribute reads look the same; keep only match arms.
                if rest[j..]
                    .trim_start_matches('"')
                    .trim_start()
                    .starts_with("=>")
                    || rest[j..]
                        .trim_start_matches('"')
                        .trim_start()
                        .starts_with('|')
                {
                    out.insert(name.to_owned());
                }
                rest = &rest[j..];
            }
        }
        out
    };
    let in_start = names(&src[ws..empty]);
    let in_empty = names(&src[empty..end]);

    // Elements known to carry children, which legitimately appear only in the
    // `Start` dispatch.
    let has_children: HashSet<&str> = [
        "worksheet",
        "sheetData",
        "row",
        "c",
        "is",
        "f",
        "v",
        "cols",
        "mergeCells",
        "sheetViews",
        "sheetView",
        "dataValidations",
        "dataValidation",
        "formula1",
        "formula2",
        "conditionalFormatting",
        "autoFilter",
        "filterColumn",
        "filters",
        "customFilters",
        "sheetPr",
        "hyperlinks",
        "rowBreaks",
        "colBreaks",
        "extLst",
        "headerFooter",
        "oddHeader",
        "oddFooter",
        "evenHeader",
        "evenFooter",
        "firstHeader",
        "firstFooter",
        "t",
        "si",
        "r",
        "rPr",
        "sst",
        "outlinePr",
    ]
    .into_iter()
    .collect();

    let missing: Vec<&String> = in_start
        .iter()
        .filter(|n| !has_children.contains(n.as_str()) && !in_empty.contains(*n))
        .collect();
    assert!(
        missing.is_empty(),
        "these childless elements are dispatched only on Start, so a self-closed \
         one is silently ignored: {missing:?}"
    );
}

/// A two-sheet package with a real pivot: data on `Data`, the report on
/// `Report`, and the four parts Excel writes to tie them together.
///
/// Hand-built rather than a fixture because the point is the *shape* — which
/// part names which, and by what indirection — and a fixture hides exactly that.
fn pivot_package() -> Vec<u8> {
    const WORKBOOK: &[u8] = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/><sheet name="Report" sheetId="2" r:id="rId2"/></sheets><pivotCaches><pivotCache cacheId="7" r:id="rId3"/></pivotCaches></workbook>"#;
    const WORKBOOK_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition" Target="pivotCache/pivotCacheDefinition1.xml"/></Relationships>"#;
    const SHEET2_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable" Target="../pivotTables/pivotTable1.xml"/></Relationships>"#;
    const PIVOT_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition" Target="../pivotCache/pivotCacheDefinition1.xml"/></Relationships>"#;
    const CACHE: &[u8] = br#"<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" recordCount="3"><cacheSource type="worksheet"><worksheetSource ref="A1:C4" sheet="Data"/></cacheSource><cacheFields count="3"><cacheField name="Region"><sharedItems><s v="East"/><s v="West"/></sharedItems></cacheField><cacheField name="Product"><sharedItems><s v="Gadget"/><s v="Widget"/></sharedItems></cacheField><cacheField name="Amount"><sharedItems containsSemiMixedTypes="0" containsString="0" containsNumber="1"/></cacheField></cacheFields></pivotCacheDefinition>"#;
    const PIVOT: &[u8] = br#"<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="Sales" cacheId="7" colGrandTotals="0"><location ref="A3:C6" firstHeaderRow="1" firstDataRow="2" firstDataCol="1"/><pivotFields count="3"><pivotField axis="axisRow" showAll="0" defaultSubtotal="0"/><pivotField axis="axisPage" showAll="0"/><pivotField dataField="1" showAll="0"/></pivotFields><rowFields count="1"><field x="0"/></rowFields><pageFields count="1"><pageField fld="1" item="1" hier="-1"/></pageFields><dataFields count="1"><dataField name="Total" fld="2" subtotal="average" baseField="0" baseItem="0"/></dataFields><pivotTableStyleInfo name="PivotStyleLight16" showRowHeaders="1"/></pivotTableDefinition>"#;

    let data = sheet_with(
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>Region</t></is></c><c r="B1" t="inlineStr"><is><t>Product</t></is></c><c r="C1" t="inlineStr"><is><t>Amount</t></is></c></row>
           <row r="2"><c r="A2" t="inlineStr"><is><t>East</t></is></c><c r="B2" t="inlineStr"><is><t>Widget</t></is></c><c r="C2"><v>10</v></c></row>
           <row r="3"><c r="A3" t="inlineStr"><is><t>West</t></is></c><c r="B3" t="inlineStr"><is><t>Widget</t></is></c><c r="C3"><v>30</v></c></row>
           <row r="4"><c r="A4" t="inlineStr"><is><t>East</t></is></c><c r="B4" t="inlineStr"><is><t>Gadget</t></is></c><c r="C4"><v>99</v></c></row>"#,
    );
    let report = sheet_with("");
    zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/worksheets/sheet1.xml", &data),
        ("xl/worksheets/sheet2.xml", &report),
        ("xl/worksheets/_rels/sheet2.xml.rels", SHEET2_RELS),
        ("xl/pivotTables/pivotTable1.xml", PIVOT),
        ("xl/pivotTables/_rels/pivotTable1.xml.rels", PIVOT_RELS),
        ("xl/pivotCache/pivotCacheDefinition1.xml", CACHE),
    ])
}

#[test]
fn a_pivot_is_read_into_the_model_with_its_fields_resolved() {
    let import = import_package(pivot_package()).unwrap();
    let wb = &import.workbook;
    assert!(wb.sheets[0].pivots.is_empty(), "the data sheet has none");
    let pivot = &wb.sheets[1].pivots[0];

    assert_eq!(pivot.name, "Sales");
    // The cache says where the records are; the pivot part never does.
    assert_eq!(pivot.source_sheet, wb.sheets[0].id);
    assert_eq!(
        pivot.source,
        casual_calc_model::CellRange::new(CellRef::new(0, 0), CellRef::new(3, 2))
    );
    assert_eq!(pivot.anchor, CellRef::new(2, 0), "from <location ref>");

    // `<rowFields><field x="0"/>` gives the nesting order, and the matching
    // `<pivotField defaultSubtotal="0">` gives the flag — reading only one of
    // the two loses either the order or the setting.
    assert_eq!(pivot.rows.len(), 1);
    assert_eq!(pivot.rows[0].source_column, 0);
    assert!(!pivot.rows[0].subtotal);
    assert!(pivot.cols.is_empty());

    // A page field's `item` is an index into that cache field's shared items,
    // so resolving it needs the cache as well.
    assert_eq!(pivot.filters.len(), 1);
    assert_eq!(pivot.filters[0].source_column, 1);
    assert_eq!(pivot.filters[0].selected, vec!["Widget".to_owned()]);

    assert_eq!(pivot.values.len(), 1);
    assert_eq!(pivot.values[0].source_column, 2);
    assert_eq!(pivot.values[0].name, "Total");
    assert_eq!(
        pivot.values[0].aggregate,
        casual_calc_model::PivotAggregate::Average
    );

    assert!(pivot.row_grand_totals, "the schema default is on");
    assert!(!pivot.col_grand_totals, "and this one was turned off");
    assert_eq!(pivot.style, "PivotStyleLight16");
    assert_eq!(
        pivot.part.as_deref(),
        Some("xl/pivotTables/pivotTable1.xml"),
        "still written back from its own bytes until it is edited"
    );
    // The block Excel already wrote, so the first refresh clears exactly it.
    assert_eq!(pivot.output.map(|r| r.end), Some(CellRef::new(5, 2)));
}

#[test]
fn the_pivot_cache_declaration_survives_the_round_trip() {
    // `<pivotCaches>` is what binds a pivot table's `cacheId` to the cache
    // part. It was being dropped: the parts and the relationship were all
    // retained, with nothing left in workbook.xml declaring them, which Excel
    // reports as a file needing repair rather than as a missing pivot.
    let import = import_package(pivot_package()).unwrap();
    let refs = &import.workbook.retained_refs;
    assert!(
        refs.iter().any(|(name, attrs)| name == "pivotCache"
            && attrs.get("cacheId").map(String::as_str) == Some("7")
            && attrs.get("id").map(String::as_str) == Some("rId3")),
        "{refs:?}"
    );
}

#[test]
fn absurd_axis_sizes_are_clamped_instead_of_overflowing() {
    // `width` and `ht` are plain `xsd:double`s off the wire, and nothing between
    // the attribute and `(px as i64) * 15` bounded them. `width="1e300"`
    // saturated the float→int cast to `i64::MAX` and the multiply panicked with
    // "attempt to multiply with overflow" under the dev profile — a crafted
    // `<col>` aborted the host process instead of failing the import — while the
    // release profile wrapped it, so `width="-1e300"` handed layout a *negative*
    // column width. NaN and INF are legal `xsd:double` spellings and arrive here
    // too.
    for value in ["1e300", "-1e300", "1e308", "-1e308", "NaN", "INF", "-INF"] {
        let sheet_xml = format!(
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
            <sheetFormatPr defaultColWidth="{value}" defaultRowHeight="{value}"/>
            <cols><col min="1" max="1" width="{value}" customWidth="1"/></cols>
            <sheetData><row r="1" ht="{value}" customHeight="1"><c r="A1"><v>1</v></c></row></sheetData>
        </worksheet>"#
        )
        .into_bytes();
        let import = import_package(package_with_sheet(sheet_xml, None))
            .unwrap_or_else(|e| panic!("{value} should import, not fail: {e:?}"));
        let sheet = &import.workbook.sheets[0];

        // Excel itself refuses a column past 255 characters (26_850 twips) or a
        // row past 409.5 points (8_190 twips); anything beyond is a crafted or
        // corrupt file, not a width.
        let sizes = |axis: &casual_calc_model::AxisSizing, ceiling: i64| {
            for &size in axis.default.iter().chain(axis.sizes.values()) {
                assert!(
                    (0..=ceiling).contains(&size),
                    "{value}: {size} twips is outside 0..={ceiling}"
                );
            }
        };
        sizes(&sheet.columns, 26_850);
        sizes(&sheet.rows, 8_190);
    }
}

/// An ordinary Excel package: the four relationships Excel always writes at the
/// package root, and the parts they reach.
///
/// Hand-built rather than a fixture because the point is *where* the
/// relationships hang — everything here is attached to the package root, not to
/// `workbook.xml`, and a fixture hides exactly that distinction.
fn package_with_root_parts() -> Vec<u8> {
    const CONTENT_TYPES: &[u8] = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
      <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
      <Default Extension="xml" ContentType="application/xml"/>
      <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
      <Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
      <Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>
      <Override PartName="/customXml/itemProps1.xml" ContentType="application/vnd.openxmlformats-officedocument.customXmlProperties+xml"/>
    </Types>"#;
    const ROOT_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
      <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
      <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
      <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/>
      <Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="customXml/item1.xml"/>
    </Relationships>"#;
    const CORE: &[u8] = br#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:creator>Ada Lovelace</dc:creator><dc:title>Q3 Ledger</dc:title></cp:coreProperties>"#;
    const APP: &[u8] = br#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Company>Analytical Engines</Company></Properties>"#;
    const ITEM: &[u8] = br#"<invoice xmlns="urn:example:invoice"><number>4711</number></invoice>"#;
    const ITEM_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps" Target="itemProps1.xml"/></Relationships>"#;
    const ITEM_PROPS: &[u8] = br#"<ds:datastoreItem xmlns:ds="http://schemas.openxmlformats.org/officeDocument/2006/customXml" ds:itemID="{DEADBEEF}"/>"#;

    let sheet = sheet_with(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#);
    zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("docProps/core.xml", CORE),
        ("docProps/app.xml", APP),
        ("customXml/item1.xml", ITEM),
        ("customXml/_rels/item1.xml.rels", ITEM_RELS),
        ("customXml/itemProps1.xml", ITEM_PROPS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/worksheets/sheet1.xml", &sheet),
    ])
}

#[test]
fn parts_attached_at_the_package_root_are_retained() {
    let import = import_package(package_with_root_parts()).unwrap();
    let wb = &import.workbook;
    let paths: Vec<&str> = wb.retained_parts.iter().map(|p| p.path.as_str()).collect();

    // The author, the title and the company live in these two parts and nowhere
    // else in the file: dropping them loses the document's own metadata.
    assert!(paths.contains(&"docProps/core.xml"), "{paths:?}");
    assert!(paths.contains(&"docProps/app.xml"), "{paths:?}");
    // A customXml item is a whole payload the host may be round-tripping for a
    // system that reads nothing else in the file.
    assert!(paths.contains(&"customXml/item1.xml"), "{paths:?}");
    // Retention is transitive from the root as well: the item's properties are
    // reached through the item's own rels, and an item without them is one Excel
    // reports as needing repair.
    assert!(paths.contains(&"customXml/itemProps1.xml"), "{paths:?}");

    // The content-type override travels with the part, without which the
    // package is invalid and Excel refuses to open it.
    let core = wb
        .retained_parts
        .iter()
        .find(|p| p.path == "docProps/core.xml")
        .unwrap();
    assert_eq!(
        core.content_type.as_deref(),
        Some("application/vnd.openxmlformats-package.core-properties+xml")
    );
    assert!(String::from_utf8_lossy(&core.bytes).contains("Ada Lovelace"));

    // The workbook is reached from the root too, and the writer regenerates it:
    // retaining it would write a stale copy beside the fresh one, and re-emit a
    // second `rId1` into `_rels/.rels`.
    assert!(!paths.contains(&"xl/workbook.xml"), "{paths:?}");

    let root: Vec<_> = wb
        .retained_rels
        .iter()
        .filter(|r| r.source.is_empty())
        .collect();
    assert_eq!(root.len(), 3, "{:?}", wb.retained_rels);
    assert!(
        root.iter()
            .all(|r| !r.rel_type.ends_with("/officeDocument")),
        "{root:?}"
    );
    // Ids travel verbatim, as everywhere else: `_rels/.rels` is regenerated and
    // a re-minted id would point at nothing.
    assert!(
        root.iter()
            .any(|r| r.id == "rId4" && r.target == "customXml/item1.xml")
    );
}

#[test]
fn a_root_relationship_to_a_missing_part_is_counted_rather_than_dropped_silently() {
    // Nothing can be retained for a part the package does not carry, and
    // `Omitted` + `NotRetained` is the one way data leaves the system — so it is
    // counted and reported. See docs/34.
    const ROOT_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
      <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
      <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
    </Relationships>"#;
    let sheet = sheet_with(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#);
    let bytes = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/worksheets/sheet1.xml", &sheet),
    ]);
    let import = import_package(bytes).unwrap();
    let entry = import
        .report
        .entries()
        .into_iter()
        .find(|e| e.feature == "docProps/core.xml")
        .expect("the part the file names but does not carry is reported");
    assert_eq!(entry.model, ModelOutcome::Omitted);
    assert_eq!(entry.retention, crate::RetentionOutcome::NotRetained);
    assert_eq!(entry.count, 1);
}

/// A retained part whose type the file states with a `<Default Extension>`.
///
/// This is FID-17 at the seam it was lost at. The importer read the
/// `<Override>` list and nothing else, so `printerSettings1.bin` — whose type
/// the file declares perfectly clearly, by extension, the way every real
/// producer declares a repeated binary part — arrived with
/// `content_type: None`. The writer then had nothing to declare it with, and
/// wrote it into the saved package undeclared: a package Excel refuses, or
/// offers to repair by discarding what it cannot account for.
///
/// Both halves are asserted, because a `<Default>` is a claim about an
/// extension and not about a part: the `.bin` with its own `<Override>` must
/// keep the type the override gives it, or the saved file would call an OLE
/// object a set of printer settings.
#[test]
fn a_retained_part_typed_by_a_default_extension_keeps_its_content_type() {
    const CONTENT_TYPES: &[u8] = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
      <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
      <Default Extension="xml" ContentType="application/xml"/>
      <Default Extension="bin" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.printerSettings"/>
      <Default Extension="jpeg" ContentType="image/jpeg"/>
      <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
      <Override PartName="/xl/embeddings/oleObject1.bin" ContentType="application/vnd.openxmlformats-officedocument.oleObject"/>
    </Types>"#;
    const SHEET_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
      <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/printerSettings" Target="../printerSettings/printerSettings1.bin"/>
      <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.jpeg"/>
      <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject" Target="../embeddings/oleObject1.bin"/>
    </Relationships>"#;

    let sheet = sheet_with(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#);
    let bytes = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/worksheets/sheet1.xml", &sheet),
        ("xl/worksheets/_rels/sheet1.xml.rels", SHEET_RELS),
        ("xl/printerSettings/printerSettings1.bin", b"\x00PRN"),
        ("xl/media/image1.jpeg", b"\xff\xd8\xff\xe0JFIF"),
        ("xl/embeddings/oleObject1.bin", b"\xd0\xcf\x11\xe0OLE"),
    ]);

    let import = import_package(bytes).unwrap();
    let type_of = |path: &str| -> Option<String> {
        import
            .workbook
            .retained_parts
            .iter()
            .find(|p| p.path == path)
            .unwrap_or_else(|| panic!("{path} was not retained"))
            .content_type
            .clone()
    };

    assert_eq!(
        type_of("xl/printerSettings/printerSettings1.bin").as_deref(),
        Some("application/vnd.openxmlformats-officedocument.spreadsheetml.printerSettings")
    );
    assert_eq!(
        type_of("xl/media/image1.jpeg").as_deref(),
        Some("image/jpeg")
    );
    assert_eq!(
        type_of("xl/embeddings/oleObject1.bin").as_deref(),
        Some("application/vnd.openxmlformats-officedocument.oleObject"),
        "an override names one part and outranks the default for its extension"
    );
}

// --- Admission budget (SEC-002, SEC-011, docs/21) ----------------------------

mod admission_budget {
    use super::*;
    use crate::{ImportError, Overrun, import_package_with};
    use casual_calc_ooxml::{OoxmlLimits, SpreadsheetLimits};

    /// A workbook of two sheets, so the *sum* is observable.
    const TWO_SHEET_WORKBOOK: &[u8] = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="One" sheetId="1" r:id="rId1"/><sheet name="Two" sheetId="2" r:id="rId2"/></sheets></workbook>"#;
    const TWO_SHEET_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#;

    /// One row of `n` populated cells.
    fn row_of(n: usize) -> String {
        let mut cells = String::new();
        for i in 0..n {
            cells.push_str(&format!("<c r=\"{}1\"><v>{i}</v></c>", column_name(i)));
        }
        format!(
            "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData><row r=\"1\">{cells}</row></sheetData></worksheet>"
        )
    }

    /// `0 -> A`, `26 -> AA`, as SpreadsheetML spells columns.
    fn column_name(mut index: usize) -> String {
        let mut name = Vec::new();
        loop {
            name.push(b'A' + u8::try_from(index % 26).unwrap());
            if index < 26 {
                break;
            }
            index = index / 26 - 1;
        }
        name.reverse();
        String::from_utf8(name).unwrap()
    }

    fn two_sheets(each: usize) -> Vec<u8> {
        let one = row_of(each).into_bytes();
        let two = row_of(each).into_bytes();
        zip_parts(&[
            ("[Content_Types].xml", CONTENT_TYPES),
            ("_rels/.rels", ROOT_RELS),
            ("xl/workbook.xml", TWO_SHEET_WORKBOOK),
            ("xl/_rels/workbook.xml.rels", TWO_SHEET_RELS),
            ("xl/worksheets/sheet1.xml", &one),
            ("xl/worksheets/sheet2.xml", &two),
        ])
    }

    fn with_cells(max_populated_cells: usize) -> OoxmlLimits {
        OoxmlLimits {
            spreadsheet: SpreadsheetLimits {
                max_populated_cells,
                ..SpreadsheetLimits::default()
            },
            ..OoxmlLimits::default()
        }
    }

    /// **The budget is the document's, not each part's.**
    ///
    /// This is the whole of `SEC-002`. Every per-part limit was enforced and
    /// every one was passing; nothing added them up, so a package of many parts
    /// multiplied a ceiling nobody had agreed to. Two sheets of 40 cells is 80
    /// cells, and a document allowed 79 must refuse it — a check that ran per
    /// sheet would see 40 twice and admit the file.
    #[test]
    fn cells_are_counted_across_the_whole_workbook() {
        let package = two_sheets(40);

        let admitted = import_package_with(package.clone(), with_cells(80))
            .expect("80 cells inside a budget of 80");
        assert_eq!(
            admitted
                .workbook
                .sheets
                .iter()
                .map(|s| s.cells.len())
                .sum::<usize>(),
            80
        );

        let refused = import_package_with(package, with_cells(79))
            .expect_err("80 cells must not fit a budget of 79");
        match refused {
            ImportError::OverBudget { what, limit } => {
                assert_eq!(what, Overrun::PopulatedCells);
                assert_eq!(limit, 79);
            }
            other => panic!("expected an over-budget refusal, got {other}"),
        }
    }

    /// **Refused, not truncated.**
    ///
    /// docs/21 requires failing closed. A workbook admitted with some of its
    /// cells missing is worse than one refused: it looks fine, and it will be
    /// saved back over the original with the rest gone.
    #[test]
    fn an_over_budget_document_is_refused_rather_than_partly_loaded() {
        let refused = import_package_with(two_sheets(40), with_cells(50));
        assert!(
            matches!(refused, Err(ImportError::OverBudget { .. })),
            "a partial load is silent data loss"
        );
    }

    /// **The refusal carries the stable diagnostic code.**
    #[test]
    fn the_refusal_is_diagnosable() {
        let refused = import_package_with(two_sheets(40), with_cells(10)).unwrap_err();
        // The code docs/20 reserved for this condition, and which nothing
        // emitted until now — minting a fresh one would have left a registered
        // code dead and added an unregistered one beside it.
        assert_eq!(refused.code(), "OC-IMP-0003");
        let said = refused.to_string();
        assert!(said.contains("OC-IMP-0003"), "{said}");
        assert!(said.contains("populated cells"), "{said}");
    }

    /// **The shipped defaults are finite.**
    ///
    /// The cheapest way for this bound to disappear is for somebody to raise a
    /// default "temporarily". A limit that is not a limit should fail here
    /// rather than in a deployment.
    #[test]
    fn the_default_budget_is_bounded() {
        let d = SpreadsheetLimits::default();
        for (what, value) in [
            ("populated cells", d.max_populated_cells),
            ("shared strings", d.max_shared_strings),
            ("defined names", d.max_defined_names),
            ("merged ranges", d.max_merged_ranges),
        ] {
            assert!(value > 0, "{what} is zero, which admits nothing");
            assert!(
                value < usize::MAX / 2,
                "{what} is effectively unbounded ({value})"
            );
        }
        // And above the supported target, so a real workbook still opens.
        assert!(
            d.max_populated_cells >= 1_000_000,
            "the cap is below the T1 target this engine claims to support"
        );
    }

    /// **A shared-string table larger than the budget is refused before it is
    /// interned.**
    #[test]
    fn an_oversized_shared_string_table_is_refused() {
        let mut sst = String::from(
            r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
        );
        for i in 0..50 {
            sst.push_str(&format!("<si><t>s{i}</t></si>"));
        }
        sst.push_str("</sst>");
        let package = package_with_sheet(sheet_with(""), Some(sst.as_bytes()));

        let limits = OoxmlLimits {
            spreadsheet: SpreadsheetLimits {
                max_shared_strings: 49,
                ..SpreadsheetLimits::default()
            },
            ..OoxmlLimits::default()
        };
        match import_package_with(package, limits) {
            Err(ImportError::OverBudget { what, limit }) => {
                assert_eq!(what, Overrun::SharedStrings);
                assert_eq!(limit, 49);
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
}
