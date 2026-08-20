# Fuzzing

A **separate** cargo workspace so nightly-only `cargo-fuzz` / `libfuzzer-sys`
dependencies stay out of the product dependency graph. See
[`docs/21-PARSER-LIMITS.md`](../docs/21-PARSER-LIMITS.md) and
[`docs/29-PHASE-0-PLAN.md`](../docs/29-PHASE-0-PLAN.md).

## Targets

| Target | Property under test |
| --- | --- |
| `bounded_package` | `casual-calc-package` admission never panics on arbitrary bytes; rejects cleanly or admits within limits |
| `ods` | `casual-calc-ods` reads an OpenDocument spreadsheet or refuses it, and never panics, hangs or exhausts memory doing either; what it writes, it reads back |

More targets are added as crates gain parsing surface. The rule for a reader
target is that **refusing is passing**: a bound that turns a document away is
the bound working, and only a panic, a hang or an out-of-memory is a defect,
because those are what an uploader can aim at a server.

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

**A found defect that cannot be fixed in the same change is held, not
silenced.** `ods` carries one: `seeds/ods/amplifier.xml` is 574 bytes and makes
the reader materialise 16.7 M cells (measured at 2.0 GB and 7.4 s), because
`MAX_REPEAT` clamps each repeat attribute and nothing clamps their product —
`ODS-03`. The target refuses to hand the reader a document declaring more than
a million cells, so a whole run is not spent re-finding the one input it
already found; the estimate that does the refusing walks the document with the
*reader's own parser and matching rules*, because a byte scan of the same
attributes was walked around by a single mutated character in a namespace
prefix. When the row is fixed, delete the estimate and let the seed run.
