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
