//! **Text from an edit that was undone must not reach the saved file, and text
//! that came out of the opened file must still be in it** (`FID-36`).
//!
//! The two halves are one test suite on purpose: either half is easy to satisfy
//! on its own, and the failure modes are opposite. Pruning nothing leaves the
//! name, the salary or the password a user typed and took back sitting in the
//! `.xlsx` they email to somebody else — a disclosure, not untidiness, and
//! under autosave it happens on a schedule. Pruning everything unreferenced
//! deletes `<si>` entries that were in the author's own file, which is the
//! silent data loss AGENTS.md forbids.

use std::io::{Cursor, Read, Write};

use casual_calc_model::{CellRef, CellValue, Workbook};
use casual_calc_sdk::{EditOperation, WorkbookSession};
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

/// Every part of a package, as text.
fn parts(package: &[u8]) -> Vec<(String, String)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(package)).unwrap();
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let name = f.name().to_owned();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();
        out.push((name, String::from_utf8_lossy(&buf).into_owned()));
    }
    out
}

fn part(package: &[u8], name: &str) -> Option<String> {
    parts(package)
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, body)| body)
}

/// The parts `needle` appears in — the whole package, not just the string
/// table, because a leak that moved to another part is still a leak.
fn parts_containing(package: &[u8], needle: &str) -> Vec<String> {
    parts(package)
        .into_iter()
        .filter(|(_, body)| body.contains(needle))
        .map(|(name, _)| name)
        .collect()
}

const CONTENT_TYPES: &[u8] =
    b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>";
const ROOT_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
const WORKBOOK: &[u8] = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
const WORKBOOK_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;

/// Two entries, and the one **no cell names** is first — so an implementation
/// that dropped it would have to renumber the other, and a cell that still said
/// `<v>1</v>` would come back reading the wrong text.
const SHARED: &[u8] = br#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2"><si><t>UnreferencedInTheOriginal</t></si><si><t>Used</t></si></sst>"#;
const SHEET: &[u8] = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="s"><v>1</v></c></row></sheetData></worksheet>"#;

fn authored_xlsx() -> Vec<u8> {
    zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/sharedStrings.xml", SHARED),
        ("xl/worksheets/sheet1.xml", SHEET),
    ])
}

/// **The reported case.** Type into a cell, undo, save: the typed text is gone
/// from the model, and must be gone from the bytes.
#[test]
fn text_from_an_undone_edit_does_not_reach_the_saved_file() {
    let mut session = WorkbookSession::with_sheet();
    let op = session.input_edit(0, CellRef::new(0, 0), "typo");
    session.edit(op).unwrap();
    session.undo().unwrap();

    let saved = session.save().unwrap();
    assert_eq!(
        parts_containing(&saved, "typo"),
        Vec::<String>::new(),
        "the undone text is still in the package: {:?}",
        part(&saved, "xl/sharedStrings.xml")
    );
    // Nothing refers to any string, so the part is not written at all — a
    // `<sst>` with no entries would be a part declaring it holds nothing.
    assert!(
        part(&saved, "xl/sharedStrings.xml").is_none(),
        "an empty shared-string part was written anyway"
    );
}

/// A formula's **cached string result** is interned exactly as typed text is,
/// and the cell that held it is just as gone after an undo.
#[test]
fn a_formula_result_from_an_undone_edit_does_not_reach_the_saved_file() {
    let mut session = WorkbookSession::with_sheet();
    let op = session.input_edit(0, CellRef::new(0, 0), "=\"leaked-result\"");
    session.edit(op).unwrap();
    session.undo().unwrap();

    let saved = session.save().unwrap();
    assert_eq!(
        parts_containing(&saved, "leaked-result"),
        Vec::<String>::new(),
        "an undone formula's computed text is still in the package"
    );
}

/// The other direction, and the one that makes this a judgement rather than a
/// delete: an `<si>` that was in the file the user opened is **theirs**. It
/// stays, referenced or not, and it keeps its index.
#[test]
fn a_string_that_came_from_the_file_survives_an_edit_and_undo() {
    let mut session = WorkbookSession::open(authored_xlsx()).unwrap();
    let op = session.input_edit(0, CellRef::new(5, 5), "typo");
    session.edit(op).unwrap();
    session.undo().unwrap();

    let saved = session.save().unwrap();
    let sst = part(&saved, "xl/sharedStrings.xml").expect("the table is still written");
    assert!(
        sst.contains("UnreferencedInTheOriginal"),
        "an entry from the author's own file was dropped: {sst}"
    );
    assert!(
        !sst.contains("typo"),
        "the undone text is still there: {sst}"
    );

    // And the preserved entries kept their positions, so the cell that named
    // index 1 still reads the same text.
    let sheet = part(&saved, "xl/worksheets/sheet1.xml").unwrap();
    assert!(
        sheet.contains("<v>1</v>"),
        "the cell's shared-string index moved: {sheet}"
    );
    let reopened = WorkbookSession::open(saved).unwrap();
    assert_eq!(reopened.cell_input(0, CellRef::new(0, 0)), "Used");
}

/// **The table shrinks underneath a cell that names it by index.**
///
/// Two strings are interned and only the second is placed in a cell, so the
/// survivor sits at a different position in the written table than in the
/// model's. A writer that emitted the model's index would point the cell past
/// the end of the table it had just written — a file that opens with the wrong
/// text in it, which is worse than the leak this change is about.
///
/// The cell is built as a `SharedString` deliberately: text the *editor* types
/// becomes an `InlineString` and carries its own characters into the sheet, so
/// the typing path never names a table index at all. A host that places a
/// shared string through the SDK does.
#[test]
fn a_cell_still_reads_back_after_the_table_shrinks_beneath_it() {
    let mut session = WorkbookSession::open(authored_xlsx()).unwrap();
    let abandoned = session.workbook_mut().intern_string("alpha");
    let placed = session.workbook_mut().intern_string("beta");
    assert!(
        abandoned.index() < placed.index(),
        "the abandoned entry has to sit below the placed one for this to bite"
    );
    session
        .edit(EditOperation::SetValue {
            sheet: 0,
            at: CellRef::new(1, 0),
            value: CellValue::SharedString(placed),
        })
        .unwrap();

    let saved = session.save().unwrap();
    let sst = part(&saved, "xl/sharedStrings.xml").unwrap();
    assert!(
        !sst.contains("alpha"),
        "the abandoned string is still in: {sst}"
    );
    assert!(sst.contains("beta"), "the live string was dropped: {sst}");

    let reopened = WorkbookSession::open(saved).unwrap();
    assert_eq!(reopened.cell_input(0, CellRef::new(0, 0)), "Used");
    assert_eq!(reopened.cell_input(0, CellRef::new(1, 0)), "beta");
}

/// Under collaboration a document is snapshotted and restored, and a watermark
/// that did not survive that would quietly re-label this session's discarded
/// text as the document's own — the leak back, one round trip later.
#[test]
fn a_discarded_string_stays_out_across_a_snapshot_round_trip() {
    let mut session = WorkbookSession::with_sheet();
    let op = session.input_edit(0, CellRef::new(0, 0), "typo");
    session.edit(op).unwrap();
    session.undo().unwrap();

    let snapshot = session.workbook().to_snapshot().expect("serialises");
    let restored = Workbook::from_snapshot(&snapshot).expect("loads");
    let saved = WorkbookSession::from_workbook(restored).save().unwrap();

    assert_eq!(
        parts_containing(&saved, "typo"),
        Vec::<String>::new(),
        "the snapshot laundered the undone text back into the file"
    );
}

/// The same round trip must not lose the author's unreferenced entry either:
/// the watermark has to survive in both directions or it is only half a fix.
#[test]
fn a_string_from_the_file_survives_a_snapshot_round_trip() {
    let session = WorkbookSession::open(authored_xlsx()).unwrap();
    let snapshot = session.workbook().to_snapshot().expect("serialises");
    let restored = Workbook::from_snapshot(&snapshot).expect("loads");

    let mut session = WorkbookSession::from_workbook(restored);
    let op = session.input_edit(0, CellRef::new(5, 5), "typo");
    session.edit(op).unwrap();
    session.undo().unwrap();

    let saved = session.save().unwrap();
    let sst = part(&saved, "xl/sharedStrings.xml").expect("the table is still written");
    assert!(
        sst.contains("UnreferencedInTheOriginal"),
        "the snapshot lost the author's unreferenced entry: {sst}"
    );
    assert!(
        !sst.contains("typo"),
        "the undone text is still there: {sst}"
    );
}

/// Saving twice must produce the same package: the second write sees the table
/// the first one wrote, and a rule that kept shrinking it would make a workbook
/// that never settles.
#[test]
fn writing_the_written_file_again_is_a_fixed_point() {
    let mut session = WorkbookSession::open(authored_xlsx()).unwrap();
    let op = session.input_edit(0, CellRef::new(0, 1), "kept");
    session.edit(op).unwrap();
    let once = session.save().unwrap();

    let twice = WorkbookSession::open(once.clone()).unwrap().save().unwrap();
    assert_eq!(
        part(&twice, "xl/sharedStrings.xml"),
        part(&once, "xl/sharedStrings.xml"),
        "the shared-string table changed on a second write"
    );
    assert_eq!(
        part(&twice, "xl/worksheets/sheet1.xml"),
        part(&once, "xl/worksheets/sheet1.xml"),
        "the sheet changed on a second write"
    );
}
