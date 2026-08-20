//! The SpreadsheetML importer, against arbitrary bytes.
//!
//! `.xlsx` is **the** format this engine exists to read, and it arrives from
//! somewhere else by definition. `casual-calc-import` walks a dozen readers —
//! shared strings, styles, theme, worksheets, comments, threaded comments,
//! defined names, tables, drawings, charts, pivot tables and pivot caches —
//! and until this target none of them had ever been fuzzed. The two targets
//! that looked as though they covered this do not:
//!
//! * `bounded_package` stops at the ZIP/OPC container and never decodes XML.
//! * `ooxml_xml` fuzzes `parse_relationships` and `parse_sheet_refs`, which are
//!   the two OPC helpers in `casual-calc-ooxml` — not the readers that build
//!   the model.
//!
//! That is the same gap the `ods` target closed in the sibling format, where it
//! found three defects in one session, two of them P0.
//!
//! # Refusing is passing
//!
//! What is asserted is that the importer **returns** — with an [`Import`], or
//! with an `ImportError`. `NotAPackage` on bytes that are not a zip,
//! `Malformed` on XML that does not parse, `OverBudget` on a document past a
//! limit are all correct answers. Only a panic, a hang or an out-of-memory is a
//! defect, because those are what an uploader can aim at a server.
//!
//! # Why the input is wrapped, and also not
//!
//! Three passes over the same bytes, for the reason `ods.rs` and `ooxml_xml.rs`
//! both record: arbitrary bytes almost never form a valid ZIP, so a target that
//! only did pass 1 would spend its whole budget being turned away by the
//! container and never reach a reader.
//!
//! 1. **As a whole package.** The real entry point, and the only pass that
//!    covers the container boundary and the relationship graph. Seeded with
//!    real workbooks from five producers.
//! 2. **As `xl/worksheets/sheet1.xml`** inside an otherwise valid skeleton.
//!    This is the largest reader and the one that materialises cells, which is
//!    where the ODS amplification defect lived.
//! 3. **As `xl/styles.xml`** inside the same skeleton. Styles are read before
//!    any sheet, resolve theme slots and tints, and index `cellXfs` — a table
//!    a hostile file controls the length of.
//!
//! Each pass runs twice: once at the shipped limits, and once at [`TIGHT`],
//! which is small enough for a fuzzer's input to cross. See that constant for
//! why the second run exists at all.
//!
//! The part readers are private to the crate, so the harness builds the package
//! around the mutated document rather than calling them: every byte the fuzzer
//! changes still lands in XML.
//!
//! Seeded from `fuzz/seeds/xlsx/`.

#![no_main]

use std::io::{Cursor, Write};

use casual_calc_model::Workbook;
use casual_calc_ooxml::{OoxmlLimits, SpreadsheetLimits};
use libfuzzer_sys::fuzz_target;

/// A document budget small enough that a fuzzer's input can cross it.
///
/// **This is what makes the budget assertion a test rather than a decoration.**
/// The shipped `SpreadsheetLimits` allow eight million cells, and no input a
/// fuzzer produces under any sane `-max_len` comes within three orders of
/// magnitude of that — so an assertion against the default limit could never
/// fire, in this harness or in CI, and would be a branch that reports on its
/// own existence. Every real workbook in the corpus is *over* these numbers,
/// which is the point: at this budget the importer must refuse, and if it
/// admits anyway the assertion below says so.
///
/// The numbers are deliberately not round multiples of anything in the corpus,
/// so a fixture growing by a row does not quietly move the boundary.
const TIGHT: SpreadsheetLimits = SpreadsheetLimits {
    max_populated_cells: 37,
    max_shared_strings: 11,
    max_defined_names: 3,
    max_merged_ranges: 2,
};

fuzz_target!(|data: &[u8]| {
    let limits = OoxmlLimits::default();
    let tight = OoxmlLimits {
        spreadsheet: TIGHT,
        ..limits
    };

    // Pass 1: the container boundary, and any real `.xlsx` in the corpus.
    admit(data.to_vec(), limits, tight);

    // Passes 2 and 3: the readers, reached with the container out of the way.
    for part in [WORKSHEET_PART, STYLES_PART] {
        if let Some(package) = as_package(part, data) {
            admit(package, limits, tight);
        }
    }
});

/// Import once at the shipped limits and once at [`TIGHT`], and hold each
/// result to the budget it was given.
fn admit(bytes: Vec<u8>, limits: OoxmlLimits, tight: OoxmlLimits) {
    if let Ok(import) = casual_calc_import::import_package_with(bytes.clone(), limits) {
        within_its_own_budget(&import.workbook, &limits);
    }
    // Over budget is the expected answer here for anything but a trivial
    // document, and it is a correct one — the assertion is about what happens
    // when it says yes.
    if let Ok(import) = casual_calc_import::import_package_with(bytes, tight) {
        within_its_own_budget(&import.workbook, &tight);
    }
}

const WORKSHEET_PART: &str = "xl/worksheets/sheet1.xml";
const STYLES_PART: &str = "xl/styles.xml";

/// **`SEC-002`'s regression proof, measured rather than predicted.**
///
/// Every per-part limit in this engine was already enforced and every one was
/// passing; nothing added them up, so a package of many parts multiplied a
/// bound nobody had agreed to. [`SpreadsheetLimits`] is the answer — one budget
/// per *document*, across every part that contributes — and this is what
/// watches it.
///
/// It asserts on the workbook that **came back**, never on what this target
/// guessed the importer would decide. Each of these is counted before the thing
/// is stored, so the model can legitimately hold fewer than the budget counted
/// (two cells at the same reference cost two of the budget and occupy one
/// slot); it can never hold more. `<=` is therefore the exact relation, and a
/// violation means the budget was not applied on some path into the model.
///
/// [`SpreadsheetLimits`]: casual_calc_ooxml::SpreadsheetLimits
fn within_its_own_budget(workbook: &Workbook, limits: &OoxmlLimits) {
    let cells: usize = workbook.sheets.iter().map(|s| s.cells.len()).sum();
    assert!(
        cells <= limits.spreadsheet.max_populated_cells,
        "the importer materialised {cells} cells, over the document budget of {}",
        limits.spreadsheet.max_populated_cells
    );

    let merges: usize = workbook.sheets.iter().map(|s| s.merges.len()).sum();
    assert!(
        merges <= limits.spreadsheet.max_merged_ranges,
        "the importer kept {merges} merged ranges, over the document budget of {}",
        limits.spreadsheet.max_merged_ranges
    );

    assert!(
        workbook.defined_names.len() <= limits.spreadsheet.max_defined_names,
        "the importer kept {} defined names, over the document budget of {}",
        workbook.defined_names.len(),
        limits.spreadsheet.max_defined_names
    );
}

/// Wrap arbitrary bytes as one part of an otherwise valid `.xlsx`.
///
/// Deliberately minimal and deliberately *valid*: the container, the content
/// types and the relationship graph are not what passes 2 and 3 are testing, so
/// none of them may be the reason a document is turned away. The skeleton is
/// `fixtures/generated/minimal.xlsx` written out by hand, so a fixture change
/// and a harness change stay independent.
///
/// Stored rather than deflated — the fuzzer's bytes go in uncompressed, so a
/// one-byte mutation is a one-byte change to the XML the reader sees.
fn as_package(part: &str, content: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(content.len() + 1024);
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut out));
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, body) in SKELETON {
            if *name == part {
                continue;
            }
            zip.start_file(*name, stored).ok()?;
            zip.write_all(body.as_bytes()).ok()?;
        }
        zip.start_file(part, stored).ok()?;
        zip.write_all(content).ok()?;
        zip.finish().ok()?;
    }
    Some(out)
}

/// The smallest package this importer admits: content types, the package
/// relationship to `xl/workbook.xml`, one sheet, and that sheet's part.
const SKELETON: &[(&str, &str)] = &[
    (
        "[Content_Types].xml",
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#,
    ),
    (
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
    ),
    (
        "xl/workbook.xml",
        r#"<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
    ),
    (
        "xl/_rels/workbook.xml.rels",
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
    ),
    (
        WORKSHEET_PART,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="n"><v>42</v></c></row></sheetData></worksheet>"#,
    ),
];
