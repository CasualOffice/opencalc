# Fuzzing

A **separate** cargo workspace so nightly-only `cargo-fuzz` / `libfuzzer-sys`
dependencies stay out of the product dependency graph. See
[`docs/21-PARSER-LIMITS.md`](../docs/21-PARSER-LIMITS.md) and
[`docs/29-PHASE-0-PLAN.md`](../docs/29-PHASE-0-PLAN.md).

## Targets

| Target | Property under test |
| --- | --- |
| `bounded_package` | `casual-calc-package` admission never panics on arbitrary bytes; rejects cleanly or admits within limits |

More targets (SpreadsheetML XML, formula parser) are added as those crates gain
parsing surface.

## Running

Requires the pinned nightly toolchain and `cargo-fuzz`:

```sh
cargo install cargo-fuzz --locked
cargo +nightly-2026-07-20 fuzz build          # CI builds all targets
cargo +nightly-2026-07-20 fuzz run bounded_package -- -max_total_time=60
```

## Crash policy

A reproducer is minimized, given a provenance note and checksum, added to the
fixture corpus, and covered by a regression test — the raw artifact is not
committed. `fuzz/Cargo.lock` is committed and CI asserts it is unchanged so the
fuzz dependency set can't drift silently.
