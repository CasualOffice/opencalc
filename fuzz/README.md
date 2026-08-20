# Fuzzing

A **separate** cargo workspace so nightly-only `cargo-fuzz` / `libfuzzer-sys`
dependencies stay out of the product dependency graph. See
[`docs/21-PARSER-LIMITS.md`](../docs/21-PARSER-LIMITS.md) and
[`docs/29-PHASE-0-PLAN.md`](../docs/29-PHASE-0-PLAN.md).

## Targets

One per parser that reads bytes it did not write. The list is the answer to
"what can a user, a host or a peer hand this engine", and every entry on that
list has a row here.

| Target | Reads | Property under test |
| --- | --- | --- |
| `bounded_package` | a ZIP/OPC container | admission never panics on arbitrary bytes; rejects cleanly or admits within limits |
| `ooxml_xml` | `.rels` and `workbook.xml` | the two OPC helpers return rather than hang or panic |
| `xlsx` | a whole SpreadsheetML package | the importer returns, and an admitted document is inside the budget it was given (`SEC-002`) |
| `ods` | an OpenDocument spreadsheet | reads or refuses, never panics, hangs or exhausts memory; what it writes, it reads back; a document is inside `MAX_POPULATED_CELLS` (`ODS-05`) |
| `delimited` | CSV / TSV / PSV | the reader returns, and a parse → write → parse round trip settles, keeps every cell, its kind and its number format |
| `snapshot` | the model as JSON | an admitted snapshot re-serializes to the same structure and the same bytes |
| `formula_parse` | a formula | the parser returns |
| `number_format` | a number-format code | the formatter returns |
| `wire_operation` | an operation from a peer | parsing and localising decide rather than panic |
| `transform_tp1` | two concurrent operations | TP1 holds |
| `token_verify` | a JWT | it returns, and it never accepts a token this key did not sign |

The rule for a reader target is that **refusing is passing**: a bound that turns
a document away is the bound working, and only a panic, a hang or an
out-of-memory is a defect, because those are what an uploader can aim at a
server.

The second rule is that an assertion must be **measured on what came back**,
never on a prediction of what the parser will decide. `ods.rs` records paying
for that lesson twice in one day, in two different disguises; `xlsx.rs` records
the other half of it, which is that an assertion whose threshold no input can
reach is a branch reporting on its own existence.

## Running

Requires the pinned nightly toolchain and `cargo-fuzz`:

```sh
cargo install cargo-fuzz --locked
cargo +nightly-2026-07-20 fuzz build          # CI builds all targets
cargo +nightly-2026-07-20 fuzz run bounded_package -- -max_total_time=60
```

Pass the seeds alongside the corpus, and **name the corpus directory first**:
libFuzzer writes what it finds into the first directory it is given, so a
tracked one there fills the repository with fuzzer artefacts.

```sh
cargo +nightly-2026-07-20 fuzz run ods corpus/ods seeds/ods

# `xlsx` reads the fixture corpus in place — five producers, already
# rights-reviewed and checksummed, so there is no second copy to drift.
cargo +nightly-2026-07-20 fuzz run xlsx \
  corpus/xlsx seeds/xlsx seeds/ooxml_xml ../fixtures/corpus ../fixtures/generated \
  -- -max_len=65536
```

## Crash policy

A reproducer is minimized, given a provenance note and checksum, added to the
fixture corpus, and covered by a regression test — the raw artifact is not
committed. `fuzz/Cargo.lock` is committed and CI asserts it is unchanged so the
fuzz dependency set can't drift silently.

**A found defect that cannot be fixed in the same change is held, not
silenced.** A hold is written so that *deleting it* is the regression proof: the
reproducer stays in `seeds/`, the assertion stays in the target, and one
predicate stands between them until the row is fixed. A hold that is a `return`
at the top of the target, or an assertion quietly weakened, is a defect that has
been forgotten rather than deferred.

`ods` shows the end state. `seeds/ods/amplifier.xml` was 574 bytes that made the
reader materialise 16.7 M cells; `ODS-05` bounded the repeat *product*, the hold
came out, and the same measurement is now `within_its_own_bound` — the seed runs
and is refused in about a millisecond.

Two holds are open, both found by the targets added for `PROD-08`:

- **`delimited`** — `casual_calc_io::write_delimited` writes any number with
  `|n| < 5e-16` as `0`. Not rounded, erased: `1e-16` in a `.csv` is `0` after
  one save. The negative side is worse — `-1e-300` writes as `-0`, which the
  *next* save writes as `0`, so the file is not a fixed point after one round
  trip and the documented "parse → write → parse settles" claim fails in its
  strongest form. Reproducer `seeds/delimited/underflow.csv`; the hold is
  `held_underflow`, one predicate, one `continue`.
- **`snapshot`** — `casual-calc-model` uses `serde_json` without the
  `float_roundtrip` feature, so its float reader is a fast approximation while
  its writer (`ryu`) is exact. `to_snapshot` then `from_snapshot` moves
  **13.29 %** of values at ordinary spreadsheet magnitudes by one unit in the
  last place. Reproducer `seeds/snapshot/float-drift.json`; the hold is
  `numbers_drifted`, and the structural half of the assertion still runs on
  every input.
