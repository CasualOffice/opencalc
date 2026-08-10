# 18 — Support Matrix

This matrix separates **target** (what OpenCalc is designed to support) from
**implemented** (what actually passes its gates). A target is **not supported**
until its CI/oracle gate is green.

Legend: ○ target · ◑ in progress · ● implemented & gated.

Statuses here are claims about **gates**, not about effort. If a row says ● and
you cannot point at the test that would fail when it regresses, the row is
wrong — fix the row or add the gate.

## Platforms

| Platform | Rust triple | Tier | Status |
| --- | --- | --- | --- |
| macOS arm64 | `aarch64-apple-darwin` | 1 | ● `platform` matrix job |
| Linux x64 | `x86_64-unknown-linux-gnu` | 1 | ● full job set + MSRV |
| Windows x64 | `x86_64-pc-windows-msvc` | 1 | ● `platform` matrix job |
| WebAssembly | `wasm32-unknown-unknown` | 1 | ● `wasm` job (`cargo check --target wasm32-unknown-unknown`) |
| Linux arm64 | `aarch64-unknown-linux-gnu` | 2 | ○ not in CI |

Tier 1 = full test matrix in CI. Tier 2 = build + smoke.

## Host modes

| Mode | Description | Status |
| --- | --- | --- |
| **Web app (WASM)** | Browser editor via `casual-calc-wasm`; calc + import + render run in WASM | ● the editor in `webapp/` is the reference host |
| **Embeddable browser SDK** | `<opencalc-sheet>` custom element wrapping that editor for third-party pages | ● published as [`@opencalc/sheet`](https://www.npmjs.com/package/@opencalc/sheet); designed in [55](55-SDK-EMBEDDING-AND-INTEGRATION-DESIGN.md). Alpha — the API is not stable until 1.0 |
| Embedded SDK (Rust) | `casual-calc-sdk` in any Rust host app | ● `WorkbookSession` open / edit / recalc / render / save, with `SessionConfig` |
| Headless native | Read/model/calc/write, no rendering surface | ● the same session with no render call |
| Headless render | + display list → PNG via CPU raster | ● incl. frozen panes, split and composed the way the editor canvas draws them (RND-02) |
| **Tauri desktop app** | Native Rust engine via `casual-calc-sdk` from Tauri commands; calc runs native | ○ `casual-calc-tauri` not written (TAURI-001) |

Both the Tauri and web hosts compose the same host-agnostic core; the calc engine
runs **native** on desktop and **WASM** in the browser, producing identical
results. See [02](02-ARCHITECTURE.md) §Host targets.

## Formats

| Format | Import | Export | Notes |
| --- | --- | --- | --- |
| `.xlsx` (SpreadsheetML) | ● | ● | Primary path; byte-identical + semantic writers; round-trip is a gated semantic fixed point |
| CSV / TSV / PSV | ● | ● | Delimited text with typed fields, RFC 4180 quoting and encoding detection (`casual-calc-io`) |
| Normalized JSON | ● | ● | The engine's own deterministic snapshot format |
| `.ods` (OpenDocument) | ○ | ○ | `casual-calc-ods` is a **skeleton with no logic yet**; the crate boundary is reserved, nothing behind it |

## SpreadsheetML feature capability

| Feature | Model | Render | Calc | Status |
| --- | --- | --- | --- | --- |
| Cells: number / string / bool / error | ● | ● | n/a | — |
| Shared strings, inline strings | ● | ● | n/a | — |
| Number formats (built-in + custom) | ● | ● | n/a | Sections, colours, literal runs, elapsed time, locale-specific month/day names |
| Styles: fonts, fills, borders, alignment | ● | ● | n/a | Incl. gradients, patterns, diagonals, double lines, super/subscript |
| Rich text runs | ● | ● | n/a | FID-02, FC-11 |
| Merged ranges, frozen panes, splits | ● | ● | n/a | Frozen panes split in the canvas **and** in the PNG backend (RND-02) |
| Column/row sizing, hidden, outline | ● | ● | n/a | — |
| Defined names | ● | n/a | ● | Incl. print area and titles; unparseable ones are retained |
| Formulas: parse & preserve (AST) | ● | n/a | n/a | — |
| Formulas: evaluate | n/a | n/a | ● | Full recalculation |
| Function library | n/a | n/a | ● | **347 of the 356** in §18.17.7; the nine absent are named in [52](52-FIDELITY-TRACKER.md), not counted |
| `LET` / `LAMBDA` + helpers | n/a | n/a | ● | First-class function values; `MAP`/`REDUCE`/`SCAN`/`BYROW`/`BYCOL`/`MAKEARRAY` |
| Spill / dynamic arrays | ● | ● | ● | `#SPILL!` refuses rather than overwrites |
| Dependency graph + incremental recalc | n/a | n/a | ◑ | Recalculation is correct; the **persistent incremental graph** and the <50 ms budget are the open Phase 2 work |
| Tables & structured references | ● | ● | ● | Banding, totals row, auto-expand, per-table filter |
| Conditional formatting | ● | ● | ● | Ranked rules, priority, colour scales, data bars, font effects |
| Data validation | ● | ● | ● | Every OOXML kind, modelled and enforced |
| Autofilter & sort | ● | ● | n/a | Per-column filter, multi-key sort, recorded sort state |
| Sheet & workbook protection | ● | ● | n/a | Enforced, not merely preserved |
| Print setup & page breaks | ● | ● | n/a | Page setup, print area/titles, manual breaks |
| Comments / threaded comments | ● | ● | n/a | Replies, authors, timestamps |
| Charts | ● | ● | n/a | Read, drawn, and **authored** as real chart parts (7 types) |
| Pivot tables/caches | ● | ● | ● | Built, filtered, refreshed, `GETPIVOTDATA`. One gap: a pivot *created here* exports as its cells, not as a live Excel pivot ([PIV-02](54-PIVOT-TABLES.md)) |
| Images / drawings | ● | ● | n/a | Anchored with EMU offsets; survive a save |
| VBA / macros | preserve only | n/a | **never** | Opaque; never executed |
| Digital signatures, customXml | preserve only | n/a | n/a | Opaque side table |

Anything not listed is covered by the retention path — unmodelled parts survive
a save byte for byte rather than being dropped ([34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)).
The measured per-construct register is [51](51-FIDELITY-GAP-AUDIT.md).

## Editor & SDK capability

| Feature | Status |
| --- | --- |
| Canvas grid: selection, editing, formatting, structural ops, undo/redo | ● [45](45-EDITOR-PARITY-TRACKER.md), [50](50-UX-COMPLETENESS-TRACKER.md) (closed at 63 rows) |
| Accessibility tree mirroring the visible grid | ● UX-A03 |
| Shadow-DOM isolation, typed theme tokens, chrome & command control | ● [55](55-SDK-EMBEDDING-AND-INTEGRATION-DESIGN.md) |
| `edit` / `view` / `preview` access, enforced in the engine | ● |
| Cancellable `before*` events carrying a `source` | ● |
| Localization | ◑ menus, submenus and toolbar tooltips only; panels, dialogs and status messages are still English |
| npm packaging (`@opencalc/sheet`, `/react`, `/engine`) | ● published, released by an `sdk-v*` tag ([15](15-CI-AND-RELEASE-GATES.md) §Release tags) |
| Collaborative editing | ○ Phase 5, decided and unbuilt — server-mediated OT, not a CRDT ([ADR-011](08-ADR-REGISTER.md), [56](56-COLLABORATION-CONCURRENCY-DESIGN.md)). Explicitly **not** in the embeddable SDK, which stays single-user |

## Capacity & performance targets

| Target | Value | Gate | Status |
| --- | --- | --- | --- |
| Populated cells | 1,000,000+ | memory-ceiling benchmark | ◑ benchmark-smoke runs; the ceiling is not yet asserted |
| Grid scroll | 60 fps visible-window repaint | scroll benchmark | ◑ same |
| Worst-case incremental recalc | < 50 ms | recalc-latency benchmark | ○ waits on the persistent incremental graph |

These three are the reason the architecture looks the way it does, and they are
the least gated things in this document. See
[30-PERFORMANCE-AND-CAPACITY-TARGETS](30-PERFORMANCE-AND-CAPACITY-TARGETS.md).

## Required release evidence

A feature moves from ○/◑ to ● only when it ships with:

- a design note and (if triggered) an Accepted ADR,
- import→model→(calc)→render→export tests,
- a fidelity-oracle diff where applicable,
- a fixture in the checksummed corpus,
- a green tracker row.
