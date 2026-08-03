# 42 — Grid Layout, Virtualization & Rendering Architecture

How OpenCalc lays out and paints a spreadsheet grid — including a million-cell
sheet — at 60 fps, deterministically. This is the second design-critical pillar
(with [19](19-WORKSPACE-SCAFFOLD-DESIGN.md)); virtualization is designed in from
the model up, never retrofitted (ADR-009).

Owning crates: `casual-calc-layout` (geometry + display list) and
`casual-calc-render` (CPU raster). Layout reads the model's **cached cell
values**, so the whole grid renders before the calc engine exists and keeps
working after.

## The layout problem, restated

A worksheet is a grid of up to 2^20 × 2^14 cells. We must, every frame:

1. Know which cells fall in the current viewport (a pixel rectangle + scroll).
2. Lay out just those cells (with merges, frozen panes, in-cell text).
3. Emit a display list for that window.
4. Rasterize it.

Doing this in O(sheet) is impossible at 60 fps. Everything below makes it
O(visible).

## Units

All geometry is in **twips** (1/1440 inch); device pixels appear only at raster
time (DPI applied in `casual-calc-render`). This matches OpenDoc and keeps layout
resolution-independent and golden-testable.

## Geometry: the cumulative offset index

The core structure that makes viewport queries cheap.

- Column widths and row heights are stored as **sparse spans** in the model
  (default size + explicit overrides + hidden ranges), not per-line.
- Layout maintains, per axis, a **cumulative-offset index**: prefix sums over the
  spans, so:
  - `offset(line)` → the twip position of a row/column edge, and
  - `line_at(offset)` → the row/column at a scroll position,
  both in **O(log n)** over the number of spans (not the number of lines), with
  an O(1) amortized fast path for uniform regions.
- Scroll position → visible cell range is therefore two `line_at` lookups per
  axis. Independent of how many cells are populated (T1) or how far down you
  scroll (T2).
- The index updates incrementally when a row/column is resized, inserted, or
  deleted — the same edits the transaction layer performs.

## Tiles: the unit of caching and incremental repaint

- The grid is partitioned into fixed **tiles** (a block of rows × columns, size
  tuned in Phase 1D). A tile is the unit of layout caching and paint.
- On scroll, only tiles newly entering the viewport (+ overscan) are laid out;
  tiles leaving are evicted. Already-visible tiles are reused unchanged →
  **incremental repaint**.
- On edit, only the tile(s) containing changed cells are invalidated and rebuilt
  (a `DirtySet` of cell addresses → affected tiles), mirroring OpenDoc's
  incremental-layout `DirtySet`/galley-cache idea but on a 2-D grid.
- Tile layout caches the shaped in-cell text (glyph runs), so scrolling never
  re-shapes.

## Per-cell layout

For each visible cell the layout engine computes:

- The cell rectangle (from the offset index; merged cells span multiple lines).
- The **displayed text**: the cached value formatted through the cell's
  **number format** (dates, currency, scientific, fractions, custom codes) —
  this is where format-code interpretation lives, matched to the oracle.
- Text shaping via **`parley`** (shared `LineShaper` seam with OpenDoc), honoring
  font/size/weight/color from the style, plus alignment, wrap, shrink-to-fit,
  rotation, and indent.
- Overflow behavior: unclipped spill into empty neighbors vs clip vs `####`
  when a number doesn't fit — matched to Excel semantics.
- Borders, fills, and (Phase 3) conditional-format overlays.

### Merged cells & frozen panes

- **Merged ranges** lay out as a single rectangle anchored at the top-left cell;
  the offset index gives the union rectangle directly.
- **Frozen panes / splits** partition the viewport into up to four independently
  scrolled regions; each region runs the same visible-range query against its own
  scroll offset. Row/column headers are a always-present frozen band.

## The display list (the render seam)

Layout emits a **backend-neutral, serializable `DisplayList`** — the single
contract between layout and any renderer (ADR-008). It is golden-tested.

Paint items for a grid window include:

- `Rect` / `RoundedRect` — cell fills, selection, header bands.
- `Line` — gridlines and borders (batched per run for efficiency).
- `Glyphs { GlyphRun }` — shaped in-cell text, clipped to the cell.
- `PushClip` / `PopClip` — per-cell / per-pane clipping.
- `Image { media, rect, crop }` — embedded pictures / (Phase 3) chart rasters.

Painter's-algorithm order; clips nest. The **virtualized-viewport display list is
identical to the full-layout display list restricted to that window** — the
golden invariant that lets us virtualize without risking correctness.

## Rendering: the CPU backend

`casual-calc-render` executes a `DisplayList` onto a `tiny-skia` `Pixmap`:

- Glyph outlines come from **`skrifa`**, from the *exact* face the shaper used,
  so shaping and rasterization never disagree.
- `GlyphSource` / `MediaSource` traits decouple font/image byte resolution (the
  host supplies bytes; the engine stays pure).
- Secure image-decode caps (max dimension / pixel count), matching OpenDoc.
- Headless `encode_png()` for tests and server-side rendering.
- A **GPU backend is a future additive target** — it consumes the same display
  list; no layout change is needed (this is why the seam is fixed now).

## Hit-testing

The inverse of layout: pixel → cell. A `LayoutSnapshot` for the current viewport
answers `hit_test(x, y)` → `(sheet, row, column, sub-cell position)` using the
same offset index. Drives selection, editing, and fill handles.

## Determinism & fidelity

- Layout is a pure function of (model, viewport, font set). Same inputs → same
  display list.
- Golden display lists gate layout; the **viewport path is diffed against the
  full path** so virtualization can't drift.
- Number-format display and rendered cells are diffed against the LibreOffice
  Calc / Excel oracle on a format corpus (Phase 1C exit gate).

## Why this meets the targets

- **T1 (1M cells):** layout never touches non-visible cells; the offset index and
  sparse spans are independent of populated-cell count.
- **T2 (60 fps):** O(visible) layout + tile reuse + cached glyph runs keep a
  scroll frame within the ≤ 8 ms engine budget
  ([30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)).
- **Determinism:** the display-list seam is golden-tested and backend-neutral.
