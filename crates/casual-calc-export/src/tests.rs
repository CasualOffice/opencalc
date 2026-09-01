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

    // The formula survived as an AST — and comes back written the way it was
    // written, not re-bracketed, since this is the text the file carries and
    // the text another spreadsheet will show.
    let a3 = sheet.cells.get(CellRef::new(2, 0)).unwrap();
    let expr = wb.formula(a3.formula.unwrap()).unwrap();
    // Printed **at A3**, which is where it lives: a stored tree's references
    // are offsets from the holding cell (`PERF-11`), so `Display` shows the
    // absolute form — `#REF!*2` here, since "two rows up" from `A1` is off the
    // sheet.
    assert_eq!(
        casual_calc_formula::print_at(expr, casual_calc_formula::stored::Origin::at(2, 0)),
        "A1*2"
    );

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
    workbook.sheets[0].validations.push(DataValidation::list(
        CellRange::new(CellRef::new(0, 0), CellRef::new(4, 0)),
        vec!["Yes".to_owned(), "No".to_owned(), "Maybe".to_owned()],
    ));
    let written = write_workbook(&workbook).unwrap();
    let wb = import_package(written).unwrap().workbook;
    let v = &wb.sheets[0].validations;
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].values, vec!["Yes", "No", "Maybe"]);
    assert_eq!(v[0].range.start, CellRef::new(0, 0));
    assert_eq!(v[0].range.end, CellRef::new(4, 0));
}

#[test]
fn conditional_formatting_keeps_its_text_colour_and_does_not_collide_on_fill() {
    use casual_calc_model::{CellRange, CellRef, CfRule, ConditionalFormat};
    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    let r = |r0, c0, r1, c1| CellRange::new(CellRef::new(r0, c0), CellRef::new(r1, c1));

    let mut preset = ConditionalFormat::new(r(0, 0, 8, 0), CfRule::LessThan(3.0), "FFC7CE");
    preset.font_color = Some("9C0006".into()); // "Light Red Fill with Dark Red Text"
    // The same fill, a different text colour. `dxf_id` was chosen by
    // `position(|f| *f == cf.fill)`, so these two shared one dxf and the second
    // rule silently took the first one's text colour.
    let mut same_fill = ConditionalFormat::new(r(0, 1, 8, 1), CfRule::GreaterThan(9.0), "FFC7CE");
    same_fill.font_color = Some("006100".into());
    same_fill.bold = true;
    // Excel's "Red Text" preset has no fill at all. With the id keyed on fill,
    // an empty fill matched nothing and fell to `unwrap_or(0)` — another rule's
    // dxf entirely.
    let mut text_only = ConditionalFormat::new(r(0, 2, 8, 2), CfRule::EqualTo(0.0), "");
    text_only.font_color = Some("FF0000".into());

    workbook.sheets[0].conditional_formats = vec![preset, same_fill, text_only];
    let written = write_workbook(&workbook).unwrap();
    let cfs = import_package(written).unwrap().workbook.sheets[0]
        .conditional_formats
        .clone();

    assert_eq!(cfs.len(), 3, "all three rules survive");
    assert_eq!(cfs[0].fill, "FFC7CE");
    assert_eq!(
        cfs[0].font_color.as_deref(),
        Some("9C0006"),
        "the text colour of Excel's most-used preset"
    );
    assert_eq!(cfs[1].fill, "FFC7CE");
    assert_eq!(
        cfs[1].font_color.as_deref(),
        Some("006100"),
        "a rule sharing a fill must not inherit the other's text colour"
    );
    assert!(cfs[1].bold, "and its bold");
    assert_eq!(cfs[2].fill, "", "a text-only rule keeps having no fill");
    assert_eq!(cfs[2].font_color.as_deref(), Some("FF0000"));
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
        CellComment::note(CellRef::new(0, 0), "First note", Some("sachin".to_owned())),
        CellComment::note(CellRef::new(3, 2), "Needs <review> & sign-off", None),
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
    // OOXML requires every note to occupy a slot in the authors list, but an
    // empty one means anonymous — so an unsigned note stays unsigned rather
    // than coming back attributed to a name the file never held.
    assert_eq!(comments[1].author, None);
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

/// The inclusive and negated `cellIs` operators — `>=`, `<=`, `<>` and "not
/// between" — are everyday rules, and neither end of the pipe carried them:
/// the writer had no arm for them and the reader dropped them on the way back.
#[test]
fn inclusive_and_negated_cell_is_rules_round_trip() {
    use casual_calc_model::{CellRange, CellRef, CfRule, ConditionalFormat};

    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    let r = CellRange::new(CellRef::new(0, 0), CellRef::new(9, 0));
    let rules = vec![
        CfRule::GreaterThanOrEqual(100.0),
        CfRule::LessThanOrEqual(3.5),
        CfRule::NotEqualTo(7.0),
        CfRule::NotBetween(2.0, 10.0),
    ];
    workbook.sheets[0].conditional_formats = rules
        .iter()
        .map(|rule| ConditionalFormat::new(r, rule.clone(), "FFD166"))
        .collect();

    let written = write_workbook(&workbook).unwrap();
    let xml = xml_of(&written, "xl/worksheets/sheet1.xml");
    assert!(xml.contains("operator=\"greaterThanOrEqual\""), "{xml}");
    assert!(xml.contains("operator=\"lessThanOrEqual\""), "{xml}");
    assert!(xml.contains("operator=\"notEqual\""), "{xml}");
    assert!(xml.contains("operator=\"notBetween\""), "{xml}");

    let back = import_package(written).unwrap().workbook.sheets[0]
        .conditional_formats
        .iter()
        .map(|c| c.rule.clone())
        .collect::<Vec<_>>();
    assert_eq!(back, rules, "a rule was lost between writing and reading");
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

#[test]
fn sheet_visibility_round_trips() {
    use casual_calc_model::SheetVisibility;

    // A hidden sheet used to come back visible: nothing parsed or wrote `state`,
    // so saving quietly exposed data its author had put away.
    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    assert!(workbook.sheets[0].visibility.is_visible());
    let written = write_workbook(&workbook).unwrap();
    assert!(
        !xml_of(&written, "xl/workbook.xml").contains("state="),
        "a visible sheet must not write a state attribute"
    );

    for state in [SheetVisibility::Hidden, SheetVisibility::VeryHidden] {
        workbook.sheets[0].visibility = state;
        let written = write_workbook(&workbook).unwrap();
        let token = state.ooxml().unwrap();
        assert!(
            xml_of(&written, "xl/workbook.xml").contains(&format!("state=\"{token}\"")),
            "{state:?} did not write state=\"{token}\""
        );
        let wb = import_package(written).unwrap().workbook;
        // veryHidden must not be flattened to hidden — the difference is the
        // whole reason the state exists.
        assert_eq!(wb.sheets[0].visibility, state);
    }
}

#[test]
fn the_1904_date_epoch_round_trips() {
    // Dropping this flag is the worst kind of loss: the serials keep their
    // values while their meaning moves by 1462 days, so every date in the file
    // is silently wrong from then on.
    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    assert!(!workbook.date1904, "the sample is a 1900-epoch workbook");
    let written = write_workbook(&workbook).unwrap();
    assert!(
        !xml_of(&written, "xl/workbook.xml").contains("date1904"),
        "the default epoch is written by omission"
    );

    workbook.date1904 = true;
    let written = write_workbook(&workbook).unwrap();
    assert!(xml_of(&written, "xl/workbook.xml").contains("date1904=\"1\""));
    assert!(import_package(written).unwrap().workbook.date1904);
}

#[test]
fn sheet_and_cell_protection_round_trip() {
    use casual_calc_model::{CellRef, SheetProtection, Style};
    use std::collections::BTreeMap;

    // A protected sheet used to come back unprotected, and locked cells came
    // back unlocked — both silently granting permissions the author withheld.
    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    let mut attrs = BTreeMap::new();
    attrs.insert("sheet".to_owned(), "1".to_owned());
    attrs.insert("formatCells".to_owned(), "0".to_owned());
    // A password hash must survive byte-for-byte: regenerating one would lock
    // the author out of their own sheet.
    attrs.insert("algorithmName".to_owned(), "SHA-512".to_owned());
    attrs.insert("hashValue".to_owned(), "abc123==".to_owned());
    attrs.insert("saltValue".to_owned(), "zzz999==".to_owned());
    attrs.insert("spinCount".to_owned(), "100000".to_owned());
    workbook.sheets[0].protection = Some(SheetProtection {
        attrs: attrs.clone(),
    });

    let at = CellRef::new(0, 0);
    let id = workbook.intern_style(Style {
        locked: Some(false),
        formula_hidden: Some(true),
        ..Default::default()
    });
    let sheet = &mut workbook.sheets[0];
    let mut cell = sheet.cells.get(at).cloned().unwrap_or_default();
    cell.style = Some(id);
    sheet.cells.set(at, cell);

    let written = write_workbook(&workbook).unwrap();
    let ws = xml_of(&written, "xl/worksheets/sheet1.xml");
    assert!(ws.contains("<sheetProtection "), "{ws:.400}");
    assert!(ws.contains("hashValue=\"abc123==\""));
    assert!(xml_of(&written, "xl/styles.xml").contains("<protection locked=\"0\" hidden=\"1\"/>"));

    let wb = import_package(written).unwrap().workbook;
    let p = wb.sheets[0]
        .protection
        .as_ref()
        .expect("protection dropped");
    assert!(p.is_enabled());
    assert_eq!(p.attrs, attrs, "an attribute was lost or rewritten");
    let round = wb.sheets[0].cells.get(at).unwrap().style.unwrap();
    let st = wb.styles.get(round).unwrap();
    assert_eq!(st.locked, Some(false));
    assert_eq!(st.formula_hidden, Some(true));
}

#[test]
fn non_list_validations_round_trip() {
    use casual_calc_model::{CellRange, CellRef, DataValidation, DvKind, DvOperator};

    // Only `type="list"` was parsed, so a file's number, date, text-length and
    // custom rules were dropped the moment it was saved — the cell kept looking
    // constrained in Excel until you reopened it and found it was not.
    let mut workbook = import_package(sample_xlsx()).unwrap().workbook;
    let r = CellRange::new(CellRef::new(0, 0), CellRef::new(9, 0));
    let mut whole = DataValidation {
        kind: DvKind::Whole,
        operator: DvOperator::Between,
        formula1: "1".into(),
        formula2: "10".into(),
        error_title: "Out of range".into(),
        error_text: "Pick 1 to 10".into(),
        prompt_title: "Quantity".into(),
        prompt_text: "How many?".into(),
        ..DataValidation::none(r)
    };
    whole.allow_blank = false;
    let custom = DataValidation {
        kind: DvKind::Custom,
        formula1: "A1>0".into(),
        ..DataValidation::none(r)
    };
    workbook.sheets[0].validations = vec![whole.clone(), custom.clone()];

    let written = write_workbook(&workbook).unwrap();
    let xml = xml_of(&written, "xl/worksheets/sheet1.xml");
    assert!(xml.contains("type=\"whole\""), "{xml:.400}");
    assert!(xml.contains("type=\"custom\""));
    assert!(xml.contains("errorTitle=\"Out of range\""));
    assert!(xml.contains("promptTitle=\"Quantity\""));
    // `allowBlank` is false here, so it must not be written.
    assert!(!xml.contains("type=\"whole\" allowBlank"));

    let back = &import_package(written).unwrap().workbook.sheets[0].validations;
    assert_eq!(back.len(), 2);
    assert_eq!(back[0], whole, "the number rule changed on the way through");
    assert_eq!(back[1], custom);
}

#[test]
fn threaded_comments_round_trip() {
    use casual_calc_model::{CellComment, CellRef, CommentReply};

    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    wb.sheets[0].comments.push(CellComment {
        at: CellRef::new(1, 2),
        text: "Is this figure final?".to_owned(),
        author: Some("Ana".to_owned()),
        created: Some("2026-08-08T09:15:00.00".to_owned()),
        resolved: false,
        replies: vec![
            CommentReply {
                text: "Checking with finance.".to_owned(),
                author: Some("Bo".to_owned()),
                created: Some("2026-08-08T09:40:00.00".to_owned()),
            },
            CommentReply {
                text: "Confirmed.".to_owned(),
                author: Some("Ana".to_owned()),
                created: Some("2026-08-08T11:02:00.00".to_owned()),
            },
        ],
    });
    // A plain note alongside it, which must stay a plain note.
    wb.sheets[0]
        .comments
        .push(CellComment::note(CellRef::new(3, 0), "just a note", None));

    let written = write_workbook(&wb).unwrap();
    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.sheets[0].comments, wb.sheets[0].comments);
}

#[test]
fn resolved_flag_and_authorless_thread_round_trip() {
    use casual_calc_model::{CellComment, CellRef, CommentReply};

    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    wb.sheets[0].comments.push(CellComment {
        at: CellRef::new(0, 0),
        text: "done with this".to_owned(),
        author: None,
        created: Some("2026-01-02T03:04:05.00".to_owned()),
        resolved: true,
        replies: vec![CommentReply {
            text: "agreed".to_owned(),
            author: None,
            created: Some("2026-01-02T03:05:00.00".to_owned()),
        }],
    });
    let written = write_workbook(&wb).unwrap();
    let back = import_package(written).unwrap().workbook;
    let thread = &back.sheets[0].comments[0];
    assert!(thread.resolved, "the resolved flag must survive a save");
    assert_eq!(thread.replies.len(), 1);
    // And an unsigned thread stays unsigned rather than acquiring a name.
    assert_eq!(thread.author, None);
}

#[test]
fn writing_is_deterministic_with_threads() {
    use casual_calc_model::{CellComment, CellRef};

    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    wb.sheets[0].comments.push(CellComment {
        at: CellRef::new(0, 0),
        text: "hello".to_owned(),
        author: Some("Ana".to_owned()),
        created: Some("2026-08-08T09:15:00.00".to_owned()),
        resolved: false,
        replies: Vec::new(),
    });
    // Threaded comments need GUIDs; if those came from a random source, two
    // saves of one workbook would differ and every commit would show churn.
    assert_eq!(write_workbook(&wb).unwrap(), write_workbook(&wb).unwrap());
}

#[test]
fn a_plain_note_does_not_pull_in_the_threaded_parts() {
    use casual_calc_model::{CellComment, CellRef};

    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    wb.sheets[0]
        .comments
        .push(CellComment::note(CellRef::new(0, 0), "note", None));
    let written = write_workbook(&wb).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(&written)).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_owned())
        .collect();
    assert!(
        !names.iter().any(|n| n.contains("threadedComment")),
        "a note with no replies or timestamp needs only the legacy part: {names:?}"
    );
    assert!(!names.iter().any(|n| n.contains("persons")));
}

#[test]
fn threaded_comment_xml_has_the_shape_excel_expects() {
    use casual_calc_model::{CellComment, CellRef, CommentReply};

    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    wb.sheets[0].comments.push(CellComment {
        at: CellRef::new(0, 0),
        text: "root".to_owned(),
        author: Some("Ana".to_owned()),
        created: Some("2026-08-08T09:15:00.00".to_owned()),
        resolved: true,
        replies: vec![CommentReply {
            text: "reply".to_owned(),
            author: Some("Bo".to_owned()),
            created: Some("2026-08-08T09:40:00.00".to_owned()),
        }],
    });
    let written = write_workbook(&wb).unwrap();
    let tc = xml_of(&written, "xl/threadedComments/threadedComment1.xml");
    let persons = xml_of(&written, "xl/persons/person1.xml");

    // Replies are siblings carrying parentId, not nested children.
    assert_eq!(tc.matches("<threadedComment ").count(), 2);
    assert!(tc.contains("ref=\"A1\""));
    assert!(tc.contains("dT=\"2026-08-08T09:15:00.00\""));
    assert!(tc.contains("done=\"1\""));
    assert!(tc.contains("parentId="), "a reply must point at its root");
    assert!(tc.contains("<text>reply</text>"));

    // The reply's parentId must be the root's id, or Excel drops the reply.
    let root_id = tc
        .split("id=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap()
        .to_owned();
    assert!(tc.contains(&format!("parentId=\"{root_id}\"")));

    // Both people are declared, and personId resolves into that list.
    assert!(persons.contains("displayName=\"Ana\""));
    assert!(persons.contains("displayName=\"Bo\""));
    for person_id in tc.split("personId=\"").skip(1) {
        let id = person_id.split('"').next().unwrap();
        assert!(
            persons.contains(&format!("id=\"{id}\"")),
            "personId {id} is not in the persons part"
        );
    }

    // And the parts are actually declared, or Excel treats the package as
    // corrupt rather than silently ignoring them.
    let types = xml_of(&written, "[Content_Types].xml");
    assert!(types.contains("threadedcomments+xml"));
    assert!(types.contains("person+xml"));
    let rels = xml_of(&written, "xl/worksheets/_rels/sheet1.xml.rels");
    assert!(rels.contains("threadedComment1.xml"));
    let wb_rels = xml_of(&written, "xl/_rels/workbook.xml.rels");
    assert!(wb_rels.contains("persons/person1.xml"));
}

#[test]
fn theme_linked_colors_round_trip_as_references() {
    use casual_calc_model::{Style, ThemeTint};

    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    // A workbook theme that is *not* the stock one, so resolving against the
    // wrong palette would show up as a different RGB.
    wb.theme_colors = vec![
        "FFFFFF", "1A1A1A", "EEEEEE", "223344", "C0392B", "27AE60", "2980B9", "8E44AD", "F39C12",
        "16A085", "0563C1", "954F72",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();

    let mut style = Style::default();
    style.set_font_color(
        Some(wb.theme_slot(4).to_owned()),
        Some(ThemeTint {
            slot: 4,
            tint_micro: 0,
        }),
    );
    let plain = wb.intern_style(style);

    let mut tinted = Style::default();
    // A tinted accent, which is what Excel's own cell styles use.
    tinted.set_fill_color(
        Some("D5E8D4".to_owned()),
        Some(ThemeTint::from_tint(5, -0.499985)),
    );
    let tinted_id = wb.intern_style(tinted);

    // A literal colour alongside, which must stay literal.
    let mut literal = Style::default();
    literal.set_font_color(Some("FF00FF".to_owned()), None);
    let literal_id = wb.intern_style(literal);

    for (row, id) in [(0, plain), (1, tinted_id), (2, literal_id)] {
        let at = casual_calc_model::CellRef::new(row, 0);
        let mut cell = wb.sheets[0].cells.get(at).cloned().unwrap();
        cell.style = Some(id);
        wb.sheets[0].cells.set(at, cell);
    }

    let written = write_workbook(&wb).unwrap();
    let styles = xml_of(&written, "xl/styles.xml");
    assert!(
        styles.contains("<color theme=\"4\"/>"),
        "an untinted theme colour writes as a bare slot: {styles}"
    );
    assert!(
        styles.contains("theme=\"5\" tint=\"-0.499985\""),
        "a tinted theme colour keeps its tint: {styles}"
    );
    assert!(
        styles.contains("rgb=\"FFFF00FF\""),
        "a literal colour stays literal: {styles}"
    );
    // The theme part must ship, or the references resolve against the reader's
    // default palette instead of this workbook's.
    let theme = xml_of(&written, "xl/theme/theme1.xml");
    assert!(theme.contains("<a:accent1><a:srgbClr val=\"C0392B\"/></a:accent1>"));
    // clrScheme lists dk1 before lt1 while `theme="N"` indexes lt1 first, so
    // this is the pair most easily written the wrong way round.
    assert!(theme.contains("<a:dk1><a:srgbClr val=\"1A1A1A\"/></a:dk1>"));
    assert!(theme.contains("<a:lt1><a:srgbClr val=\"FFFFFF\"/></a:lt1>"));

    let back = import_package(written).unwrap().workbook;
    assert_eq!(
        back.theme_colors, wb.theme_colors,
        "the theme itself survives"
    );
    let style_of = |row: u32| {
        let id = back.sheets[0]
            .cells
            .get(casual_calc_model::CellRef::new(row, 0))
            .unwrap()
            .style;
        back.styles.get(id.unwrap()).unwrap().clone()
    };
    assert_eq!(
        style_of(0).font_theme,
        Some(ThemeTint {
            slot: 4,
            tint_micro: 0
        })
    );
    assert_eq!(style_of(0).font_color.as_deref(), Some("C0392B"));
    assert_eq!(style_of(1).fill_theme.map(|t| t.slot), Some(5));
    assert_eq!(
        style_of(2).font_theme,
        None,
        "a literal colour gains no link"
    );
    assert_eq!(style_of(2).font_color.as_deref(), Some("FF00FF"));
}

#[test]
fn setting_a_literal_color_clears_a_stale_theme_link() {
    use casual_calc_model::{Style, ThemeTint};
    let mut style = Style::default();
    style.set_font_color(
        Some("C0392B".to_owned()),
        Some(ThemeTint::from_tint(4, 0.0)),
    );
    assert!(style.font_theme.is_some());
    // Repainting with a hand-picked colour must drop the link, or the file
    // would claim a theme colour while showing a different one.
    style.set_font_color(Some("123456".to_owned()), None);
    assert_eq!(style.font_theme, None);
    // And clearing the colour clears the link with it.
    style.set_font_color(None, Some(ThemeTint::from_tint(4, 0.0)));
    assert_eq!(style.font_theme, None);
}

#[test]
fn quote_prefix_and_protection_flags_round_trip() {
    use casual_calc_model::{CellRef, Style};

    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    let mut style = Style {
        quote_prefix: true,
        ..Style::default()
    };
    let quoted = wb.intern_style(style.clone());
    style = Style {
        locked: Some(false),
        formula_hidden: Some(true),
        ..Style::default()
    };
    let protected = wb.intern_style(style);
    for (row, id) in [(0, quoted), (1, protected)] {
        let at = CellRef::new(row, 0);
        let mut cell = wb.sheets[0].cells.get(at).cloned().unwrap();
        cell.style = Some(id);
        wb.sheets[0].cells.set(at, cell);
    }

    let written = write_workbook(&wb).unwrap();
    let styles = xml_of(&written, "xl/styles.xml");
    assert!(styles.contains("quotePrefix=\"1\""));
    // Excel honours a <protection> child only when applyProtection says to, so
    // writing the child without the flag stores the setting and ignores it —
    // indistinguishable from having lost it.
    assert!(
        styles.contains("applyProtection=\"1\""),
        "protection without applyProtection is silently ignored: {styles}"
    );

    let back = import_package(written).unwrap().workbook;
    let style_of = |row: u32| {
        let id = back.sheets[0]
            .cells
            .get(CellRef::new(row, 0))
            .unwrap()
            .style;
        back.styles.get(id.unwrap()).unwrap().clone()
    };
    assert!(
        style_of(0).quote_prefix,
        "a quote prefix must survive a save"
    );
    assert_eq!(style_of(1).locked, Some(false));
    assert_eq!(style_of(1).formula_hidden, Some(true));
}

#[test]
fn hyperlinks_round_trip_with_their_targets() {
    use casual_calc_model::{CellRange, CellRef, Hyperlink};

    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    let cell = |r: u32, c: u32| CellRange {
        start: CellRef::new(r, c),
        end: CellRef::new(r, c),
    };
    wb.sheets[0].hyperlinks = vec![
        Hyperlink {
            range: cell(0, 0),
            target: Some("https://example.com/a?x=1&y=2".to_owned()),
            location: None,
            tooltip: Some("Open the report".to_owned()),
            display: Some("Report".to_owned()),
        },
        // Internal: no relationship at all, just a location in this workbook.
        Hyperlink {
            range: cell(1, 0),
            target: None,
            location: Some("Sheet1!A5".to_owned()),
            tooltip: None,
            display: None,
        },
        // A second cell pointing at the *same* external address.
        Hyperlink {
            range: cell(2, 0),
            target: Some("https://example.com/a?x=1&y=2".to_owned()),
            location: None,
            tooltip: None,
            display: None,
        },
    ];

    let written = write_workbook(&wb).unwrap();
    let rels = xml_of(&written, "xl/worksheets/_rels/sheet1.xml.rels");
    // Without TargetMode="External" the URI is re-read as a path inside the
    // package, which destroys the link.
    assert!(rels.contains("TargetMode=\"External\""));
    assert_eq!(
        rels.matches("/hyperlink\"").count(),
        1,
        "two cells sharing an address need one relationship, not two: {rels}"
    );

    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.sheets[0].hyperlinks, wb.sheets[0].hyperlinks);
}

#[test]
fn a_sheet_with_links_and_no_notes_still_gets_its_rels_part() {
    use casual_calc_model::{CellRange, CellRef, Hyperlink};
    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    assert!(wb.sheets[0].comments.is_empty());
    wb.sheets[0].hyperlinks = vec![Hyperlink {
        range: CellRange {
            start: CellRef::new(0, 0),
            end: CellRef::new(0, 0),
        },
        target: Some("https://example.com".to_owned()),
        location: None,
        tooltip: None,
        display: None,
    }];
    let written = write_workbook(&wb).unwrap();
    // The rels part used to be written only for comments; a link's target lives
    // nowhere else, so without it the destination is simply gone.
    let rels = xml_of(&written, "xl/worksheets/_rels/sheet1.xml.rels");
    assert!(rels.contains("https://example.com"));
    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.sheets[0].hyperlinks.len(), 1);
}

#[test]
fn rich_text_runs_round_trip() {
    use casual_calc_model::{CellRef, CellValue, RunFont, TextRun, Underline, VertAlign};

    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    let runs = vec![
        TextRun {
            text: "Hello".to_owned(),
            font: Some(RunFont {
                bold: true,
                color: Some("FF0000".to_owned()),
                size_hp: Some(26),
                ..RunFont::default()
            }),
        },
        TextRun {
            text: " world".to_owned(),
            font: None,
        },
        TextRun {
            text: "2".to_owned(),
            font: Some(RunFont {
                vert_align: Some(VertAlign::Superscript),
                underline: Some(Underline::Double),
                name: Some("Verdana".to_owned()),
                ..RunFont::default()
            }),
        },
    ];
    let id = wb.intern_rich_text(runs.clone());
    let at = CellRef::new(0, 0);
    let mut cell = wb.sheets[0].cells.get(at).cloned().unwrap();
    cell.value = CellValue::SharedString(id);
    wb.sheets[0].cells.set(at, cell);

    // The flattened text stays available to everything that only wants
    // characters — rendering, search, CSV export.
    assert_eq!(wb.strings.get(id), Some("Hello world2"));

    let written = write_workbook(&wb).unwrap();
    let sst = xml_of(&written, "xl/sharedStrings.xml");
    assert!(sst.contains("<vertAlign val=\"superscript\"/>"));
    assert!(sst.contains("<u val=\"double\"/>"));

    let back = import_package(written).unwrap().workbook;
    let back_id = match back.sheets[0].cells.get(at).unwrap().value {
        CellValue::SharedString(id) | CellValue::InlineString(id) => id,
        ref other => panic!("expected a string, got {other:?}"),
    };
    assert_eq!(back.strings.runs(back_id), Some(runs.as_slice()));
}

#[test]
fn plain_text_does_not_become_rich() {
    use casual_calc_model::{RunFont, TextRun};
    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    // A file that wraps unformatted text in a single <r> must not create a rich
    // entry: it would write runs back for a string that has no formatting, and
    // stop deduplicating against the identical plain string.
    let id = wb.intern_rich_text(vec![TextRun {
        text: "plain".to_owned(),
        font: Some(RunFont::default()),
    }]);
    assert_eq!(wb.strings.runs(id), None);
    assert_eq!(id, wb.intern_string("plain"));
}

#[test]
fn identical_text_with_different_formatting_stays_distinct() {
    use casual_calc_model::{RunFont, TextRun};
    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    let plain = wb.intern_string("Total");
    let bold = wb.intern_rich_text(vec![TextRun {
        text: "Total".to_owned(),
        font: Some(RunFont {
            bold: true,
            ..RunFont::default()
        }),
    }]);
    // Interning on text alone would hand the second cell the first one's
    // formatting — or rather, strip it.
    assert_ne!(plain, bold);
    assert_eq!(wb.strings.get(bold), Some("Total"));
    assert!(wb.strings.runs(bold).is_some());
}

#[test]
fn cell_font_variants_round_trip() {
    use casual_calc_model::{CellRef, Style, Underline, VertAlign};

    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    let id = wb.intern_style(Style {
        underline: Some(Underline::DoubleAccounting),
        vert_align: Some(VertAlign::Subscript),
        font_family: Some(2),
        font_scheme: Some("minor".to_owned()),
        font_charset: Some(1),
        ..Style::default()
    });
    let at = CellRef::new(0, 0);
    let mut cell = wb.sheets[0].cells.get(at).cloned().unwrap();
    cell.style = Some(id);
    wb.sheets[0].cells.set(at, cell);

    let written = write_workbook(&wb).unwrap();
    let styles = xml_of(&written, "xl/styles.xml");
    assert!(styles.contains("<u val=\"doubleAccounting\"/>"));
    assert!(styles.contains("<vertAlign val=\"subscript\"/>"));

    let back = import_package(written).unwrap().workbook;
    let st = back
        .styles
        .get(back.sheets[0].cells.get(at).unwrap().style.unwrap())
        .unwrap();
    // A bool would have flattened this to a plain single underline, which is a
    // visible change to a ledger whose whole purpose is looking a certain way.
    assert_eq!(st.underline, Some(Underline::DoubleAccounting));
    assert_eq!(st.vert_align, Some(VertAlign::Subscript));
    assert_eq!(st.font_family, Some(2));
    assert_eq!(st.font_scheme.as_deref(), Some("minor"));
    assert_eq!(st.font_charset, Some(1));
}

#[test]
fn u_with_no_val_is_single_and_val_none_is_not_underlined() {
    // `<u/>` means single; `<u val="none"/>` is the one spelling that means the
    // font is not underlined at all. Reading the element's presence as truth
    // would underline the second.
    use casual_calc_model::Underline;
    assert_eq!(Underline::from_ooxml(""), Some(Underline::Single));
    assert_eq!(Underline::from_ooxml("single"), Some(Underline::Single));
    assert_eq!(Underline::from_ooxml("none"), None);
}

#[test]
fn gradient_and_pattern_fills_round_trip() {
    use casual_calc_model::{CellRef, GradientFill, GradientStop, Style, to_micro};

    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    let gradient = wb.intern_style(Style {
        fill_gradient: Some(GradientFill {
            kind: None,
            degree_micro: to_micro(90.0),
            stops: vec![
                GradientStop {
                    position_micro: 0,
                    color: "FF0000".to_owned(),
                    color_theme: None,
                },
                GradientStop {
                    position_micro: to_micro(1.0),
                    color: "0000FF".to_owned(),
                    color_theme: None,
                },
            ],
            ..GradientFill::default()
        }),
        ..Style::default()
    });
    let patterned = wb.intern_style(Style {
        fill_color: Some("112233".to_owned()),
        fill_pattern: Some("lightGrid".to_owned()),
        fill_bg_color: Some("FFEECC".to_owned()),
        ..Style::default()
    });
    // Same foreground as the pattern above but solid: these must not collapse
    // into one fill.
    let solid = wb.intern_style(Style {
        fill_color: Some("112233".to_owned()),
        ..Style::default()
    });
    assert_ne!(patterned, solid);

    for (row, id) in [(0, gradient), (1, patterned), (2, solid)] {
        let at = CellRef::new(row, 0);
        let mut cell = wb.sheets[0].cells.get(at).cloned().unwrap();
        cell.style = Some(id);
        wb.sheets[0].cells.set(at, cell);
    }

    let written = write_workbook(&wb).unwrap();
    let styles = xml_of(&written, "xl/styles.xml");
    assert!(styles.contains("<gradientFill degree=\"90\">"), "{styles}");
    assert!(styles.contains("patternType=\"lightGrid\""));
    assert!(styles.contains("bgColor"));

    let back = import_package(written).unwrap().workbook;
    let style_of = |row: u32| {
        let id = back.sheets[0]
            .cells
            .get(CellRef::new(row, 0))
            .unwrap()
            .style;
        back.styles.get(id.unwrap()).unwrap().clone()
    };
    let g = style_of(0)
        .fill_gradient
        .expect("the gradient must survive");
    assert_eq!(g.degree_micro, to_micro(90.0));
    assert_eq!(g.stops.len(), 2);
    assert_eq!(g.stops[1].color, "0000FF");
    assert_eq!(style_of(1).fill_pattern.as_deref(), Some("lightGrid"));
    assert_eq!(style_of(1).fill_bg_color.as_deref(), Some("FFEECC"));
    // The solid one keeps its colour and gains no pattern.
    assert_eq!(style_of(2).fill_color.as_deref(), Some("112233"));
    assert_eq!(style_of(2).fill_pattern, None);
}

#[test]
fn print_setup_round_trips_verbatim() {
    let source = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/sharedStrings.xml", SHARED),
        ("xl/styles.xml", STYLES),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
              <sheetPr><pageSetUpPr fitToPage="1"/></sheetPr>
              <sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData>
              <printOptions horizontalCentered="1" gridLines="1"/>
              <pageMargins left="0.25" right="0.25" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>
              <pageSetup paperSize="9" orientation="landscape" scale="80" fitToWidth="1" fitToHeight="0"/>
              <headerFooter differentFirst="1"><oddHeader>&amp;CQuarterly &amp;A</oddHeader><oddFooter>&amp;LPage &amp;P of &amp;N</oddFooter></headerFooter>
              <rowBreaks count="2" manualBreakCount="2"><brk id="10" max="16383" man="1"/><brk id="20" max="16383" man="1"/></rowBreaks>
              <colBreaks count="1" manualBreakCount="1"><brk id="3" max="1048575" man="1"/></colBreaks>
            </worksheet>"#,
        ),
    ]);
    let wb = import_package(source).unwrap().workbook;
    let print = &wb.sheets[0].print;
    assert_eq!(print.margins.get("left").map(String::as_str), Some("0.25"));
    assert_eq!(
        print.page.get("orientation").map(String::as_str),
        Some("landscape")
    );
    assert_eq!(
        print.options.get("gridLines").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        print.setup_pr.get("fitToPage").map(String::as_str),
        Some("1")
    );
    // Header/footer bodies are element text, not attributes; the `&` codes are
    // Excel's field syntax and must survive unescaped-then-reescaped.
    assert_eq!(
        print
            .header_footer_text
            .get("oddHeader")
            .map(String::as_str),
        Some("&CQuarterly &A")
    );
    // `<brk>` is identical under rowBreaks and colBreaks, so only the enclosing
    // element says which list it joins — getting that wrong silently moves a
    // page break from a row to a column.
    assert_eq!(print.row_breaks.len(), 2);
    assert_eq!(print.col_breaks.len(), 1);
    assert_eq!(print.col_breaks[0].get("id").map(String::as_str), Some("3"));

    let written = write_workbook(&wb).unwrap();
    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.sheets[0].print, wb.sheets[0].print);
}

#[test]
fn workbook_settings_round_trip_verbatim() {
    let source = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        (
            "xl/workbook.xml",
            br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
              <fileVersion appName="xl" lastEdited="7" lowestEdited="7" rupBuild="26925"/>
              <workbookPr defaultThemeVersion="166925"/>
              <workbookProtection lockStructure="1" workbookAlgorithmName="SHA-512" workbookHashValue="abc123" workbookSaltValue="salt" workbookSpinCount="100000"/>
              <bookViews><workbookView xWindow="0" yWindow="0" windowWidth="20000" windowHeight="9000" activeTab="0"/></bookViews>
              <sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets>
              <calcPr calcId="191029" iterate="1" iterateCount="50" fullCalcOnLoad="1"/>
            </workbook>"#,
        ),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/worksheets/sheet1.xml", worksheet()),
    ]);
    let wb = import_package(source).unwrap().workbook;
    let st = &wb.settings;
    assert_eq!(st.calc.get("iterateCount").map(String::as_str), Some("50"));
    // A regenerated hash locks the author out of their own workbook, so these
    // travel byte for byte rather than being re-derived.
    assert_eq!(
        st.protection.get("workbookHashValue").map(String::as_str),
        Some("abc123")
    );
    assert_eq!(st.views.len(), 1);
    assert_eq!(
        st.file_version.get("appName").map(String::as_str),
        Some("xl")
    );

    let written = write_workbook(&wb).unwrap();
    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.settings, wb.settings);
    assert!(!back.date1904);
}

#[test]
fn date1904_wins_over_a_stale_workbook_pr_entry() {
    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    // A carried-through map that still says 1904 while the interpreted flag says
    // otherwise must not win: every serial would shift by 1462 days.
    wb.settings
        .workbook_pr
        .insert("date1904".to_owned(), "1".to_owned());
    wb.date1904 = false;
    let back = import_package(write_workbook(&wb).unwrap())
        .unwrap()
        .workbook;
    assert!(!back.date1904);

    wb.date1904 = true;
    let back = import_package(write_workbook(&wb).unwrap())
        .unwrap()
        .workbook;
    assert!(back.date1904);
}

#[test]
fn external_references_and_unmodelled_parts_are_retained() {
    let source = zip_parts(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
              <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
              <Override PartName="/xl/externalLinks/externalLink1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.externalLink+xml"/>
            </Types>"#,
        ),
        ("_rels/.rels", ROOT_RELS),
        (
            "xl/workbook.xml",
            br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
              <sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets>
              <externalReferences><externalReference r:id="rId9"/></externalReferences>
            </workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
              <Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/externalLink" Target="externalLinks/externalLink1.xml"/>
            </Relationships>"#,
        ),
        ("xl/worksheets/sheet1.xml", worksheet()),
        (
            "xl/externalLinks/externalLink1.xml",
            br#"<externalLink xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><externalBook/></externalLink>"#,
        ),
    ]);
    let wb = import_package(source).unwrap().workbook;
    assert_eq!(wb.retained_parts.len(), 1);
    assert_eq!(
        wb.retained_parts[0].path,
        "xl/externalLinks/externalLink1.xml"
    );
    assert_eq!(wb.retained_refs.len(), 1);

    let written = write_workbook(&wb).unwrap();
    // The part itself survives byte for byte...
    let link = xml_of(&written, "xl/externalLinks/externalLink1.xml");
    assert!(link.contains("<externalBook/>"));
    // ...its content type is re-declared, without which Excel refuses to open
    // the package rather than ignoring the undeclared part...
    assert!(xml_of(&written, "[Content_Types].xml").contains("externalLink+xml"));
    // ...the relationship keeps its original id...
    assert!(xml_of(&written, "xl/_rels/workbook.xml.rels").contains("Id=\"rId9\""));
    // ...and the element naming it travels too, because a retained part nothing
    // points at is invisible, which is the same as having dropped it.
    assert!(xml_of(&written, "xl/workbook.xml").contains("<externalReference r:id=\"rId9\"/>"));

    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.retained_parts, wb.retained_parts);
    assert_eq!(back.retained_refs, wb.retained_refs);
}

/// An `externalLink` whose target is another *file* rather than a part.
///
/// The workbook on the other end of `TargetMode="External"` is real data the
/// author put there, and nothing in the model represents it — so retention is
/// the only thing standing between a save and a formula that used to read
/// `[1]Sheet1!A1` and now reads nothing. It is not a part: there is no content
/// type, no bytes, and `file:///other.xlsx` resolved against `xl/workbook.xml`
/// is the path `xl/file:/other.xlsx`, which names nothing in any package.
#[test]
fn an_external_relationship_survives_with_its_target_mode() {
    let source = zip_parts(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
              <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
            </Types>"#,
        ),
        ("_rels/.rels", ROOT_RELS),
        (
            "xl/workbook.xml",
            br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
              <sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets>
              <externalReferences><externalReference r:id="rId9"/></externalReferences>
            </workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
              <Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/externalLink" Target="file:///other.xlsx" TargetMode="External"/>
            </Relationships>"#,
        ),
        // Deliberately plain, so the only thing the report could possibly hold
        // is the external relationship.
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#,
        ),
        // A sheet hangs external relationships too — a linked OLE object — and
        // a worksheet's `.rels` is written by a different function from the
        // workbook's, so one of them getting the mode right proves nothing
        // about the other.
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject" Target="http://example.com/book.xlsx" TargetMode="External"/>
            </Relationships>"#,
        ),
    ]);
    let imported = import_package(source).unwrap();
    let wb = imported.workbook;

    // Retained, with its mode — and *not* as a part, because there are no bytes
    // in the package to keep.
    assert!(wb.retained_parts.is_empty(), "{:?}", wb.retained_parts);
    assert_eq!(wb.retained_rels.len(), 2, "{:?}", wb.retained_rels);
    let rel = wb
        .retained_rels
        .iter()
        .find(|r| r.id == "rId9")
        .expect("the external link");
    assert_eq!(rel.source, "xl/workbook.xml");
    assert!(rel.rel_type.ends_with("/externalLink"), "{}", rel.rel_type);
    assert_eq!(rel.target, "file:///other.xlsx");
    assert!(rel.external, "the target is a URI, not a path in the zip");
    // Nothing left the system, so nothing is reported as having left it — the
    // Omitted + NotRetained pair docs/34 says is the only silent-loss shape.
    assert!(
        imported.report.entries().is_empty(),
        "{:?}",
        imported.report.entries()
    );

    let written = write_workbook(&wb).unwrap();
    let rels = xml_of(&written, "xl/_rels/workbook.xml.rels");
    // The id, because `<externalReference r:id="rId9"/>` names it; the mode,
    // because without it the URI is read back as a path inside the package and
    // the reference is destroyed.
    assert!(
        rels.contains(
            r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/externalLink" Target="file:///other.xlsx" TargetMode="External"/>"#
        ),
        "{rels}"
    );
    assert!(xml_of(&written, "xl/workbook.xml").contains("<externalReference r:id=\"rId9\"/>"));
    let sheet_rels = xml_of(&written, "xl/worksheets/_rels/sheet1.xml.rels");
    assert!(
        sheet_rels.contains(r#"Target="http://example.com/book.xlsx" TargetMode="External"/>"#),
        "{sheet_rels}"
    );

    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.retained_rels, wb.retained_rels);
    assert_eq!(back.retained_refs, wb.retained_refs);
}

/// The value of an attribute on the first element in `xml` that carries it.
fn attr_after(xml: &str, from: &str, attr: &str) -> String {
    let at = xml
        .find(from)
        .unwrap_or_else(|| panic!("no {from} in {xml}"));
    let rest = &xml[at..];
    let at = rest
        .find(attr)
        .unwrap_or_else(|| panic!("no {attr} after {from}"));
    let rest = &rest[at + attr.len()..];
    rest[..rest.find('"').expect("a closing quote")].to_owned()
}

/// The `Id` of the single relationship in `rels` whose `Type` ends `suffix`.
fn rel_id_of_type(rels: &str, suffix: &str) -> String {
    let mut found: Vec<String> = Vec::new();
    for element in rels.split("<Relationship ").skip(1) {
        let element = &element[..element.find("/>").expect("a closed element")];
        let ty = attr_after(element, "Type=\"", "Type=\"");
        if ty.ends_with(suffix) {
            found.push(attr_after(element, "Id=\"", "Id=\""));
        }
    }
    assert_eq!(found.len(), 1, "expected one {suffix} in {rels}");
    found.remove(0)
}

/// Every `Id` in a `.rels` part, in document order.
fn rel_ids(rels: &str) -> Vec<&str> {
    let mut ids = Vec::new();
    let mut rest = rels;
    while let Some(at) = rest.find("Id=\"") {
        rest = &rest[at + 4..];
        let end = rest.find('"').expect("a closing quote");
        ids.push(&rest[..end]);
        rest = &rest[end..];
    }
    ids
}

/// `Id` is an `xsd:ID`: a repeat within one part is a package Excel offers to
/// repair, and repairing it drops relationships. Collected in order so a repeat
/// is visible as a repeat rather than inferred from a count.
fn assert_ids_are_unique(rels: &str) {
    let ids = rel_ids(rels);
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for id in &ids {
        assert!(seen.insert(id), "{id} written twice in one .rels: {ids:?}");
    }
}

/// Entity references survive the reader.
///
/// quick-xml 0.41 stopped inlining entity references in `Event::Text` and began
/// emitting each as its own `Event::GeneralRef`. Nothing failed to compile: the
/// `Text`-only arms kept working and simply **dropped** every `&amp;`, `&lt;`
/// and `&#65;` in the document. The upgrade was taken for two high-severity
/// advisories in the parser (RUSTSEC-2026-0194/0195), so the risk of *not*
/// upgrading was real — and the cost of upgrading carelessly was every
/// ampersand in every workbook, silently.
///
/// Both spellings, because a producer may write either, and in three places
/// that read text through different code paths.
#[test]
fn entity_references_in_text_survive_the_reader() {
    use casual_calc_model::CellRef;

    let source = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        (
            "xl/sharedStrings.xml",
            br#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>Tom &amp; Jerry &lt;b&gt; &#65;&#x42;</t></si></sst>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>
              <row r="1">
                <c r="A1" t="s"><v>0</v></c>
                <c r="B1" t="inlineStr"><is><t>inline &amp; &lt;here&gt;</t></is></c>
                <c r="C1"><f>IF(A1&lt;&gt;"","yes &amp; no","")</f></c>
              </row>
            </sheetData></worksheet>"#,
        ),
    ]);
    let wb = import_package(source).unwrap().workbook;

    let shared = wb.sheets[0].cells.get(CellRef::new(0, 0)).unwrap();
    let text = match &shared.value {
        casual_calc_model::CellValue::SharedString(id) => wb.strings.get(*id).unwrap_or_default(),
        other => panic!("A1 should be a shared string, got {other:?}"),
    };
    assert_eq!(
        text, "Tom & Jerry <b> AB",
        "named and numeric entities both resolve in a shared string"
    );
    let sheet = &wb.sheets[0];
    // Inline text is read by a different arm from a shared string, so one
    // passing says nothing about the other.
    let inline = sheet.cells.get(CellRef::new(0, 1)).unwrap();
    let inline_text = match &inline.value {
        casual_calc_model::CellValue::InlineString(id) => wb.strings.get(*id).unwrap_or_default(),
        other => panic!("B1 should be inline text, got {other:?}"),
    };
    assert_eq!(
        inline_text, "inline & <here>",
        "entities resolve in inline text too, which a different arm reads"
    );
    // A formula's `<>` operator is written `&lt;&gt;` in the file. Losing it
    // turns an inequality into something else entirely — the quietest possible
    // way for a spreadsheet to start giving different answers.
    let formula = sheet.cells.get(CellRef::new(0, 2)).unwrap();
    let expr = wb.formula(formula.formula.unwrap()).unwrap().to_string();
    assert!(
        expr.contains("<>") && expr.contains('&'),
        "the formula kept its operator and its ampersand: {expr:?}"
    );
}

/// FID-21 — a relationship id this writer mints must step aside for a retained
/// one, rather than being emitted twice.
///
/// `Id` is an `xsd:ID`, so two `<Relationship>` elements sharing one in a single
/// `.rels` is not a duplicate to be ignored: Excel reports the package as
/// needing repair, and repairing it drops relationships. The retained id is the
/// one that cannot move, because `<externalReference r:id="rId1"/>` names it and
/// travels verbatim — so the minted ids are the ones that give way, exactly as
/// `root_rels` already does for the workbook's own relationship.
///
/// Two sheets and two colliding ids, because an allocator that merely starts one
/// higher survives a single collision and fails the second.
#[test]
fn a_minted_relationship_id_steps_aside_for_a_retained_one() {
    const EXTERNAL: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/externalLink";
    const WORKSHEET: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
    let source = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        (
            "xl/workbook.xml",
            br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
              <sheets>
                <sheet name="Sheet1" sheetId="1" r:id="rId7"/>
                <sheet name="Sheet2" sheetId="2" r:id="rId8"/>
              </sheets>
              <externalReferences><externalReference r:id="rId1"/><externalReference r:id="rId2"/></externalReferences>
            </workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
              <Relationship Id="rId8" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/>
              <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/externalLink" Target="file:///first.xlsx" TargetMode="External"/>
              <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/externalLink" Target="file:///second.xlsx" TargetMode="External"/>
            </Relationships>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#,
        ),
        (
            "xl/worksheets/sheet2.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>2</v></c></row></sheetData></worksheet>"#,
        ),
    ]);
    let wb = import_package(source).unwrap().workbook;
    assert_eq!(wb.sheets.len(), 2);
    assert_eq!(wb.retained_rels.len(), 2, "{:?}", wb.retained_rels);

    let written = write_workbook(&wb).unwrap();
    let rels = xml_of(&written, "xl/_rels/workbook.xml.rels");

    assert_ids_are_unique(&rels);

    // The retained ids kept, because the `<externalReference>` elements name
    // them and were written out verbatim.
    for (id, target) in [
        ("rId1", "file:///first.xlsx"),
        ("rId2", "file:///second.xlsx"),
    ] {
        assert!(
            rels.contains(&format!(
                r#"<Relationship Id="{id}" Type="{EXTERNAL}" Target="{target}" TargetMode="External"/>"#
            )),
            "{rels}"
        );
    }
    let workbook_xml = xml_of(&written, "xl/workbook.xml");
    assert!(
        workbook_xml
            .contains(r#"<externalReference r:id="rId1"/><externalReference r:id="rId2"/>"#),
        "{workbook_xml}"
    );

    // And each sheet's `r:id` still resolves — the ids moved in `workbook.xml`
    // and `workbook.xml.rels` together, or the sheet points at nothing.
    let mut sheet_ids: Vec<&str> = Vec::new();
    let mut rest = workbook_xml.as_str();
    while let Some(at) = rest.find("<sheet ") {
        rest = &rest[at..];
        let tag_end = rest.find("/>").expect("a closed <sheet>");
        let tag = &rest[..tag_end];
        let at = tag.find("r:id=\"").expect("a sheet names its part");
        let after = &tag[at + 6..];
        sheet_ids.push(&after[..after.find('"').expect("a closing quote")]);
        rest = &rest[tag_end..];
    }
    assert_eq!(sheet_ids.len(), 2, "{workbook_xml}");
    for (i, id) in sheet_ids.iter().enumerate() {
        assert!(
            rels.contains(&format!(
                r#"<Relationship Id="{id}" Type="{WORKSHEET}" Target="worksheets/sheet{}.xml"/>"#,
                i + 1
            )),
            "sheet {} names {id}, which is not a worksheet relationship: {rels}",
            i + 1
        );
    }

    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.sheets.len(), 2);
    assert_eq!(back.retained_rels, wb.retained_rels);
    assert_eq!(back.retained_refs, wb.retained_refs);
}

/// FID-21 on a worksheet's `.rels`, where the same collision is far easier to
/// reach: `sheet_rels` mints a bare `rId1`/`rId2`/`rId3` for the parts behind a
/// note, and any retained relationship the sheet carried in — a linked OLE
/// object, a picture's drawing — is appended verbatim beside them.
///
/// The duplicate `Id` is the lesser half. `<legacyDrawing r:id="rId1"/>` in the
/// worksheet names the VML that draws the note markers, and with two `rId1` in
/// the part it resolves to whichever Excel reaches first — so a sheet with a
/// picture and a note loses the marker, and the note becomes unreachable while
/// still being in the file.
#[test]
fn a_note_and_a_retained_sheet_relationship_do_not_both_claim_rid1() {
    use casual_calc_model::{CellComment, CellRef, RetainedRel};

    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    wb.sheets[0].comments.push(CellComment {
        at: CellRef::new(0, 0),
        text: "Check this".to_owned(),
        author: Some("Ana".to_owned()),
        created: None,
        resolved: false,
        replies: Vec::new(),
    });
    // What an imported sheet carrying a linked object leaves behind: external,
    // so it needs no bytes in the package, and named `rId1` because that is
    // what a producer numbering from one calls its first relationship.
    wb.retained_rels.push(RetainedRel {
        id: "rId1".to_owned(),
        source: "xl/worksheets/sheet1.xml".to_owned(),
        rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject"
            .to_owned(),
        target: "http://example.com/thing.bin".to_owned(),
        external: true,
    });

    let written = write_workbook(&wb).unwrap();
    let rels = xml_of(&written, "xl/worksheets/_rels/sheet1.xml.rels");
    assert_ids_are_unique(&rels);

    // The retained one keeps `rId1`, because the sheet's own carried content
    // names it and travels verbatim.
    assert!(
        rels.contains(r#"Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject""#),
        "{rels}"
    );

    // And the note still resolves: whatever id the VML took, that is the id the
    // worksheet's `<legacyDrawing>` names.
    let vml = rel_id_of_type(&rels, "/vmlDrawing");
    assert_ne!(vml, "rId1", "the VML must have stepped aside: {rels}");
    let sheet_xml = xml_of(&written, "xl/worksheets/sheet1.xml");
    assert_eq!(
        attr_after(&sheet_xml, "<legacyDrawing", "r:id=\""),
        vml,
        "the note marker points at something other than its VML: {rels}"
    );

    let back = import_package(written).unwrap().workbook;
    assert!(
        back.sheets[0]
            .comments
            .iter()
            .any(|c| c.text == "Check this"),
        "the note survived: {:?}",
        back.sheets[0].comments
    );
    assert!(
        back.retained_rels.iter().any(|r| r.id == "rId1"
            && r.source == "xl/worksheets/sheet1.xml"
            && r.rel_type.ends_with("/oleObject")),
        "{:?}",
        back.retained_rels
    );
}

/// FID-20 — a `/hyperlink` is modelled only when a *worksheet* declares it.
///
/// `Sheet::hyperlinks` carries the links on cells, resolved from the worksheet's
/// own `.rels` and re-minted on write, so retaining those too would write each
/// one twice. But the same relationship type also hangs off a **drawing** — the
/// web address behind a clickable picture — and nothing in the model carries
/// that one. Skipping on relationship type alone dropped it with the others.
///
/// The drawing's bytes are retained, so `<a:hlinkClick r:id="rId3"/>` inside it
/// survives the save while the relationship it names does not: a dangling `r:id`
/// in a part this writer emits verbatim, and a picture that has quietly stopped
/// being a link. Neither `Omitted` nor counted, which is the one shape
/// [34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md) forbids.
#[test]
fn a_hyperlink_on_a_drawing_is_not_modelled_and_so_is_retained() {
    let source = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData><drawing r:id="rId1"/></worksheet>"#,
        ),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/>
            </Relationships>"#,
        ),
        (
            "xl/drawings/drawing1.xml",
            br#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><xdr:twoCellAnchor><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="1" name="Picture 1"><a:hlinkClick r:id="rId3"/></xdr:cNvPr><xdr:cNvPicPr/></xdr:nvPicPr></xdr:pic></xdr:twoCellAnchor></xdr:wsDr>"#,
        ),
        (
            "xl/drawings/_rels/drawing1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/report" TargetMode="External"/>
            </Relationships>"#,
        ),
    ]);
    let imported = import_package(source).unwrap();
    let wb = imported.workbook;

    // The model carries no hyperlink, because the link is not on a cell.
    assert!(
        wb.sheets[0].hyperlinks.is_empty(),
        "{:?}",
        wb.sheets[0].hyperlinks
    );
    let link = wb
        .retained_rels
        .iter()
        .find(|r| r.rel_type.ends_with("/hyperlink"))
        .unwrap_or_else(|| {
            panic!(
                "the drawing's hyperlink was dropped: {:?}",
                wb.retained_rels
            )
        });
    assert_eq!(link.source, "xl/drawings/drawing1.xml");
    assert_eq!(link.id, "rId3");
    assert_eq!(link.target, "https://example.com/report");
    assert!(
        link.external,
        "a web address is a URI, not a path in the zip"
    );
    // Nothing left the system, so nothing is reported as having left it. Stated
    // as the `Omitted` + `NotRetained` pair rather than an empty report, because
    // the fixture's `definedName` is reported as `Mapped` and that is not a loss.
    use casual_calc_import::{ModelOutcome, RetentionOutcome};
    assert!(
        !imported
            .report
            .entries()
            .iter()
            .any(|e| e.model == ModelOutcome::Omitted
                && e.retention == RetentionOutcome::NotRetained),
        "{:?}",
        imported.report.entries()
    );

    // And it is still there on the way out: the `<a:hlinkClick r:id="rId3"/>`
    // inside the retained drawing must not be left naming nothing.
    let written = write_workbook(&wb).unwrap();
    let rels = xml_of(&written, "xl/drawings/_rels/drawing1.xml.rels");
    assert_ids_are_unique(&rels);
    assert_eq!(rel_id_of_type(&rels, "/hyperlink"), "rId3", "{rels}");
    assert!(
        rels.contains(r#"Target="https://example.com/report" TargetMode="External"/>"#),
        "{rels}"
    );

    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.retained_rels, wb.retained_rels);
}

/// The other half of the same rule: a hyperlink is *modelled*, so it must not
/// also be retained.
///
/// Without this a fix that simply keeps every external relationship passes the
/// test above and writes the link twice — once from `sheet.hyperlinks` with a
/// freshly minted id, once verbatim — which is two `<Relationship>` entries
/// pointing at one URL and, when the ids collide, a package Excel repairs.
#[test]
fn a_hyperlink_is_modelled_and_so_is_not_retained() {
    let source = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
              <sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData>
              <hyperlinks><hyperlink ref="A1" r:id="rId1"/></hyperlinks>
            </worksheet>"#,
        ),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/" TargetMode="External"/>
            </Relationships>"#,
        ),
    ]);
    let wb = import_package(source).unwrap().workbook;
    assert_eq!(
        wb.sheets[0].hyperlinks[0].target.as_deref(),
        Some("https://example.com/")
    );
    assert!(wb.retained_rels.is_empty(), "{:?}", wb.retained_rels);

    let written = write_workbook(&wb).unwrap();
    let rels = xml_of(&written, "xl/worksheets/_rels/sheet1.xml.rels");
    assert_eq!(
        rels.matches("https://example.com/").count(),
        1,
        "the link is written once, from the model: {rels}"
    );

    let back = import_package(written).unwrap().workbook;
    assert!(back.retained_rels.is_empty(), "{:?}", back.retained_rels);
    assert_eq!(back.sheets[0].hyperlinks, wb.sheets[0].hyperlinks);
}

#[test]
fn a_chart_survives_a_save_even_though_nothing_models_it() {
    let source = zip_parts(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
              <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
              <Override PartName="/xl/drawings/drawing1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawing+xml"/>
              <Override PartName="/xl/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/>
            </Types>"#,
        ),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
              <sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData>
              <drawing r:id="rId7"/>
            </worksheet>"#,
        ),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/>
            </Relationships>"#,
        ),
        (
            "xl/drawings/drawing1.xml",
            br#"<wsDr xmlns="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing"><twoCellAnchor/></wsDr>"#,
        ),
        (
            "xl/drawings/_rels/drawing1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart1.xml"/>
            </Relationships>"#,
        ),
        (
            "xl/charts/chart1.xml",
            br#"<chartSpace xmlns="http://schemas.openxmlformats.org/drawingml/2006/chart"><chart/></chartSpace>"#,
        ),
    ]);
    let wb = import_package(source).unwrap().workbook;
    // Retention is transitive: the drawing reaches the chart through its own
    // rels, and keeping the drawing while dropping the chart leaves a reference
    // to nothing, which Excel reports as a repair.
    let paths: Vec<&str> = wb.retained_parts.iter().map(|p| p.path.as_str()).collect();
    assert!(paths.contains(&"xl/drawings/drawing1.xml"), "{paths:?}");
    assert!(paths.contains(&"xl/charts/chart1.xml"), "{paths:?}");

    let written = write_workbook(&wb).unwrap();
    assert!(xml_of(&written, "xl/charts/chart1.xml").contains("<chart/>"));
    // The sheet still points at the drawing, or the chart is in the package and
    // on no sheet.
    assert!(xml_of(&written, "xl/worksheets/sheet1.xml").contains("<drawing r:id=\"rId7\"/>"));
    assert!(xml_of(&written, "xl/worksheets/_rels/sheet1.xml.rels").contains("drawing1.xml"));
    // And the drawing still points at the chart.
    assert!(xml_of(&written, "xl/drawings/_rels/drawing1.xml.rels").contains("chart1.xml"));

    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.retained_parts.len(), wb.retained_parts.len());
    assert_eq!(back.sheets[0].retained_refs, wb.sheets[0].retained_refs);
}

#[test]
fn tables_round_trip_with_their_columns_and_style() {
    let source = zip_parts(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
              <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
              <Override PartName="/xl/tables/table1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml"/>
            </Types>"#,
        ),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
              <sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Region</t></is></c></row></sheetData>
              <tableParts count="1"><tablePart r:id="rId4"/></tableParts>
            </worksheet>"#,
        ),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table1.xml"/>
            </Relationships>"#,
        ),
        (
            "xl/tables/table1.xml",
            br#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="1" name="Sales" displayName="Sales" ref="A1:C4" totalsRowCount="1">
              <autoFilter ref="A1:C3"/>
              <tableColumns count="3">
                <tableColumn id="1" name="Region" totalsRowLabel="Total"/>
                <tableColumn id="2" name="Amount" totalsRowFunction="sum"/>
                <tableColumn id="3" name="Margin"><calculatedColumnFormula>Sales[Amount]*0.1</calculatedColumnFormula></tableColumn>
              </tableColumns>
              <tableStyleInfo name="TableStyleMedium2" showRowStripes="1" showColumnStripes="0"/>
            </table>"#,
        ),
    ]);
    let wb = import_package(source).unwrap().workbook;
    let table = &wb.sheets[0].tables[0];
    assert_eq!(table.name, "Sales");
    assert_eq!(table.totals_row_count, 1);
    assert_eq!(table.columns.len(), 3);
    assert_eq!(table.columns[1].totals_row_function.as_deref(), Some("sum"));
    // A calculated column's formula is element text, not an attribute.
    assert_eq!(
        table.columns[2].calculated_column_formula.as_deref(),
        Some("Sales[Amount]*0.1")
    );
    assert_eq!(
        table.style.get("name").map(String::as_str),
        Some("TableStyleMedium2")
    );

    let written = write_workbook(&wb).unwrap();
    // Without <tableParts> the part is in the package but attached to no sheet,
    // so Excel shows a plain range.
    assert!(xml_of(&written, "xl/worksheets/sheet1.xml").contains("<tableParts count=\"1\">"));
    assert!(xml_of(&written, "xl/worksheets/_rels/sheet1.xml.rels").contains("tables/table1.xml"));
    assert!(xml_of(&written, "[Content_Types].xml").contains("spreadsheetml.table+xml"));
    let part = xml_of(&written, "xl/tables/table1.xml");
    assert!(part.contains("name=\"Sales\""));
    assert!(part.contains("<calculatedColumnFormula>"));

    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.sheets[0].tables, wb.sheets[0].tables);
}

#[test]
fn legacy_font_effects_round_trip_on_fonts_and_runs() {
    use casual_calc_model::{CellRef, CellValue, RunFont, Style, TextRun};

    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    let id = wb.intern_style(Style {
        font_outline: true,
        font_shadow: true,
        font_condense: true,
        font_extend: true,
        ..Style::default()
    });
    let at = CellRef::new(0, 0);
    let mut cell = wb.sheets[0].cells.get(at).cloned().unwrap();
    cell.style = Some(id);
    wb.sheets[0].cells.set(at, cell);

    let runs = vec![TextRun {
        text: "old".to_owned(),
        font: Some(RunFont {
            outline: true,
            shadow: true,
            ..RunFont::default()
        }),
    }];
    let sid = wb.intern_rich_text(runs.clone());
    let at2 = CellRef::new(1, 0);
    let mut cell2 = wb.sheets[0].cells.get(at2).cloned().unwrap();
    cell2.value = CellValue::SharedString(sid);
    wb.sheets[0].cells.set(at2, cell2);

    let written = write_workbook(&wb).unwrap();
    let back = import_package(written).unwrap().workbook;
    let st = back
        .styles
        .get(back.sheets[0].cells.get(at).unwrap().style.unwrap())
        .unwrap();
    // No current Excel exposes these, but a file carrying one is usually old
    // and irreplaceable; dropping it is the same silent edit as dropping any
    // other formatting.
    assert!(st.font_outline && st.font_shadow && st.font_condense && st.font_extend);
    let back_sid = match back.sheets[0].cells.get(at2).unwrap().value {
        CellValue::SharedString(id) | CellValue::InlineString(id) => id,
        ref other => panic!("expected a string, got {other:?}"),
    };
    assert_eq!(back.strings.runs(back_sid), Some(runs.as_slice()));
}

#[test]
fn two_fonts_differing_only_in_a_legacy_effect_stay_distinct() {
    use casual_calc_model::Style;
    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    // The dedup key is a struct precisely so adding a property and forgetting
    // the key cannot silently merge two different fonts.
    let plain = wb.intern_style(Style {
        bold: true,
        ..Style::default()
    });
    let shadowed = wb.intern_style(Style {
        bold: true,
        font_shadow: true,
        ..Style::default()
    });
    assert_ne!(plain, shadowed);
    let written = write_workbook(&wb).unwrap();
    let styles = xml_of(&written, "xl/styles.xml");
    assert_eq!(styles.matches("<shadow/>").count(), 1);
}

#[test]
fn unevaluated_filters_and_sort_state_survive_without_hiding_rows() {
    use casual_calc_model::FilterRule;
    let source = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
              <sheetData>
                <row r="1"><c r="A1" t="inlineStr"><is><t>H</t></is></c></row>
                <row r="2"><c r="A2"><v>5</v></c></row>
                <row r="3"><c r="A3"><v>9</v></c></row>
              </sheetData>
              <autoFilter ref="A1:A3"><filterColumn colId="0"><top10 val="1" percent="0" top="1"/></filterColumn></autoFilter>
              <sortState ref="A2:A3"><sortCondition ref="A2:A3" descending="1"/></sortState>
            </worksheet>"#,
        ),
    ]);
    let wb = import_package(source).unwrap().workbook;
    let sheet = &wb.sheets[0];
    let rule = sheet.auto_filter.as_ref().unwrap().rules.get(&0).unwrap();
    assert!(matches!(rule, FilterRule::Unevaluated { element, .. } if element == "top10"));
    // Not evaluated, so every row passes. Hiding rows on a rule we do not
    // understand would be a guess; showing them all is visibly incomplete,
    // which is the failure worth having.
    assert!(rule.matches("5", Some(5.0)));
    assert!(rule.matches("9", Some(9.0)));

    let sort = sheet.sort_state.as_ref().unwrap();
    assert_eq!(sort.conditions.len(), 1);
    assert_eq!(
        sort.conditions[0].get("descending").map(String::as_str),
        Some("1")
    );

    let written = write_workbook(&wb).unwrap();
    let xml = xml_of(&written, "xl/worksheets/sheet1.xml");
    // Re-emitted exactly, so Excel applies the filter it wrote even though we
    // never evaluated it.
    assert!(xml.contains("<top10"));
    assert!(xml.contains("<sortCondition"));
    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.sheets[0].sort_state, wb.sheets[0].sort_state);
    assert_eq!(back.sheets[0].auto_filter, wb.sheets[0].auto_filter);
}

#[test]
fn carried_sheet_elements_keep_their_wrappers() {
    let source = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
              <dimension ref="A1:C9"/>
              <sheetViews><sheetView workbookViewId="0"><selection activeCell="B2" sqref="B2"/></sheetView></sheetViews>
              <sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData>
              <protectedRanges><protectedRange sqref="A1:B2" name="R1" algorithmName="SHA-512" hashValue="abc" saltValue="s" spinCount="100000"/></protectedRanges>
              <ignoredErrors><ignoredError sqref="A1" numberStoredAsText="1"/></ignoredErrors>
            </worksheet>"#,
        ),
    ]);
    let wb = import_package(source).unwrap().workbook;
    let names: Vec<&str> = wb.sheets[0]
        .carried
        .iter()
        .map(|(n, _)| n.as_str())
        .collect();
    for expected in ["dimension", "selection", "protectedRange", "ignoredError"] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }

    let written = write_workbook(&wb).unwrap();
    let xml = xml_of(&written, "xl/worksheets/sheet1.xml");
    // The wrappers carry nothing themselves, so they are synthesized — writing
    // the child alone is invalid and Excel refuses the package.
    assert!(xml.contains("<protectedRanges><protectedRange"));
    assert!(xml.contains("<ignoredErrors><ignoredError"));
    // selection belongs inside sheetView, not beside it.
    assert!(xml.contains("<selection"));
    assert!(
        xml.find("<selection").unwrap() < xml.find("</sheetView>").unwrap(),
        "selection must sit inside sheetView"
    );
    // A protected range holds a password hash; regenerating one would lock the
    // author out of their own range.
    assert!(xml.contains("hashValue=\"abc\""));

    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.sheets[0].carried, wb.sheets[0].carried);
}

#[test]
fn alignment_view_and_format_attributes_round_trip() {
    let source = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/sharedStrings.xml", SHARED),
        (
            "xl/styles.xml",
            br#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
              <fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>
              <fills count="1"><fill><patternFill patternType="none"/></fill></fills>
              <borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>
              <cellXfs count="2"><xf numFmtId="0"/>
                <xf numFmtId="0" applyAlignment="1"><alignment shrinkToFit="1" readingOrder="2" justifyLastLine="1" relativeIndent="3"/></xf>
              </cellXfs></styleSheet>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
              <sheetViews><sheetView workbookViewId="0" rightToLeft="1" showFormulas="1" showZeros="0" tabSelected="1"/></sheetViews>
              <sheetFormatPr defaultRowHeight="15" zeroHeight="1" thickTop="1" outlineLevelRow="2"/>
              <sheetData><row r="1"><c r="A1" s="1"><v>1</v></c></row></sheetData>
            </worksheet>"#,
        ),
    ]);
    let wb = import_package(source).unwrap().workbook;
    let view = &wb.sheets[0].view;
    assert!(view.right_to_left, "an RTL sheet must not come back LTR");
    assert!(view.show_formulas);
    // showZeros defaults to true, so only an explicit "0" hides them.
    assert!(view.hide_zeros);
    assert!(view.tab_selected);
    assert_eq!(
        wb.sheets[0].format_pr.get("zeroHeight").map(String::as_str),
        Some("1")
    );
    // The two interpreted defaults are removed from the carried map, or they
    // would be written twice.
    assert!(!wb.sheets[0].format_pr.contains_key("defaultRowHeight"));

    let style = wb
        .styles
        .get(
            wb.sheets[0]
                .cells
                .get(casual_calc_model::CellRef::new(0, 0))
                .unwrap()
                .style
                .unwrap(),
        )
        .unwrap();
    assert!(style.shrink_to_fit);
    assert!(style.justify_last_line);
    assert_eq!(style.reading_order, Some(2));
    assert_eq!(style.relative_indent, Some(3));

    let back = import_package(write_workbook(&wb).unwrap())
        .unwrap()
        .workbook;
    assert_eq!(back.sheets[0].view, wb.sheets[0].view);
    assert_eq!(back.sheets[0].format_pr, wb.sheets[0].format_pr);
}

#[test]
fn inside_borders_round_trip() {
    use casual_calc_model::{BorderEdge, Borders, CellRef, Style};
    let mut wb = import_package(sample_xlsx()).unwrap().workbook;
    let edge = |style: &str| {
        Some(BorderEdge {
            style: style.to_owned(),
            color: None,
        })
    };
    let id = wb.intern_style(Style {
        border: Some(Borders {
            left: edge("thin"),
            // `horizontal` and `vertical` inside <border> are the *inside*
            // rules of a range border, not alignment — the format reuses two
            // names that mean something else on <alignment>.
            inside_horizontal: edge("hair"),
            inside_vertical: edge("dotted"),
            ..Borders::default()
        }),
        ..Style::default()
    });
    let at = CellRef::new(0, 0);
    let mut cell = wb.sheets[0].cells.get(at).cloned().unwrap();
    cell.style = Some(id);
    wb.sheets[0].cells.set(at, cell);

    let written = write_workbook(&wb).unwrap();
    let styles = xml_of(&written, "xl/styles.xml");
    assert!(styles.contains("<horizontal style=\"hair\">"), "{styles}");
    assert!(styles.contains("<vertical style=\"dotted\">"));

    let back = import_package(written).unwrap().workbook;
    let b = back
        .styles
        .get(back.sheets[0].cells.get(at).unwrap().style.unwrap())
        .unwrap()
        .border
        .clone()
        .unwrap();
    assert_eq!(b.inside_horizontal.unwrap().style, "hair");
    assert_eq!(b.inside_vertical.unwrap().style, "dotted");
}

/// A table filters independently of the sheet it sits on, so its rules have to
/// live on the table. Storing only the `ref` string left a table's header
/// buttons with nowhere to keep a rule: clicking one found no filter and
/// offered no values.
#[test]
fn a_table_carries_its_own_filter_rules() {
    use casual_calc_model::FilterRule;

    let source = zip_parts(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
              <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
              <Override PartName="/xl/tables/table1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml"/>
            </Types>"#,
        ),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
              <sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Region</t></is></c></row></sheetData>
              <tableParts count="1"><tablePart r:id="rId4"/></tableParts>
            </worksheet>"#,
        ),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table1.xml"/>
            </Relationships>"#,
        ),
        (
            "xl/tables/table1.xml",
            br#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="1" name="Sales" displayName="Sales" ref="A1:B4">
              <autoFilter ref="A1:B4">
                <filterColumn colId="0"><filters><filter val="West"/><filter val="East"/></filters></filterColumn>
                <filterColumn colId="1"><customFilters><customFilter operator="greaterThan" val="100"/></customFilters></filterColumn>
              </autoFilter>
              <tableColumns count="2">
                <tableColumn id="1" name="Region"/>
                <tableColumn id="2" name="Amount"/>
              </tableColumns>
            </table>"#,
        ),
    ]);
    let wb = import_package(source).unwrap().workbook;
    let filter = wb.sheets[0].tables[0]
        .auto_filter
        .as_ref()
        .expect("the table's <autoFilter> is its own, not the sheet's");
    // The sheet itself has no filter — reading only `sheet.auto_filter` here is
    // exactly what made the table's buttons inert.
    assert!(wb.sheets[0].auto_filter.is_none());
    assert_eq!(filter.range.end.col, 1);
    assert!(matches!(filter.rules.get(&0), Some(FilterRule::Values(v)) if v.len() == 2));
    assert!(matches!(
        filter.rules.get(&1),
        Some(FilterRule::Custom { .. })
    ));

    let written = write_workbook(&wb).unwrap();
    let part = xml_of(&written, "xl/tables/table1.xml");
    assert!(part.contains("<filterColumn colId=\"0\">"), "{part}");
    assert!(
        part.contains("<customFilter operator=\"greaterThan\""),
        "{part}"
    );

    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.sheets[0].tables, wb.sheets[0].tables);
}

/// A `definedName` whose target this parser cannot read used to be dropped
/// outright, so every workbook carrying `Print_Titles` — whose value is a
/// whole-row reference like `Sheet1!$1:$2` — lost it on save. The name is now
/// kept verbatim and written back byte for byte.
#[test]
fn a_defined_name_the_parser_cannot_read_survives_the_round_trip() {
    let workbook = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
      <sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets>
      <definedNames>
        <definedName name="_xlnm.Print_Titles" localSheetId="0">Sheet1!$1:$2</definedName>
        <definedName name="WholeCol">Sheet1!$A:$A</definedName>
        <definedName name="Rng">Sheet1!$A$1:$A$3</definedName>
      </definedNames>
    </workbook>"#;
    let source = zip_parts(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
              <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
            </Types>"#,
        ),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#,
        ),
    ]);

    let wb = import_package(source).unwrap().workbook;
    assert_eq!(
        wb.defined_names.len(),
        3,
        "an unreadable target must not cost the whole name: {:?}",
        wb.defined_names.iter().map(|d| &d.name).collect::<Vec<_>>()
    );

    let written = write_workbook(&wb).unwrap();
    let xml = xml_of(&written, "xl/workbook.xml");
    // Verbatim, `$` anchors and all — the point of keeping it.
    assert!(xml.contains("Sheet1!$1:$2"), "{xml}");
    assert!(xml.contains("Sheet1!$A:$A"), "{xml}");
    // ...and the readable one still round-trips as before.
    assert!(xml.contains("Sheet1!$A$1:$A$3"), "{xml}");

    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.defined_names, wb.defined_names);
}

/// A chart is read far enough to draw — and the part is still written back from
/// its own bytes, because the model is a display projection and nothing else.
#[test]
fn a_chart_is_read_for_display_and_still_round_trips_verbatim() {
    let chart = br#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
      <c:chart>
        <c:title><c:tx><c:rich><a:p><a:r><a:t>Quarterly sales</a:t></a:r></a:p></c:rich></c:tx></c:title>
        <c:plotArea>
          <c:barChart>
            <c:barDir val="col"/>
            <c:ser>
              <c:tx><c:strRef><c:f>Sheet1!$B$1</c:f><c:strCache><c:pt idx="0"><c:v>Amount</c:v></c:pt></c:strCache></c:strRef></c:tx>
              <c:cat><c:strRef><c:f>Sheet1!$A$2:$A$4</c:f></c:strRef></c:cat>
              <c:val><c:numRef><c:f>Sheet1!$B$2:$B$4</c:f></c:numRef></c:val>
            </c:ser>
          </c:barChart>
        </c:plotArea>
      </c:chart>
    </c:chartSpace>"#;
    let drawing = br#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
      <xdr:twoCellAnchor>
        <xdr:from><xdr:col>4</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>
        <xdr:to><xdr:col>11</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>16</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
        <xdr:graphicFrame><a:graphic><a:graphicData><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId1"/></a:graphicData></a:graphic></xdr:graphicFrame>
        <xdr:clientData/>
      </xdr:twoCellAnchor>
    </xdr:wsDr>"#;
    let source = zip_parts(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
              <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
              <Override PartName="/xl/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/>
            </Types>"#,
        ),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
              <sheetData/><drawing r:id="rId9"/>
            </worksheet>"#,
        ),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/>
            </Relationships>"#,
        ),
        ("xl/drawings/drawing1.xml", drawing),
        (
            "xl/drawings/_rels/drawing1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart1.xml"/>
            </Relationships>"#,
        ),
        ("xl/charts/chart1.xml", chart),
    ]);

    let wb = import_package(source).unwrap().workbook;
    let charts = &wb.sheets[0].charts;
    assert_eq!(charts.len(), 1, "the sheet's one chart was found");
    let c = &charts[0];
    // Anchored in cells, which is why a chart moves with the rows under it.
    assert_eq!((c.anchor.start.row, c.anchor.start.col), (1, 4));
    // `<xdr:to>` says row 16, column 11 — and it is **exclusive**: with
    // `colOff` zero the frame's right edge sits on the left edge of that
    // column, so the last cell it covers is the one before. This asserted 16/11
    // and was wrong: every imported chart drew a row and a column too large.
    // Invisible until the writer started emitting anchors of its own, at which
    // point a chart would have grown on every save.
    assert_eq!((c.anchor.end.row, c.anchor.end.col), (15, 10));
    // `barDir="col"` means columns; bar and column are one element in OOXML.
    assert_eq!(c.kind, casual_calc_model::ChartKind::Column);
    // The title comes from <c:title>, not from the first <a:t> in the part.
    assert_eq!(c.title, "Quarterly sales");
    assert_eq!(c.series.len(), 1);
    // The literal cached name wins over the reference it came from.
    assert_eq!(c.series[0].name, "Amount");
    assert_eq!(c.series[0].categories.as_deref(), Some("Sheet1!$A$2:$A$4"));
    assert_eq!(c.series[0].values, "Sheet1!$B$2:$B$4");

    // ...and the parts are still written back untouched, because reading a
    // chart for display must not make the model authoritative for it.
    let written = write_workbook(&wb).unwrap();
    assert_eq!(
        xml_of(&written, "xl/charts/chart1.xml").replace(['\n', ' '], ""),
        String::from_utf8_lossy(chart).replace(['\n', ' '], ""),
        "the chart part is retained verbatim"
    );
    assert!(xml_of(&written, "xl/worksheets/sheet1.xml").contains("<drawing"));

    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.sheets[0].charts, wb.sheets[0].charts);
}

/// A picture is located for the renderer while its bytes stay in the retained
/// media part — stored once, and written back as they arrived.
#[test]
fn an_image_is_located_without_copying_its_bytes() {
    let drawing = br#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
      <xdr:twoCellAnchor>
        <xdr:from><xdr:col>1</xdr:col><xdr:row>2</xdr:row></xdr:from>
        <xdr:to><xdr:col>5</xdr:col><xdr:row>10</xdr:row></xdr:to>
        <xdr:pic><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill></xdr:pic>
        <xdr:clientData/>
      </xdr:twoCellAnchor>
    </xdr:wsDr>"#;
    let source = zip_parts(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
              <Default Extension="png" ContentType="image/png"/>
              <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
            </Types>"#,
        ),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData/><drawing r:id="rId9"/></worksheet>"#,
        ),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/>
            </Relationships>"#,
        ),
        ("xl/drawings/drawing1.xml", drawing),
        (
            "xl/drawings/_rels/drawing1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/>
            </Relationships>"#,
        ),
        ("xl/media/image1.png", b"\x89PNG\r\n\x1a\nnot-a-real-png"),
    ]);

    let wb = import_package(source).unwrap().workbook;
    let images = &wb.sheets[0].images;
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].part, "xl/media/image1.png");
    assert_eq!(
        (images[0].anchor.start.row, images[0].anchor.start.col),
        (2, 1)
    );
    // The bytes are not on the sheet — they belong to the retained part, so a
    // picture is stored once however many places point at it.
    let part = wb
        .retained_parts
        .iter()
        .find(|p| p.path == "xl/media/image1.png")
        .expect("the media part is retained");
    assert!(part.bytes.starts_with(b"\x89PNG"));

    let written = write_workbook(&wb).unwrap();
    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.sheets[0].images, wb.sheets[0].images);
}

#[test]
fn a_pivot_cache_is_declared_after_calc_pr_not_among_the_external_references() {
    // Two retained references of different element names, which CT_Workbook
    // puts in two different places: `<externalReferences>` right after
    // `<sheets>`, `<pivotCaches>` after `<calcPr>`. Writing both into one
    // wrapper is not a cosmetic slip — the sequence is validated, and Excel
    // refuses the package rather than ignoring the stray child.
    let source = zip_parts(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
              <Override PartName="/xl/pivotCache/pivotCacheDefinition1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml"/>
            </Types>"#,
        ),
        ("_rels/.rels", ROOT_RELS),
        (
            "xl/workbook.xml",
            br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
              <sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets>
              <calcPr calcId="191029"/>
              <pivotCaches><pivotCache cacheId="41" r:id="rId9"/></pivotCaches>
            </workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
              <Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition" Target="pivotCache/pivotCacheDefinition1.xml"/>
            </Relationships>"#,
        ),
        ("xl/worksheets/sheet1.xml", worksheet()),
        (
            "xl/pivotCache/pivotCacheDefinition1.xml",
            br#"<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cacheSource type="worksheet"><worksheetSource ref="A1:B2" sheet="Sheet1"/></cacheSource></pivotCacheDefinition>"#,
        ),
    ]);
    let wb = import_package(source).unwrap().workbook;
    let written = write_workbook(&wb).unwrap();
    let xml = xml_of(&written, "xl/workbook.xml");

    assert!(
        xml.contains("<pivotCaches><pivotCache cacheId=\"41\" r:id=\"rId9\"/></pivotCaches>"),
        "{xml}"
    );
    assert!(!xml.contains("<externalReferences>"), "{xml}");
    let calc = xml.find("<calcPr").expect("calcPr");
    let caches = xml.find("<pivotCaches>").expect("pivotCaches");
    assert!(calc < caches, "pivotCaches must follow calcPr: {xml}");

    // And it comes back the same, so a second save is not a second edit.
    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.retained_refs, wb.retained_refs);
}

/// A workbook with data and one chart made here rather than read from a file.
fn authored_chart_workbook() -> casual_calc_model::Workbook {
    use casual_calc_model::{
        Cell, CellRange, CellRef, CellValue, ChartKind, ChartSeries, ChartView, Id, Sheet, SheetId,
        Workbook,
    };
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "Sales");
    for (r, (label, n)) in [("Q1", 10.0), ("Q2", 20.0), ("Q3", 30.0)]
        .iter()
        .enumerate()
    {
        let id = wb.intern_string(label);
        sheet.cells.set(
            CellRef::new(r as u32 + 1, 0),
            Cell::value(CellValue::SharedString(id)),
        );
        sheet.cells.set(
            CellRef::new(r as u32 + 1, 1),
            Cell::value(CellValue::Number(*n)),
        );
    }
    let mut chart = ChartView::new(
        CellRange::new(CellRef::new(0, 3), CellRef::new(14, 10)),
        ChartKind::Column,
    );
    chart.title = "Revenue & growth".to_owned();
    chart.y_title = "Amount".to_owned();
    chart.legend = Some("r".to_owned());
    chart.series.push(ChartSeries {
        name: "Revenue".to_owned(),
        categories: Some("Sales!$A$2:$A$4".to_owned()),
        values: "Sales!$B$2:$B$4".to_owned(),
        ..ChartSeries::default()
    });
    sheet.charts.push(chart);
    wb.sheets.push(sheet);
    wb
}

#[test]
fn a_chart_made_here_is_written_as_a_real_chart_part() {
    let written = write_workbook(&authored_chart_workbook()).unwrap();

    // The chart part, with the pieces Excel needs paired: a group naming two
    // axis ids and two axes carrying them.
    let chart = xml_of(&written, "xl/charts/chart1.xml");
    assert!(chart.contains("<c:barChart>"), "{chart}");
    assert!(chart.contains("<c:barDir val=\"col\"/>"), "{chart}");
    assert!(chart.contains("<c:f>Sales!$B$2:$B$4</c:f>"), "{chart}");
    assert!(chart.contains("<c:f>Sales!$A$2:$A$4</c:f>"), "{chart}");
    assert!(chart.contains("<c:axId val=\"111111111\"/>"), "{chart}");
    assert!(chart.contains("<c:catAx>"), "{chart}");
    assert!(chart.contains("<c:valAx>"), "{chart}");
    assert!(chart.contains("<c:legendPos val=\"r\"/>"), "{chart}");
    // `&` in a title has to be escaped or the part is not XML at all.
    assert!(chart.contains("Revenue &amp; growth"), "{chart}");

    // The three references. Any one missing and Excel reports a file needing
    // repair rather than a chart it cannot draw.
    assert!(
        xml_of(&written, "xl/worksheets/sheet1.xml").contains("<drawing r:id="),
        "the worksheet must name its drawing"
    );
    let sheet_rels = xml_of(&written, "xl/worksheets/_rels/sheet1.xml.rels");
    assert!(
        sheet_rels.contains("../drawings/drawing1.xml"),
        "{sheet_rels}"
    );
    let drawing_rels = xml_of(&written, "xl/drawings/_rels/drawing1.xml.rels");
    assert!(
        drawing_rels.contains("../charts/chart1.xml"),
        "{drawing_rels}"
    );
    let types = xml_of(&written, "[Content_Types].xml");
    assert!(types.contains("/xl/charts/chart1.xml"), "{types}");
    assert!(types.contains("/xl/drawings/drawing1.xml"), "{types}");

    // The anchor's `to` corner is exclusive, so a frame ending at column 10
    // is written as 11 — otherwise the chart is one row and one column short.
    let drawing = xml_of(&written, "xl/drawings/drawing1.xml");
    assert!(drawing.contains("<xdr:col>3</xdr:col>"), "{drawing}");
    assert!(drawing.contains("<xdr:col>11</xdr:col>"), "{drawing}");
    assert!(drawing.contains("<xdr:row>15</xdr:row>"), "{drawing}");
}

#[test]
fn a_chart_made_here_reads_back_as_the_same_chart() {
    let written = write_workbook(&authored_chart_workbook()).unwrap();
    let back = import_package(written).unwrap().workbook;
    let chart = &back.sheets[0].charts[0];

    assert_eq!(chart.kind, casual_calc_model::ChartKind::Column);
    assert_eq!(chart.title, "Revenue & growth");
    assert_eq!(chart.y_title, "Amount");
    assert_eq!(chart.legend.as_deref(), Some("r"));
    assert_eq!(chart.series.len(), 1);
    assert_eq!(chart.series[0].name, "Revenue");
    assert_eq!(chart.series[0].values, "Sales!$B$2:$B$4");
    assert_eq!(
        chart.series[0].categories.as_deref(),
        Some("Sales!$A$2:$A$4")
    );
    // The whole frame, not just its corner: `<xdr:to>` is exclusive, so a
    // writer that emits `end + 1` and a reader that takes it as the end would
    // grow every chart by a row and a column on each save.
    assert_eq!(
        chart.anchor,
        casual_calc_model::CellRange::new(
            casual_calc_model::CellRef::new(0, 3),
            casual_calc_model::CellRef::new(14, 10)
        )
    );
    // Read back from a file, so it is now retained rather than authored — the
    // part is authoritative until something edits it.
    assert!(chart.part.is_some());
}

#[test]
fn a_pie_gets_no_axes_because_writing_them_is_invalid() {
    use casual_calc_model::ChartKind;
    let mut wb = authored_chart_workbook();
    wb.sheets[0].charts[0].kind = ChartKind::Pie;
    let chart = xml_of(&write_workbook(&wb).unwrap(), "xl/charts/chart1.xml");
    assert!(chart.contains("<c:pieChart>"), "{chart}");
    assert!(!chart.contains("<c:catAx>"), "a pie has no axes: {chart}");
    assert!(!chart.contains("<c:axId"), "{chart}");
    // Colour varies per point, not per series, or one series is one colour and
    // the pie reads as a single disc.
    assert!(chart.contains("<c:varyColors val=\"1\"/>"), "{chart}");
}

#[test]
fn a_scatters_horizontal_axis_is_a_value_axis_not_a_category_axis() {
    use casual_calc_model::ChartKind;
    let mut wb = authored_chart_workbook();
    wb.sheets[0].charts[0].kind = ChartKind::Scatter;
    let chart = xml_of(&write_workbook(&wb).unwrap(), "xl/charts/chart1.xml");
    assert!(chart.contains("<c:scatterChart>"), "{chart}");
    assert!(
        chart.contains("<c:xVal><c:numRef>"),
        "numbers, not labels: {chart}"
    );
    assert!(chart.contains("<c:yVal><c:numRef>"), "{chart}");
    // A catAx would space the points evenly and lose the very relationship a
    // scatter is drawn to show.
    assert!(!chart.contains("<c:catAx>"), "{chart}");
    assert_eq!(chart.matches("<c:valAx>").count(), 2, "{chart}");
}

#[test]
fn an_authored_chart_joins_a_retained_drawing_instead_of_replacing_it() {
    // A worksheet may have only one drawing part, so a chart made here on a
    // sheet that already has one has to go *into* it. Rebuilding that drawing
    // from the model would delete the text box below, which nothing models.
    use casual_calc_model::{CellRange, CellRef, ChartKind, ChartSeries, ChartView};
    let source = zip_parts(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
              <Override PartName="/xl/drawings/drawing1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawing+xml"/>
            </Types>"#,
        ),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
              <sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData>
              <drawing r:id="rId7"/>
            </worksheet>"#,
        ),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/>
            </Relationships>"#,
        ),
        (
            "xl/drawings/drawing1.xml",
            br#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing"><xdr:twoCellAnchor><xdr:sp><xdr:txBody>a text box nothing here models</xdr:txBody></xdr:sp></xdr:twoCellAnchor></xdr:wsDr>"#,
        ),
    ]);
    let mut wb = import_package(source).unwrap().workbook;
    let mut chart = ChartView::new(
        CellRange::new(CellRef::new(2, 2), CellRef::new(9, 8)),
        ChartKind::Line,
    );
    chart.series.push(ChartSeries {
        name: "Added".to_owned(),
        categories: None,
        values: "Sheet1!$A$1:$A$1".to_owned(),
        ..ChartSeries::default()
    });
    wb.sheets[0].charts.push(chart);

    let written = match write_workbook(&wb) {
        Ok(w) => w,
        Err(e) => panic!("write failed: {e:?}"),
    };
    let drawing = xml_of(&written, "xl/drawings/drawing1.xml");
    assert!(
        drawing.contains("a text box nothing here models"),
        "the retained anchors travel untouched: {drawing}"
    );
    assert!(
        drawing.contains("<xdr:graphicFrame"),
        "and ours is spliced in beside them: {drawing}"
    );
    assert_eq!(drawing.matches("</xdr:wsDr>").count(), 1, "{drawing}");

    // The drawing's rels must carry both the retained entries and the new
    // chart, at ids that do not collide.
    let rels = xml_of(&written, "xl/drawings/_rels/drawing1.xml.rels");
    assert!(rels.contains("../charts/chart1.xml"), "{rels}");
    // The sheet keeps its original `<drawing r:id>`, because the drawing it
    // names is the one that was extended.
    let sheet = xml_of(&written, "xl/worksheets/sheet1.xml");
    assert!(sheet.contains("<drawing r:id=\"rId7\"/>"), "{sheet}");
    assert_eq!(sheet.matches("<drawing ").count(), 1, "{sheet}");
}

#[test]
fn a_frames_offsets_survive_the_round_trip_so_a_drag_lands_where_it_was_dropped() {
    // Without these a frame can only start and end on a gridline: dragging an
    // edge does nothing until it crosses one, then jumps a whole column, and
    // the chart never comes to rest where it was dropped.
    use casual_calc_model::Emu;
    let mut wb = authored_chart_workbook();
    wb.sheets[0].charts[0].from_offset = Emu {
        x: 38_100,
        y: 19_050,
    };
    wb.sheets[0].charts[0].to_offset = Emu {
        x: 57_150,
        y: 9_525,
    };

    let written = match write_workbook(&wb) {
        Ok(w) => w,
        Err(e) => panic!("write failed: {e:?}"),
    };
    let drawing = xml_of(&written, "xl/drawings/drawing1.xml");
    assert!(
        drawing.contains("<xdr:colOff>38100</xdr:colOff>"),
        "{drawing}"
    );
    assert!(
        drawing.contains("<xdr:rowOff>19050</xdr:rowOff>"),
        "{drawing}"
    );
    assert!(
        drawing.contains("<xdr:colOff>57150</xdr:colOff>"),
        "{drawing}"
    );

    let back = import_package(written).unwrap().workbook;
    let chart = &back.sheets[0].charts[0];
    assert_eq!(
        chart.from_offset,
        Emu {
            x: 38_100,
            y: 19_050
        }
    );
    assert_eq!(
        chart.to_offset,
        Emu {
            x: 57_150,
            y: 9_525
        }
    );
    // And the cells are unchanged, so the frame as a whole is identical. The
    // `to` corner is exclusive with its offset measured into the cell after,
    // which is the same number as one measured past the cell before — write it
    // against the wrong corner and a saved chart drifts a column each time.
    assert_eq!(chart.anchor, wb.sheets[0].charts[0].anchor);
}

#[test]
fn a_degenerate_anchor_keeps_no_trailing_offset() {
    // `to` on or before `from` collapses the frame to one cell, and an offset
    // left over from the original would then be measured from an edge the
    // frame does not reach.
    let source = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData/><drawing r:id="rId7"/></worksheet>"#,
        ),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#,
        ),
        (
            "xl/drawings/drawing1.xml",
            br#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
              <xdr:twoCellAnchor>
                <xdr:from><xdr:col>4</xdr:col><xdr:colOff>1000</xdr:colOff><xdr:row>2</xdr:row><xdr:rowOff>2000</xdr:rowOff></xdr:from>
                <xdr:to><xdr:col>4</xdr:col><xdr:colOff>99999</xdr:colOff><xdr:row>2</xdr:row><xdr:rowOff>88888</xdr:rowOff></xdr:to>
                <xdr:graphicFrame><a:graphic><a:graphicData><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId1"/></a:graphicData></a:graphic></xdr:graphicFrame>
              </xdr:twoCellAnchor>
            </xdr:wsDr>"#,
        ),
        (
            "xl/drawings/_rels/drawing1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart1.xml"/></Relationships>"#,
        ),
        (
            "xl/charts/chart1.xml",
            br#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart/></c:chartSpace>"#,
        ),
    ]);
    let wb = import_package(source).unwrap().workbook;
    let chart = &wb.sheets[0].charts[0];
    assert_eq!(chart.anchor.start, casual_calc_model::CellRef::new(2, 4));
    assert_eq!(chart.anchor.end, casual_calc_model::CellRef::new(2, 4));
    assert_eq!(
        chart.from_offset,
        casual_calc_model::Emu { x: 1000, y: 2000 }
    );
    assert!(chart.to_offset.is_zero(), "{:?}", chart.to_offset);
}

#[test]
fn deleting_an_imported_chart_takes_its_anchor_with_it() {
    // The chart part and its relationship go when the chart does, but the
    // anchor naming that relationship lives in the retained drawing's bytes.
    // Left behind it points at a relationship that does not exist, and Excel
    // reports the file as needing repair rather than as a chart it cannot draw.
    let source = zip_parts(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
              <Override PartName="/xl/drawings/drawing1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawing+xml"/>
              <Override PartName="/xl/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/>
            </Types>"#,
        ),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData/><drawing r:id="rId7"/></worksheet>"#,
        ),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#,
        ),
        (
            "xl/drawings/drawing1.xml",
            br#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
              <xdr:twoCellAnchor>
                <xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>
                <xdr:to><xdr:col>6</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>10</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
                <xdr:graphicFrame><a:graphic><a:graphicData><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId1"/></a:graphicData></a:graphic></xdr:graphicFrame>
              </xdr:twoCellAnchor>
              <xdr:twoCellAnchor>
                <xdr:sp><xdr:txBody>a text box nothing here models</xdr:txBody></xdr:sp>
              </xdr:twoCellAnchor>
            </xdr:wsDr>"#,
        ),
        (
            "xl/drawings/_rels/drawing1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart1.xml"/></Relationships>"#,
        ),
        (
            "xl/charts/chart1.xml",
            br#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart/></c:chartSpace>"#,
        ),
    ]);
    let mut wb = import_package(source).unwrap().workbook;
    assert_eq!(wb.sheets[0].charts.len(), 1);

    // Delete it the way the host does: the chart, its part, and the
    // relationship reaching it.
    wb.sheets[0].charts.clear();
    wb.retained_parts
        .retain(|p| p.path != "xl/charts/chart1.xml");
    wb.retained_rels
        .retain(|r| !r.target.ends_with("charts/chart1.xml"));

    let written = match write_workbook(&wb) {
        Ok(w) => w,
        Err(e) => panic!("write failed: {e:?}"),
    };
    let drawing = xml_of(&written, "xl/drawings/drawing1.xml");
    assert!(
        !drawing.contains("rId1"),
        "the anchor names a relationship that is gone: {drawing}"
    );
    // ...and only that anchor. The text box has no relationship at all, which
    // is exactly the content this must never touch.
    assert!(
        drawing.contains("a text box nothing here models"),
        "{drawing}"
    );
    // One anchor left, counted by its closing tag.
    assert_eq!(
        drawing.matches("</xdr:twoCellAnchor>").count(),
        1,
        "{drawing}"
    );
    // The package must still be readable, and must no longer carry the chart.
    let back = import_package(written).unwrap().workbook;
    assert!(back.sheets[0].charts.is_empty());
}

/// An ordinary Excel package: the four relationships Excel always writes at the
/// package root, and the parts they reach.
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
        ("xl/worksheets/sheet1.xml", worksheet()),
    ])
}

/// Every path in a written package, in archive order.
fn entry_names(package: &[u8]) -> Vec<String> {
    zip::ZipArchive::new(Cursor::new(package))
        .unwrap()
        .file_names()
        .map(str::to_owned)
        .collect()
}

#[test]
fn parts_attached_at_the_package_root_survive_a_save() {
    let wb = import_package(package_with_root_parts()).unwrap().workbook;
    let written = write_workbook(&wb).unwrap();

    // The author and title, the company, and a whole custom payload — none of
    // which is reachable from `workbook.xml`, all of which the file carries.
    assert!(xml_of(&written, "docProps/core.xml").contains("<dc:title>Q3 Ledger</dc:title>"));
    assert!(xml_of(&written, "docProps/app.xml").contains("<Company>Analytical Engines</Company>"));
    assert!(xml_of(&written, "customXml/item1.xml").contains("<number>4711</number>"));
    assert!(xml_of(&written, "customXml/itemProps1.xml").contains("DEADBEEF"));
    // The item's own rels reach its properties; without them Excel reports the
    // package as needing repair.
    assert!(xml_of(&written, "customXml/_rels/item1.xml.rels").contains("itemProps1.xml"));

    // The relationships that reach them are re-emitted at the root, keeping
    // their ids, and beside — not instead of — the workbook relationship.
    let root = xml_of(&written, "_rels/.rels");
    assert!(
        root.contains("Id=\"rId1\"") && root.contains("xl/workbook.xml"),
        "{root}"
    );
    assert!(
        root.contains("Id=\"rId4\"") && root.contains("customXml/item1.xml"),
        "{root}"
    );
    assert_eq!(
        root.matches("relationships/officeDocument\"").count(),
        1,
        "one officeDocument relationship, not two: {root}"
    );
    // One `_rels/.rels` in the archive. A second entry at the same path is a
    // package readers disagree about — most take the first, which would be the
    // one without the root parts.
    assert_eq!(
        entry_names(&written)
            .iter()
            .filter(|p| p.as_str() == "_rels/.rels")
            .count(),
        1
    );
    // And not smuggled into the workbook's rels, where `docProps/core.xml`
    // resolves to `xl/docProps/core.xml` and reaches nothing.
    let wb_rels = xml_of(&written, "xl/_rels/workbook.xml.rels");
    assert!(!wb_rels.contains("docProps"), "{wb_rels}");

    // The content types are re-declared, without which Excel refuses the
    // package rather than ignoring the undeclared part.
    let types = xml_of(&written, "[Content_Types].xml");
    assert!(types.contains("/docProps/core.xml"), "{types}");
    assert!(types.contains("/customXml/itemProps1.xml"), "{types}");

    // Reopening is the real check: the second import must find exactly what the
    // first one did, or a save loses the parts one generation later.
    let back = import_package(written).unwrap().workbook;
    assert_eq!(back.retained_parts, wb.retained_parts);
    assert_eq!(back.retained_rels, wb.retained_rels);
}

#[test]
fn a_root_part_that_claims_rid1_does_not_collide_with_the_workbook() {
    // Nothing numbers the root relationships: `rId1` is this writer's habit, not
    // a rule, and a producer is free to have given it to `docProps/core.xml`. Two
    // `Id="rId1"` in one `.rels` is not a package with a duplicate — `Id` is an
    // xsd:ID, so it is a package Excel repairs, and the loser is whichever
    // relationship the reader drops.
    const ROOT_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
      <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
      <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
    </Relationships>"#;
    let source = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        (
            "docProps/core.xml",
            br#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"/>"#,
        ),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/worksheets/sheet1.xml", worksheet()),
    ]);
    let wb = import_package(source).unwrap().workbook;
    let written = write_workbook(&wb).unwrap();
    let root = xml_of(&written, "_rels/.rels");

    let ids: Vec<&str> = root
        .split("Id=\"")
        .skip(1)
        .map(|c| c.split('"').next().unwrap())
        .collect();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(ids.len(), unique.len(), "ids must be distinct: {root}");
    assert_eq!(ids.len(), 2, "{root}");
    // The retained id is the one that must not move: it is the file's, and the
    // workbook's own is named by nothing but its type.
    assert!(root.contains("Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\""), "{root}");
    assert!(xml_of(&written, "docProps/core.xml").contains("coreProperties"));
}

/// SpreadsheetML's `_xlfn.` prefix, which the writer must add and the reader
/// must take off again.
///
/// Found by the oracle diff (P2-003): a corpus put through LibreOffice came
/// back with `#NAME?` for `CONCAT`, `TEXTJOIN`, `SWITCH`, `IFNA` and `UNICHAR`,
/// because the format requires a function it postdates to be written prefixed
/// and this writer emitted the bare name. No test here could have caught it —
/// both ends of every round-trip were this codebase, which agreed with itself
/// perfectly while producing a file Excel would not read.
mod future_functions {
    use casual_calc_formula::parse;
    use casual_calc_model::{Cell, CellRef, Id, Sheet, SheetId, Workbook};

    use super::*;

    fn book_with(formula: &str) -> Vec<u8> {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let handle = wb.store_formula(parse(formula).unwrap());
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        sheet.cells.set(
            CellRef::new(0, 0),
            Cell {
                formula: Some(handle),
                ..Cell::default()
            },
        );
        wb.sheets.push(sheet);
        write_workbook(&wb).unwrap()
    }

    fn sheet_xml(bytes: &[u8]) -> String {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
        let mut out = String::new();
        use std::io::Read;
        zip.by_name("xl/worksheets/sheet1.xml")
            .unwrap()
            .read_to_string(&mut out)
            .unwrap();
        out
    }

    #[test]
    fn a_function_the_format_postdates_is_written_with_its_prefix() {
        // The exact bug: without this the cell reads `#NAME?` in Excel and the
        // user has no way to see what the formula used to say.
        let xml = sheet_xml(&book_with("CONCAT(\"a\",\"b\")"));
        assert!(
            xml.contains(r#"<f>_XLFN.CONCAT("a","b")</f>"#),
            "expected a prefixed function in: {xml}"
        );
    }

    #[test]
    fn a_function_the_format_already_had_is_written_bare() {
        // The other half: prefixing `SUM` breaks it just as thoroughly.
        let xml = sheet_xml(&book_with("SUM(A1:A3)"));
        assert!(xml.contains("<f>SUM(A1:A3)</f>"), "in: {xml}");
        assert!(!xml.contains("_XLFN.SUM"));
    }

    #[test]
    fn what_the_writer_prefixed_the_reader_takes_off_again() {
        // The round trip must land back in the language, not in the format:
        // everything downstream — the evaluator, the formula bar, the transform
        // — knows `CONCAT` and not `_XLFN.CONCAT`.
        for formula in [
            "CONCAT(\"a\",\"b\")",
            "TEXTJOIN(\",\",TRUE,A1:A3)",
            "SWITCH(2,1,\"one\",2,\"two\")",
            "IF(ISFORMULA(A1),XLOOKUP(1,A1:A3,B1:B3),SUM(C1:C3))",
        ] {
            let reopened = import_package(book_with(formula)).unwrap().workbook;
            let cell = reopened.sheets[0].cells.get(CellRef::new(0, 0)).unwrap();
            let expr = reopened.formula(cell.formula.unwrap()).unwrap();
            assert_eq!(
                expr.to_string(),
                formula,
                "round trip of {formula} came back as {expr}"
            );
        }
    }

    #[test]
    fn a_defined_name_carries_the_prefix_too() {
        // The second place a formula reaches the file, and the one easiest to
        // forget: a defined name is an expression as much as a cell is.
        use casual_calc_model::DefinedName;

        let mut wb = Workbook::new(Id::from_parts(1, 1));
        wb.sheets
            .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "S"));
        wb.defined_names.push(DefinedName {
            name: "Joined".to_owned(),
            sheet: None,
            formula: parse("TEXTJOIN(\",\",TRUE,A1:A3)").unwrap(),
        });

        let bytes = write_workbook(&wb).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes.clone())).unwrap();
        let mut xml = String::new();
        use std::io::Read;
        zip.by_name("xl/workbook.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("_XLFN.TEXTJOIN"), "in: {xml}");

        let reopened = import_package(bytes).unwrap().workbook;
        assert_eq!(
            reopened.defined_names[0].formula.to_string(),
            "TEXTJOIN(\",\",TRUE,A1:A3)",
            "and comes back as the language, not the format"
        );
    }
}

/// An imported chart is written back from its retained part, so shifting the
/// model's series (FID-26) moved the picture on screen and left the saved file
/// saying what it always said. Insert a row above the data and the file must
/// name the shifted rows (FID-27).
///
/// The part carries a `<c:serAx>` before the series and a `<c:valAx>` after it,
/// both holding a `<c:f>`, so that an axis definition is never rewritten as if
/// it were data. That pair is a regression guard, not a proof: valid OOXML
/// orders the axes outside the series, so element ordering alone would spare
/// them even from a sloppier matcher — the guard against matching `ser` inside
/// `serAx` is proved directly, in `chart::tests`.
///
/// A `<c:tx>` reference and a `<c:spPr>` fill are here to be preserved: a
/// series *name* is not a position, and the formatting is the whole reason the
/// part is retained at all.
#[test]
fn a_retained_chart_part_is_re_emitted_with_its_shifted_series() {
    use casual_calc_model::{
        CellRange, CellRef, ChartKind, ChartSeries, ChartView, Id, RetainedPart, Sheet, SheetId,
        Workbook,
    };

    const PART: &str = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">"#,
        r#"<c:chart><c:plotArea>"#,
        r#"<c:serAx><c:f>S!$Z$1:$Z$9</c:f></c:serAx>"#,
        r#"<c:barChart><c:barDir val="col"/><c:ser>"#,
        r#"<c:idx val="0"/>"#,
        r#"<c:tx><c:strRef><c:f>S!$D$1</c:f></c:strRef></c:tx>"#,
        r#"<c:spPr><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></c:spPr>"#,
        r#"<c:cat><c:strRef><c:f>S!$A$2:$A$11</c:f></c:strRef></c:cat>"#,
        r#"<c:val><c:numRef><c:f>S!$D$2:$D$11</c:f></c:numRef></c:val>"#,
        r#"</c:ser></c:barChart>"#,
        r#"<c:valAx><c:f>S!$Y$1:$Y$9</c:f></c:valAx>"#,
        r#"</c:plotArea></c:chart></c:chartSpace>"#,
    );

    let mut wb = Workbook::new(Id::from_parts(1, 1));
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "S"));
    wb.retained_parts.push(RetainedPart {
        path: "xl/charts/chart1.xml".into(),
        bytes: PART.as_bytes().to_vec(),
        content_type: Some(
            "application/vnd.openxmlformats-officedocument.drawingml.chart+xml".into(),
        ),
    });
    let mut chart = ChartView::new(
        CellRange::new(CellRef::new(0, 5), CellRef::new(9, 5)),
        ChartKind::Column,
    );
    chart.part = Some("xl/charts/chart1.xml".into());
    // What FID-26 already leaves in the model after inserting two rows at row 2.
    chart.series.push(ChartSeries {
        name: "Amount".into(),
        categories: Some("S!$A$4:$A$13".into()),
        values: "S!$D$4:$D$13".into(),
        ..ChartSeries::default()
    });
    wb.sheets[0].charts.push(chart);

    let written = write_workbook(&wb).unwrap();
    let part = xml_of(&written, "xl/charts/chart1.xml");

    assert!(
        part.contains("<c:f>S!$D$4:$D$13</c:f>"),
        "the saved values reference must be the shifted one: {part}"
    );
    assert!(
        part.contains("<c:f>S!$A$4:$A$13</c:f>"),
        "and so must the categories: {part}"
    );
    assert!(
        part.contains("<c:f>S!$Z$1:$Z$9</c:f>") && part.contains("<c:f>S!$Y$1:$Y$9</c:f>"),
        "an axis is not a series: `serAx` and `valAx` must be untouched: {part}"
    );
    assert!(
        part.contains("<c:f>S!$D$1</c:f>"),
        "a series name is not a position: {part}"
    );
    assert!(
        part.contains(r#"<a:srgbClr val="FF0000"/>"#),
        "the formatting the retained part exists to keep must survive: {part}"
    );
}

/// A chart nobody moved must come back byte for byte. Re-emitting an untouched
/// part in our own spelling would be a silent rewrite of somebody's file, and
/// would defeat the point of retaining it.
#[test]
fn an_unshifted_retained_chart_part_is_written_back_unchanged() {
    use casual_calc_model::{
        CellRange, CellRef, ChartKind, ChartSeries, ChartView, Id, RetainedPart, Sheet, SheetId,
        Workbook,
    };

    const PART: &str = concat!(
        r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">"#,
        r#"<c:ser><c:val><c:numRef><c:f>S!$D$2:$D$11</c:f></c:numRef></c:val></c:ser>"#,
        r#"</c:chartSpace>"#,
    );

    let mut wb = Workbook::new(Id::from_parts(1, 1));
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "S"));
    wb.retained_parts.push(RetainedPart {
        path: "xl/charts/chart1.xml".into(),
        bytes: PART.as_bytes().to_vec(),
        content_type: Some(
            "application/vnd.openxmlformats-officedocument.drawingml.chart+xml".into(),
        ),
    });
    let mut chart = ChartView::new(
        CellRange::new(CellRef::new(0, 5), CellRef::new(9, 5)),
        ChartKind::Column,
    );
    chart.part = Some("xl/charts/chart1.xml".into());
    chart.series.push(ChartSeries {
        name: String::new(),
        categories: None,
        values: "S!$D$2:$D$11".into(),
        ..ChartSeries::default()
    });
    wb.sheets[0].charts.push(chart);

    let written = write_workbook(&wb).unwrap();
    assert_eq!(xml_of(&written, "xl/charts/chart1.xml"), PART);
}

/// An imported chart's *frame* lives in the retained drawing, where nothing
/// names the chart — the link is the anchor's `r:id`. So a shifted frame never
/// reached the file even after FID-27 fixed the series (FID-29).
///
/// The offsets below are non-zero so that surviving unchanged means something:
/// a row insert moves a frame by whole rows, and where it sits *within* a row
/// is not the insert's business.
///
/// This case does **not** prove that element names are matched whole rather
/// than as a prefix, even though `<xdr:col>` sits beside `<xdr:colOff>`. A
/// prefix matcher still lands on `col` first, because `col` precedes `colOff`
/// in the document — the same ordering luck that spares the axes in the series
/// test. Prefix matching only bites when the *longer* name comes first, which
/// is the shape `chart::tests` exercises directly.
#[test]
fn a_retained_drawing_anchor_follows_the_frame_the_model_holds() {
    use casual_calc_model::{
        CellRange, CellRef, ChartKind, ChartView, Emu, Id, RetainedPart, Sheet, SheetId, Workbook,
    };

    const DRAWING: &str = concat!(
        r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<xdr:twoCellAnchor>"#,
        r#"<xdr:from><xdr:col>5</xdr:col><xdr:colOff>12700</xdr:colOff><xdr:row>4</xdr:row><xdr:rowOff>19050</xdr:rowOff></xdr:from>"#,
        r#"<xdr:to><xdr:col>10</xdr:col><xdr:colOff>38100</xdr:colOff><xdr:row>14</xdr:row><xdr:rowOff>57150</xdr:rowOff></xdr:to>"#,
        r#"<xdr:graphicFrame><a:graphic><a:graphicData><c:chart r:id="rId1"/></a:graphicData></a:graphic></xdr:graphicFrame>"#,
        r#"<xdr:clientData/>"#,
        r#"</xdr:twoCellAnchor>"#,
        r#"</xdr:wsDr>"#,
    );

    let mut wb = Workbook::new(Id::from_parts(1, 1));
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "S"));
    wb.retained_parts.push(RetainedPart {
        path: "xl/drawings/drawing1.xml".into(),
        bytes: DRAWING.as_bytes().to_vec(),
        content_type: None,
    });
    wb.retained_parts.push(RetainedPart {
        path: "xl/charts/chart1.xml".into(),
        bytes: b"<c:chartSpace/>".to_vec(),
        content_type: None,
    });
    // The sheet points at the drawing, and the drawing at the chart.
    wb.retained_rels.push(casual_calc_model::RetainedRel {
        source: "xl/worksheets/sheet1.xml".into(),
        id: "rId9".into(),
        rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing"
            .into(),
        target: "../drawings/drawing1.xml".into(),
        external: false,
    });
    wb.retained_rels.push(casual_calc_model::RetainedRel {
        source: "xl/drawings/drawing1.xml".into(),
        id: "rId1".into(),
        rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart"
            .into(),
        target: "../charts/chart1.xml".into(),
        external: false,
    });

    // Where the model says the frame is now: two rows lower than the file says.
    let mut chart = ChartView::new(
        CellRange::new(CellRef::new(6, 5), CellRef::new(16, 9)),
        ChartKind::Column,
    );
    chart.part = Some("xl/charts/chart1.xml".into());
    chart.from_offset = Emu { x: 12700, y: 19050 };
    chart.to_offset = Emu { x: 38100, y: 57150 };
    wb.sheets[0].charts.push(chart);

    let written = match write_workbook(&wb) {
        Ok(w) => w,
        Err(e) => panic!("write failed: {e:?}"),
    };
    let drawing = xml_of(&written, "xl/drawings/drawing1.xml");

    assert!(
        drawing.contains("<xdr:col>5</xdr:col><xdr:colOff>12700</xdr:colOff><xdr:row>6</xdr:row>"),
        "the `from` row must follow the model, and its offsets survive: {drawing}"
    );
    assert!(
        drawing
            .contains("<xdr:col>10</xdr:col><xdr:colOff>38100</xdr:colOff><xdr:row>17</xdr:row>"),
        "the `to` corner is exclusive, so row 16 inclusive is 17: {drawing}"
    );
    assert!(
        drawing.contains("<xdr:rowOff>19050</xdr:rowOff>")
            && drawing.contains("<xdr:rowOff>57150</xdr:rowOff>"),
        "a row insert moves a frame by whole rows; where it sits inside a row is untouched: {drawing}"
    );
}

/// Deleting an imported chart makes the workbook refuse to save at all.
///
/// The drawing that loses a dangling anchor is rebuilt, so the sheet
/// contributes a drawing part with **no** chart parts. Two places then write
/// that drawing's `.rels`: the chart builder writes it for every non-empty
/// `drawing_part`, and the retained-relationship pass writes it for every
/// source it has not already seen — but that pass only steps aside for a build
/// with chart parts. With none, both fire, the package gets two entries at one
/// path, and the zip writer rejects the whole file.
///
/// Nothing is lost quietly here: the save fails outright. That is the only
/// reason it went unnoticed rather than being noticed as corruption.
#[test]
fn a_drawing_rebuilt_without_chart_parts_does_not_write_its_rels_twice() {
    use casual_calc_model::{Id, RetainedPart, RetainedRel, Sheet, SheetId, Workbook};

    // One live anchor (rId1, an image) and one whose relationship is gone
    // (rId2) — which is what deleting an imported chart leaves behind.
    const DRAWING: &str = concat!(
        r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<xdr:twoCellAnchor><xdr:pic><a:blip r:embed="rId1"/></xdr:pic></xdr:twoCellAnchor>"#,
        r#"<xdr:twoCellAnchor><xdr:graphicFrame><c:chart r:id="rId2"/></xdr:graphicFrame></xdr:twoCellAnchor>"#,
        r#"</xdr:wsDr>"#,
    );

    let mut wb = Workbook::new(Id::from_parts(1, 1));
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "S"));
    wb.retained_parts.push(RetainedPart {
        path: "xl/drawings/drawing1.xml".into(),
        bytes: DRAWING.as_bytes().to_vec(),
        content_type: None,
    });
    wb.retained_rels.push(RetainedRel {
        source: "xl/worksheets/sheet1.xml".into(),
        id: "rId9".into(),
        rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing"
            .into(),
        target: "../drawings/drawing1.xml".into(),
        external: false,
    });
    // rId1 survives; rId2 is deliberately absent.
    wb.retained_rels.push(RetainedRel {
        source: "xl/drawings/drawing1.xml".into(),
        id: "rId1".into(),
        rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image"
            .into(),
        target: "../media/image1.png".into(),
        external: false,
    });

    let written = write_workbook(&wb).expect("a workbook with a stale anchor must still save");
    let rels = xml_of(&written, "xl/drawings/_rels/drawing1.xml.rels");
    assert!(
        rels.contains(r#"Id="rId1""#),
        "the surviving image relationship must be written: {rels}"
    );
}

/// **A pivot created here is written as a pivot, not as the cells it produced.**
///
/// Until now only *imported* pivots reached the file, written back from their
/// retained part; one created in the editor arrived as whatever values its last
/// refresh happened to write, so Excel opened a block of numbers with no field
/// list, no layout to change and nothing to refresh (`PIV-02`).
///
/// Read back through this project's **own importer**, which was written against
/// real Excel files — so a shape it accepts is a shape Excel writes. That is not
/// the same as Excel accepting ours, which is what the row's acceptance
/// criterion asks and what no oracle here can answer.
#[test]
fn a_created_pivot_is_written_as_a_pivot_and_reads_back() {
    use casual_calc_model::{
        CellRange, CellRef, Id, PivotAggregate, PivotAxisField, PivotTable, PivotValueField, Sheet,
        SheetId, Workbook,
    };

    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let data_id = SheetId(Id::from_parts(2, 1));
    wb.sheets.push(Sheet::new(data_id, "Data"));
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(3, 1)), "Report"));

    // A header row, because that is where the cache's field names come from.
    for (col, name) in ["Region", "Rep", "Amount"].iter().enumerate() {
        let id = wb.strings.intern(name);
        wb.sheets[0].cells.set(
            CellRef::new(0, col as u32),
            casual_calc_model::Cell {
                value: casual_calc_model::CellValue::SharedString(id),
                ..Default::default()
            },
        );
    }

    let mut pivot = PivotTable::new(
        7,
        "Sales".into(),
        data_id,
        CellRange::new(CellRef::new(0, 0), CellRef::new(9, 2)),
        CellRef::new(2, 0),
    );
    pivot.rows.push(PivotAxisField {
        source_column: 0,
        sort: casual_calc_model::PivotSort::Ascending,
        subtotal: true,
    });
    pivot.values.push(PivotValueField {
        source_column: 2,
        aggregate: PivotAggregate::Sum,
        name: "Total".into(),
        number_format: None,
    });
    pivot.output = Some(CellRange::new(CellRef::new(2, 0), CellRef::new(5, 1)));
    wb.sheets[1].pivots.push(pivot);

    let written = write_workbook(&wb).expect("a workbook with a created pivot saves");

    let table = xml_of(&written, "xl/pivotTables/pivotTable1.xml");
    assert!(table.contains(r#"name="Sales""#), "{table}");
    assert!(
        table.contains(r#"<rowFields count="1"><field x="0"/></rowFields>"#),
        "the row axis names the source column it was built from: {table}"
    );
    assert!(
        table.contains(r#"subtotal="sum""#),
        "and the value field keeps its aggregate: {table}"
    );

    let cache = xml_of(&written, "xl/pivotCache/pivotCacheDefinition1.xml");
    assert!(
        cache.contains(r#"saveData="0""#) && cache.contains(r#"refreshOnLoad="1""#),
        "the records are not in the file; the reader is told to fetch them: {cache}"
    );
    assert!(
        cache.contains(r#"<worksheetSource ref="A1:C10" sheet="Data"/>"#),
        "pointing at the source range on the source sheet: {cache}"
    );
    assert!(
        cache.contains(r#"name="Region""#) && cache.contains(r#"name="Amount""#),
        "with field names taken from the header row: {cache}"
    );

    // The package has to hang together: the workbook names the cache, and the
    // sheet names the table. A part nothing points at is a part Excel ignores.
    let wbx = xml_of(&written, "xl/workbook.xml");
    assert!(
        wbx.contains("<pivotCaches>") && wbx.contains(r#"cacheId="7""#),
        "{wbx}"
    );
    let wbrels = xml_of(&written, "xl/_rels/workbook.xml.rels");
    assert!(wbrels.contains("pivotCacheDefinition1.xml"), "{wbrels}");
    let sheet_rels = xml_of(&written, "xl/worksheets/_rels/sheet2.xml.rels");
    assert!(sheet_rels.contains("pivotTable1.xml"), "{sheet_rels}");

    // And our own reader gets the pivot back, not a block of cells.
    let back = import_package(written).unwrap().workbook;
    let read = &back.sheets[1].pivots;
    assert_eq!(read.len(), 1, "the pivot comes back as a pivot");
    assert_eq!(read[0].name, "Sales");
    assert_eq!(read[0].rows[0].source_column, 0);
    assert_eq!(read[0].values[0].aggregate, PivotAggregate::Sum);
}

// ---------------------------------------------------------------------------
// `docs/85` §9 row **B** — the exporter says what the model means.
//
// The cache is written `saveData="0" refreshOnLoad="1"`, so **Excel rebuilds
// the whole report from the definition when the file is opened**. A feature the
// definition does not state is therefore a feature the reopened file does not
// have — it comes back as raw sums under our caption, which is `PIV-05`'s P0
// written from the writing end (`docs/85` §1.4).
//
// The model has nowhere to hold Show Values As, grouping or a calculated field
// yet; those fields arrive with slices C, D and E, each paying a protocol bump
// this slice does not. So these tests hand the writer a
// [`crate::pivot::PivotDerived`] directly — the seam
// [`crate::pivot::PivotDerived::of`] will read off the model once it can — and
// assert the **bytes**, because a file-format change is bytes and nothing else.
// ---------------------------------------------------------------------------

/// A workbook with a `Data` sheet whose header row is `headers`, and an empty
/// `Report` sheet for a pivot to sit on.
#[cfg(test)]
fn pivot_workbook(headers: &[&str]) -> (casual_calc_model::Workbook, casual_calc_model::SheetId) {
    use casual_calc_model::{CellRef, Id, Sheet, SheetId, Workbook};

    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let data_id = SheetId(Id::from_parts(2, 1));
    wb.sheets.push(Sheet::new(data_id, "Data"));
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(3, 1)), "Report"));
    for (col, name) in headers.iter().enumerate() {
        let id = wb.strings.intern(name);
        wb.sheets[0].cells.set(
            CellRef::new(0, col as u32),
            casual_calc_model::Cell {
                value: casual_calc_model::CellValue::SharedString(id),
                ..Default::default()
            },
        );
    }
    (wb, data_id)
}

/// A pivot over the whole of `Data`, anchored on `Report`.
#[cfg(test)]
fn pivot_over(data_id: casual_calc_model::SheetId, width: u32) -> casual_calc_model::PivotTable {
    use casual_calc_model::{CellRange, CellRef, PivotTable};

    PivotTable::new(
        7,
        "Sales".into(),
        data_id,
        CellRange::new(CellRef::new(0, 0), CellRef::new(9, width - 1)),
        CellRef::new(2, 0),
    )
}

#[cfg(test)]
fn value(source_column: u32, name: &str) -> casual_calc_model::PivotValueField {
    casual_calc_model::PivotValueField {
        source_column,
        aggregate: casual_calc_model::PivotAggregate::Sum,
        name: name.into(),
        number_format: None,
    }
}

#[cfg(test)]
fn axis(source_column: u32) -> casual_calc_model::PivotAxisField {
    casual_calc_model::PivotAxisField {
        source_column,
        sort: casual_calc_model::PivotSort::Ascending,
        subtotal: true,
    }
}

/// The two parts one authored pivot produces, as `(cache, table)`.
#[cfg(test)]
fn pivot_parts(
    wb: &casual_calc_model::Workbook,
    derived: &crate::pivot::PivotDerived,
) -> (String, String) {
    let built = crate::pivot::build_with(wb, &wb.sheets[1], 1, 1, std::slice::from_ref(derived));
    let cache = built
        .parts
        .iter()
        .find(|(path, _)| path.contains("pivotCacheDefinition"))
        .expect("a cache definition")
        .1
        .clone();
    let table = built
        .parts
        .iter()
        .find(|(path, _)| path.contains("pivotTables/"))
        .expect("a table definition")
        .1
        .clone();
    (cache, table)
}

/// **A measure with no derivation keeps the bytes it has always had.**
///
/// The point of this test is not that the writer works — the test above covers
/// that — but that the seam is *inert*. `PivotDerived::of` answers "nothing
/// derived" until slices C/D/E give the model somewhere to hold it, so every
/// pivot written today must come out identical to one written before this
/// change. If this ever disagrees with the empty-derivation output, the seam
/// has started deciding something on its own.
#[test]
fn an_undived_measure_is_written_exactly_as_it_was_before_the_derived_half_existed() {
    let (mut wb, data_id) = pivot_workbook(&["Region", "Rep", "Amount"]);
    let mut pivot = pivot_over(data_id, 3);
    pivot.rows.push(axis(0));
    pivot.values.push(value(2, "Total"));
    wb.sheets[1].pivots.push(pivot);

    let (cache, table) = pivot_parts(&wb, &crate::pivot::PivotDerived::default());
    assert!(
        table.contains(
            r#"<dataField name="Total" fld="2" subtotal="sum" baseField="0" baseItem="0"/>"#
        ),
        "an ordinary measure keeps `baseField=\"0\" baseItem=\"0\"` and states no mode: {table}"
    );
    assert!(
        !table.contains("showDataAs"),
        "`normal` is the schema's default and writing it would change every file: {table}"
    );
    assert!(
        cache.contains(r#"<cacheField name="Region" numFmtId="0"><sharedItems/></cacheField>"#),
        "and a source cache field grows neither a formula nor a fieldGroup: {cache}"
    );
    assert!(
        !cache.contains("databaseField") && !cache.contains("fieldGroup"),
        "{cache}"
    );

    // The two paths must agree byte for byte, which is what makes the seam
    // inert rather than merely usually-empty.
    let built = crate::pivot::build(&wb, &wb.sheets[1], 1, 1);
    assert_eq!(
        built.parts,
        crate::pivot::build_with(
            &wb,
            &wb.sheets[1],
            1,
            1,
            &[crate::pivot::PivotDerived::default()]
        )
        .parts,
        "reading the derivation off the model and being handed an empty one \
         must produce the same file"
    );
}

/// **`<pivotTableDefinition>` carries the `@dataCaption` the schema requires.**
///
/// `use="required"` (`schemas/ooxml/sml.xsd:1096`), and every pivot this module
/// has written since `PIV-02` omitted it — so the part did not validate. Found
/// by running one of the written parts through `xmllint` against the vendored
/// schema, which nothing had done: LibreOffice reads the part happily either
/// way, so the tolerant oracle this repository has could not see it.
///
/// It is not a defect of the derived half, and it is fixed here because a
/// document that fails to validate is a worse place to add new attributes than
/// one that validates.
#[test]
fn a_pivot_table_definition_carries_the_data_caption_the_schema_makes_required() {
    let (mut wb, data_id) = pivot_workbook(&["Region", "Amount"]);
    let mut pivot = pivot_over(data_id, 2);
    pivot.rows.push(axis(0));
    pivot.values.push(value(1, "Total"));
    wb.sheets[1].pivots.push(pivot);

    let (_, table) = pivot_parts(&wb, &crate::pivot::PivotDerived::default());
    assert!(
        table.contains(r#" dataCaption="Values" "#),
        "`@dataCaption` is required, and `Values` is what Excel writes: {table}"
    );
}

/// **A `% of Grand Total` measure says so in the file.**
///
/// This is the P0 the row exists to prevent. `refreshOnLoad="1"` means Excel
/// recomputes the report on open; a `<dataField>` with no `@showDataAs` is a
/// plain sum, so the reopened file shows raw sums under a caption reading
/// `% of Grand Total` — `PIV-05` again, from the writing end (`docs/85` §1.4).
///
/// `@baseItem` moves to `1048832`, "no item chosen": the schema's default
/// (`sml.xsd:1279`) and what a real `showDataAs="percentOfTotal"` carries — see
/// the fixture at `crates/casual-calc-import/src/tests.rs:1679`.
#[test]
fn a_show_values_as_measure_states_its_mode_instead_of_being_rebuilt_as_a_sum() {
    use crate::pivot::{PivotDerived, PivotValueDerived};
    use casual_calc_model::PivotShowAs;

    let (mut wb, data_id) = pivot_workbook(&["Region", "Rep", "Amount"]);
    let mut pivot = pivot_over(data_id, 3);
    pivot.rows.push(axis(0));
    pivot.values.push(value(2, "% of Grand Total"));
    wb.sheets[1].pivots.push(pivot);

    let mut derived = PivotDerived::default();
    derived.values.insert(
        0,
        PivotValueDerived {
            show_as: Some(PivotShowAs::PercentOfTotal),
            ..Default::default()
        },
    );

    let (_, table) = pivot_parts(&wb, &derived);
    assert!(
        table.contains(
            r#"<dataField name="% of Grand Total" fld="2" subtotal="sum" showDataAs="percentOfTotal" baseField="0" baseItem="1048832"/>"#
        ),
        "the caption and the arithmetic must arrive together: {table}"
    );
}

/// **A base-relative mode carries its base field and base item.**
///
/// `difference`, `percent`, `percentDiff` and `runTotal` are refused by the
/// first release (`docs/85` §7) precisely because they need these two, and the
/// writer has to be able to state them before the release that honours them —
/// otherwise adding the mode later is a file-format change as well as a
/// behaviour one.
#[test]
fn a_base_relative_mode_carries_its_base_field_and_item() {
    use crate::pivot::{PivotDerived, PivotValueDerived};
    use casual_calc_model::{PivotBaseItem, PivotShowAs};

    let (mut wb, data_id) = pivot_workbook(&["Region", "Rep", "Amount"]);
    let mut pivot = pivot_over(data_id, 3);
    pivot.rows.push(axis(0));
    pivot.values.push(value(2, "vs West"));
    pivot.values.push(value(2, "vs previous"));
    wb.sheets[1].pivots.push(pivot);

    let mut derived = PivotDerived::default();
    derived.values.insert(
        0,
        PivotValueDerived {
            show_as: Some(PivotShowAs::Percent),
            base_field: Some(0),
            base_item: Some(PivotBaseItem::Item(3)),
            ..Default::default()
        },
    );
    derived.values.insert(
        1,
        PivotValueDerived {
            show_as: Some(PivotShowAs::RunTotal),
            base_field: Some(1),
            base_item: Some(PivotBaseItem::Previous),
            ..Default::default()
        },
    );

    let (_, table) = pivot_parts(&wb, &derived);
    assert!(
        table.contains(
            r#"<dataField name="vs West" fld="2" subtotal="sum" showDataAs="percent" baseField="0" baseItem="3"/>"#
        ),
        "{table}"
    );
    assert!(
        table.contains(
            r#"<dataField name="vs previous" fld="2" subtotal="sum" showDataAs="runTotal" baseField="1" baseItem="1048828"/>"#
        ),
        "{table}"
    );
}

/// **A base field that is not a source column is not written.**
///
/// `@baseField` is a *cache* field index and `docs/85` §8 makes the model's
/// `base_field` a `source_column`, which `structural.rs` renumbers with the
/// sheet (`PIV-06`, `PIV-08`). One past the source columns names a calculated
/// or a group field, which is not a thing a measure can be relative to; writing
/// it would be a plausible number pointing at the wrong field.
#[test]
fn a_base_field_past_the_source_columns_falls_back_rather_than_naming_a_derived_one() {
    use crate::pivot::{PivotDerived, PivotValueDerived};
    use casual_calc_model::PivotShowAs;

    let (mut wb, data_id) = pivot_workbook(&["Region", "Amount"]);
    let mut pivot = pivot_over(data_id, 2);
    pivot.rows.push(axis(0));
    pivot.values.push(value(1, "vs base"));
    wb.sheets[1].pivots.push(pivot);

    let mut derived = PivotDerived::default();
    derived.values.insert(
        0,
        PivotValueDerived {
            show_as: Some(PivotShowAs::Difference),
            base_field: Some(9),
            ..Default::default()
        },
    );

    let (_, table) = pivot_parts(&wb, &derived);
    assert!(
        table.contains(r#"showDataAs="difference" baseField="0" baseItem="1048832""#),
        "{table}"
    );
}

/// **Three group levels over one date column are three cache fields.**
///
/// Excel's Group dialog with Years, Quarters and Months ticked puts *three*
/// fields on the row axis, each a cache field with `@base` naming the original
/// (`docs/85` §3.2). Our model says the same thing as three `PivotAxisField`s
/// on one `source_column` with three different groups, and translating that
/// into OOXML's single cache-field space is this module's job.
///
/// **Where this diverges from Excel, deliberately.** Excel puts the *finest*
/// level on the base cache field itself and derives only the coarser ones. Here
/// the base field is never grouped and every level is derived, because the base
/// column can also be on an axis ungrouped in the same pivot, and one rule that
/// covers both beats two rules with an ordering question between them. The base
/// field is still written — it is what `@base` points at — but it is on no
/// axis.
#[test]
fn a_date_column_grouped_three_ways_becomes_three_cache_fields_the_row_axis_names() {
    use crate::pivot::{PivotAxisKind, PivotDerived};
    use casual_calc_model::{PivotGroup, PivotGroupBy};

    let (mut wb, data_id) = pivot_workbook(&["Date", "Amount"]);
    let mut pivot = pivot_over(data_id, 2);
    pivot.rows.push(axis(0));
    pivot.rows.push(axis(0));
    pivot.rows.push(axis(0));
    pivot.values.push(value(1, "Total"));
    wb.sheets[1].pivots.push(pivot);

    let group = |by| PivotGroup {
        by,
        interval: None,
        start: None,
        end: None,
    };
    let mut derived = PivotDerived::default();
    derived
        .groups
        .insert((PivotAxisKind::Rows, 0), group(PivotGroupBy::Years));
    derived
        .groups
        .insert((PivotAxisKind::Rows, 1), group(PivotGroupBy::Quarters));
    derived
        .groups
        .insert((PivotAxisKind::Rows, 2), group(PivotGroupBy::Months));

    let (cache, table) = pivot_parts(&wb, &derived);
    assert!(cache.contains(r#"<cacheFields count="5">"#), "{cache}");
    for (name, token) in [
        ("Years", "years"),
        ("Quarters", "quarters"),
        ("Months", "months"),
    ] {
        assert!(
            cache.contains(&format!(
                "<cacheField name=\"{name}\" numFmtId=\"0\" databaseField=\"0\"><sharedItems/>\
<fieldGroup base=\"0\"><rangePr groupBy=\"{token}\"/></fieldGroup></cacheField>"
            )),
            "{name} must be a derived field bucketing cache field 0: {cache}"
        );
    }
    assert!(
        table.contains(
            r#"<rowFields count="3"><field x="2"/><field x="3"/><field x="4"/></rowFields>"#
        ),
        "the axis names the derived fields, not the column they came from: {table}"
    );
    assert!(
        table.contains(r#"<pivotFields count="5"><pivotField showAll="0"/><pivotField dataField="1" showAll="0"/>"#),
        "the base column is written and is on no axis: {table}"
    );
}

/// **A group's interval and numeric bounds reach the file; a date bound does
/// not, and that is stated rather than silent.**
///
/// `@startNum`/`@endNum` are numbers. The seven time units want
/// `@startDate`/`@endDate`, which are `xsd:dateTime`, and turning a serial into
/// one needs a calendar this crate cannot reach — `serial_to_ymd` is in
/// `casual-calc-layout`, which is not a dependency and making it one is a DAG
/// change rather than an export change. It costs nothing in the first release,
/// whose three honoured units are all auto-bounded (`docs/85` §5.2).
#[test]
fn a_numeric_group_carries_its_bounds_and_a_dated_one_carries_its_interval_only() {
    use crate::pivot::{PivotAxisKind, PivotDerived};
    use casual_calc_model::{PivotGroup, PivotGroupBy};

    let (mut wb, data_id) = pivot_workbook(&["Score", "Date", "Amount"]);
    let mut pivot = pivot_over(data_id, 3);
    pivot.rows.push(axis(0));
    pivot.rows.push(axis(1));
    pivot.values.push(value(2, "Total"));
    wb.sheets[1].pivots.push(pivot);

    let mut derived = PivotDerived::default();
    derived.groups.insert(
        (PivotAxisKind::Rows, 0),
        PivotGroup {
            by: PivotGroupBy::Range,
            interval: Some(10.0),
            start: Some(0.0),
            end: Some(100.0),
        },
    );
    derived.groups.insert(
        (PivotAxisKind::Rows, 1),
        PivotGroup {
            by: PivotGroupBy::Years,
            interval: Some(2.0),
            start: Some(45_000.0),
            end: None,
        },
    );

    let (cache, _) = pivot_parts(&wb, &derived);
    assert!(
        cache.contains(
            r#"<rangePr groupBy="range" autoStart="0" startNum="0" autoEnd="0" endNum="100" groupInterval="10"/>"#
        ),
        "a numeric group states its own bounds: {cache}"
    );
    assert!(
        cache.contains(r#"<rangePr groupBy="years" groupInterval="2"/>"#),
        "a dated one keeps its interval and leaves the bounds automatic: {cache}"
    );
}

/// **A calculated field is a cache field with a formula and no source column.**
///
/// `<cacheField @formula databaseField="0">` is how Excel is told the field is
/// computed; without `databaseField="0"` it looks for a source column of that
/// name and does not find one. The measure's `fld` points past the source
/// columns at it, which is OOXML's single index space and exactly the collision
/// `docs/85` §5.3 refuses to reintroduce into the *model* — the translation
/// lives here and nowhere else.
///
/// The written `@subtotal` is `sum` whatever the model's aggregate says, because
/// Excel applies the formula rather than an aggregate and the aggregate is not
/// read for a calculated measure.
#[test]
fn a_calculated_field_is_a_cache_field_with_a_formula_and_the_measure_points_past_the_source() {
    use crate::pivot::{PivotDerived, PivotValueDerived};
    use casual_calc_model::{PivotAggregate, PivotCalculatedField};

    let (mut wb, data_id) = pivot_workbook(&["Region", "Rep", "Amount"]);
    let mut pivot = pivot_over(data_id, 3);
    pivot.rows.push(axis(0));
    pivot.values.push(value(2, "Total"));
    let mut bonus = value(0, "");
    bonus.aggregate = PivotAggregate::Average;
    pivot.values.push(bonus);
    wb.sheets[1].pivots.push(pivot);

    let mut derived = PivotDerived::default();
    derived.calculated.push(PivotCalculatedField {
        name: "Bonus".into(),
        formula: "Amount*0.1".into(),
        number_format: None,
    });
    derived.values.insert(
        1,
        PivotValueDerived {
            calculated: Some(0),
            ..Default::default()
        },
    );

    let (cache, table) = pivot_parts(&wb, &derived);
    assert!(cache.contains(r#"<cacheFields count="4">"#), "{cache}");
    assert!(
        cache.contains(
            r#"<cacheField name="Bonus" numFmtId="0" formula="Amount*0.1" databaseField="0"><sharedItems/></cacheField>"#
        ),
        "{cache}"
    );
    assert!(
        table.contains(
            r#"<dataField name="Bonus" fld="3" subtotal="sum" baseField="0" baseItem="0"/>"#
        ),
        "the measure names the calculated cache field, and `sum` rather than the \
         aggregate it will never apply: {table}"
    );
    assert!(
        table.contains(
            r#"<pivotFields count="4"><pivotField axis="axisRow" showAll="0"><items count="1"><item t="default"/></items></pivotField><pivotField showAll="0"/><pivotField dataField="1" showAll="0"/><pivotField dataField="1" showAll="0"/></pivotFields>"#
        ),
        "the calculated field is a measure like any other: {table}"
    );
}

/// **A formula's XML is escaped, and a derived field never takes a name another
/// field already has.**
///
/// Excel refuses two cache fields sharing a name, and it uniquifies the same way
/// — a second `Years` level is `Years2`. A `<` in a formula is legal in Excel's
/// pivot dialect and would end the attribute early if it reached the file raw.
#[test]
fn a_derived_field_name_never_collides_and_a_formula_is_escaped() {
    use crate::pivot::{PivotAxisKind, PivotDerived, PivotValueDerived};
    use casual_calc_model::{PivotCalculatedField, PivotGroup, PivotGroupBy};

    let (mut wb, data_id) = pivot_workbook(&["Opened", "Closed", "Years", "Amount"]);
    let mut pivot = pivot_over(data_id, 4);
    pivot.rows.push(axis(0));
    pivot.rows.push(axis(1));
    pivot.values.push(value(3, ""));
    wb.sheets[1].pivots.push(pivot);

    let group = PivotGroup {
        by: PivotGroupBy::Years,
        interval: None,
        start: None,
        end: None,
    };
    let mut derived = PivotDerived::default();
    derived.groups.insert((PivotAxisKind::Rows, 0), group);
    derived.groups.insert((PivotAxisKind::Rows, 1), group);
    derived.calculated.push(PivotCalculatedField {
        name: "Amount".into(),
        formula: r#"IF(Amount<0,"under & over",Amount)"#.into(),
        number_format: None,
    });
    derived.values.insert(
        0,
        PivotValueDerived {
            calculated: Some(0),
            ..Default::default()
        },
    );

    let (cache, table) = pivot_parts(&wb, &derived);
    assert!(
        cache.contains(
            r#"<cacheField name="Amount2" numFmtId="0" formula="IF(Amount&lt;0,&quot;under &amp; over&quot;,Amount)" databaseField="0">"#
        ),
        "the calculated field steps around the source column of the same name, \
         and its formula is escaped: {cache}"
    );
    assert!(
        cache.contains(r#"name="Years2""#) && cache.contains(r#"name="Years3""#),
        "two group levels step around the source column already called Years: {cache}"
    );
    assert!(
        table.contains(r#"<rowFields count="2"><field x="5"/><field x="6"/></rowFields>"#),
        "and the axis names them: {table}"
    );
    assert!(
        table.contains(r#"<dataField name="Amount2" fld="4""#),
        "an unnamed measure over a calculated field takes the field's own \
         written name, not the source column's: {table}"
    );
}

/// **Two slots asking for the same bucketing of the same column are two
/// fields, not one.**
///
/// Folding them into one looks like the tidy answer and is the wrong one: the
/// single field would be named by both `<rowFields>` and `<pageFields>`, which
/// is a field on two axes, and `<pivotField @axis>` can carry only one. A second
/// cache field with the same `@base` and the same `<rangePr>` is redundant; a
/// field on two axes is unreadable. This was caught by running the writer, not
/// by reading it — the first version deduplicated.
#[test]
fn one_bucketing_asked_for_by_two_slots_is_two_cache_fields_so_neither_is_on_two_axes() {
    use crate::pivot::{PivotAxisKind, PivotDerived};
    use casual_calc_model::{PivotFilterField, PivotGroup, PivotGroupBy};

    let (mut wb, data_id) = pivot_workbook(&["Date", "Amount"]);
    let mut pivot = pivot_over(data_id, 2);
    pivot.rows.push(axis(0));
    pivot.filters.push(PivotFilterField {
        source_column: 0,
        selected: Vec::new(),
    });
    pivot.values.push(value(1, "Total"));
    wb.sheets[1].pivots.push(pivot);

    let group = PivotGroup {
        by: PivotGroupBy::Months,
        interval: None,
        start: None,
        end: None,
    };
    let mut derived = PivotDerived::default();
    derived.groups.insert((PivotAxisKind::Rows, 0), group);
    derived.groups.insert((PivotAxisKind::Filters, 0), group);

    let (cache, table) = pivot_parts(&wb, &derived);
    assert!(
        cache.contains(r#"<cacheFields count="4">"#),
        "one derived field per slot: {cache}"
    );
    assert!(
        cache.contains(r#"name="Months""#) && cache.contains(r#"name="Months2""#),
        "{cache}"
    );
    assert!(
        table.contains(r#"<rowFields count="1"><field x="2"/></rowFields>"#)
            && table
                .contains(r#"<pageFields count="1"><pageField fld="3" hier="-1"/></pageFields>"#),
        "each slot names its own: {table}"
    );
    assert!(
        table.contains(
            r#"<pivotField axis="axisRow" showAll="0"><items count="1"><item t="default"/></items></pivotField><pivotField axis="axisPage" showAll="0"><items count="1"><item t="default"/></items></pivotField></pivotFields>"#
        ),
        "and each carries the one `@axis` a field can carry: {table}"
    );
}

/// **A calculated index past the end of the list falls back to the source
/// column.**
///
/// `fld` past the field count is the one way to make Excel offer to repair the
/// file. Nothing in the model can produce it today — the field does not exist —
/// and this keeps that true for the release that adds it.
#[test]
fn a_calculated_index_with_no_field_behind_it_never_reaches_the_file() {
    use crate::pivot::{PivotDerived, PivotValueDerived};

    let (mut wb, data_id) = pivot_workbook(&["Region", "Amount"]);
    let mut pivot = pivot_over(data_id, 2);
    pivot.rows.push(axis(0));
    pivot.values.push(value(1, "Total"));
    wb.sheets[1].pivots.push(pivot);

    let mut derived = PivotDerived::default();
    derived.values.insert(
        0,
        PivotValueDerived {
            calculated: Some(4),
            ..Default::default()
        },
    );

    let (cache, table) = pivot_parts(&wb, &derived);
    assert!(cache.contains(r#"<cacheFields count="2">"#), "{cache}");
    assert!(
        table.contains(r#"<dataField name="Total" fld="1" subtotal="sum""#),
        "{table}"
    );
}

/// **A package carrying all three still hangs together, and our own reader
/// still gets a pivot back.**
///
/// The parts are spliced into a written `.xlsx` because `write_workbook` reads
/// the derivation off the model, which cannot hold one yet; the bytes are the
/// ones slices C/D/E will produce through the ordinary path. The importer
/// reports the three as losses today (`PIV-05`) and *removing* those counters is
/// slice F — what matters here is that the definition is still readable as a
/// pivot rather than being dropped.
#[test]
fn a_package_carrying_all_three_derivations_is_still_read_back_as_a_pivot() {
    use crate::pivot::{PivotAxisKind, PivotDerived, PivotValueDerived};
    use casual_calc_model::{
        PivotAggregate, PivotCalculatedField, PivotGroup, PivotGroupBy, PivotShowAs,
    };

    let (mut wb, data_id) = pivot_workbook(&["Date", "Region", "Amount"]);
    let mut pivot = pivot_over(data_id, 3);
    pivot.rows.push(axis(0));
    pivot.rows.push(axis(1));
    pivot.values.push(value(2, "% of Grand Total"));
    pivot.values.push(value(0, "Bonus"));
    pivot.output = Some(casual_calc_model::CellRange::new(
        casual_calc_model::CellRef::new(2, 0),
        casual_calc_model::CellRef::new(5, 3),
    ));
    wb.sheets[1].pivots.push(pivot);

    let mut derived = PivotDerived::default();
    derived.groups.insert(
        (PivotAxisKind::Rows, 0),
        PivotGroup {
            by: PivotGroupBy::Years,
            interval: None,
            start: None,
            end: None,
        },
    );
    derived.calculated.push(PivotCalculatedField {
        name: "Bonus".into(),
        formula: "Amount*0.1".into(),
        number_format: None,
    });
    derived.values.insert(
        0,
        PivotValueDerived {
            show_as: Some(PivotShowAs::PercentOfTotal),
            ..Default::default()
        },
    );
    derived.values.insert(
        1,
        PivotValueDerived {
            calculated: Some(0),
            ..Default::default()
        },
    );

    let (cache, table) = pivot_parts(&wb, &derived);
    let written = write_workbook(&wb).expect("a workbook with a created pivot saves");
    let spliced = replace_parts(
        &written,
        &[
            ("xl/pivotCache/pivotCacheDefinition1.xml", cache.as_bytes()),
            ("xl/pivotTables/pivotTable1.xml", table.as_bytes()),
        ],
    );

    let back = import_package(spliced).unwrap().workbook;
    let read = &back.sheets[1].pivots;
    assert_eq!(read.len(), 1, "the pivot survives all three: {table}");
    assert_eq!(read[0].name, "Sales");
    // `Amount` is cache field 2 and is the only measure the importer keeps: the
    // `% of Grand Total` one is dropped as a `shown_as` loss and `Bonus` as a
    // `calculated_fields` loss, which is `PIV-05`'s honest accounting and what
    // slice F removes.
    assert_eq!(read[0].values.len(), 1, "{:?}", read[0].values);
    assert_eq!(read[0].values[0].aggregate, PivotAggregate::Sum);
}

/// Rebuild a `.xlsx` with some of its parts replaced. Used to hand a reader
/// bytes the ordinary write path cannot produce yet.
#[cfg(test)]
fn replace_parts(package: &[u8], replacements: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Read;

    let mut zip = zip::ZipArchive::new(Cursor::new(package)).unwrap();
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let name = file.name().to_owned();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        if let Some((_, replacement)) = replacements.iter().find(|(path, _)| *path == name) {
            data = replacement.to_vec();
        }
        out.start_file(&name, opts).unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

/// A `.xlsx` whose sheet carries one `<cfRule type="expression">` over
/// `A2:H10` — the whole-row highlight every real workbook uses — plus a `<dxf>`
/// for it to paint with.
///
/// Written out longhand rather than built from the model because the point of
/// the test below is what happens to a rule *arriving from a file*: the loss it
/// demonstrates happened on import, before the model was ever consulted.
fn expression_cf_xlsx() -> Vec<u8> {
    const DXF_STYLES: &[u8] =
        br#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
        <cellXfs count="1"><xf numFmtId="0"/></cellXfs>
        <dxfs count="1">
            <dxf><fill><patternFill><bgColor rgb="FFFFC7CE"/></patternFill></fill></dxf>
        </dxfs>
    </styleSheet>"#;
    let sheet_xml =
        br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
        <sheetData>
          <row r="2"><c r="A2" t="s"><v>0</v></c><c r="D2"><v>150</v></c></row>
          <row r="3"><c r="A3" t="s"><v>0</v></c><c r="D3"><v>50</v></c></row>
        </sheetData>
        <conditionalFormatting sqref="A2:H10">
            <cfRule type="expression" dxfId="0" priority="1"><formula>$D2&gt;100</formula></cfRule>
        </conditionalFormatting>
    </worksheet>"#
            .to_vec();
    zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/sharedStrings.xml", SHARED),
        ("xl/styles.xml", DXF_STYLES),
        ("xl/worksheets/sheet1.xml", &sheet_xml),
    ])
}

/// **A formula rule opened here and saved comes back out of the file.**
///
/// The `expression` rule used to be dropped on import — counted in the
/// compatibility report, but gone — so a user who opened a workbook with
/// whole-row highlighting and pressed save lost the rule from the *file* as
/// well as from the screen. That is the half of the defect that costs something
/// unrecoverable, so it is asserted end to end: file in, file out, rule still
/// there, formula unchanged.
#[test]
fn an_expression_conditional_format_survives_a_round_trip() {
    let wb = import_package(expression_cf_xlsx()).unwrap().workbook;
    assert_eq!(
        wb.sheets[0].conditional_formats.len(),
        1,
        "the file's one expression rule reaches the model"
    );

    let written = write_workbook(&wb).unwrap();
    let sheet_xml = xml_of(&written, "xl/worksheets/sheet1.xml");
    assert!(
        sheet_xml.contains(r#"type="expression""#),
        "and is written back as an expression rule: {sheet_xml}"
    );
    assert!(
        sheet_xml.contains("<formula>$D2&gt;100</formula>"),
        "with its formula anchored to the range's top-left, as it arrived: {sheet_xml}"
    );

    let back = import_package(written).unwrap().workbook;
    assert_eq!(
        back.sheets[0].conditional_formats, wb.sheets[0].conditional_formats,
        "import -> write -> import is a fixed point for an expression rule"
    );
}

// --- CHT-05: an edit must not convert a chart into a different chart --------
//
// The measured defect: the writer spelled `<c:grouping val="clustered"/>` as a
// literal for every bar and column chart, so a stacked chart that had been
// **retitled, dragged or resized** — all of which route through
// `session_set_chart` and therefore through `ChartView::detach` — was written
// back to the file as a clustered chart. Not a picture that was wrong on
// screen: the `.xlsx` on disk stopped saying what the author wrote, with
// nothing reported and nothing in the user's action to suggest a conversion.
//
// Retention is not what protects against this. All six probe packages
// round-trip byte-identical while they are *unedited*, because `retune_series`
// rewrites series references inside the retained part. It is the moment the
// part is dropped that the model becomes the whole of the chart, and the model
// had no grouping, no second group and no second axis to give back.

mod chart_survives_an_edit {
    use super::*;
    use casual_calc_model::{CellRange, CellRef, ChartGrouping, ChartKind};

    fn ser(idx: usize, name: &str, col: &str, dlbls: &str) -> String {
        format!(
            "<c:ser><c:idx val=\"{idx}\"/><c:order val=\"{idx}\"/><c:tx><c:v>{name}</c:v></c:tx>{dlbls}\
<c:cat><c:strRef><c:f>Sheet1!$A$2:$A$4</c:f></c:strRef></c:cat>\
<c:val><c:numRef><c:f>Sheet1!${col}$2:${col}$4</c:f></c:numRef></c:val></c:ser>"
        )
    }

    fn axes(cat: u32, val: u32) -> String {
        format!(
            "<c:catAx><c:axId val=\"{cat}\"/><c:crossAx val=\"{val}\"/></c:catAx>\
<c:valAx><c:axId val=\"{val}\"/><c:crossAx val=\"{cat}\"/></c:valAx>"
        )
    }

    fn chart_part(plot: &str) -> Vec<u8> {
        format!(
            "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" \
xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">\
<c:chart><c:plotArea><c:layout/>{plot}</c:plotArea>\
<c:plotVisOnly val=\"1\"/></c:chart></c:chartSpace>"
        )
        .into_bytes()
    }

    const DRAWING: &[u8] = br#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
      <xdr:twoCellAnchor>
        <xdr:from><xdr:col>5</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>
        <xdr:to><xdr:col>12</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>16</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
        <xdr:graphicFrame><a:graphic><a:graphicData><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId1"/></a:graphicData></a:graphic></xdr:graphicFrame>
        <xdr:clientData/>
      </xdr:twoCellAnchor>
    </xdr:wsDr>"#;

    fn package(chart: &[u8]) -> Vec<u8> {
        zip_parts(&[
            (
                "[Content_Types].xml",
                br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
                  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
                  <Override PartName="/xl/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/>
                </Types>"#,
            ),
            ("_rels/.rels", ROOT_RELS),
            ("xl/workbook.xml", WORKBOOK),
            ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
            (
                "xl/worksheets/sheet1.xml",
                br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
                  <sheetData>
                    <row r="2"><c r="B2"><v>100</v></c><c r="C2"><v>60</v></c><c r="D2"><v>0.4</v></c></row>
                    <row r="3"><c r="B3"><v>120</v></c><c r="C3"><v>70</v></c><c r="D3"><v>0.42</v></c></row>
                    <row r="4"><c r="B4"><v>140</v></c><c r="C4"><v>80</v></c><c r="D4"><v>0.43</v></c></row>
                  </sheetData>
                  <drawing r:id="rId9"/>
                </worksheet>"#,
            ),
            (
                "xl/worksheets/_rels/sheet1.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
                  <Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/>
                </Relationships>"#,
            ),
            ("xl/drawings/drawing1.xml", DRAWING),
            (
                "xl/drawings/_rels/drawing1.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
                  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart1.xml"/>
                </Relationships>"#,
            ),
            ("xl/charts/chart1.xml", chart),
        ])
    }

    /// **A stacked-to-clustered edit keeps the formatting the model cannot
    /// describe** (`CHT-16`).
    ///
    /// `CHT-05` fixed what detaching *loses*: the model carries grouping now, so
    /// a rebuild no longer converts a stacked chart to a clustered one. It did
    /// not stop the detach, and a rebuilt part still drops every element the
    /// model has no field for — which in a chart anybody has formatted is most
    /// of the file.
    ///
    /// The marker here is a gradient fill on the plot area. Nothing in
    /// `ChartView` can hold it, so it survives only if the retained part is
    /// edited rather than replaced. Asserting on the grouping alone would pass
    /// against a rebuild, which is exactly what this row is about.
    #[test]
    fn changing_the_grouping_keeps_formatting_the_model_cannot_hold() {
        const MARKER: &str = "<a:gradFill><a:gsLst><a:gs pos=\"0\"><a:srgbClr val=\"FF0000\"/></a:gs></a:gsLst></a:gradFill>";
        let plot = format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"stacked\"/>\
<c:spPr>{MARKER}</c:spPr>{}{}</c:barChart>{}",
            ser(0, "Rev", "B", ""),
            ser(1, "Cost", "C", ""),
            axes(1, 2),
        );
        let mut wb = import_package(package(&chart_part(&plot)))
            .unwrap()
            .workbook;
        assert!(
            wb.sheets[0].charts[0].part.is_some(),
            "the chart came from a file"
        );

        // What `session_set_chart` now does for a grouping change: rewrite the
        // model and **keep** the part, because this is expressible in place.
        wb.sheets[0].charts[0].grouping = Some(ChartGrouping::Clustered);

        let written = write_workbook(&wb).unwrap();
        let part = xml_of(&written, "xl/charts/chart1.xml");

        assert!(
            part.contains("<c:grouping val=\"clustered\"/>"),
            "the grouping was not rewritten in the retained part:\n{part}"
        );
        assert!(
            !part.contains("val=\"stacked\""),
            "the old grouping is still in the file:\n{part}"
        );
        assert!(
            part.contains("gradFill"),
            "the gradient the model cannot describe was dropped, which is the \
             detach this row is about:\n{part}"
        );
    }

    /// Open the package, drag the chart two columns to the left, save. The drag
    /// is deliberately something that has **nothing to do with what the chart
    /// is** — it changes only the anchor.
    fn opened_dragged_and_saved(chart: &[u8]) -> (casual_calc_model::Workbook, String) {
        let mut wb = import_package(package(chart)).unwrap().workbook;
        {
            let c = &mut wb.sheets[0].charts[0];
            assert!(c.part.is_some(), "the chart came from a file");
            c.anchor = CellRange::new(
                CellRef::new(c.anchor.start.row, 3),
                CellRef::new(c.anchor.end.row, 10),
            );
            // What `session_set_chart` does on any edit: the retained part no
            // longer describes the chart, so it stops being authoritative.
            c.detach();
        }
        let written = write_workbook(&wb).unwrap();
        let part = xml_of(&written, "xl/charts/chart1.xml");
        (wb, part)
    }

    /// **The one that matters most.** Dragging a stacked chart used to convert
    /// it, in the file, to a clustered one.
    #[test]
    fn dragging_a_stacked_chart_does_not_make_it_a_clustered_chart() {
        let source = chart_part(&format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"stacked\"/><c:varyColors val=\"0\"/>{}{}\
<c:overlap val=\"100\"/><c:axId val=\"111\"/><c:axId val=\"222\"/></c:barChart>{}",
            ser(0, "Rev", "B", ""),
            ser(1, "Cost", "C", ""),
            axes(111, 222)
        ));
        let (wb, part) = opened_dragged_and_saved(&source);

        assert_eq!(
            wb.sheets[0].charts[0].grouping,
            Some(ChartGrouping::Stacked),
            "the model kept what the file said"
        );
        assert!(
            part.contains("<c:grouping val=\"stacked\"/>"),
            "the written part is still a stacked chart:\n{part}"
        );
        assert!(
            !part.contains("clustered"),
            "the written part says clustered:\n{part}"
        );
        // Without this Excel draws the bands side by side, which is the
        // clustered picture again with a taller axis.
        assert!(
            part.contains("<c:overlap val=\"100\"/>"),
            "a stacked group needs its overlap:\n{part}"
        );
        // And it reads back as the chart it was, which is what makes the
        // survival a round trip rather than one lucky write.
        let back = import_package(write_workbook(&wb).unwrap())
            .unwrap()
            .workbook;
        assert_eq!(
            back.sheets[0].charts[0].grouping,
            Some(ChartGrouping::Stacked)
        );
    }

    #[test]
    fn dragging_a_percent_stacked_chart_keeps_it_normalised() {
        let source = chart_part(&format!(
            "<c:barChart><c:barDir val=\"bar\"/><c:grouping val=\"percentStacked\"/>{}{}\
<c:overlap val=\"100\"/><c:axId val=\"111\"/><c:axId val=\"222\"/></c:barChart>{}",
            ser(0, "Rev", "B", ""),
            ser(1, "Cost", "C", ""),
            axes(111, 222)
        ));
        let (_, part) = opened_dragged_and_saved(&source);
        assert!(
            part.contains("<c:grouping val=\"percentStacked\"/>"),
            "{part}"
        );
        assert!(part.contains("<c:barDir val=\"bar\"/>"), "{part}");
    }

    /// A combination chart used to come back as one group: the line series was
    /// written as a third column, so the file lost the line.
    #[test]
    fn dragging_a_combination_chart_keeps_both_of_its_groups() {
        let source = chart_part(&format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}{}\
<c:axId val=\"111\"/><c:axId val=\"222\"/></c:barChart>\
<c:lineChart><c:grouping val=\"standard\"/>{}\
<c:axId val=\"111\"/><c:axId val=\"222\"/></c:lineChart>{}",
            ser(0, "Rev", "B", ""),
            ser(1, "Cost", "C", ""),
            ser(2, "Margin", "D", ""),
            axes(111, 222)
        ));
        let (wb, part) = opened_dragged_and_saved(&source);

        assert_eq!(wb.sheets[0].charts[0].series[2].kind, Some(ChartKind::Line));
        assert!(part.contains("<c:barChart>"), "{part}");
        assert!(part.contains("<c:lineChart>"), "{part}");
        // The two bars in the first group, the line on its own.
        assert_eq!(part.matches("<c:ser>").count(), 3, "{part}");
        // Series order survives, which is what makes a run-based split worth
        // preferring to a gather by kind.
        let back = import_package(write_workbook(&wb).unwrap())
            .unwrap()
            .workbook;
        let names: Vec<&str> = back.sheets[0].charts[0]
            .series
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["Rev", "Cost", "Margin"]);
        assert_eq!(
            back.sheets[0].charts[0].series[2].kind,
            Some(ChartKind::Line)
        );
    }

    /// A secondary axis is a second `<c:axId>` pair and a second `<c:valAx>`.
    /// Dragging the chart used to leave one value axis, which puts a margin
    /// percentage on a revenue scale — drawn, and invisible.
    #[test]
    fn dragging_a_secondary_axis_chart_keeps_the_second_axis() {
        let source = chart_part(&format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}\
<c:axId val=\"111\"/><c:axId val=\"222\"/></c:barChart>\
<c:lineChart><c:grouping val=\"standard\"/>{}\
<c:axId val=\"333\"/><c:axId val=\"444\"/></c:lineChart>{}{}",
            ser(0, "Rev", "B", ""),
            ser(1, "Margin", "D", ""),
            axes(111, 222),
            axes(333, 444)
        ));
        let (wb, part) = opened_dragged_and_saved(&source);

        assert!(wb.sheets[0].charts[0].series[1].secondary_axis);
        assert_eq!(
            part.matches("<c:valAx>").count(),
            2,
            "two value axes, not one:\n{part}"
        );
        assert!(
            part.contains("<c:crosses val=\"max\"/>"),
            "the second axis sits opposite the first:\n{part}"
        );
        let back = import_package(write_workbook(&wb).unwrap())
            .unwrap()
            .workbook;
        assert!(back.sheets[0].charts[0].series[1].secondary_axis);
        assert!(!back.sheets[0].charts[0].series[0].secondary_axis);
    }

    #[test]
    fn dragging_a_labelled_chart_keeps_its_labels() {
        let source = chart_part(&format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}\
<c:axId val=\"111\"/><c:axId val=\"222\"/></c:barChart>{}",
            ser(0, "Rev", "B", "<c:dLbls><c:showVal val=\"1\"/></c:dLbls>"),
            axes(111, 222)
        ));
        let (wb, part) = opened_dragged_and_saved(&source);
        assert!(wb.sheets[0].charts[0].series[0].data_labels);
        assert!(part.contains("<c:showVal val=\"1\"/>"), "{part}");
        // The other five are written explicitly off: their defaults differ by
        // chart type, so leaving them unsaid means a label that says something
        // other than the value.
        assert!(part.contains("<c:showPercent val=\"0\"/>"), "{part}");
        let back = import_package(write_workbook(&wb).unwrap())
            .unwrap()
            .workbook;
        assert!(back.sheets[0].charts[0].series[0].data_labels);
    }

    /// The control: a clustered chart is written exactly as it was before this
    /// change, so nothing was fixed by making every chart stacked.
    #[test]
    fn a_clustered_chart_is_still_written_clustered() {
        let source = chart_part(&format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}{}\
<c:axId val=\"111\"/><c:axId val=\"222\"/></c:barChart>{}",
            ser(0, "Rev", "B", ""),
            ser(1, "Cost", "C", ""),
            axes(111, 222)
        ));
        let (_, part) = opened_dragged_and_saved(&source);
        assert!(part.contains("<c:grouping val=\"clustered\"/>"), "{part}");
        assert!(!part.contains("<c:overlap"), "{part}");
        assert_eq!(part.matches("<c:valAx>").count(), 1, "{part}");
        assert!(!part.contains("<c:dLbls>"), "{part}");
    }

    /// A grouping the group's own element cannot take is written as that
    /// element's default rather than as itself: `ST_Grouping` has no
    /// `clustered`, and a package that says otherwise is one Excel refuses.
    #[test]
    fn a_line_group_never_writes_clustered() {
        let mut chart = casual_calc_model::ChartView::new(
            CellRange::new(CellRef::new(0, 0), CellRef::new(9, 5)),
            ChartKind::Line,
        );
        chart.grouping = Some(ChartGrouping::Clustered);
        chart.series.push(casual_calc_model::ChartSeries {
            values: "Sheet1!$B$2:$B$4".to_owned(),
            ..Default::default()
        });
        let xml = crate::chart::chart_xml(&chart);
        assert!(xml.contains("<c:grouping val=\"standard\"/>"), "{xml}");
        assert!(!xml.contains("clustered"), "{xml}");
    }

    /// **`compatibility entries mentioning "chart": 0` — in every one of six
    /// packages, including four the engine drew wrongly.** AGENTS.md forbids
    /// silent loss, and the chart path reported nothing at all: not the type it
    /// could not draw, not the label kind it ignored, not the axis scale it
    /// discarded. Four of those six are modelled now; what is still not is
    /// named here rather than dropped in silence.
    #[test]
    fn a_chart_the_model_cannot_draw_is_named_in_the_report() {
        let radar = chart_part(
            "<c:radarChart><c:radarStyle val=\"marker\"/>\
<c:ser><c:tx><c:v>Rev</c:v></c:tx>\
<c:dLbls><c:showPercent val=\"1\"/></c:dLbls>\
<c:trendline><c:trendlineType val=\"linear\"/></c:trendline>\
<c:val><c:numRef><c:f>Sheet1!$B$2:$B$4</c:f></c:numRef></c:val></c:ser>\
</c:radarChart>",
        );
        let report = import_package(package(&radar)).unwrap().report;
        let named: Vec<String> = report
            .entries()
            .iter()
            .filter(|e| e.feature.starts_with("chart/"))
            .map(|e| e.feature.clone())
            .collect();
        assert!(
            named.contains(&"chart/unsupportedType".to_owned()),
            "{named:?}"
        );
        assert!(named.contains(&"chart/dLbls/kind".to_owned()), "{named:?}");
        assert!(named.contains(&"chart/trendline".to_owned()), "{named:?}");
        // Retained either way — a refused chart type is a lost picture, never a
        // lost file, and the report has to say which.
        for entry in report.entries() {
            if entry.feature.starts_with("chart/") {
                assert_eq!(
                    entry.retention,
                    casual_calc_import::RetentionOutcome::Preserved
                );
            }
        }

        // And a chart the model *can* express reports nothing, so the entries
        // above are a signal rather than noise every chart carries.
        let plain = chart_part(&format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"stacked\"/>{}\
<c:overlap val=\"100\"/><c:axId val=\"111\"/><c:axId val=\"222\"/></c:barChart>{}",
            ser(0, "Rev", "B", "<c:dLbls><c:showVal val=\"1\"/></c:dLbls>"),
            axes(111, 222)
        ));
        let quiet: Vec<String> = import_package(package(&plain))
            .unwrap()
            .report
            .entries()
            .iter()
            .filter(|e| e.feature.starts_with("chart/"))
            .map(|e| e.feature.clone())
            .collect();
        assert!(quiet.is_empty(), "{quiet:?}");
    }
}
