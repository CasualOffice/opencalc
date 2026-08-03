//! Fuzz target: bounded OPC/ZIP package admission must never panic on arbitrary
//! input — it either rejects cleanly or admits within limits, and part reads
//! stay bounded. See `docs/21-PARSER-LIMITS.md`.
#![no_main]

use casual_calc_package::{Package, PackageLimits};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(mut package) = Package::open(data.to_vec(), PackageLimits::default()) {
        for name in package.entry_names() {
            let _ = package.read_part(&name);
        }
    }
});
