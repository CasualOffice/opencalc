# 30 — Performance & Capacity Targets

These are **hard design constraints**, not aspirations to tune toward later. They
shape the model, the layout engine, and the calc engine from the start. Every one
is a gated benchmark ([15](15-CI-AND-RELEASE-GATES.md)).

## The three headline targets

| # | Target | Meaning | Gate | Phase |
| --- | --- | --- | --- | --- |
| T1 | **1,000,000+ populated cells** | The engine holds and operates on a sheet with ≥1M non-empty cells within a bounded memory budget | memory-ceiling benchmark | 1D |
| T2 | **60 fps grid** | A visible-window repaint (scroll or edit) completes within the frame budget (~16.6 ms; working budget **≤ 8 ms** engine-side) | scroll/paint benchmark | 1D |
| T3 | **< 50 ms worst-case recalc** | Worst-case *incremental* recalculation after an edit completes in under 50 ms on the baseline environment | recalc-latency benchmark | 2 |

The baseline environment is a committed named machine profile (as OpenDoc pins
`mac16-12-m4-10c-16gb.json`); budgets are asserted against it, and regressions
beyond a basis-point threshold fail CI.

## What each target forces on the design

### T1 — 1M cells ⇒ a sparse model (ADR-004)

- The cell store is **keyed by address**, never a dense `rows × cols` array. A
  sheet addresses up to 2^20 rows × 2^14 columns (≈17 billion cells); only
  populated cells cost memory.
- Per-cell overhead is budgeted. The model targets a compact cell representation
  (value tag + payload + style index + optional formula-AST handle + cached
  value), not a fat enum-of-boxes. The exact byte budget is fixed in
  [22](22-NORMALIZED-SCHEMA.md) and asserted by the memory benchmark.
- Styles, number formats, and strings are **interned** (shared-string table,
  style index) so 1M cells sharing a format cost one format, not a million.
- Row/column metadata (sizing, hidden, outline) is stored as **run-length /
  sparse spans**, not per-row records, so uniform sheets stay tiny.

### T2 — 60 fps ⇒ viewport virtualization (ADR-009)

- Layout and paint are **O(visible)**, not O(sheet). The layout engine answers
  "what cells intersect this pixel rectangle" without walking the whole sheet —
  via a **cumulative row/column offset index** (prefix sums over sizing spans)
  that maps a scroll position to a cell range in O(log n) or O(1) amortized.
- Only the visible window **plus a small overscan** is laid out and turned into
  display-list items each frame.
- Scrolling reuses cached tile layout; only newly exposed tiles are (re)built —
  **incremental repaint**, not full relayout.
- The invariant: the **virtualized-viewport output is byte-identical to the
  full-layout output** for the same window. This is golden-tested so
  virtualization can never silently diverge from correctness.

### T3 — <50 ms recalc ⇒ an incremental dependency graph

(Rests on the reserved calc seams, ADR-005; the dependency-graph *representation*
itself is a pending ADR to be Accepted before Phase 2, [08](08-ADR-REGISTER.md).)

- Recalculation is **incremental**: an edit dirties a cell and its transitive
  dependents; only the dirty sub-graph recomputes, in dependency order.
- The dependency graph is built and maintained as formulas are set (from the
  reserved model edges), not rebuilt from scratch per calc.
- Range dependencies use **block/interval buckets** so a formula over `A1:A100000`
  doesn't create 100k individual edges.
- Worst case is defined and bounded: the benchmark's worst-case scenario (a long
  dependency chain and a wide fan-out edit on a large sheet) must stay < 50 ms.
  If a naive approach can't, the design (not the target) changes.

## Memory budget (indicative, to be finalized in Phase 0)

| Component | Budget approach |
| --- | --- |
| Populated cell | Compact fixed record; interned style/string; formula AST by handle |
| Empty cell | Zero cost (absent from the sparse store) |
| Row/col metadata | Sparse spans, not per-line |
| Dependency edge | Bucketed; ranges don't explode into per-cell edges |
| Layout | Only cached for laid-out (near-viewport) tiles; evicted on scroll-away |

The concrete numeric ceilings are set when the model lands (Phase 0) and asserted
by the memory-ceiling benchmark; this doc is updated with the measured figures.

## Latency budgets (per frame / per edit)

| Operation | Budget | Notes |
| --- | --- | --- |
| Scroll repaint (visible window) | ≤ 8 ms engine-side | Leaves headroom under the 16.6 ms frame |
| Single-cell edit → layout → paint | ≤ 8 ms | No full relayout |
| Incremental recalc (typical edit) | ≤ 5 ms | Small dirty set |
| Incremental recalc (worst case) | < 50 ms | The T3 hard cap |
| Full recalc (open large workbook) | bounded & cancellable | Not on the interactive path |
| Open / save, page-thread share | no frame gap ≥ 50 ms | `PERF-16`, below |

### Open and save off the page's thread (`PERF-16`)

Open and save were one engine call each on the thread that draws the grid —
933 ms and 700 ms without a frame for a 250,000-cell workbook in Chromium. The
browser editor now runs both in a dedicated Web Worker (`webapp/file.worker.js`,
driven by `webapp/editor.files.js`), as `MACRO-01` runs macros:

- **Open.** The worker runs the same `session_open_as` under the same
  admission bounds and time budget (`SEC-017`), then sends the workbook back
  as **transfer parts** (`casual_calc_model::transfer`). The page reads one
  part (≤ 4,096 formulas or cells, ~5 ms) at a time, yielding between them,
  and installs only after the last: `WorkbookSession::from_opened` builds the
  session the page-thread open would have built (same report, format,
  calculation mode and source bytes), through `set_session`, so the host's
  read-only flag and clock apply as before. A cancelled or failed open leaves
  the session untouched.
- **Save.** The page copies the workbook (4 ms at 250,000 cells — this fixes
  the moment saved; later edits stay dirty) and sends it as parts; the worker
  installs the copy and runs the ordinary writer. An unedited save of the
  opened format is the opened bytes and needs no worker.
- **Why not the snapshot.** The collaboration snapshot carries formulas in
  absolute form, and reading one in a single call is 128 ms at 250,000 cells
  on the page's thread. Transfer parts are never persisted and never cross a
  build (the worker loads the page's own `?b=` wasm), so they carry the
  in-memory form and are not a protocol; `PROTOCOL_VERSION` is unchanged.
  The reader is held to the snapshot limits as parts arrive and checks every
  cell with the rule `Workbook::validate` applies.
- **Fallback.** No `Worker`, or a worker or engine that fails to load: the
  page-thread path, unchanged.
- **What is left on the page.** The first volatile-clock pass on a new session
  (~24 ms at 250,000 cells, the recalculation graph's first build) runs in its
  own task before the redraw. Recalculation off the page is `PERF-17`.

Measured (`tests/browser/editor.open-save-worker.spec.mjs`, Chromium, 250,000
cells): longest frame gap during open 933 → 33 ms, during save 700 → 17 ms.

## Non-negotiable interactions

- **Virtualization must not compromise determinism or fidelity.** The viewport
  path and the full path produce identical display lists (T2 invariant above).
- **The sparse model must not compromise O(visible) layout.** The offset index
  makes viewport queries independent of populated-cell count.
- **Incremental recalc must not compromise correctness.** The incremental result
  equals a full recalc, every time — golden-tested (a Phase 2 exit-gate condition).

## How these targets are protected over time

Each target is a committed benchmark with a baseline and a regression threshold.
A PR that pushes any of T1–T3 past its threshold fails CI. New features that
touch the model, layout, or calc engine must show they stay within budget — this
is part of the [15](15-CI-AND-RELEASE-GATES.md) contract, not optional.
