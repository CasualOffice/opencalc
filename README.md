# OpenCalc

[![CI](https://github.com/CasualOffice/opencalc/actions/workflows/ci.yml/badge.svg)](https://github.com/CasualOffice/opencalc/actions/workflows/ci.yml)
[![Status: Pre-release](https://img.shields.io/badge/status-pre--release-orange.svg)](docs/06-ROADMAP-AND-DELIVERY.md)
[![Rust: TBD](https://img.shields.io/badge/rust-MSRV%20TBD-black.svg?logo=rust)](docs/19-WORKSPACE-SCAFFOLD-DESIGN.md)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**A deterministic, embeddable spreadsheet engine written in Rust** — it reads and
writes `.xlsx`, holds the workbook in a normalized editable model, calculates it,
and lays out and renders a live cell grid to pixels, for native, WebAssembly, and
headless hosts that need real SpreadsheetML fidelity without a browser, a server,
or a UI framework.

One host-agnostic core, **two first-class hosts**: a **Tauri desktop app** where
the engine (including the formula/calc engine) runs as **native Rust** at full
speed, and a **web app** where the same engine compiles to WebAssembly — scroll a
million-cell sheet at 60 fps, edit cells, and recalculate, all client-side.

OpenCalc is the spreadsheet counterpart to [OpenDoc](../opendoc-fixes) (the
`.docx` engine): it reuses OpenDoc's format-neutral spine — bounded OPC package
admission, the loss-aware preservation model, deterministic snapshots, the
display-list/render seam, and the whole CI/fuzz/benchmark gate scaffold — and
replaces the document-specific layers with a **workbook model**, a **formula &
recalculation engine**, and a **virtualized grid layout**.

Developed by [CasualOffice](https://github.com/CasualOffice) as the spreadsheet
engine for Casual Sheets and an SDK others can embed.

> **Status: Phase 0 — Foundation (just started).** The documentation phase is
> closed ([exit report](docs/31-PHASE-D-EXIT-REPORT.md)); the design record — the
> architecture, phased roadmap, format/schema contracts, and process — is
> complete and the Cargo workspace skeleton now builds. **No engine logic has
> been written yet** beyond crate stubs. See
> [docs/29-PHASE-0-PLAN.md](docs/29-PHASE-0-PLAN.md) for the Phase 0 work items,
> [docs/06-ROADMAP-AND-DELIVERY.md](docs/06-ROADMAP-AND-DELIVERY.md) for the plan,
> and [docs/14-EXECUTION-TRACKER.md](docs/14-EXECUTION-TRACKER.md) for live state.

## Why OpenCalc

Most ways to work with `.xlsx` force a trade-off: a full office suite you can't
embed, a converter that silently drops anything it doesn't understand, or a web
grid that treats the DOM as the source of truth and recomputes formulas on a
server. OpenCalc is built the other way around:

- **Loss-aware by design.** Content the workbook model doesn't yet represent —
  a chart, a pivot cache, a rare style bit — is preserved and reproduced
  verbatim, or reported — never silently discarded.
- **Deterministic.** The same input, the same engine version, and the same
  recalculation produce the same model, the same cell values, the same layout,
  and the same bytes, every time — so both calculation and rendering can be
  regression-tested against golden files.
- **Embeddable and host-agnostic.** No mandatory DOM, server, React, or
  collaboration provider. The core targets Rust hosts,
  `wasm32-unknown-unknown`, desktop, and headless services alike.
- **Safe with untrusted files.** Packages are parsed under explicit entry, path,
  size, expansion, and resource limits (the same bounded OPC substrate OpenDoc
  uses). No macro execution, no automatic external fetches.
- **Built for scale.** The engine is architected from day one around hard
  targets: **1,000,000+ populated cells**, **60 fps** grid scrolling, and
  **sub-50 ms worst-case incremental recalculation**. See
  [docs/30-PERFORMANCE-AND-CAPACITY-TARGETS.md](docs/30-PERFORMANCE-AND-CAPACITY-TARGETS.md).

## Scope, in a planned order

OpenCalc is ambitious in full but delivered in capability-gated phases. The
formula/calculation engine — the single largest and riskiest surface — is
deliberately **held back to a later phase**: the workbook must be read, modeled,
preserved, written back byte-faithfully, laid out, and rendered *before* any cell
is evaluated. Formulas are imported and preserved from Phase 1A, but they are not
*calculated* until the dedicated calc-engine phase.

| Phase | Delivers | Formula status |
| --- | --- | --- |
| 0 — Foundation | workspace, CI, fixture corpus, bounded XLSX reader, minimal model | — |
| 1A — Import & model | workbook → sheets → cells, shared strings, styles, number formats, merged ranges, defined names; preservation ledger + compatibility report | parsed & preserved, **not evaluated** |
| 1B — Semantic writer | model → valid editable `.xlsx`; round-trip fixed point; opens in LibreOffice Calc | preserved verbatim on write |
| 1C — Grid layout | column/row geometry, merged cells, frozen panes, in-cell rich text, number-format display, display list | — |
| 1D — Grid render & virtualization | CPU raster (tiny-skia + skrifa), viewport virtualization for 1M-cell sheets, hit-testing | — |
| 1E — Browser grid editor (WASM) | cell edit, selection, fill, undo/redo | still static values |
| **2 — Formula & calc engine** | tokenizer/parser, dependency graph, incremental recalc, function library, spill/array semantics | **evaluated** |
| 3 — Spreadsheet features | conditional formatting, data validation, tables/structured refs, autofilter, sort, charts, pivots | — |
| 4 — SDK beta / embedding | stable host surfaces, native + WASM packaging | — |
| 5 — Collaboration / web | operation model for shared editing | — |
| 6 — 1.0 | stable SDK, support guarantees | — |

Full detail: [docs/06-ROADMAP-AND-DELIVERY.md](docs/06-ROADMAP-AND-DELIVERY.md).

## Planned workspace

OpenCalc will be a Cargo workspace of small, layered crates. Each layer depends
only on those below it. (None of these exist yet — this is the target scaffold
designed in [docs/19-WORKSPACE-SCAFFOLD-DESIGN.md](docs/19-WORKSPACE-SCAFFOLD-DESIGN.md).)

| Crate | Responsibility |
| --- | --- |
| `casual-calc-sdk` | Host-facing engine and workbook-session facade |
| `casual-calc-model` | Normalized workbook: sheets, sparse cell grid, shared strings, styles, number formats, defined names, invariants, snapshot I/O |
| `casual-calc-formula` | Formula tokenizer, parser, AST, A1/R1C1 reference algebra, pretty-printer |
| `casual-calc-eval` | Dependency graph, incremental recalculation, cycle detection, and the built-in function library *(Phase 2)* |
| `casual-calc-transaction` | Atomic operations, inverses, and reference/position mapping (insert/delete rows & columns, edit cells) |
| `casual-calc-selection` | Active-cell and range selection validation and mapping |
| `casual-calc-package` | Format-neutral, security-bounded ZIP/OPC admission and part reads |
| `casual-calc-ooxml` | Security-bounded SpreadsheetML (OPC) package inspection |
| `casual-calc-ods` | Security-bounded OpenDocument Spreadsheet admission and semantic import |
| `casual-calc-import` | SpreadsheetML semantic import into the normalized model + preservation ledger |
| `casual-calc-export` | XLSX writers: byte-identical reconstruction and the semantic model → SpreadsheetML writer |
| `casual-calc-io` | Format-neutral identities, detection/dispatch, and built-in adapters (XLSX, ODS, CSV/TSV/PSV, normalized JSON) |
| `casual-calc-layout` | Grid geometry, column/row sizing, frozen panes, merged cells, in-cell text shaping (`parley`), viewport virtualization, and the backend-neutral display list |
| `casual-calc-render` | CPU render backend: executes the display list on a `tiny-skia` pixmap, rasterizing glyphs from `skrifa` outlines |
| `casual-calc-wasm` | `wasm-bindgen` bridge that drives the browser grid editor |
| `casual-calc-tauri` | *(optional glue)* Tauri command wrappers for the desktop app; the desktop host otherwise consumes `casual-calc-sdk` directly with the calc engine running **native** |

## Prior art we study

Openly, and on the record (see
[docs/12-COMPETITIVE-ANALYSIS.md](docs/12-COMPETITIVE-ANALYSIS.md)):

- **Calculation & format semantics** — Microsoft Excel (the semantics oracle),
  LibreOffice Calc (our open fidelity oracle, as LibreOffice is OpenDoc's),
  OnlyOffice, and the OOXML/ECMA-376 spec.
- **Open Rust engines** — [IronCalc](https://www.ironcalc.com/),
  [Formualizer](https://github.com/psu3d0/formualizer),
  [calamine](https://github.com/tafia/calamine) (reader),
  `umya-spreadsheet`, and `rust_xlsxwriter`.
- **Grid UI/UX** — MS Sheets 2026 and Google Sheets.
- **Web-native architecture** — [Univer](https://github.com/dream-num/univer).

## Status

**Pre-code / documentation phase.** There is no runnable engine yet. This
repository currently defines *what* OpenCalc is, *how* it will be built, and *in
what order*. The next milestone is Phase 0: standing up the Cargo workspace, the
CI gate matrix, the fixture corpus, and the bounded XLSX package reader.

Details: [architecture](docs/02-ARCHITECTURE.md) ·
[roadmap](docs/06-ROADMAP-AND-DELIVERY.md) ·
[support matrix](docs/18-SUPPORT-MATRIX.md) ·
[calc engine design](docs/40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md).

## License

Apache-2.0 (planned) — see `LICENSE` once added.
