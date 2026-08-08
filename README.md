# OpenCalc

[![CI](https://github.com/CasualOffice/opencalc/actions/workflows/ci.yml/badge.svg)](https://github.com/CasualOffice/opencalc/actions/workflows/ci.yml)
[![Status: Alpha](https://img.shields.io/badge/status-alpha-yellow.svg)](docs/06-ROADMAP-AND-DELIVERY.md)
[![Rust: MSRV 1.88](https://img.shields.io/badge/rust-MSRV%201.88-black.svg?logo=rust)](rust-toolchain.toml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**A deterministic, embeddable spreadsheet engine written in Rust** — it reads and
writes `.xlsx`, holds the workbook in a normalized editable model, calculates it,
and lays out and renders a live cell grid to pixels, for native, WebAssembly, and
headless hosts that need real SpreadsheetML fidelity without a browser, a server,
or a UI framework.

One host-agnostic core, **two first-class hosts**: a **Tauri desktop app** where
the engine (including the formula/calc engine) runs as **native Rust** at full
speed, and a **web app** where the same engine compiles to WebAssembly — evaluate
formulas, open an `.xlsx`, recalculate, and render the grid, all client-side. A
**live WebAssembly demo** is deployed to GitHub Pages from
[`webapp/`](webapp/) via [`.github/workflows/pages.yml`](.github/workflows/pages.yml).

OpenCalc is the spreadsheet counterpart to [OpenDoc](../opendoc-fixes) (the
`.docx` engine): it reuses OpenDoc's format-neutral spine — bounded OPC package
admission, the loss-aware preservation model, deterministic snapshots, the
display-list/render seam, and the whole CI/fuzz/benchmark gate scaffold — and
replaces the document-specific layers with a **workbook model**, a **formula &
recalculation engine**, and a **virtualized grid layout**.

Developed by [CasualOffice](https://github.com/CasualOffice) as the spreadsheet
engine for Casual Sheets and an SDK others can embed.

> **Status: Alpha — the engine and a working editor are live.** The full pipeline
> runs end to end: read `.xlsx` → normalized model → edit → recalculate → write
> `.xlsx` → virtualized layout → render. A **browser editor** (canvas grid over
> the WebAssembly engine) supports real editing and formatting, and CSV/TSV/PSV
> import & export work alongside XLSX. The formula engine is live for a growing
> function set (full incremental recalc + the <50 ms budget are the remaining calc
> work). Track live state in
> [docs/14-EXECUTION-TRACKER.md](docs/14-EXECUTION-TRACKER.md),
> [docs/33-FIDELITY-LEDGER.md](docs/33-FIDELITY-LEDGER.md), and — for the editor —
> [docs/45-EDITOR-PARITY-TRACKER.md](docs/45-EDITOR-PARITY-TRACKER.md).

## What works today

- **Round-trip `.xlsx`** — import → edit → write is a *semantic fixed point* (gated
  test). Values, formulas (as an AST), number formats, fonts (family/size/bold/
  italic/underline/color), fills, **borders**, **horizontal + vertical alignment**,
  **text wrap**, **merged ranges**, **column/row sizing**, frozen-pane metadata,
  and defined names all survive.
- **Reads what other writers actually emit** — OOXML booleans in either spelling
  (`customWidth="true"` as well as `="1"`, which LibreOffice, Apache POI and
  ExcelJS all use), **theme + tint and legacy indexed colours** (the form every
  Excel built-in cell style uses), **shared formulas** expanded from their master,
  multi-area `sqref`, and comments bound through the OPC relationship graph.
- **Delimited text** — open and save **CSV / TSV / PSV** with typed fields and RFC
  4180 quoting.
- **Formula engine** — tokenizer + Pratt parser + AST + recalculation, with a
  library incl. `SUM/AVERAGE/COUNT/MIN/MAX/IF/IFERROR/AND/OR/NOT`, `COUNTIF/SUMIF/
  AVERAGEIF`, text (`LEFT/RIGHT/MID/LEN/UPPER/LOWER/TRIM/CONCAT`) and math
  (`INT/MOD/POWER/SQRT/ROUND/ABS`).
- **Number formats** — `General`, fixed decimals, thousands, percent, currency and
  literal runs, dates/times by token layout, **scientific**, the
  positive/negative/zero/**text** sections, and **section colours** (`[Red]`).
- **Editor** (WASM canvas grid): inline + formula-bar editing that share **one
  edit session** — function autocomplete, an **argument hint**, click/drag and
  arrow-key **reference picking**, **F4 anchor cycling**, and a **range finder**
  that outlines the cells a formula reads; **Alt+Enter** line breaks; drag/shift/
  header selection, fluid scrolling with **custom scrollbars**, **drag-to-resize**
  (+ resize-all, auto-fit), a **formatting toolbar** with an in-face font picker,
  **merge cells** (warning before it discards values), row/column **header menus**,
  insert/delete rows & columns with **formula-reference rewriting**, Excel-style
  **keyboard navigation**, a selection **status bar**, **find & replace**,
  conditional-formatting / data-validation / notes panels, multi-sheet **tabs**,
  themes, and undo/redo.
- **Render** — deterministic PNG raster of a viewport (tiny-skia) with glyphs from
  bundled metric-compatible faces, and a live in-browser canvas renderer.

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

| Phase | Delivers | Status |
| --- | --- | --- |
| 0 — Foundation | workspace, CI, fixture corpus, bounded XLSX reader, minimal model | ✅ done |
| 1A — Import & model | workbook → sheets → cells, shared strings, styles, number formats, merged ranges, defined names; preservation ledger + compatibility report | ✅ done |
| 1B — Semantic writer | model → valid editable `.xlsx`; round-trip fixed point | ✅ done |
| 1C — Grid layout | column/row geometry, merged cells, number-format display, display list | ✅ done |
| 1D — Grid render & virtualization | CPU raster (tiny-skia), viewport virtualization, hit-testing | ✅ core (the PNG backend does not split frozen panes; the editor canvas does) |
| 1E — Browser grid editor (WASM) | cell edit, selection, formatting, structural ops, undo/redo | ✅ done (see the parity + UX trackers) |
| **2 — Formula & calc engine** | tokenizer/parser, recalc, function library; dependency graph, incremental recalc, spill/array | 🟡 recalc + core functions live; incremental graph pending |
| 3 — Spreadsheet features | conditional formatting, data validation, tables/structured refs, autofilter, sort, charts, pivots | 🟡 conditional formatting, data validation, single-key sort and a basic filter are live; tables, charts and pivots are not |
| 4 — SDK beta / embedding | stable host surfaces, native + WASM packaging | ⬜ |
| 5 — Collaboration / web | operation model for shared editing | ⬜ |
| 6 — 1.0 | stable SDK, support guarantees | ⬜ |

Full detail: [docs/06-ROADMAP-AND-DELIVERY.md](docs/06-ROADMAP-AND-DELIVERY.md).

## Workspace

OpenCalc is a Cargo workspace of small, layered crates; each layer depends only on
those below it (layer division in
[docs/19-WORKSPACE-SCAFFOLD-DESIGN.md](docs/19-WORKSPACE-SCAFFOLD-DESIGN.md)).

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
| `casual-calc-layout` | Grid geometry, column/row sizing, frozen panes, merged cells, number-format display text (incl. section colours and the text section), the metric-compatible font-substitution table, viewport virtualization, and the backend-neutral display list |
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

## Try it

```sh
cargo test --workspace                 # engine test suite
# live editor (WebAssembly):
wasm-pack build crates/casual-calc-wasm --release --target web --out-dir "$PWD/webapp/pkg"
python3 webapp/serve.py                 # then open http://localhost:8099/editor.html
```

A live demo is also deployed to GitHub Pages from [`webapp/`](webapp/).

## Status

**Alpha.** The engine and editor are functional and improving; the API is not yet
stable. Remaining before beta: the incremental dependency graph (<50 ms recalc), a
broader function library, frozen panes in the **PNG** backend, multi-key sort with
header detection, and the rest of Phase 3 (tables/structured refs, charts,
pivots). Known editor gaps are itemised — with severity — in
[docs/50-UX-COMPLETENESS-TRACKER.md](docs/50-UX-COMPLETENESS-TRACKER.md).

Details: [architecture](docs/02-ARCHITECTURE.md) ·
[roadmap](docs/06-ROADMAP-AND-DELIVERY.md) ·
[editor parity](docs/45-EDITOR-PARITY-TRACKER.md) ·
[UX completeness](docs/50-UX-COMPLETENESS-TRACKER.md) ·
[fidelity ledger](docs/33-FIDELITY-LEDGER.md) ·
[calc engine design](docs/40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md).

## License

Apache-2.0 (planned) — see `LICENSE` once added.
