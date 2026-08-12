//! The XML readers, against arbitrary bytes.
//!
//! These parse parts of a `.xlsx` — a file format whose whole point is that it
//! arrives from somewhere else. Every one of these readers is walking untrusted
//! XML with a bounded walker, and the bounds are the interesting part: a
//! reader that trusts a depth, an attribute count or a name length is a reader
//! that a small file can make allocate for a long time.
//!
//! What is asserted is that it **returns** — with a parse, or with an error.
//! Which of the two is a fidelity question; hanging or panicking is a document
//! that cannot be opened, and one anybody can construct.
//!
//! Seeded from `fuzz/seeds/ooxml_xml/`, which is every XML part of the smallest
//! real fixture. Random bytes are a poor start for a structured format — a
//! fuzzer given nothing spends its budget being rejected at the first byte and
//! never reaches the code that reads an attribute or descends an element.
//!
//! Deliberately pointed at the readers rather than at `import_package`, because
//! arbitrary bytes almost never form a valid ZIP: a whole-package target spends
//! its budget being rejected by the container and never reaches the XML. The
//! package layer has its own target, with a corpus, for that reason.

#![no_main]

use casual_calc_ooxml::OoxmlLimits;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = OoxmlLimits::default();
    // Both readers see the same bytes: a `.rels` part and a workbook part are
    // different shapes, and a mutation that is nonsense to one is often nearly
    // valid for the other.
    let _ = casual_calc_ooxml::parse_relationships(data, &limits);
    let _ = casual_calc_ooxml::parse_sheet_refs(data, &limits);
});
