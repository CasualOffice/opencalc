# 18 — Support Matrix

This matrix separates **target** (what OpenCalc is designed to support) from
**implemented** (what actually passes its gates). A target is **not supported**
until its CI/oracle gate is green. Today, everything is a target — no code exists.

Legend: ○ target · ◑ in progress · ● implemented & gated.

## Platforms

| Platform | Rust triple | Tier | Status |
| --- | --- | --- | --- |
| macOS arm64 | `aarch64-apple-darwin` | 1 | ○ |
| Linux x64 | `x86_64-unknown-linux-gnu` | 1 | ○ |
| Windows x64 | `x86_64-pc-windows-msvc` | 1 | ○ |
| WebAssembly | `wasm32-unknown-unknown` | 1 | ○ |
| Linux arm64 | `aarch64-unknown-linux-gnu` | 2 | ○ |

Tier 1 = full test matrix in CI. Tier 2 = build + smoke.

## Host modes

| Mode | Description | Status |
| --- | --- | --- |
| **Tauri desktop app** | Native Rust engine via `casual-calc-sdk` from Tauri commands; calc runs native | ○ |
| **Web app (WASM)** | Browser grid editor via `casual-calc-wasm`; calc runs in WASM | ○ |
| Headless native | Read/model/calc/write, no rendering surface | ○ |
| Headless render | + display list → PNG via CPU raster | ◑ (P1D-001: grid raster; glyphs pending) |
| Embedded SDK | `casual-calc-sdk` in any Rust host app | ○ |

Both the Tauri and web hosts compose the same host-agnostic core; the calc engine
runs **native** on desktop and **WASM** in the browser, producing identical
results. See [02](02-ARCHITECTURE.md) §Host targets.

## Formats

| Format | Import | Export | Notes |
| --- | --- | --- | --- |
| `.xlsx` (SpreadsheetML) | ○ | ○ | Primary path; byte-identical + semantic writers |
| `.ods` (OpenDocument) | ○ | ○ | Secondary open-format path (`casual-calc-ods`); shares the OPC substrate |
| CSV / TSV / PSV | ○ | ○ | Delimited text (comma/tab/pipe); lossy tabular interchange; not packages |
| Normalized JSON | ○ | ○ | The engine's own snapshot format |

## SpreadsheetML feature capability (target)

| Feature | Model | Render | Calc | Status |
| --- | --- | --- | --- | --- |
| Cells: number / string / bool / error | ○ | ○ | n/a | Phase 1A/1C |
| Shared strings | ○ | ○ | n/a | Phase 1A |
| Inline strings | ○ | ○ | n/a | Phase 1A |
| Number formats (built-in + custom) | ◑ | ○ | n/a | Phase 1A (P1A-003, model); render 1C |
| Styles: fonts, fills, borders, alignment | ○ | ○ | n/a | Phase 1A (P1A-003b) / 1C |
| Merged ranges | ◑ | ○ | n/a | Phase 1A (P1A-004, model); render 1C |
| Frozen panes / defined names | ◑ | ○ | n/a | Phase 1A (P1A-004, model) |
| Column/row sizing, hidden, outline | ○ | ○ | n/a | Phase 1A/1C |
| Frozen panes / splits | ○ | ○ | n/a | Phase 1A/1C |
| Defined names | ○ | n/a | ○ | Phase 1A / used in Phase 2 |
| Formulas: parse & preserve (AST) | ● | n/a | n/a | **Phase 1A** (P1A-002; A1 subset) |
| Formulas: evaluate | n/a | n/a | ◑ | **Phase 2** (P2-001; full recalc, subset) |
| Dependency graph + incremental recalc | n/a | n/a | ○ | Phase 2 (P2-002) |
| Function library (math/text/lookup/…) | n/a | n/a | ◑ | Phase 2 (SUM/AVG/MIN/MAX/COUNT/IF/ABS/ROUND) |
| Spill / dynamic arrays | ○ | ○ | ○ | Phase 2 |
| Tables & structured references | ○ | ○ | ○ | Phase 3 |
| Conditional formatting | ○ | ○ | ○ | Phase 3 |
| Data validation | ○ | ○ | ○ | Phase 3 |
| Autofilter & sort | ○ | ○ | n/a | Phase 3 |
| Charts | preserve → ○ | ○ | n/a | Preserved 1A; rendered Phase 3 |
| Pivot tables/caches | preserve → ○ | ○ | ○ | Preserved 1A; Phase 3 |
| VBA / macros | preserve only | n/a | **never** | Opaque; never executed |
| Digital signatures, customXml | preserve only | n/a | n/a | Opaque side table |

## Capacity & performance targets

| Target | Value | Gate | Status |
| --- | --- | --- | --- |
| Populated cells | 1,000,000+ | memory-ceiling benchmark | ○ (Phase 1D) |
| Grid scroll | 60 fps visible-window repaint | scroll benchmark | ○ (Phase 1D) |
| Worst-case incremental recalc | < 50 ms | recalc-latency benchmark | ○ (Phase 2) |

See [30-PERFORMANCE-AND-CAPACITY-TARGETS](30-PERFORMANCE-AND-CAPACITY-TARGETS.md).

## Required release evidence

A feature moves from ○/◑ to ● only when it ships with:

- a design note and (if triggered) an Accepted ADR,
- import→model→(calc)→render→export tests,
- a fidelity-oracle diff where applicable,
- a fixture in the checksummed corpus,
- a green tracker row.
