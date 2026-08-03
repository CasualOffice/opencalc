# 15 — CI & Release Gates

CI is part of the architecture, not an afterthought. The job names below are a
**contract**: they are stable, and a PR is not mergeable until they pass. This
doc defines the gates for when the workspace exists (Phase 0 onward) and records
which gates are not yet built.

> **Current state:** no workspace, so no CI runs yet. During the documentation
> phase the only meaningful gate is a docs/link check. Everything below is the
> designed target, to be stood up in Phase 0 (tracker `F-###`).

## PR gates (target)

| Job | Command (intent) | Enforces |
| --- | --- | --- |
| `format` | `cargo fmt --all -- --check` | Formatting |
| `lint` | `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Lints as errors |
| `test` | `cargo test --workspace --all-features --locked` (+ doc tests) | Correctness, determinism, round-trip, recalc goldens |
| `docs` | `cargo doc --workspace --all-features --no-deps` with `RUSTDOCFLAGS=-D warnings` | Doc builds; intra-doc links valid |
| `wasm` | `cargo check --target wasm32-unknown-unknown` | The engine stays WASM-clean |
| `benchmark-smoke` | run `casual-calc-benchmark --smoke`, validate JSON shape with `jq` | Perf harness shape + determinism (**implemented**, F-007) |
| `fuzz-build` | build cargo-fuzz targets on pinned nightly; assert `fuzz/Cargo.lock` unchanged | Fuzz targets compile |
| `platform` | matrix: macOS-arm64 + Windows-x64 full tests; **MSRV** check | Cross-platform + minimum Rust |
| `dependency-policy` | `cargo deny check bans licenses sources` + `cargo audit --deny warnings` | Supply-chain policy |
| `repository-policy` | fixture manifest SHA-256 check; reject merge-conflict markers; validate benchmark/baseline JSON | Repo integrity |
| `browser-smoke` | `wasm-pack` build + Playwright unit/e2e on the grid editor | The editor loads and paints |

Actions are pinned to full commit SHAs; workflows run read-only where possible;
concurrency-cancel is on.

## Determinism & fidelity gates (spreadsheet-specific)

These are the gates that make OpenCalc trustworthy and are additions over
OpenDoc's set:

- **Round-trip fixed point** — `import(retention) → write → reopen` yields an
  identical model; an unedited `.xlsx` reconstructs byte-for-byte
  ([34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)).
- **Golden snapshots** — the normalized model serializes byte-stably; goldens are
  committed and diffed.
- **Golden display lists** — layout output for fixture sheets is golden-tested,
  including the virtualized-viewport path (which must equal the full-layout path).
- **Recalc oracle** *(Phase 2)* — computed cell values are diffed against a
  LibreOffice Calc / Excel oracle for the formula corpus.
- **Recalc latency budget** *(Phase 2)* — the benchmark harness asserts the
  worst-case incremental recalc stays under the [30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)
  budget on the named baseline environment.
- **Scroll/paint budget** — the grid-render benchmark asserts a visible-window
  repaint fits the 60 fps frame budget on the baseline environment.

## Scheduled / security

A weekly workflow re-runs `cargo audit` / `cargo deny` and a bounded seeded
fuzz campaign against the XLSX package reader.

## Future gates (not built yet)

- Everything above (no workspace exists).
- A CSV/ODS interop gate once those adapters land.
- A memory-ceiling gate asserting the 1M-cell model stays within the
  [30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md) memory budget.

When a needed gate does not exist, name it here and add a tracker row rather than
silently proceeding without it.
