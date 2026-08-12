//! An operation arriving from another replica.
//!
//! This is the most directly untrusted input in the collaboration path: bytes
//! from a peer, parsed before anything has authenticated their *contents*. The
//! token says who may edit the document; it says nothing about whether the JSON
//! below it is well formed, and a server that panics on a malformed operation is
//! one that any participant can stop.
//!
//! Two things are exercised. Parsing, which must reject rather than crash — and
//! **localising**, which is where the interned handles in an operation are
//! resolved against a workbook that has never seen them. That resolution is
//! exactly where a hostile message would aim: an id naming a table entry that
//! does not exist, or one that exists and means something else.
//!
//! Bugs of this shape have already happened here in the benign case. COL-22 was
//! a `StringId` crossing replicas and resolving to different text on each side,
//! and the interned-key wire format serialized perfectly while being unreadable.
//! Both were found by running the thing; this is the same idea pointed at input
//! nobody wrote on purpose.

#![no_main]

use casual_calc_model::{Id, Sheet, SheetId, Workbook};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = core::str::from_utf8(data) else {
        return;
    };
    let Ok(wire) = serde_json::from_str::<casual_calc_transaction::wire::WireOperation>(text) else {
        // Refusing malformed input is the correct outcome and the common one.
        return;
    };

    // A workbook that has never seen any of the sender's handles, which is the
    // situation `localise` exists for and the one a hostile message would use.
    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    workbook
        .sheets
        .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "S"));

    let op = wire.localise(&mut workbook);
    // Applying it may legitimately fail — an operation naming a sheet that does
    // not exist is refused, not a crash — but it must decide, rather than
    // panicking or running away.
    let _ = casual_calc_transaction::apply(&mut workbook, op);
});
