//! **An edited save does not carry a stale digital signature** (`XL-FEAT-010`).
//!
//! An OOXML package signature lives in `_xmlsignatures/`, reached from the
//! package root by a `digital-signature/origin` relationship. Nothing here
//! models it, so it used to be retained like any unknown part and written back
//! verbatim — over content the edit had just changed. A broken signature reads
//! to the recipient as tampering. Excel removes the signature when a signed
//! workbook is edited and saved, and says so; this does the same, and names
//! the removal in the save-loss report.
//!
//! An unedited save hands back the opened bytes, where the signature is still
//! valid, and that is what it must keep doing.

use std::io::{Cursor, Read, Write};

use casual_calc_model::{CellRef, CellValue};
use casual_calc_sdk::{ModelOutcome, RetentionOutcome, WorkbookSession};
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

fn names(package: &[u8]) -> Vec<String> {
    let zip = zip::ZipArchive::new(Cursor::new(package)).unwrap();
    zip.file_names().map(|n| n.unwrap().into_owned()).collect()
}

fn part(package: &[u8], name: &str) -> Option<String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(package)).unwrap();
    let mut f = zip.by_name(name).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).unwrap();
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Declared the way Excel declares them: a `Default` for the `.sigs` origin
/// part, an `Override` per signature part.
const CONTENT_TYPES: &[u8] = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="sigs" ContentType="application/vnd.openxmlformats-package.digital-signature-origin"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/_xmlsignatures/sig1.xml" ContentType="application/vnd.openxmlformats-package.digital-signature-xmlsignature+xml"/></Types>"#;
const ROOT_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/origin" Target="_xmlsignatures/origin.sigs"/></Relationships>"#;
const ORIGIN_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/signature" Target="sig1.xml"/></Relationships>"#;
/// Not a cryptographically valid signature — nothing here verifies one, and
/// the defect is about carrying it, not checking it.
const SIG1: &[u8] = br#"<Signature xmlns="http://www.w3.org/2000/09/xmldsig#" Id="idPackageSignature"><SignedInfo/><SignatureValue>AAAA</SignatureValue></Signature>"#;
const WORKBOOK: &[u8] = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
const WORKBOOK_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
const SHEET1: &[u8] = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#;

fn signed_xlsx() -> Vec<u8> {
    zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/worksheets/sheet1.xml", SHEET1),
        ("_xmlsignatures/origin.sigs", b""),
        ("_xmlsignatures/_rels/origin.sigs.rels", ORIGIN_RELS),
        ("_xmlsignatures/sig1.xml", SIG1),
    ])
}

/// The id the save-loss report names the removal under.
const REMOVED: &str = "signature/removed";

#[test]
fn an_edited_save_removes_the_signature_and_says_so() {
    let mut session = WorkbookSession::open(signed_xlsx()).unwrap();
    let op = session.input_edit(0, CellRef::new(0, 1), "edited");
    session.edit(op).unwrap();
    assert!(!session.is_unmodified(), "the semantic writer has to run");

    // Asked before the save, where the person can still decide.
    let loss = session.format_loss();
    let saved = session.save().unwrap();
    let parts = names(&saved);
    assert!(
        !parts.iter().any(|p| p.starts_with("_xmlsignatures/")),
        "no signature part survives an edit: {parts:?}"
    );
    let rels = part(&saved, "_rels/.rels").unwrap();
    assert!(
        !rels.contains("digital-signature"),
        "no relationship to a removed part: {rels}"
    );
    let types = part(&saved, "[Content_Types].xml").unwrap();
    assert!(
        !types.contains("digital-signature") && !types.contains("_xmlsignatures"),
        "no content type for a removed part: {types}"
    );

    // Named in the report asked for before the save.
    let entry = loss
        .entries()
        .into_iter()
        .find(|e| e.feature == REMOVED)
        .unwrap_or_else(|| panic!("the removal is named: {:?}", loss.entries()));
    assert_eq!(entry.model, ModelOutcome::Omitted);
    assert_eq!(entry.retention, RetentionOutcome::NotRetained);

    // And the package is one this engine reads back, with the edit in it.
    let reopened = WorkbookSession::open(saved).unwrap();
    let b1 = reopened.workbook().sheets[0]
        .cells
        .get(CellRef::new(0, 1))
        .map(|c| c.value.clone())
        .unwrap_or(CellValue::Empty);
    assert!(
        matches!(b1, CellValue::SharedString(_) | CellValue::InlineString(_)),
        "the edit is in the saved file: {b1:?}"
    );
    assert!(!reopened.is_signed(), "the saved file is not signed");
}

#[test]
fn an_unedited_save_keeps_the_signature_byte_for_byte() {
    let source = signed_xlsx();
    let session = WorkbookSession::open(source.clone()).unwrap();
    assert!(
        session.is_signed(),
        "import records that the file is signed"
    );
    assert!(
        session
            .compatibility_report()
            .entries()
            .iter()
            .any(|e| e.feature == "signature" && e.retention == RetentionOutcome::Preserved),
        "and its report says so"
    );
    assert!(
        !session
            .format_loss()
            .entries()
            .iter()
            .any(|e| e.feature == REMOVED),
        "nothing is removed from the bytes handed back"
    );
    assert_eq!(session.save().unwrap(), source);
}
