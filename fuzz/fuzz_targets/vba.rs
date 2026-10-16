//! The VBA project decoder, against arbitrary bytes (`MACRO-05`).
//!
//! `decode_vba_project` reads the `vbaProject.bin` a macro-enabled workbook
//! carries — a Compound File holding a compressed `[MS-OVBA]` project — when a
//! person opens **View VBA**. The file came from whoever sent the workbook, so
//! every sector id, chain link, stream size, directory link, record length and
//! copy token in it is attacker-chosen.
//!
//! # Refusing is passing
//!
//! Every `VbaError` is a correct answer. Only a panic, a hang or an
//! out-of-memory is a defect.
//!
//! # The property
//!
//! Beyond returning: what the decoder **produces** stays inside the contract
//! (docs/99 §4) — no more than `MAX_MODULES` modules, and no more decompressed
//! source in total than `MAX_DECOMPRESSED`, however the bytes are arranged.

#![no_main]

use casual_calc_import::vba::{decode_vba_project, MAX_DECOMPRESSED, MAX_MODULES};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(project) = decode_vba_project(data) else {
        return;
    };
    assert!(project.modules.len() <= MAX_MODULES);
    let text: usize = project
        .modules
        .iter()
        .filter_map(|m| m.source.as_ref().ok())
        .map(String::len)
        .sum();
    // Decoded text can be longer than the bytes it came from (one byte becomes
    // up to three UTF-8 bytes), never more than that.
    assert!(text <= 3 * MAX_DECOMPRESSED);
});
