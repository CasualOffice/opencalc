//! FID-18: what this engine writes must be inside the addressable grid.
//!
//! These live in `casual-calc-export`'s tests rather than in the importer's,
//! for one reason: the assertion is about the **written package**, so it needs
//! both crates. `casual-calc-export` already dev-depends on
//! `casual-calc-import`; adding the opposite edge to run it from the other side
//! would have made the two mutually dev-dependent, and the crate DAG is fixed
//! by ADR-003 rather than by whichever direction a test happened to be written
//! in first.
//!
//! The importer's own half of this — that the reference is dropped and reported
//! — is asserted in `casual-calc-import`'s unit tests, which need no writer.

use std::io::{Cursor, Write};

use casual_calc_import::{ModelOutcome, RetentionOutcome, import_package};
use casual_calc_model::{CellRef, CellValue};
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

const ROOT_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
const WORKBOOK_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;

/// The last addressable row and column, **one-based**, as a file spells them.
/// `docs/21-PARSER-LIMITS.md`: 2^20 rows x 2^14 cols.
const LAST_ROW_1: u64 = 1_048_576;
const LAST_COL_1: u64 = 16_384;

const CONTENT_TYPES: &[u8] =
    b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>";

/// Every value in a written package that names a row or a column, with the part
/// it came from — so the assertion below is about the *file*, not about one cell
/// somebody remembered to look at.
///
/// Four shapes carry an address in SpreadsheetML, and all four are collected:
/// an A1 attribute (`<c r>`, `<mergeCell ref>`, `sqref`), a numeric axis
/// attribute (`<row r>`, `<col min max>`), and the element text of `<f>` and
/// `<definedName>`.
fn out_of_grid_in_package(bytes: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let name = entry.name().to_owned();
        if !(name.ends_with(".xml") || name.ends_with(".rels")) {
            continue;
        }
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut entry, &mut xml).unwrap();

        // Numeric axis attributes. `<row r="N">` is a row; `<col min max>` are
        // columns. Matched with a leading space so `r:id=` cannot be mistaken
        // for `r=`.
        for (attr, limit, axis) in [
            (" r=\"", LAST_ROW_1, "row"),
            (" min=\"", LAST_COL_1, "col"),
            (" max=\"", LAST_COL_1, "col"),
        ] {
            for value in attr_values(&xml, attr) {
                if let Ok(index) = value.parse::<u64>()
                    && !(1..=limit).contains(&index)
                {
                    found.push(format!("{name}: {axis} index {index}"));
                }
            }
        }

        // A1 attributes, including the space-separated multi-area `sqref`.
        for attr in [" r=\"", " ref=\"", " sqref=\"", " topLeftCell=\""] {
            for value in attr_values(&xml, attr) {
                for area in value.split_whitespace() {
                    for token in area.split(':') {
                        check_a1(&name, token, &mut found);
                    }
                }
            }
        }

        // Element text: a formula and a defined name's target are references
        // written as prose, and a package is just as corrupt when the address
        // is in one of those.
        for element in ["f", "definedName"] {
            for text in element_texts(&xml, element) {
                for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '$')) {
                    check_a1(&name, token, &mut found);
                }
            }
        }
    }
    found
}

/// Values of the attribute `attr` (given with its leading space and `="`).
fn attr_values<'a>(xml: &'a str, attr: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find(attr) {
        rest = &rest[at + attr.len()..];
        let Some(end) = rest.find('"') else { break };
        out.push(&rest[..end]);
        rest = &rest[end + 1..];
    }
    out
}

/// The text content of every `<element>` in `xml`.
fn element_texts<'a>(xml: &'a str, element: &str) -> Vec<&'a str> {
    let open = format!("<{element}");
    let close = format!("</{element}>");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find(&open) {
        rest = &rest[at + open.len()..];
        let Some(gt) = rest.find('>') else { break };
        if rest.as_bytes()[gt.saturating_sub(1)] == b'/' {
            rest = &rest[gt + 1..]; // self-closing: no text
            continue;
        }
        rest = &rest[gt + 1..];
        let Some(end) = rest.find(&close) else { break };
        out.push(&rest[..end]);
        rest = &rest[end + close.len()..];
    }
    out
}

/// Record `token` if it is an A1 address outside the grid. Anything that is not
/// an address at all (a function name, a sheet name, a number) is ignored — the
/// point is to catch addresses, not to parse the language a second time.
fn check_a1(part: &str, token: &str, found: &mut Vec<String>) {
    let token = token.rsplit('!').next().unwrap_or(token);
    let bare: String = token.chars().filter(|c| *c != '$').collect();
    let letters: String = bare.chars().take_while(char::is_ascii_alphabetic).collect();
    let digits = &bare[letters.len()..];
    if letters.is_empty() || digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return;
    }
    let mut col: u64 = 0;
    for c in letters.bytes() {
        col = col * 26 + u64::from(c.to_ascii_uppercase() - b'A' + 1);
        if col > u64::from(u32::MAX) {
            break;
        }
    }
    let row: u64 = digits.parse().unwrap_or(u64::MAX);
    if col > LAST_COL_1 || row > LAST_ROW_1 {
        found.push(format!("{part}: reference {token}"));
    }
}

/// A worksheet naming a row, a column, a merge and a defined name past the end
/// of the addressable grid.
///
/// Every out-of-range construct here has an in-range neighbour, because the fix
/// has to be a scalpel: refusing the whole package would have been the easy
/// answer and it throws away the nine tenths of the file that were fine.
fn beyond_the_grid_package() -> Vec<u8> {
    const WORKBOOK_DN: &[u8] = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets><definedNames><definedName name="Fine">Data!$A$1</definedName><definedName name="Beyond">Data!$A$1:$ZZZZ$4294967295</definedName></definedNames></workbook>"#;

    let sheet_xml = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
        <dimension ref="A1:ZZZZ4294967295"/>
        <cols>
          <col min="1" max="1" width="12" customWidth="1"/>
          <col min="20000" max="20005" width="30" customWidth="1"/>
          <col min="16380" max="16390" hidden="1"/>
        </cols>
        <sheetData>
          <row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>SUM(A1:A3)</f><v>1</v></c></row>
          <row r="2"><c r="A2"><f>SUM(A1:ZZZZ4294967295)</f><v>9</v></c></row>
          <row r="4294967295" ht="20" customHeight="1" hidden="1"><c r="ZZZZ4294967295"><v>7</v></c></row>
        </sheetData>
        <mergeCells count="2"><mergeCell ref="C1:D2"/><mergeCell ref="A1:ZZZZ4294967295"/></mergeCells>
      </worksheet>"#;

    zip_parts(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK_DN),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/worksheets/sheet1.xml", sheet_xml),
    ])
}

/// FID-18. A row or column past the SpreadsheetML maximum used to be imported
/// unbounded and written straight back: `row=4294967294 col=475253` landed in
/// the cell store, a 475,254-column merge landed in `sheet.merges` for layout to
/// walk, and the compatibility report was **empty**. The saved package was
/// outside ECMA-376's addressable grid — a file Excel and LibreOffice refuse to
/// open — so a bad input became a corrupt output with no diagnostic at all.
///
/// The disposition is drop-and-report (`Omitted` + `NotRetained`), never clamp:
/// see the module comment on `a1.rs` for why.
#[test]
fn references_past_the_grid_are_dropped_reported_and_never_written_back() {
    let import = import_package(beyond_the_grid_package()).unwrap();
    let sheet = &import.workbook.sheets[0];

    // 1. What was in range is still there. The point of reporting rather than
    //    refusing is that the rest of the workbook survives.
    assert_eq!(
        sheet.cells.get(CellRef::new(0, 0)).map(|c| c.value.clone()),
        Some(CellValue::Number(1.0))
    );
    assert_eq!(
        sheet.merges,
        vec![casual_calc_model::CellRange::new(
            CellRef::new(0, 2),
            CellRef::new(1, 3)
        )]
    );
    assert!(
        import
            .workbook
            .defined_names
            .iter()
            .any(|d| d.name == "Fine"),
        "an in-range name must not be collateral damage"
    );
    assert!(
        sheet.columns.sizes.contains_key(&0),
        "column A keeps its width"
    );

    // 2. Nothing out of range reached the model.
    assert_eq!(sheet.cells.len(), 3, "the out-of-grid cell is not stored");
    assert!(
        !import
            .workbook
            .defined_names
            .iter()
            .any(|d| d.name == "Beyond")
    );
    for (at, _) in sheet.cells.iter() {
        assert!(
            at.row < 1_048_576 && at.col < 16_384,
            "{at:?} escaped the grid"
        );
    }

    // 3. The report says so. `Omitted` + `NotRetained` is the one way data
    //    leaves the system, and docs/34 requires it to be counted and named.
    let features: Vec<(String, ModelOutcome, crate::RetentionOutcome)> = import
        .report
        .entries()
        .into_iter()
        .map(|e| (e.feature, e.model, e.retention))
        .collect();
    for expected in [
        "cellRef/outOfGrid",
        "mergeCell/outOfGrid",
        "definedName/outOfGrid",
        "col/outOfGrid",
        "row/outOfGrid",
        "f/outOfGrid",
        // `<dimension>` travels verbatim and is re-emitted verbatim, which is
        // exactly why it needs the same bound as everything parsed.
        "dimension/outOfGrid",
    ] {
        assert!(
            features.iter().any(|(f, m, r)| f == expected
                && *m == ModelOutcome::Omitted
                && *r == RetentionOutcome::NotRetained),
            "report is missing {expected}: {features:?}"
        );
    }

    // 4. And what we write is inside the grid — checked by walking the whole
    //    written package, because the invariant is about the file, not about
    //    the one construct the fix happened to think of.
    let written = casual_calc_export::write_workbook(&import.workbook).unwrap();
    let escapes = out_of_grid_in_package(&written);
    assert!(
        escapes.is_empty(),
        "the written package addresses cells outside the grid: {escapes:#?}"
    );
}

/// The walker is the whole gate, so it is itself tested: given a package that
/// *does* carry an out-of-grid address, it has to say so.
#[test]
fn the_grid_walker_catches_what_it_is_pointed_at() {
    let escapes = out_of_grid_in_package(&beyond_the_grid_package());
    assert!(
        escapes.len() >= 4,
        "the walker must flag the source package's own escapes: {escapes:#?}"
    );
}
