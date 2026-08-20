//! The workbook snapshot reader, against arbitrary bytes.
//!
//! A snapshot is the model as JSON, and `Workbook::from_snapshot` is the third
//! admission path into this engine alongside a package and an operation. Its
//! own documentation says what it is: *"untrusted in exactly the way an
//! uploaded package is — it arrives from a host, a resumed session, or a
//! cluster peer"*, and it was the one admission path with no ceiling at all
//! until `SEC-013`. The browser reaches it from `casual_calc_wasm`, loading
//! bytes the collaboration server captured; `casual_calc_transaction::session`
//! reaches it on resume.
//!
//! Nothing fuzzed it.
//!
//! # Refusing is passing
//!
//! `SnapshotTooLarge` over either ceiling, `Snapshot` on bytes that are not a
//! snapshot, and every `Invariant` `validate` raises are correct answers. Only
//! a panic, a hang or an out-of-memory is a defect.
//!
//! # The property
//!
//! Admission already validates, so re-asserting the invariants `from_snapshot`
//! just checked would be a test that cannot fail. What is asserted instead is
//! the thing nothing checks: **what the model writes, the model reads back
//! unchanged.**
//!
//! Both halves of the comparison come out of `to_snapshot`, so this is not a
//! claim about the fuzzer's bytes — it is the round trip the collaboration
//! design rests on. A snapshot is how a document reaches a new node, how a
//! resuming client catches up, and how a leader hands a document back to its
//! host. This repository has already paid for the class once: an interned-key
//! wire format that *serialized perfectly and could not be read back*. That
//! defect was invisible to reading and visible the moment something ran the
//! round trip.
//!
//! Three things are checked on what came back, in increasing strength:
//!
//! 1. **Structure.** The same sheets, in the same order, with the same
//!    identities and the same populated cell references, and the same string
//!    table. None of this can be masked by a numeric difference.
//! 2. **Values.** No cell's number changed. This one is [held](numbers_drifted).
//! 3. **Bytes.** `to_snapshot` twice over gives the same bytes, which is what
//!    "deterministic, byte-stable" means.
//!
//! The second load lifts the ceilings deliberately. Whether a workbook is
//! *small enough* was decided by the first load, under the real limits; whether
//! it survives a round trip is a different question, and mixing them in would
//! turn a serialization defect into a size refusal and hide it.
//!
//! Seeded from `fuzz/seeds/snapshot/`: snapshots of real workbooks from four
//! producers, an empty workbook, and the held reproducer below.

#![no_main]

use casual_calc_model::{CellValue, SnapshotLimits, Workbook};
use libfuzzer_sys::fuzz_target;

/// No ceiling, for the second load only. See the module note.
const UNBOUNDED: SnapshotLimits = SnapshotLimits {
    max_bytes: u64::MAX,
    max_populated_cells: usize::MAX,
};

fuzz_target!(|data: &[u8]| {
    let Ok(workbook) = Workbook::from_snapshot(data) else {
        // Refusing malformed or oversized input is the correct outcome and the
        // common one.
        return;
    };

    // A workbook that was admitted must be storable. A model that cannot be
    // written is a document that cannot be handed to another node, cannot be
    // sent back to the host, and cannot be saved.
    let once = workbook
        .to_snapshot()
        .expect("a workbook that from_snapshot admitted could not be serialized");

    let reloaded = Workbook::from_snapshot_with(&once, UNBOUNDED)
        .expect("to_snapshot produced bytes from_snapshot refuses");

    let twice = reloaded
        .to_snapshot()
        .expect("a workbook that from_snapshot admitted could not be serialized");

    assert_eq!(
        shape(&workbook),
        shape(&reloaded),
        "the workbook's structure changed across its own snapshot round trip"
    );

    // **The hold became the proof.** This used to `return` here: serde_json's
    // reader was a fast approximation and changed roughly one number in ten
    // across a round trip, so the byte-stability assertion below could not be
    // reached honestly. `MODEL-01` turned on `float_roundtrip`, so the same
    // measurement is now an assertion — and `seeds/snapshot/float-drift.json`,
    // which was the reproducer, is an ordinary regression input.
    //
    // Kept rather than deleted, because a fix with nothing watching it is a fix
    // that comes back out: the feature is one word in a manifest, and a
    // dependency edit could drop it without any other test noticing.
    assert!(
        !numbers_drifted(&workbook, &reloaded),
        "a number changed across a snapshot round trip: serde_json is reading \
         floats approximately again, which means the `float_roundtrip` feature \
         has been lost from casual-calc-model (MODEL-01)"
    );

    // `assert!` rather than `assert_eq!`: a snapshot is up to half a gigabyte
    // and printing two of them is not a diagnosis. The reproducer is the input.
    assert!(
        once == twice,
        "to_snapshot is documented as byte-stable, and a load → store round \
         trip changed the bytes ({} then {})",
        once.len(),
        twice.len()
    );
});

/// **A defect this target found, now fixed and watched.**
///
/// `casual-calc-model` depends on `serde_json` with default features, and
/// `serde_json`'s float **parser** is a fast approximation unless the
/// `float_roundtrip` feature is on. Its float **writer** is `ryu`, which is
/// exact and shortest. The two are therefore not inverses: `to_snapshot` writes
/// the shortest representation of an `f64`, and `from_snapshot` reads it back
/// as a value that can be one unit in the last place away.
///
/// Measured (`serde_json::from_str(&serde_json::to_string(n))`, on this
/// lockfile):
///
/// * **13.29 %** of 1 000 000 values at ordinary spreadsheet magnitudes —
///   a mantissa in `[1, 2)` scaled by `10^-6 … 10^9` and divided by a small
///   integer, which is what a computed cell holds — did **not** come back
///   equal. The first one found was `4.2037590490107677e-4`, written
///   `0.00042037590490107677`, read back one ULP high.
/// * **29.7 %** of 2 000 000 uniformly random finite bit patterns.
/// * `f64::from_str` on the *same* string is correct in every one of those
///   cases, so the writer and the standard library agree and only
///   `serde_json`'s reader disagrees.
///
/// Reproducer: `seeds/snapshot/float-drift.json`, 449 bytes, one cell.
/// `2.0333333333333333e+128` (`0x5a92c612b9130c9d`) comes back as
/// `0x5a92c612b9130c9c`. The seed states it in full decimal so the *first*
/// parse takes `serde_json`'s slow path and is correct; that is what puts the
/// exactly-representable value into the model, and what the round trip then
/// loses.
///
/// What it means: the first time a workbook crosses a snapshot boundary — a
/// resume, a hand-off to another cluster node, a save back to the host — a
/// double-digit percentage of its numbers change in the last bit. It converges
/// after that one step, so it is not a drift that compounds; it is a document
/// whose numbers are not the numbers that were imported, on the two priorities
/// this project puts first (never produce wrong cell values, and identical
/// input ⇒ identical model).
///
/// Held rather than silenced, and held on the *observation* rather than on a
/// prediction: this compares the numbers in the two models that came back. The
/// structural assertion above still runs on every input. **When the row is
/// fixed — `serde_json = { version = "1", features = ["float_roundtrip"] }` in
/// `casual-calc-model` — delete this function and the `return` that calls
/// it**, and `seeds/snapshot/float-drift.json` is the input that proves it.
fn numbers_drifted(before: &Workbook, after: &Workbook) -> bool {
    before
        .sheets
        .iter()
        .zip(after.sheets.iter())
        .flat_map(|(a, b)| a.cells.iter().zip(b.cells.iter()))
        .any(|((_, a), (_, b))| match (&a.value, &b.value) {
            (CellValue::Number(x), CellValue::Number(y)) => x.to_bits() != y.to_bits(),
            _ => false,
        })
}

/// Everything about a workbook that is not a number: its sheets, their
/// identities, which cells are populated, and the interned strings.
///
/// Compared as a whole so a snapshot that dropped a sheet, renamed one,
/// reordered the cell store or lost a string fails loudly — none of which the
/// held numeric defect can disguise.
fn shape(workbook: &Workbook) -> Vec<String> {
    let mut out = vec![format!(
        "schema={} id={:?} strings={:?} styles={} names={}",
        workbook.schema_version,
        workbook.workbook_id,
        workbook.strings.iter().collect::<Vec<_>>(),
        workbook.styles.len(),
        workbook.defined_names.len(),
    )];
    for sheet in &workbook.sheets {
        out.push(format!(
            "sheet {:?} {:?} merges={} cells={:?}",
            sheet.id,
            sheet.name,
            sheet.merges.len(),
            sheet
                .cells
                .iter()
                .map(|(at, cell)| (at.row, at.col, kind(&cell.value)))
                .collect::<Vec<_>>(),
        ));
    }
    out
}

/// Which variant a value is, with the payload dropped.
fn kind(value: &CellValue) -> &'static str {
    match value {
        CellValue::Empty => "empty",
        CellValue::Number(_) => "number",
        CellValue::Bool(_) => "bool",
        CellValue::SharedString(_) => "shared",
        CellValue::InlineString(_) => "inline",
        CellValue::Error(_) => "error",
    }
}
