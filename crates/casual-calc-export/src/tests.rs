//! Semantic round-trip tests: `import → write → import` is a model fixed point.

use std::io::{Cursor, Write};

use casual_calc_import::import_package;
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::write_workbook;

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
const WORKBOOK: &[u8] = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets><definedNames><definedName name="Rng">Sheet1!$A$1:$A$3</definedName></definedNames></workbook>"#;
const WORKBOOK_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
const SHARED: &[u8] = br#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>Hello</t></si></sst>"#;
const STYLES: &[u8] = br#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="1"><numFmt numFmtId="164" formatCode="0.00"/></numFmts><fonts count="5"><font><sz val="11"/><name val="Calibri"/></font><font><b/><color rgb="FFFF0000"/><sz val="11"/><name val="Calibri"/></font><font><i/><u/><sz val="11"/><name val="Calibri"/></font><font><sz val="14"/><name val="Arial"/></font><font><strike/><sz val="11"/><name val="Calibri"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/></patternFill></fill></fills><borders count="2"><border><left/><right/><top/><bottom/><diagonal/></border><border><left style="thin"><color rgb="FF0000FF"/></left><right style="thin"/><top style="thin"/><bottom style="medium"/><diagonal/></border></borders><cellXfs count="7"><xf numFmtId="0"/><xf numFmtId="164"/><xf numFmtId="0" fontId="1" fillId="2"/><xf numFmtId="0" borderId="1"/><xf numFmtId="0" fontId="2" applyAlignment="1"><alignment horizontal="center" vertical="top"/></xf><xf numFmtId="0" fontId="3" applyFont="1"/><xf numFmtId="0" fontId="4" applyFont="1"/></cellXfs></styleSheet>"#;

fn worksheet() -> &'static [u8] {
    br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
      <sheetViews><sheetView><pane xSplit="1" ySplit="1" topLeftCell="B2" state="frozen"/></sheetView></sheetViews>
      <sheetFormatPr defaultColWidth="10" defaultRowHeight="15"/>
      <cols><col min="2" max="2" width="20" customWidth="1"/><col min="6" max="6" hidden="1"/></cols>
      <sheetData>
        <row r="1"><c r="A1" s="2"><v>42</v></c><c r="B1" t="b"><v>1</v></c><c r="C1" t="e"><v>#DIV/0!</v></c></row>
        <row r="2" ht="30" customHeight="1"><c r="A2" t="s"><v>0</v></c><c r="B2" s="4"><v>7</v></c><c r="C2" s="6"><v>1</v></c></row>
        <row r="3" hidden="1"><c r="A3"><f>A1*2</f><v>84</v></c><c r="B3" s="1"><v>3.14</v></c><c r="C3" s="3"><v>5</v></c><c r="D3" s="5"><v>9</v></c></row>
      </sheetData>
      <mergeCells count="1"><mergeCell ref="D1:E2"/></mergeCells>
    </worksheet>"#
}

fn sample_xlsx() -> Vec<u8> {
    zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/sharedStrings.xml", SHARED),
        ("xl/styles.xml", STYLES),
        ("xl/worksheets/sheet1.xml", worksheet()),
    ])
}

#[test]
fn semantic_round_trip_is_a_fixed_point() {
    let original = import_package(sample_xlsx()).unwrap().workbook;
    let written = write_workbook(&original).unwrap();
    let reimported = import_package(written).unwrap().workbook;
    assert_eq!(
        original, reimported,
        "import -> write -> import must yield an equal model"
    );
}

#[test]
fn write_is_deterministic() {
    let workbook = import_package(sample_xlsx()).unwrap().workbook;
    let first = write_workbook(&workbook).unwrap();
    let second = write_workbook(&workbook).unwrap();
    assert_eq!(first, second, "the writer must be byte-deterministic");
}

#[test]
fn written_package_reopens_and_preserves_data() {
    use casual_calc_model::{CellRef, CellValue};
    let original = import_package(sample_xlsx()).unwrap().workbook;
    let written = write_workbook(&original).unwrap();
    let wb = import_package(written).unwrap().workbook;
    let sheet = &wb.sheets[0];

    assert_eq!(
        sheet.cells.get(CellRef::new(0, 0)).unwrap().value,
        CellValue::Number(42.0)
    );
    assert_eq!(sheet.merges.len(), 1);
    assert_eq!(sheet.view.frozen_rows, 1);
    assert_eq!(wb.defined_names.len(), 1);

    // The formula survived as an AST.
    let a3 = sheet.cells.get(CellRef::new(2, 0)).unwrap();
    let expr = wb.formula(a3.formula.unwrap()).unwrap();
    assert_eq!(expr.to_string(), "(A1*2)");

    // The number-format style survived.
    let b3 = sheet.cells.get(CellRef::new(2, 1)).unwrap();
    assert_eq!(
        wb.styles
            .get(b3.style.unwrap())
            .unwrap()
            .number_format
            .as_deref(),
        Some("0.00")
    );
}

#[test]
fn tab_color_round_trips() {
    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    workbook.sheets[0].tab_color = Some("1E88E5".to_owned());

    let written = write_workbook(&workbook).unwrap();
    let wb = import_package(written).unwrap().workbook;
    assert_eq!(wb.sheets[0].tab_color.as_deref(), Some("1E88E5"));
    // A sheet with no color stays uncolored (no phantom <tabColor>).
    if wb.sheets.len() > 1 {
        assert_eq!(wb.sheets[1].tab_color, None);
    }
}

#[test]
fn outline_levels_and_collapsed_round_trip() {
    use casual_calc_model::OutlinePr;

    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    {
        let sheet = &mut workbook.sheets[0];
        sheet.row_outline_levels.insert(1, 1);
        sheet.row_outline_levels.insert(2, 2);
        sheet.collapsed_rows.insert(2);
        sheet.col_outline_levels.insert(3, 1);
        sheet.col_outline_levels.insert(4, 1);
        sheet.collapsed_cols.insert(4);
        sheet.outline = OutlinePr {
            summary_below: false,
            summary_right: true,
        };
    }

    let written = write_workbook(&workbook).unwrap();
    let wb = import_package(written).unwrap().workbook;
    let sheet = &wb.sheets[0];

    assert_eq!(sheet.row_outline_levels.get(&1), Some(&1));
    assert_eq!(sheet.row_outline_levels.get(&2), Some(&2));
    assert!(sheet.collapsed_rows.contains(&2));
    assert_eq!(sheet.col_outline_levels.get(&3), Some(&1));
    assert_eq!(sheet.col_outline_levels.get(&4), Some(&1));
    assert!(sheet.collapsed_cols.contains(&4));
    assert!(!sheet.outline.summary_below);
    assert!(sheet.outline.summary_right);

    // A sheet left at the outline default writes no <outlinePr>, so a re-import
    // sees the default again (no phantom flag flip).
    let plain = import_package(sample_xlsx()).unwrap().workbook;
    assert!(plain.sheets[0].outline.is_default());
}

#[test]
fn zoom_scale_round_trips() {
    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    workbook.sheets[0].view.zoom = 150;

    let written = write_workbook(&workbook).unwrap();
    let wb = import_package(written).unwrap().workbook;
    assert_eq!(wb.sheets[0].view.zoom, 150);
    // The frozen pane still round-trips alongside the zoom scale.
    assert_eq!(wb.sheets[0].view.frozen_rows, 1);
    assert_eq!(wb.sheets[0].view.frozen_cols, 1);
}

#[test]
fn indent_round_trips() {
    use casual_calc_model::{Cell, CellRef, CellValue, HAlign, Style};

    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    let style = workbook.intern_style(Style {
        align: Some(HAlign::Left),
        indent: 3,
        ..Style::default()
    });
    let mut cell = Cell::value(CellValue::Number(1.0));
    cell.style = Some(style);
    workbook.sheets[0].cells.set(CellRef::new(5, 0), cell);

    let written = write_workbook(&workbook).unwrap();
    let wb = import_package(written).unwrap().workbook;
    let cell = wb.sheets[0].cells.get(CellRef::new(5, 0)).unwrap();
    let style = wb.styles.get(cell.style.unwrap()).unwrap();
    assert_eq!(style.indent, 3);
    assert_eq!(style.align, Some(HAlign::Left));
}

#[test]
fn data_validation_list_round_trips() {
    use casual_calc_model::{CellRange, CellRef, DataValidation};
    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    workbook.sheets[0].validations.push(DataValidation {
        range: CellRange::new(CellRef::new(0, 0), CellRef::new(4, 0)),
        values: vec!["Yes".to_owned(), "No".to_owned(), "Maybe".to_owned()],
    });
    let written = write_workbook(&workbook).unwrap();
    let wb = import_package(written).unwrap().workbook;
    let v = &wb.sheets[0].validations;
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].values, vec!["Yes", "No", "Maybe"]);
    assert_eq!(v[0].range.start, CellRef::new(0, 0));
    assert_eq!(v[0].range.end, CellRef::new(4, 0));
}

#[test]
fn conditional_formatting_round_trips() {
    use casual_calc_model::{CellRange, CellRef, CfRule, ConditionalFormat};
    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    let r = |r0, c0, r1, c1| CellRange::new(CellRef::new(r0, c0), CellRef::new(r1, c1));
    workbook.sheets[0].conditional_formats = vec![
        ConditionalFormat {
            range: r(0, 0, 4, 3),
            rule: CfRule::GreaterThan(5.0),
            fill: "FFD166".into(),
        },
        ConditionalFormat {
            range: r(1, 1, 1, 1),
            rule: CfRule::Between(2.0, 10.0),
            fill: "D1F0D6".into(),
        },
        ConditionalFormat {
            range: r(0, 0, 0, 0),
            rule: CfRule::TextContains("total".into()),
            fill: "FFD6E0".into(),
        },
    ];
    let written = write_workbook(&workbook).unwrap();
    let cfs = import_package(written).unwrap().workbook.sheets[0]
        .conditional_formats
        .clone();
    assert_eq!(cfs.len(), 3);
    assert_eq!(cfs[0].rule, CfRule::GreaterThan(5.0));
    assert_eq!(cfs[0].fill, "FFD166");
    assert_eq!(cfs[0].range.end, CellRef::new(4, 3));
    assert_eq!(cfs[1].rule, CfRule::Between(2.0, 10.0));
    assert_eq!(cfs[1].fill, "D1F0D6");
    assert_eq!(cfs[2].rule, CfRule::TextContains("total".into()));
    assert_eq!(cfs[2].fill, "FFD6E0");
}

#[test]
fn cell_comments_round_trip() {
    use casual_calc_model::{CellComment, CellRef};
    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    workbook.sheets[0].comments = vec![
        CellComment {
            at: CellRef::new(0, 0),
            text: "First note".to_owned(),
            author: Some("sachin".to_owned()),
        },
        CellComment {
            at: CellRef::new(3, 2),
            text: "Needs <review> & sign-off".to_owned(),
            author: None,
        },
    ];
    let written = write_workbook(&workbook).unwrap();
    let comments = import_package(written).unwrap().workbook.sheets[0]
        .comments
        .clone();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].at, CellRef::new(0, 0));
    assert_eq!(comments[0].text, "First note");
    assert_eq!(comments[0].author.as_deref(), Some("sachin"));
    assert_eq!(comments[1].at, CellRef::new(3, 2));
    assert_eq!(comments[1].text, "Needs <review> & sign-off");
    // OOXML requires every note to carry an author; a `None` author is written
    // as our sentinel and comes back attributed to it.
    assert_eq!(comments[1].author.as_deref(), Some("OpenCalc"));
}
