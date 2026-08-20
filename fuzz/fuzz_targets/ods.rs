//! The OpenDocument reader, against arbitrary bytes.
//!
//! `casual-calc-ods` reads files that arrive from somewhere else — a `.ods` on
//! an upload path is as untrusted as a `.xlsx`, and the reader is a hand-rolled
//! walk over ODF's element tree with two bounds on it (`MAX_CONTENT_BYTES`,
//! `MAX_REPEAT`). Nothing fuzzed it. Of the five defects found in it by hand,
//! **four were parser-shaped**: a repeat count applied to the wrong variable, a
//! self-closing element that skipped the column cursor, an unescaping step that
//! never ran, and a bound clamped in the wrong place.
//!
//! # Refusing is passing
//!
//! What is asserted is that the reader **returns** — with a workbook, or with
//! an `OdsError`. A refusal is the bounds doing their job and is not a finding:
//! `TooLarge` on a `content.xml` over the limit, `Malformed` on XML that does
//! not parse, and `NotAPackage` on bytes that are not a zip are all correct
//! answers. Only a panic, a hang or an out-of-memory is a defect, because those
//! are what an uploader can aim at a server.
//!
//! # Why the input is wrapped, and also not
//!
//! Two passes over the same bytes:
//!
//! 1. **As a whole package.** This is the real entry point and covers the
//!    container boundary — the zip that lies about its sizes, the archive with
//!    no `content.xml`. Arbitrary bytes are rejected here in microseconds,
//!    which costs nothing, and a corpus entry that *is* a `.ods` runs the whole
//!    path.
//! 2. **As the `content.xml` of an otherwise valid package.** Arbitrary bytes
//!    almost never form a valid ZIP, so a target that only did (1) would spend
//!    its entire budget being turned away by the container and never reach the
//!    element walk where the defects were — the same reasoning `ooxml_xml`
//!    records for pointing at the readers rather than at `import_package`.
//!    The walker is private, so the harness builds the package around the
//!    mutated document instead: every byte the fuzzer changes lands in the XML.
//!
//! Seeded from `fuzz/seeds/ods/`: a real LibreOffice `.ods` for pass 1, and the
//! documents that exercise repeats, escaping, self-closing cells, value types
//! and formula translation for pass 2.

#![no_main]

use std::io::{Cursor, Write};

use libfuzzer_sys::fuzz_target;

/// How much of an imported workbook the round-trip below will look at.
///
/// The writer is quadratic in a sheet's extent (it re-scans the cell store once
/// per row), and the reader will happily build a very wide sheet out of a very
/// short document. Round-tripping one of those would spend the whole run inside
/// the writer and starve the reader, which is the surface under test — so the
/// property is checked on the small workbooks, where it is just as true.
const ROUND_TRIP_CELLS: usize = 1024;

fuzz_target!(|data: &[u8]| {
    // **The hold became a proof.** This used to `return`, because the reader
    // built 16.7 million cells out of a few hundred bytes and the target would
    // otherwise have spent every run re-finding the one input it had already
    // found. `ODS-05` bounded the repeat *product*, so the same measurement is
    // now an assertion: an input declaring more cells than the engine's
    // documented capacity must be refused, not materialised.
    //
    // Kept rather than deleted because a bound with nothing watching it is a
    // bound that comes back off. `seeds/ods/amplifier.xml` is refused in ~1 ms
    // where it once took 16 seconds and 1.5 GB.
    // **No estimate of what the reader will do.** Two CI runs died here on
    // that idea in different clothes: first the threshold was eight times
    // stricter than the reader's, then the estimate disagreed with its
    // clamping. Predicting another component's decision is a copy of its logic,
    // and a copy drifts.
    //
    // A skip is not needed either. With the repeat-product bound in place an
    // amplifying document is refused in about a millisecond; without it, the
    // run OOMs or times out, and libFuzzer reports both as failures. So the
    // expensive case is exactly the case that should fail, and `ODS-05`'s
    // proof is `within_its_own_bound` below, measured on what came back.

    // Pass 1: the container boundary, and any real `.ods` in the corpus.
    if let Ok((workbook, _report)) = casual_calc_ods::import_ods(data) {
        within_its_own_bound(&workbook);
        round_trip(&workbook);
    }

    // Pass 2: the element walk, reached with the container out of the way.
    if let Some(package) = as_package(data) {
        if let Ok((workbook, _report)) = casual_calc_ods::import_ods(&package) {
            within_its_own_bound(&workbook);
            round_trip(&workbook);
        }
    }
});

/// Wrap arbitrary bytes as the `content.xml` of an otherwise valid `.ods`.
///
/// Deliberately minimal and deliberately *valid*: the container is not what
/// this pass is testing, so it must never be the reason a document is turned
/// away.
fn as_package(content: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(content.len() + 512);
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut out));
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("mimetype", stored).ok()?;
        zip.write_all(b"application/vnd.oasis.opendocument.spreadsheet")
            .ok()?;
        zip.start_file("content.xml", stored).ok()?;
        zip.write_all(content).ok()?;
        zip.finish().ok()?;
    }
    Some(out)
}

/// What the writer produces, its own reader must admit.
///
/// A converter that writes a document it cannot read back is the failure this
/// crate exists to avoid — the user saves, the file looks fine, and the next
/// open is empty. The assertion is deliberately weak on *content*: the model
/// carries less than ODF does, so cells legitimately change on the way through.
/// It is strong on **admission**, which nothing about a lossy conversion
/// excuses.
fn round_trip(workbook: &casual_calc_model::Workbook) {
    let cells: usize = workbook.sheets.iter().map(|s| s.cells.len()).sum();
    if cells > ROUND_TRIP_CELLS {
        return;
    }
    // Writing is allowed to fail — the zip writer owns that error — so a
    // failure there is not the property being asserted. Being unable to read
    // back what was written is.
    if let Ok(bytes) = casual_calc_ods::export_ods(workbook) {
        assert!(
            casual_calc_ods::import_ods(&bytes).is_ok(),
            "the writer produced a package its own reader refuses ({cells} cells)"
        );
    }
}

/// **`ODS-05`'s regression proof, measured rather than predicted.**
///
/// A repeat attribute lets a few hundred bytes declare millions of cells: the
/// reader once built 16.7 million from 329 bytes, at 1.5 GB and 16 seconds, on
/// a path reachable from a WOPI host's upload. It bounds the repeat *product*
/// now, and this is what watches that bound.
///
/// It asserts on the workbook that **came back**, not on what this target
/// guessed the reader would do. The guess was a copy of the reader's clamping
/// rules, and a copy drifts — it drifted twice, and both times CI caught this
/// target rather than the engine.
fn within_its_own_bound(workbook: &casual_calc_model::Workbook) {
    let populated: usize = workbook.sheets.iter().map(|s| s.cells.iter().count()).sum();
    assert!(
        populated <= casual_calc_ods::MAX_POPULATED_CELLS,
        "the reader materialised {populated} cells, over its own {} limit: the \
         repeat-product bound is gone",
        casual_calc_ods::MAX_POPULATED_CELLS
    );
}
