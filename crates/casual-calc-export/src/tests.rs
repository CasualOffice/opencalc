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

/// Read a named part out of a written `.xlsx` package as a UTF-8 string.
fn xml_of(package: &[u8], part: &str) -> String {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(Cursor::new(package)).unwrap();
    let mut file = zip.by_name(part).unwrap();
    let mut out = String::new();
    file.read_to_string(&mut out).unwrap();
    out
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
fn hide_gridlines_round_trips() {
    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    // Default: grid lines shown, and the written XML carries no showGridLines.
    assert!(!workbook.sheets[0].view.hide_gridlines);
    let written = write_workbook(&workbook).unwrap();
    assert!(
        !xml_of(&written, "xl/worksheets/sheet1.xml").contains("showGridLines"),
        "a normal sheet must not emit showGridLines"
    );

    // Hiding grid lines survives a full round trip and writes showGridLines=\"0\".
    workbook.sheets[0].view.hide_gridlines = true;
    let written = write_workbook(&workbook).unwrap();
    assert!(xml_of(&written, "xl/worksheets/sheet1.xml").contains("showGridLines=\"0\""));
    let wb = import_package(written).unwrap().workbook;
    assert!(wb.sheets[0].view.hide_gridlines);
}

#[test]
fn text_rotation_round_trips() {
    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    let sheet_id = workbook.sheets[0].id;
    let at = casual_calc_model::CellRef::new(0, 0);
    let id = workbook.intern_style(casual_calc_model::Style {
        rotation: 45,
        ..Default::default()
    });
    let sheet = workbook
        .sheets
        .iter_mut()
        .find(|s| s.id == sheet_id)
        .unwrap();
    let mut cell = sheet.cells.get(at).cloned().unwrap_or_default();
    cell.style = Some(id);
    sheet.cells.set(at, cell);

    let written = write_workbook(&workbook).unwrap();
    assert!(xml_of(&written, "xl/styles.xml").contains("textRotation=\"45\""));
    let wb = import_package(written).unwrap().workbook;
    let round = wb.sheets[0].cells.get(at).unwrap().style.unwrap();
    assert_eq!(wb.styles.get(round).unwrap().rotation, 45);
}

#[test]
fn hide_headers_round_trips() {
    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    // Default: headers shown, and nothing written.
    assert!(!workbook.sheets[0].view.hide_headers);
    let written = write_workbook(&workbook).unwrap();
    assert!(
        !xml_of(&written, "xl/worksheets/sheet1.xml").contains("showRowColHeaders"),
        "a normal sheet must not emit showRowColHeaders"
    );

    workbook.sheets[0].view.hide_headers = true;
    let written = write_workbook(&workbook).unwrap();
    assert!(xml_of(&written, "xl/worksheets/sheet1.xml").contains("showRowColHeaders=\"0\""));
    let wb = import_package(written).unwrap().workbook;
    assert!(wb.sheets[0].view.hide_headers);
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
        ConditionalFormat::new(r(0, 0, 4, 3), CfRule::GreaterThan(5.0), "FFD166"),
        ConditionalFormat::new(r(1, 1, 1, 1), CfRule::Between(2.0, 10.0), "D1F0D6"),
        ConditionalFormat::new(
            r(0, 0, 0, 0),
            CfRule::TextContains("total".into()),
            "FFD6E0",
        ),
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

#[test]
fn autofilter_value_list_round_trips() {
    use casual_calc_model::{AutoFilter, CellRange, CellRef, FilterRule};

    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    let range = CellRange::new(CellRef::new(0, 0), CellRef::new(9, 2));
    let mut filter = AutoFilter::new(range);
    // A blank is carried as the empty string and must come back as one.
    filter.rules.insert(
        1,
        FilterRule::Values(vec!["Apple".into(), "Pear".into(), String::new()]),
    );
    workbook.sheets[0].auto_filter = Some(filter);

    let written = write_workbook(&workbook).unwrap();
    let xml = xml_of(&written, "xl/worksheets/sheet1.xml");
    assert!(xml.contains("<autoFilter ref=\"A1:C10\">"), "{xml}");
    assert!(xml.contains("<filterColumn colId=\"1\">"));
    // The blank rides on the container, never as <filter val="">.
    assert!(xml.contains("<filters blank=\"1\">"));
    assert!(!xml.contains("<filter val=\"\"/>"));
    // autoFilter precedes mergeCells in the CT_Worksheet sequence.
    if let (Some(af), Some(mc)) = (xml.find("<autoFilter"), xml.find("<mergeCells")) {
        assert!(af < mc, "autoFilter must precede mergeCells");
    }

    let wb = import_package(written).unwrap().workbook;
    let back = wb.sheets[0].auto_filter.as_ref().expect("filter dropped");
    assert_eq!(back.range, range);
    match back.rules.get(&1).expect("colId 1 dropped") {
        FilterRule::Values(v) => {
            assert!(v.contains(&"Apple".to_owned()));
            assert!(v.contains(&"Pear".to_owned()));
            assert!(v.contains(&String::new()), "the blank entry was lost");
        }
        other => panic!("wrong rule kind: {other:?}"),
    }
}

#[test]
fn autofilter_custom_filters_round_trip() {
    use casual_calc_model::{AutoFilter, CellRange, CellRef, CustomFilter, FilterOp, FilterRule};

    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    let mut filter = AutoFilter::new(CellRange::new(CellRef::new(0, 0), CellRef::new(9, 2)));
    filter.rules.insert(
        0,
        FilterRule::Custom {
            first: CustomFilter {
                op: FilterOp::GreaterThanOrEqual,
                value: "10".into(),
            },
            second: Some(CustomFilter {
                op: FilterOp::LessThanOrEqual,
                value: "20".into(),
            }),
            and: true,
        },
    );
    // "contains" is `equal` plus wildcards — there is no dedicated operator.
    filter.rules.insert(
        2,
        FilterRule::Custom {
            first: CustomFilter {
                op: FilterOp::Equal,
                value: "*ap*".into(),
            },
            second: None,
            and: false,
        },
    );
    workbook.sheets[0].auto_filter = Some(filter.clone());

    let written = write_workbook(&workbook).unwrap();
    let xml = xml_of(&written, "xl/worksheets/sheet1.xml");
    assert!(xml.contains("<customFilters and=\"1\">"), "{xml}");
    assert!(xml.contains("operator=\"greaterThanOrEqual\" val=\"10\""));
    assert!(xml.contains("operator=\"lessThanOrEqual\" val=\"20\""));
    assert!(xml.contains("operator=\"equal\" val=\"*ap*\""));

    let wb = import_package(written).unwrap().workbook;
    assert_eq!(wb.sheets[0].auto_filter, Some(filter));
}

#[test]
fn filtered_rows_export_as_hidden_without_becoming_hand_hidden() {
    use casual_calc_model::{AutoFilter, CellRange, CellRef};

    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    workbook.sheets[0].auto_filter = Some(AutoFilter::new(CellRange::new(
        CellRef::new(0, 0),
        CellRef::new(9, 2),
    )));
    workbook.sheets[0].filter_hidden.insert(4);

    let written = write_workbook(&workbook).unwrap();
    let xml = xml_of(&written, "xl/worksheets/sheet1.xml");
    // Row 5 (1-based) must be emitted and marked hidden even with no cells of
    // its own — otherwise reopening the file shows the filtered-out row.
    let row5 = xml
        .split("<row r=\"5\"")
        .nth(1)
        .expect("filtered row was not emitted at all");
    assert!(
        row5[..row5.find('>').unwrap()].contains("hidden=\"1\""),
        "filtered row lost its hidden flag: {row5:.80}"
    );
}

#[test]
fn every_alignment_mode_round_trips_without_collapsing() {
    use casual_calc_model::{CellRef, HAlign, Style, VAlign};

    // `centerContinuous` used to import as `center` and vertical `justify` as
    // `bottom`, so opening and saving quietly rewrote the file's alignment.
    let cases = [
        (HAlign::Left, "left"),
        (HAlign::Center, "center"),
        (HAlign::Right, "right"),
        (HAlign::Fill, "fill"),
        (HAlign::Justify, "justify"),
        (HAlign::CenterContinuous, "centerContinuous"),
        (HAlign::Distributed, "distributed"),
    ];
    for (i, (align, token)) in cases.iter().enumerate() {
        let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
        let at = CellRef::new(i as u32, 0);
        let id = workbook.intern_style(Style {
            align: Some(*align),
            ..Default::default()
        });
        let sheet = &mut workbook.sheets[0];
        let mut cell = sheet.cells.get(at).cloned().unwrap_or_default();
        cell.style = Some(id);
        sheet.cells.set(at, cell);

        let written = write_workbook(&workbook).unwrap();
        assert!(
            xml_of(&written, "xl/styles.xml").contains(&format!("horizontal=\"{token}\"")),
            "{align:?} did not write horizontal=\"{token}\""
        );
        let wb = import_package(written).unwrap().workbook;
        let round = wb.sheets[0].cells.get(at).unwrap().style.unwrap();
        assert_eq!(
            wb.styles.get(round).unwrap().align,
            Some(*align),
            "{align:?} did not survive the round trip"
        );
    }

    for (valign, token) in [
        (VAlign::Top, "top"),
        (VAlign::Middle, "center"),
        (VAlign::Bottom, "bottom"),
        (VAlign::Justify, "justify"),
        (VAlign::Distributed, "distributed"),
    ] {
        let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
        let at = CellRef::new(0, 0);
        let id = workbook.intern_style(Style {
            valign: Some(valign),
            ..Default::default()
        });
        let sheet = &mut workbook.sheets[0];
        let mut cell = sheet.cells.get(at).cloned().unwrap_or_default();
        cell.style = Some(id);
        sheet.cells.set(at, cell);

        let written = write_workbook(&workbook).unwrap();
        assert!(
            xml_of(&written, "xl/styles.xml").contains(&format!("vertical=\"{token}\"")),
            "{valign:?} did not write vertical=\"{token}\""
        );
        let wb = import_package(written).unwrap().workbook;
        let round = wb.sheets[0].cells.get(at).unwrap().style.unwrap();
        assert_eq!(wb.styles.get(round).unwrap().valign, Some(valign));
    }
}

#[test]
fn named_cell_styles_round_trip_with_their_links() {
    use casual_calc_model::{CellRef, NamedCellStyle, Style};

    // Named styles were dropped entirely: import never read `cellStyleXfs` or
    // `cellStyles`, and the writer emitted one anonymous entry with every cell
    // pointing at it. A "Heading 1" cell came back as merely bold-and-blue.
    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    workbook.cell_styles = vec![
        NamedCellStyle {
            name: "Normal".into(),
            builtin_id: Some(0),
            style: Style::default(),
        },
        NamedCellStyle {
            name: "Heading 1".into(),
            builtin_id: Some(16),
            style: Style {
                bold: true,
                font_color: Some("1F4E79".into()),
                ..Default::default()
            },
        },
    ];
    let at = CellRef::new(0, 0);
    let id = workbook.intern_style(Style {
        bold: true,
        font_color: Some("1F4E79".into()),
        style_ref: Some(1), // the Heading 1 entry
        ..Default::default()
    });
    let sheet = &mut workbook.sheets[0];
    let mut cell = sheet.cells.get(at).cloned().unwrap_or_default();
    cell.style = Some(id);
    sheet.cells.set(at, cell);

    let written = write_workbook(&workbook).unwrap();
    let xml = xml_of(&written, "xl/styles.xml");
    assert!(xml.contains("<cellStyles count=\"2\">"), "{xml}");
    assert!(xml.contains("name=\"Heading 1\""));
    assert!(xml.contains("builtinId=\"16\""));
    // Normal must occupy cellStyleXfs slot 0 — it is what every unlinked cell's
    // xfId="0" resolves to.
    assert!(xml.contains("<cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/>"));
    // cellStyles comes after cellXfs in the CT_Stylesheet sequence.
    if let (Some(cx), Some(cs)) = (xml.find("<cellXfs"), xml.find("<cellStyles")) {
        assert!(cx < cs, "cellStyles must follow cellXfs");
    }

    let wb = import_package(written).unwrap().workbook;
    let names: Vec<&str> = wb.cell_styles.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"Heading 1"), "named styles lost: {names:?}");
    let round = wb.sheets[0].cells.get(at).unwrap().style.unwrap();
    let style = wb.styles.get(round).unwrap();
    assert!(style.bold);
    // The association survived, not just the formatting.
    let heading = wb
        .cell_styles
        .iter()
        .position(|c| c.name == "Heading 1")
        .unwrap() as u32;
    assert_eq!(
        style.style_ref,
        Some(heading),
        "the cell no longer says which named style it belongs to"
    );
}

#[test]
fn a_workbook_with_no_named_styles_still_writes_the_default_entry() {
    // cellXfs entries all carry xfId="0", so cellStyleXfs can never be empty.
    let workbook = import_package(sample_xlsx()).unwrap().workbook;
    let written = write_workbook(&workbook).unwrap();
    let xml = xml_of(&written, "xl/styles.xml");
    assert!(xml.contains("<cellStyleXfs count=\"1\">"), "{xml}");
}

#[test]
fn ranked_and_average_and_duplicate_rules_round_trip() {
    use casual_calc_model::{CellRange, CellRef, CfRule, ConditionalFormat};

    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    let r = CellRange::new(CellRef::new(0, 0), CellRef::new(9, 0));
    let rules = vec![
        CfRule::Top10 {
            rank: 3,
            bottom: false,
            percent: false,
        },
        CfRule::Top10 {
            rank: 10,
            bottom: true,
            percent: true,
        },
        CfRule::AboveAverage {
            below: false,
            equal: false,
        },
        CfRule::AboveAverage {
            below: true,
            equal: true,
        },
        CfRule::DuplicateValues { unique: false },
        CfRule::DuplicateValues { unique: true },
    ];
    workbook.sheets[0].conditional_formats = rules
        .iter()
        .map(|rule| ConditionalFormat::new(r, rule.clone(), "FFD166"))
        .collect();

    let written = write_workbook(&workbook).unwrap();
    let xml = xml_of(&written, "xl/worksheets/sheet1.xml");
    assert!(xml.contains("type=\"top10\""), "{xml}");
    assert!(xml.contains("bottom=\"1\" percent=\"1\""));
    assert!(xml.contains("type=\"aboveAverage\""));
    // The schema defaults aboveAverage to true, so only "below" is written out.
    assert!(xml.contains("aboveAverage=\"0\" equalAverage=\"1\""));
    assert!(xml.contains("type=\"duplicateValues\""));
    assert!(xml.contains("type=\"uniqueValues\""));

    let back = import_package(written).unwrap().workbook.sheets[0]
        .conditional_formats
        .iter()
        .map(|c| c.rule.clone())
        .collect::<Vec<_>>();
    assert_eq!(back, rules);
}

#[test]
fn rule_priority_and_stop_if_true_round_trip() {
    use casual_calc_model::{CellRange, CellRef, CfRule, ConditionalFormat};

    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    let r = CellRange::new(CellRef::new(0, 0), CellRef::new(4, 0));
    let mut first = ConditionalFormat::new(r, CfRule::GreaterThan(1.0), "FFD166");
    first.priority = 7;
    first.stop_if_true = true;
    workbook.sheets[0].conditional_formats = vec![first];

    let written = write_workbook(&workbook).unwrap();
    let xml = xml_of(&written, "xl/worksheets/sheet1.xml");
    assert!(xml.contains("stopIfTrue=\"1\""), "{xml}");
    assert!(xml.contains("priority=\"7\""));

    let back = &import_package(written).unwrap().workbook.sheets[0].conditional_formats[0];
    assert_eq!(back.priority, 7);
    assert!(back.stop_if_true);
}

#[test]
fn diagonal_borders_round_trip() {
    use casual_calc_model::{BorderEdge, Borders, CellRef, Style};

    // The writer used to emit a bare `<diagonal/>`, so a file's diagonal borders
    // were dropped the moment it was saved.
    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    let at = CellRef::new(0, 0);
    let border = Borders {
        diagonal: Some(BorderEdge {
            style: "thin".into(),
            color: Some("FF0000".into()),
        }),
        diagonal_up: true,
        diagonal_down: true,
        ..Default::default()
    };
    // A diagonal on its own is still a border.
    assert!(!border.is_empty());
    let id = workbook.intern_style(Style {
        border: Some(border.clone()),
        ..Default::default()
    });
    let sheet = &mut workbook.sheets[0];
    let mut cell = sheet.cells.get(at).cloned().unwrap_or_default();
    cell.style = Some(id);
    sheet.cells.set(at, cell);

    let written = write_workbook(&workbook).unwrap();
    let xml = xml_of(&written, "xl/styles.xml");
    assert!(xml.contains("diagonalUp=\"1\""), "{xml}");
    assert!(xml.contains("diagonalDown=\"1\""));
    assert!(xml.contains("<diagonal style=\"thin\">"));

    let wb = import_package(written).unwrap().workbook;
    let round = wb.sheets[0].cells.get(at).unwrap().style.unwrap();
    assert_eq!(wb.styles.get(round).unwrap().border, Some(border));
}
