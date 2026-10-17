//! **What a file carried that this engine cannot compute or parse must survive
//! an edited save** (`XL-FEAT-005`, `XL-FEAT-006`, `XL-FEAT-007`).
//!
//! An unedited save hands back the opened bytes, so every one of these losses
//! was invisible until somebody changed one unrelated cell — and then the
//! semantic writer rebuilt the sheet from the model, which had already thrown
//! the construct away:
//!
//! - a function the engine does not implement was recalculated to `#NAME?` on
//!   open, saved as `#NAME?`, and lost its `_xlfn.` prefix, so Excel itself
//!   then read `#NAME?` too (005);
//! - a formula the parser could not read — an array constant, a 3-D reference —
//!   was kept only as its cached value and saved as a constant (006);
//! - the worksheet's `<extLst>`, where sparklines live, was not read at all
//!   (007);
//!
//! and `format_loss()` said nothing about any of it.

use std::io::{Cursor, Read, Write};

use casual_calc_model::{CellRef, CellValue};
use casual_calc_sdk::{ModelOutcome, SessionFormat, WorkbookSession};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

fn zip_parts(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in parts {
        writer.start_file(*name, opts).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn part(package: &[u8], name: &str) -> Option<String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(package)).unwrap();
    let mut f = zip.by_name(name).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).unwrap();
    Some(String::from_utf8_lossy(&buf).into_owned())
}

const CONTENT_TYPES: &[u8] =
    b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>";
const ROOT_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
const WORKBOOK: &[u8] = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/><sheet name="Sheet2" sheetId="2" r:id="rId2"/></sheets></workbook>"#;
const WORKBOOK_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#;

/// The sparkline group exactly as Excel writes it: the `x14` namespace is
/// declared on the *root* element, so a passthrough that copied only the
/// `<extLst>` subtree would write a prefix nothing declares.
const SPARKLINES: &str = r#"<extLst><ext uri="{05C60535-1F16-4fd2-B633-F4F36F0B64E0}"><x14:sparklineGroups xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main"><x14:sparklineGroup displayEmptyCellsAs="gap"><x14:colorSeries rgb="FF376092"/><x14:sparklines><x14:sparkline><xm:f>Sheet1!B1:B3</xm:f><xm:sqref>D1</xm:sqref></x14:sparkline></x14:sparklines></x14:sparklineGroup></x14:sparklineGroups></ext></extLst>"#;

fn sheet1() -> String {
    concat!(
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
            r#"xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main">"#,
            "<sheetData>",
            // 005: a future function this engine has never heard of, and its
            // dependent. Excel shows 42 and 43.
            r#"<row r="1"><c r="A1"><f>_xlfn.FUTUREFN(1)</f><v>42</v></c><c r="B1"><v>3</v></c></row>"#,
            // 006: an array constant.
            r#"<row r="2"><c r="A2"><f>SUM({1,2,3})</f><v>6</v></c><c r="B2"><v>1</v></c></row>"#,
            // 006: a 3-D reference.
            r#"<row r="3"><c r="A3"><f>SUM(Sheet1:Sheet2!B1)</f><v>7</v></c><c r="B3"><v>2</v></c></row>"#,
            r#"<row r="4"><c r="A4"><f>A1+1</f><v>43</v></c></row>"#,
            // 005, the probe's own case: a real Excel function this engine
            // does not implement, written without a prefix. (It was `XOR`
            // until XL-FEAT-012 implemented that; `PHONETIC` is out of scope
            // there and predates the `_xlfn.` prefix, so it is written bare.)
            r#"<row r="5"><c r="A5"><f>PHONETIC(B1)</f><v>7</v></c></row>"#,
            "</sheetData>@EXT@</worksheet>"
    )
    .replace("@EXT@", SPARKLINES)
}

const SHEET2: &[u8] = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="B1"><v>4</v></c></row></sheetData></worksheet>"#;

fn authored_xlsx(sheet1: &str) -> Vec<u8> {
    zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/worksheets/sheet1.xml", sheet1.as_bytes()),
        ("xl/worksheets/sheet2.xml", SHEET2),
    ])
}

fn number(session: &WorkbookSession, sheet: usize, at: CellRef) -> CellValue {
    session.workbook().sheets[sheet]
        .cells
        .get(at)
        .map(|c| c.value.clone())
        .unwrap_or(CellValue::Empty)
}

/// Open, change one unrelated cell, save.
fn edited_save(bytes: Vec<u8>) -> (WorkbookSession, Vec<u8>) {
    let mut session = WorkbookSession::open(bytes).unwrap();
    let op = session.input_edit(0, CellRef::new(9, 2), "edited");
    session.edit(op).unwrap();
    assert!(!session.is_unmodified(), "the semantic writer has to run");
    let saved = session.save().unwrap();
    (session, saved)
}

/// **XL-FEAT-005, on open.** The cached value is what Excel shows; `#NAME?` is
/// what this engine would compute. The file's answer wins, and so does every
/// formula reading it.
#[test]
fn an_unsupported_function_opens_showing_its_cached_value() {
    let session = WorkbookSession::open(authored_xlsx(&sheet1())).unwrap();
    assert_eq!(
        number(&session, 0, CellRef::new(0, 0)),
        CellValue::Number(42.0),
        "_xlfn.FUTUREFN(1)"
    );
    assert_eq!(
        number(&session, 0, CellRef::new(3, 0)),
        CellValue::Number(43.0),
        "A1+1 reads the cached 42"
    );
    assert_eq!(
        number(&session, 0, CellRef::new(4, 0)),
        CellValue::Number(7.0),
        "PHONETIC(B1)"
    );
    // Kept, and said so: a value this engine did not compute is stale the
    // moment its inputs change.
    let report = session.compatibility_report();
    let named: Vec<String> = report.entries().into_iter().map(|e| e.feature).collect();
    assert!(
        named.iter().any(|f| f.contains("FUTUREFN"))
            && named.iter().any(|f| f.contains("PHONETIC")),
        "the import report does not name the unsupported functions: {named:?}"
    );
}

/// **XL-FEAT-005/006, on save.** Every formula comes back as the file wrote
/// it — prefix included — with the value the file cached.
#[test]
fn formulas_the_engine_cannot_compute_or_parse_survive_an_edited_save() {
    let (mut session, saved) = edited_save(authored_xlsx(&sheet1()));
    // An edit that dirties the kept cells' readers: the incremental path has
    // to keep the file's value too, not only the full recalc on open.
    let op = session.input_edit(1, CellRef::new(0, 1), "40");
    session.edit(op).unwrap();
    assert_eq!(
        number(&session, 0, CellRef::new(0, 0)),
        CellValue::Number(42.0)
    );
    assert_eq!(
        number(&session, 0, CellRef::new(3, 0)),
        CellValue::Number(43.0)
    );
    let op = session.input_edit(0, CellRef::new(0, 0), "=_xlfn.FUTUREFN(1)+0");
    session.edit(op).unwrap();
    assert!(
        matches!(number(&session, 0, CellRef::new(3, 0)), CellValue::Error(_)),
        "a retyped formula is the person's own and is #NAME? — and so is its reader"
    );

    let xml = part(&saved, "xl/worksheets/sheet1.xml").unwrap();
    for (formula, cached) in [
        ("_xlfn.FUTUREFN(1)", "42"),
        ("SUM({1,2,3})", "6"),
        ("SUM(Sheet1:Sheet2!B1)", "7"),
    ] {
        let wanted = format!("<f>{formula}</f><v>{cached}</v>");
        assert!(
            xml.contains(&wanted),
            "`{wanted}` did not survive the edited save:\n{xml}"
        );
    }
    assert!(
        !xml.contains("#NAME?"),
        "a cached value was replaced:\n{xml}"
    );

    // And it reads back as the same document.
    let reopened = WorkbookSession::open(saved).unwrap();
    assert_eq!(reopened.cell_input(0, CellRef::new(1, 0)), "=SUM({1,2,3})");
    assert_eq!(
        reopened.cell_input(0, CellRef::new(2, 0)),
        "=SUM(Sheet1:Sheet2!B1)"
    );
    assert_eq!(
        number(&reopened, 0, CellRef::new(0, 0)),
        CellValue::Number(42.0)
    );
    assert_eq!(
        number(&reopened, 0, CellRef::new(2, 0)),
        CellValue::Number(7.0)
    );
}

/// **XL-FEAT-007.** The sparkline group, and every namespace it uses, is still
/// in the sheet after an unrelated edit.
#[test]
fn a_worksheet_ext_lst_survives_an_edited_save() {
    let (session, saved) = edited_save(authored_xlsx(&sheet1()));
    let xml = part(&saved, "xl/worksheets/sheet1.xml").unwrap();
    assert!(
        xml.contains(
            "<x14:sparkline><xm:f>Sheet1!B1:B3</xm:f><xm:sqref>D1</xm:sqref></x14:sparkline>"
        ),
        "the sparkline group was dropped:\n{xml}"
    );
    assert!(
        xml.contains(
            r#"xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main""#
        ),
        "the x14 prefix is written undeclared:\n{xml}"
    );
    // The last child of `<worksheet>`, as the schema requires.
    assert!(
        xml.trim_end().ends_with("</extLst></worksheet>"),
        "extLst is not the last element:\n{xml}"
    );
    // The package still opens, and a second edited save keeps it again.
    let (_, again) = edited_save(saved);
    assert!(
        part(&again, "xl/worksheets/sheet1.xml")
            .unwrap()
            .contains("x14:sparklineGroups"),
        "the second round trip lost it"
    );
    assert!(
        session.format_loss().is_empty(),
        "nothing was lost, so nothing is reported: {:?}",
        session.format_loss().entries()
    );
    // A format that cannot carry it says so.
    let ods = session.loss_writing(SessionFormat::Ods);
    assert!(
        ods.entries()
            .iter()
            .any(|e| e.feature.contains("sparklines") && e.count == 1),
        "an .ods save drops the sparklines without naming them: {:?}",
        ods.entries()
    );
}

/// **What still cannot be kept is named.** A shared-formula follower carries no
/// text of its own, only "the master's formula, shifted" — and a master this
/// parser cannot read cannot be shifted. The follower is saved as its value,
/// and the save-loss report has to say so before the download.
#[test]
fn a_follower_of_an_unreadable_shared_master_is_named_in_the_save_loss() {
    let sheet = concat!(
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
        // The intersection operator (a space), which this parser does not read.
        // The example was an array constant until XL-FEAT-014 made that one
        // readable — and the follower then expanded, as it should.
        r#"<row r="1"><c r="A1"><f t="shared" ref="A1:A2" si="0">SUM(B1:C2 C1:D2)+B1</f><v>3</v></c></row>"#,
        r#"<row r="2"><c r="A2"><f t="shared" si="0"/><v>3</v></c></row>"#,
        "</sheetData></worksheet>"
    );
    let (session, saved) = edited_save(authored_xlsx(sheet));
    let loss = session.format_loss();
    let entry = loss
        .entries()
        .into_iter()
        .find(|e| e.feature.contains("shared"))
        .unwrap_or_else(|| panic!("the dropped follower is not named: {:?}", loss.entries()));
    assert_ne!(entry.model, ModelOutcome::Mapped);
    assert_eq!(entry.count, 1, "one follower, counted once");
    // The master itself is not lost.
    let xml = part(&saved, "xl/worksheets/sheet1.xml").unwrap();
    assert!(xml.contains("<f>SUM(B1:C2 C1:D2)+B1</f>"), "{xml}");
}

/// The other side of 005: a formula a *person* types with an unknown function
/// is `#NAME?`, as in Excel. Only a value the file carried is kept.
#[test]
fn a_typed_unknown_function_is_still_name_error() {
    let mut session = WorkbookSession::open(authored_xlsx(&sheet1())).unwrap();
    // Over the very cell whose cached value was being kept.
    let op = session.input_edit(0, CellRef::new(0, 0), "=FUTUREFN(2)");
    session.edit(op).unwrap();
    assert!(
        matches!(number(&session, 0, CellRef::new(0, 0)), CellValue::Error(_)),
        "a typed unknown function kept the old cached value: {:?}",
        number(&session, 0, CellRef::new(0, 0))
    );
}
