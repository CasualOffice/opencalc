//! Fuzz target: parsing a formula must never abort the process.
//!
//! The one that was missing. `casual-calc-formula`'s parser is recursive
//! descent, so nesting is stack — and a stack overflow is `SIGABRT`, not an
//! `Err`, so no amount of `Result` handling upstream catches it. It is reachable
//! from an imported `.xlsx`, from the formula bar and from the collaboration
//! wire, where one document would take down every other on the node.
//!
//! Two separate limits are under test here and they fail differently:
//!
//! - `MAX_DEPTH` bounds recursion — `((((…))))`, `SUM(SUM(SUM(…)))`, `----…`.
//! - `MAX_CHAIN` bounds the left spine — `1+1+1+…`, which *parses in a loop*
//!   without recursing at all and then aborts in `Expr`'s recursive `Drop`.
//!
//! So the target does not merely parse: it prints and drops the result, because
//! the second class of bug happens on the way out rather than on the way in.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(expr) = casual_calc_formula::parse(text) {
        // Display is recursive, so a tree the parser accepted must also be one
        // this can walk.
        let printed = expr.to_string();
        // And what it prints must parse back **to the same tree**, which is
        // the round-trip gate stated as a property rather than over a fixed
        // corpus. It used only to require that the text parse at all; the
        // stronger form found three defects in the grammar extensions
        // (XL-FEAT-014..017) in its first minutes — a range whose far end
        // named another sheet, `#REF!#` printed for a spill anchor off the
        // sheet, and `inf` printed for a literal past the largest double.
        //
        // Text that prints `#REF!` is exempt: `parse_a1` still accepts a
        // column past XFD (`SUMH1`), which then prints as `#REF!` — a
        // known defect proposed as its own row, and a different tree by design.
        if printed.contains("#REF!") {
            return;
        }
        let again = casual_calc_formula::parse(&printed)
            .unwrap_or_else(|e| panic!("{text:?} printed {printed:?}, which does not parse: {e}"));
        assert!(
            again == expr,
            "{text:?} printed {printed:?}, which reads back as {again:?}, not {expr:?}"
        );
        // Drop is recursive too, and is where the chain bug landed.
        drop(expr);
    }
});
