//! The transform's convergence law, over pairs a fuzzer chooses.
//!
//! TP1 is the property everything else rests on: given two operations made
//! concurrently, applying `a` then `b'` must reach the same document as applying
//! `b` then `a'`. If it fails, two people's spreadsheets quietly stop agreeing —
//! no error, no crash, and nothing to notice until somebody compares totals.
//!
//! There is already a test asserting this over every pair from a hand-written
//! menu, and that menu is the limit of it: it contains the cases somebody
//! thought of. This searches instead. The operations are built from the fuzzer's
//! bytes over the same op space, so the *coordinates* — which row, how many, in
//! what order, overlapping or adjacent or nested — are chosen by something with
//! no idea which of them anybody considered interesting.
//!
//! Boundaries are where transforms break: an insert exactly at another's start,
//! a delete that consumes the row an edit lands on, two deletes overlapping by
//! one. Those are precisely the values a coverage-guided fuzzer converges on,
//! and the ones a hand-written list is likeliest to be one short of.
//!
//! A pair the transform *refuses* is a fine answer and not a failure — the
//! design refuses concurrent sheet reordering rather than guessing. What must
//! never happen is answering both ways and disagreeing.

#![no_main]

use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};
use casual_calc_transaction::transform::{Side, transform};
use casual_calc_transaction::{Operation, apply};
use libfuzzer_sys::fuzz_target;

/// A document with enough rows and columns that a structural edit has room to
/// land somewhere interesting rather than always off the end.
fn seed() -> Workbook {
    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    for row in 0..12u32 {
        for col in 0..6u32 {
            sheet.cells.set(
                CellRef::new(row, col),
                Cell::value(CellValue::Number(f64::from(row * 10 + col))),
            );
        }
    }
    workbook.sheets.push(sheet);
    workbook
}

/// Everything about a document that two replicas must agree on.
fn observe(workbook: &Workbook) -> String {
    let sheet = &workbook.sheets[0];
    let mut cells: Vec<String> = sheet
        .cells
        .iter()
        .map(|(at, cell)| format!("{}:{}={:?}", at.row, at.col, cell.value))
        .collect();
    cells.sort();
    format!("{}|cols{:?}", cells.join(","), sheet.columns.sizes)
}

/// Pull one operation out of the fuzzer's bytes.
///
/// Deliberately a small, fixed menu with fuzzed *coordinates*, rather than an
/// arbitrary `Operation`: the op set is closed and known, and what is worth
/// searching is where the operations sit relative to each other, not whether an
/// unrepresentable one can be constructed.
fn op_from(bytes: &[u8]) -> Option<Operation> {
    let [kind, a, b, c, ..] = bytes else {
        return None;
    };
    // Small ranges on purpose. Two edits a thousand rows apart do not interact,
    // and a fuzzer spending its budget there is a fuzzer finding nothing; the
    // interesting pairs are the ones that overlap, touch or nest.
    let at = u32::from(*a) % 14;
    let count = (u32::from(*b) % 3) + 1;
    let value = f64::from(*c);
    Some(match kind % 5 {
        0 => Operation::SetCell {
            sheet: 0,
            at: CellRef::new(at, u32::from(*b) % 7),
            cell: Some(Cell::value(CellValue::Number(value))),
        },
        1 => Operation::InsertRows { sheet: 0, at, count },
        2 => Operation::DeleteRows { sheet: 0, at, count },
        3 => Operation::InsertColumns { sheet: 0, at: at % 7, count },
        _ => Operation::DeleteColumns { sheet: 0, at: at % 7, count },
    })
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }
    let (Some(a), Some(b)) = (op_from(&data[..4]), op_from(&data[4..])) else {
        return;
    };

    let base = seed();
    // Ops enter the protocol narrowed, so they are narrowed here too — an
    // operation still claiming to change everything reads as contending with
    // every concurrent edit, and the transform has no workbook to work that out
    // from.
    let (a, b) = (a.narrowed(&base), b.narrowed(&base));

    // One order is fixed first: two replicas must agree that `a` came before
    // `b` before they can agree on a result. Each side then transforms with the
    // role that order gives it.
    let (Ok(b_after_a), Ok(a_after_b)) = (
        transform(&b, &a, Side::Later),
        transform(&a, &b, Side::Earlier),
    ) else {
        // Refused. An honest answer: the design refuses rather than guesses.
        return;
    };

    let mut left = base.clone();
    if apply(&mut left, a.clone()).is_err() || apply(&mut left, b_after_a).is_err() {
        return;
    }
    let mut right = base;
    if apply(&mut right, b.clone()).is_err() || apply(&mut right, a_after_b).is_err() {
        return;
    }

    assert_eq!(
        observe(&left),
        observe(&right),
        "TP1 violated: two replicas applying the same pair in the two orders \
         disagree.\n  a = {a:?}\n  b = {b:?}"
    );
});
