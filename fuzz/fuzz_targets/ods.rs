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
use quick_xml::events::{BytesStart, Event};

/// How much of an imported workbook the round-trip below will look at.
///
/// The writer is quadratic in a sheet's extent (it re-scans the cell store once
/// per row), and the reader will happily build a very wide sheet out of a very
/// short document. Round-tripping one of those would spend the whole run inside
/// the writer and starve the reader, which is the surface under test — so the
/// property is checked on the small workbooks, where it is just as true.
const ROUND_TRIP_CELLS: usize = 1024;

/// The expansion this target will not hand to the reader, because the reader
/// does not survive it — **`ODS-03`, open, found by this target**.
///
/// `MAX_REPEAT` clamps each repeat attribute to 4096 and nothing clamps their
/// *product*. One `<table:table-row table:number-rows-repeated="4096">` holding
/// one `<table:table-cell office:value-type="float" office:value="1"
/// table:number-columns-repeated="4096"/>` is **574 bytes** and materialises
/// 16.7 M cells: measured at **2.0 GB resident and 7.4 s**. Ten such rows —
/// 2 KB — measured at **6.5 GB and 91 s**, which is past the `-rss_limit_mb`
/// CI runs with. On an upload path that is a denial of service with a friendly
/// file extension.
///
/// This is not a false positive being silenced; it is a **found defect being
/// held**. The fix belongs in `casual-calc-ods`, and without the hold this
/// target would spend every run re-finding the one input it already found
/// instead of searching for the next. The reproducer is committed as
/// `seeds/ods/amplifier.xml`, so the day the reader clamps the product, that
/// seed is refused in microseconds — delete [`declared_cells`] and this
/// constant then, and the seed becomes an ordinary regression input.
///
/// The ceiling is the engine's own documented capacity
/// (`docs/30-PERFORMANCE-AND-CAPACITY-TARGETS.md`): a million cells is a large
/// real spreadsheet, and no *bounded* reader should build more than that out of
/// a few hundred bytes.
const AMPLIFICATION_CEILING: u64 = 1 << 20;

/// The reader's own `MAX_REPEAT`, which it clamps each repeat attribute to.
const MAX_REPEAT: u64 = 4096;

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
    let declared = declared_cells(data);

    // **Two thresholds, because they answer different questions.** The first
    // version of this used one, and CI found the gap in four minutes: it
    // asserted refusal above `AMPLIFICATION_CEILING`, which is eight times
    // stricter than what the reader actually enforces — so a document
    // declaring two million cells was materialised, correctly, and the
    // assertion called that a defect. The fuzzer was right; the assertion was
    // wrong.
    //
    // Above the reader's own bound, refusal is the contract, so assert it.
    // Using the reader's exported constant rather than a copy, because a copy
    // is a number that drifts and this is the second time that has cost a run.
    if declared > casual_calc_ods::MAX_POPULATED_CELLS as u64 {
        if let Some(package) = as_package(data) {
            assert!(
                casual_calc_ods::import_ods(&package).is_err(),
                "a document declaring more than {} cells was materialised rather \
                 than refused: the repeat-product bound is gone",
                casual_calc_ods::MAX_POPULATED_CELLS
            );
        }
        return;
    }

    // Between the ceiling and the bound the reader legitimately materialises,
    // and doing so is merely slow. Nothing to prove, and every second here is a
    // second not spent finding the next defect.
    if declared > AMPLIFICATION_CEILING {
        return;
    }

    // Pass 1: the container boundary, and any real `.ods` in the corpus.
    if let Ok((workbook, _report)) = casual_calc_ods::import_ods(data) {
        round_trip(&workbook);
    }

    // Pass 2: the element walk, reached with the container out of the way.
    if let Some(package) = as_package(data) {
        if let Ok((workbook, _report)) = casual_calc_ods::import_ods(&package) {
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

/// How many cells a document *declares*, counting only what the reader stores.
///
/// This exists to hold `ODS-03` and must be conservative in one direction only:
/// it may over-count, but it must not mistake a real LibreOffice document for
/// an amplifier. LibreOffice ends nearly every row with
/// `table:number-columns-repeated="16384"` and every sheet with
/// `table:number-rows-repeated="1048575"` — both runs of *empty* cells, which
/// the reader moves the cursor over and stores nothing for.
///
/// **It walks with the reader's own parser and the reader's own matching
/// rules**, because a byte scan was not good enough: a mutation that put a `[`
/// in the middle of `office:value-type` slipped past a scan looking for that
/// string, while the reader — which matches an attribute by its *local* name —
/// honoured it and expanded 16.7 M cells anyway. A hold the fuzzer can walk
/// around is not a hold.
fn declared_cells(xml: &[u8]) -> u64 {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut total: u64 = 0;
    let mut row_repeat: u64 = 1;
    // The reader drops any cell that arrives before a `<table:table>`, so those
    // cost nothing however they are repeated.
    let mut in_table = false;

    loop {
        let event = match reader.read_event_into(&mut buf) {
            Ok(event) => event,
            // Malformed XML is the reader's problem, not this estimate's: it
            // will stop at the same place and store nothing beyond it.
            Err(_) => break,
        };
        match event {
            Event::Eof => break,
            Event::Start(ref e) | Event::Empty(ref e) => {
                let self_closing = matches!(event, Event::Empty(_));
                match local_name(e.name().as_ref()).as_slice() {
                    b"table" => in_table = true,
                    b"table-row" => row_repeat = repeat(e, b"number-rows-repeated"),
                    name if name.ends_with(b"table-cell") && in_table => {
                        // A cell stores something when it has a value type or a
                        // formula — and an open (non-self-closing) cell may yet
                        // hold text, which the reader stores too.
                        if !self_closing
                            || has(e, b"value-type")
                            || has(e, b"formula")
                            || has(e, b"boolean-value")
                            || has(e, b"date-value")
                        {
                            total = total.saturating_add(
                                row_repeat.saturating_mul(repeat(e, b"number-columns-repeated")),
                            );
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        // Counted up to the **reader's** bound, not to the ceiling. Stopping
        // at `AMPLIFICATION_CEILING` would cap this below
        // `MAX_POPULATED_CELLS`, making the refusal assertion above
        // unreachable — a guard that can never fire, which is worse than none
        // because it reads as covered.
        if total > casual_calc_ods::MAX_POPULATED_CELLS as u64 {
            break;
        }
        buf.clear();
    }
    total
}

/// One repeat attribute, clamped the way the reader clamps it.
fn repeat(e: &BytesStart<'_>, attribute: &[u8]) -> u64 {
    attribute_value(e, attribute)
        .and_then(|v| String::from_utf8_lossy(&v).parse::<u64>().ok())
        .unwrap_or(1)
        .clamp(1, MAX_REPEAT)
}

fn has(e: &BytesStart<'_>, attribute: &[u8]) -> bool {
    attribute_value(e, attribute).is_some()
}

/// An attribute by its **local** name — the reader's rule, deliberately.
fn attribute_value(e: &BytesStart<'_>, attribute: &[u8]) -> Option<Vec<u8>> {
    e.attributes()
        .flatten()
        .find_map(|a| (local_name(a.key.as_ref()) == attribute).then(|| a.value.to_vec()))
}

/// A qualified name without its namespace prefix.
fn local_name(raw: &[u8]) -> Vec<u8> {
    match raw.iter().rposition(|c| *c == b':') {
        Some(at) => raw[at + 1..].to_vec(),
        None => raw.to_vec(),
    }
}
