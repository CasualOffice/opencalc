//! Discovery tests over a minimal, in-memory `.xlsx`. Reaching "opens a trivial
//! .xlsx and discovers the workbook part" is the Phase 0 exit gate for F-011.

use std::io::{Cursor, Write};

use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::{OoxmlError, OoxmlLimits, SpreadsheetPackage};

const CONTENT_TYPES: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
</Types>"#;

const ROOT_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#;

const WORKBOOK: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
          xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="First" sheetId="1" r:id="rId1"/>
    <sheet name="Second" sheetId="2" r:id="rId2"/>
  </sheets>
</workbook>"#;

const WORKBOOK_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/>
</Relationships>"#;

const WORKSHEET: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#;

fn zip_parts(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in parts {
        writer.start_file(*name, opts).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn minimal_xlsx() -> Vec<u8> {
    zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/worksheets/sheet1.xml", WORKSHEET),
        ("xl/worksheets/sheet2.xml", WORKSHEET),
    ])
}

#[test]
fn discovers_workbook_and_sheets() {
    let mut pkg = SpreadsheetPackage::open(minimal_xlsx(), OoxmlLimits::default()).unwrap();

    assert_eq!(pkg.workbook_part(), "xl/workbook.xml");

    let sheets = pkg.sheets();
    assert_eq!(sheets.len(), 2);
    assert_eq!(sheets[0].name, "First");
    assert_eq!(sheets[0].sheet_id, 1);
    assert_eq!(sheets[0].part, "xl/worksheets/sheet1.xml");
    assert_eq!(sheets[1].name, "Second");
    assert_eq!(sheets[1].part, "xl/worksheets/sheet2.xml");

    // Resolved parts are readable through the bounded package.
    let sheet1 = pkg.read_part("xl/worksheets/sheet1.xml").unwrap();
    assert!(sheet1.windows(10).any(|w| w == b"<sheetData"));
}

#[test]
fn committed_minimal_fixture_parses() {
    // The checksummed corpus fixture must open through the real reader.
    let bytes = include_bytes!("../../../fixtures/generated/minimal.xlsx").to_vec();
    let pkg = SpreadsheetPackage::open(bytes, OoxmlLimits::default()).unwrap();
    assert_eq!(pkg.workbook_part(), "xl/workbook.xml");
    assert_eq!(pkg.sheets().len(), 1);
    assert_eq!(pkg.sheets()[0].name, "Sheet1");
    assert_eq!(pkg.sheets()[0].part, "xl/worksheets/sheet1.xml");
}

#[test]
fn missing_workbook_relationship_is_reported() {
    // Root rels without an officeDocument relationship.
    let broken_root = br#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://example.com/other" Target="xl/workbook.xml"/>
</Relationships>"#;
    let bytes = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", broken_root),
        ("xl/workbook.xml", WORKBOOK),
    ]);
    let err = SpreadsheetPackage::open(bytes, OoxmlLimits::default()).unwrap_err();
    assert!(matches!(err, OoxmlError::UnresolvableRelationship { .. }));
    assert_eq!(err.code(), "OC-IMP-0002");
}

#[test]
fn missing_workbook_part_is_reported() {
    // Root rels points at a workbook part that is not present.
    let bytes = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
    ]);
    let err = SpreadsheetPackage::open(bytes, OoxmlLimits::default()).unwrap_err();
    assert!(matches!(err, OoxmlError::MissingPart { .. }));
    assert_eq!(err.code(), "OC-IMP-0001");
}

#[test]
fn unresolved_sheet_relationship_is_reported() {
    // workbook.xml references rId2, but workbook rels only defines rId1.
    let short_rels = br#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#;
    let bytes = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", short_rels),
        ("xl/worksheets/sheet1.xml", WORKSHEET),
    ]);
    let err = SpreadsheetPackage::open(bytes, OoxmlLimits::default()).unwrap_err();
    assert!(matches!(err, OoxmlError::UnresolvableRelationship { .. }));
    assert_eq!(err.code(), "OC-IMP-0002");
}

#[test]
fn malformed_xml_is_reported() {
    let bad_workbook = b"<workbook><sheets><sheet name=";
    let bytes = zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", bad_workbook),
    ]);
    let err = SpreadsheetPackage::open(bytes, OoxmlLimits::default()).unwrap_err();
    assert!(matches!(err, OoxmlError::MalformedXml(_)));
    assert_eq!(err.code(), "OC-XML-0004");
}

/// Both halves of `[Content_Types].xml`, and the precedence between them.
///
/// The reader used to return the `<Override>` map alone and call that the
/// content types of the package. It reads as complete right up until the file
/// declares something by extension, which every real one does: printer
/// settings, images, embedded objects. Everything downstream then saw `None`
/// for a part whose type the file states plainly (FID-17).
#[test]
fn a_content_type_declared_by_extension_resolves_like_one_declared_by_part() {
    const TYPES: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Default Extension="bin" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.printerSettings"/>
  <Default Extension="EMF" ContentType="image/x-emf"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/EMBEDDINGS/oleObject1.bin" ContentType="application/vnd.openxmlformats-officedocument.oleObject"/>
</Types>"#;
    let bytes = zip_parts(&[
        ("[Content_Types].xml", TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/worksheets/sheet1.xml", WORKSHEET),
        ("xl/worksheets/sheet2.xml", WORKSHEET),
    ]);
    let mut pkg = SpreadsheetPackage::open(bytes, OoxmlLimits::default()).unwrap();
    let types = pkg.content_types().unwrap();

    // By extension — the half that was missing.
    assert_eq!(
        types.resolve("xl/printerSettings/printerSettings1.bin"),
        Some("application/vnd.openxmlformats-officedocument.spreadsheetml.printerSettings")
    );
    // The override wins over the default for the same extension, which is the
    // reason a `<Default Extension="bin">` cannot be assumed to describe every
    // `.bin` in the package.
    assert_eq!(
        types.resolve("xl/embeddings/oleObject1.bin"),
        Some("application/vnd.openxmlformats-officedocument.oleObject")
    );
    // Case folds on both sides: OPC compares part names and extensions
    // case-insensitively, and a file that mixes them is well-formed.
    assert_eq!(types.resolve("xl/media/image1.emf"), Some("image/x-emf"));
    assert_eq!(
        types.resolve("/xl/workbook.xml").unwrap(),
        types.resolve("xl/WORKBOOK.xml").unwrap()
    );
    // No extension, and no override: undeclared, and said so rather than
    // guessed at.
    assert_eq!(types.resolve("xl/media/stream"), None);
}
