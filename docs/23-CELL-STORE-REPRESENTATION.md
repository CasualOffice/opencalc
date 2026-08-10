# 23 — Cell-Store Representation

The internal design of the sparse cell grid — the single data structure that most
directly determines whether OpenCalc meets its 1M-cell memory target (T1) and its
O(visible) layout target (T2, [30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)). This
is a **contract** doc: the representation is chosen now so neither the layout
engine nor the calc engine needs a do-over. The concrete constants are finalized
and benchmarked in Phase 0; the *shape* is fixed here (ADR-004).

Referenced from the model schema ([22](22-NORMALIZED-SCHEMA.md)); it holds the
`CellStore` inside each `Sheet`.

## Requirements the store must satisfy

| # | Requirement | Driven by |
| --- | --- | --- |
| R1 | Empty cells cost **zero** memory | T1 — a sheet is mostly empty |
| R2 | ≥1M populated cells within the memory budget | T1 |
| R3 | **Point access** `get/set(row, col)` in ~O(1) | editing |
| R4 | **Range scan** over a rectangle in O(cells-in-range), in address order | layout (viewport), calc (range deps) |
| R5 | **Row/column insert & delete** without rewriting the whole sheet | editing ([24](24-TRANSACTION-AND-EDIT-SEMANTICS.md)) |
| R6 | Deterministic iteration order | golden snapshots |
| R7 | Bounded worst case on hostile input | security ([21](21-PARSER-LIMITS.md)) |

No single flat structure satisfies all of these well; the design is a **two-level
block partition**.

## The representation: row-blocked sparse tiles

```
Sheet.CellStore
└── blocks: BTreeMap<BlockRow, Block>        // BlockRow = row / BLOCK_H
        └── Block
            ├── base_row: u32
            └── rows: BTreeMap<u16, RowRun>  // local row → its cells
                        └── RowRun: sorted Vec<(col: u32, Cell)>  // by column
```

- The sheet is partitioned into **horizontal blocks** of `BLOCK_H` rows (e.g.
  128 or 256 — tuned in Phase 0). Only blocks containing data exist (R1).
- Within a block, only populated rows exist; each populated row is a
  **column-sorted run** of `(col, Cell)` (R1, R6).
- Point access: locate block (O(log #blocks)) → row (O(log rows-in-block)) →
  binary-search the column run (R3).
- Range scan: seek to the first block/row/col in the rectangle, then walk in
  address order — cost proportional to cells visited, not to the sheet (R4).

> **Why blocked, not a single `BTreeMap<(row,col), Cell>`:** a flat map meets
> R1/R3/R4/R6 but makes **row/column insert-delete** (R5) O(populated cells) —
> every key past the insertion point shifts. Blocking localizes structural edits
> and keeps range scans cache-friendly (a viewport row touches contiguous memory).
> Why not a dense per-block array: a block that is 1% full would waste 99% — the
> column *run* keeps blocks sparse too.

## The `Cell` record — the per-cell byte budget (R2)

The per-cell record is **compact and fixed-shape** — the 1M-cell budget is
per-cell-size × 1M, so every field is deliberate ([22](22-NORMALIZED-SCHEMA.md)
defines the fields; this doc pins the sizing intent):

| Field | Representation | Budget note |
| --- | --- | --- |
| `value` | tagged `CellValue` (number `f64` \| bool \| `StringId` \| `ErrorValue` tag) | strings interned → a cell holds an id, not text. **Measured: 32 bytes.** `StringId` wraps a 128-bit `Id`, not the 32-bit id this row originally claimed |
| `style` | `StyleId` (interned index) | 1M cells sharing a format cost **one** format. **Measured: `Option<StyleId>` is 32 bytes** — `StyleId` wraps a 128-bit `Id`, which has no spare bit pattern for `None`, so the option costs a second word |
| `formula` | `Option<FormulaHandle>` (arena index) | `None` on literal cells is a niche/zero cost; a filled column shares one AST |
| `flags` | small bitset (dirty, spill-anchor, spill-child, reserved) | packed |

- **Interning everywhere:** strings (shared-string table), styles (style table),
  and formula ASTs (arena) are shared, so bulk-uniform data (a million cells with
  the same format, or a filled formula) is near-constant extra cost.
- The target is a **small, `Copy`-friendly cell** (a handful of machine words),
  no per-cell heap allocation on the common path.

**The `size_of::<Cell>()` ceiling is asserted** — by
`crates/casual-calc-model/tests/memory_ceiling.rs`, not by the benchmark
harness this document used to name. The harness measures no memory at all and
CI checks only its report's shape, so until that test existed the capacity
claim rested on nothing that could fail.

A cell is **32 bytes**. It was 80 until [ADR-013](58-INTERNED-ID-WIDTH.md)
narrowed `StyleId` and `StringId` to `NonZeroU32`: they had always been `u32`
indices, boxed in a 128-bit `Id` with a constant namespace tag around them, so
`Option<StyleId>` cost 32 bytes and `CellValue` another 32. This document's
description — an interned index, "a 32-bit id, not text" — was right about the
intent all along; only the representation was not.

## Relationship to the offset index (T2)

The cell store answers *"what is at (row, col)"*; it does **not** store geometry.
Column widths and row heights live as sparse spans (`AxisSpans`,
[22](22-NORMALIZED-SCHEMA.md)) and feed the **cumulative offset index** in
`casual-calc-layout` ([42](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md)). A
viewport paint is therefore:

1. offset index: scroll rect → `(row_range, col_range)` in O(log spans);
2. cell store: range-scan that rectangle in O(visible) (R4).

Neither step depends on how many cells are populated elsewhere in the sheet —
this is the mechanism behind 60 fps on a 1M-cell sheet.

## Structural edits (R5)

Row/column insert & delete are performed by the transaction layer
([24](24-TRANSACTION-AND-EDIT-SEMANTICS.md)) and touch the store as follows:

- **Row insert/delete** shifts affected blocks' `base_row` / re-buckets rows at
  block boundaries — localized, not a full rewrite; blocks entirely below the
  edit shift by an offset without moving their cell data.
- **Column insert/delete** shifts the `col` key in the affected row runs
  (bounded by populated cells in those rows, not the sheet).
- Both also trigger **reference rewriting** in formula ASTs (a separate concern
  owned by `casual-calc-formula`, see [24](24-TRANSACTION-AND-EDIT-SEMANTICS.md)).
- Every structural edit produces an inverse so it is undoable.

## Iteration contracts

- **Deterministic order:** block-major, then row, then column ascending — used by
  snapshot serialization (R6) so golden snapshots are stable.
- **Used-range vs true bounds:** the imported `dimension` is a hint; the store can
  report true populated bounds by inspecting the first/last block/row/col.
- **Spill regions** ([40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md)) are ordinary
  cells whose `flags` mark anchor/child; the store doesn't special-case them.

## Security (R7)

- The admission cap on populated cells ([21](21-PARSER-LIMITS.md)) bounds total
  store size; over-cap input is rejected cleanly, never partially admitted.
- Block/run structures have no unbounded per-cell recursion or allocation; a
  hostile "one cell per block, scattered" input is bounded by the same cell cap.

## What is deferred to Phase 0 (constants, not shape)

- `BLOCK_H` and whether columns are also blocked (a second axis) — chosen by
  benchmark against the memory + scroll + range-scan workloads.
- The exact `Cell` field packing and the `size_of` ceiling.
- Whether very dense blocks switch to a dense representation adaptively.

These are tuning decisions inside the fixed shape above; changing the *shape*
(e.g. abandoning blocking) would require a new ADR.
