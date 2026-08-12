//! The number-format interpreter, against arbitrary format codes.
//!
//! A format code is **untrusted input**: it arrives inside a `.xlsx` written by
//! anything at all, and this interpreter is a small language with sections,
//! repeats, fractions, date tokens and colour names. Every one of those is a
//! place to index past the end of a string or to loop on a token that consumes
//! nothing.
//!
//! What is asserted is only that it **returns**. A format code that produces a
//! silly string is a fidelity question and belongs in the oracle tests; a format
//! code that panics or hangs is a document that cannot be opened, and one that
//! anybody can put in a file and send.
//!
//! The value is fuzzed alongside the code because the two interact: infinities,
//! NaN, subnormals and dates far outside the epoch all take different paths
//! through the same formatter.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The first eight bytes are the value, the rest the format code — so a
    // mutation can move the boundary and explore both sides.
    if data.len() < 8 {
        return;
    }
    let (head, tail) = data.split_at(8);
    let value = f64::from_le_bytes(head.try_into().expect("eight bytes"));
    let Ok(code) = core::str::from_utf8(tail) else {
        return;
    };
    // Bounded, because a fuzzer will otherwise spend its time on codes longer
    // than any real one and find only that long input is slow.
    if code.len() > 512 {
        return;
    }

    let _ = casual_calc_layout::format_number(value, code);
    let _ = casual_calc_layout::format_number_1904(value, code);
    let _ = casual_calc_layout::format_number_colored(value, code);
    // Text takes a different branch entirely: the fourth section of a format,
    // which is the one most often missing and least often exercised.
    let _ = casual_calc_layout::format_text(code, code);
});
