# 45 — Editor Parity Tracker & Competitive UX Analysis

The single source of truth for the **web editor's** journey to product-grade
parity with Excel / Google Sheets. Every requested UX item lives here with a
status, so work survives context resets and we stop re-deriving the backlog.

Status vocabulary: **Done** · **WIP** (in progress this session) · **Todo** ·
**Bug** (regression/known-broken). Priority: **P0** (broken basics) · **P1**
(expected of any sheet) · **P2** (polish / advanced).

Update this file the moment an item's status changes.

## Competitive UX baseline (what "a real sheet" has)

> **See also (generated 2026-08-06 by a parallel multi-agent audit):**
> [47 — UX & Feature Map](47-UX-AND-FEATURE-MAP.md) (every feature's status + UX
> grade + parity gap), [48 — Feature Pipeline](48-FEATURE-PIPELINE.md)
> (dependency-ordered roadmap), [49 — Design System](49-DESIGN-SYSTEM.md). Those
> supersede this hand-kept tracker as the authoritative map; this file remains
> the changelog of shipped editor work.

Reference products: Microsoft Excel (desktop + web 2026), Google Sheets,
OnlyOffice, Univer. The non-negotiable UX every one of them ships:

1. **Navigation** — visible, consistent scrollbars; smooth scroll; freeze panes;
   name box; go-to; keyboard nav (arrows, Ctrl+arrows to edges, PgUp/PgDn, Home).
2. **Selection** — cell / range (drag + shift); **whole row/column via header**;
   **whole sheet via corner**; multi-range (Ctrl); selection never scroll-jumps.
3. **Sizing** — drag resize; **double-click boundary = auto-fit**; **resize
   applies to all selected rows/cols**; hidden rows/cols.
4. **Structure** — insert / delete / hide rows & columns with **formula reference
   rewriting**; move; sort/filter.
5. **Editing** — inline + formula-bar edit; overflow of text into empty cells;
   wrap; autofill/drag-fill; cut/copy/paste (values, formats, formulas).
6. **Formatting** — bold/italic/underline/strike; font family + size; text +
   fill color; alignment (H + V); number formats; borders (all sides + styles);
   merge cells; conditional formatting.
7. **Chrome** — icon toolbar; **right-click context menus** (cell, row/col,
   tab); sheet tabs with rename/duplicate/delete/reorder/color; status bar with
   sum/avg/count; find & replace.
8. **Consistency** — same look on every OS (no native scrollbars/menus);
   light/dark; predictable shortcuts (Excel-flavored).

## Tracker

### P0 — broken basics (fix immediately)

| ID | Item | Status | Notes |
|----|------|--------|-------|
| UX-01 | Row/column header select must NOT scroll to the far end | Done | `selKind` model; focus stays at near edge, no scroll. Verified |
| UX-02 | Corner (0,0) selects the **whole sheet** (not just used range) | Done | `selKind:"all"`; highlight + headers tint span the viewport. Verified |
| UX-03 | Resizing a row/col while whole-sheet/row/col selected resizes **all** of them | Done | `session_set_all_col_width`/`_row_height` + `_range`; live preview honors scope; "resized all". Verified |
| UX-04 | **Custom, OS-consistent scrollbars** (currently none visible) | Done | Overlay DOM scrollbars (V + H), draggable thumbs styled by our CSS, sized from used-extent + buffer; hidden when nothing to scroll. Verified |

### P1 — expected of any sheet

| ID | Item | Status | Notes |
|----|------|--------|-------|
| UX-10 | Full text formatting: bold/italic/underline + font color + fill | Done | 2026-08-05 product pass |
| UX-11 | Horizontal alignment L/C/R | Done | `Style.align`, round-trips |
| UX-12 | Number formats incl. currency/percent/menu | Done | numfmt literal runs; `$`, `%` |
| UX-13 | Borders menu (all/outer/clear) | Done | `session_set_border(kind)` |
| UX-14 | Sheet tabs: rename / duplicate / delete + context menu | Done | last-sheet protected |
| UX-15 | Double-click column boundary = auto-fit | Done | widest-cell measure |
| UX-16 | **Font family + size** controls | Done | `Style.font_name` + `font_size_hp` (round-trip); toolbar font + size dropdowns; canvas renders family/size; rows auto-grow for tall fonts. Verified |
| UX-17 | **Structural insert/delete rows & columns** + formula ref rewriting | Done | Transaction ops (invertible, cross-sheet ref rewrite, 27 tests); wasm `session_insert/delete_rows/columns` (undoable); wired to the cell context menu. Verified: insert shifted SUM(D2:D4)->SUM(D3:D5), recalc correct |
| UX-18 | **Cell right-click context menu** (cut/copy/paste/clear + insert/delete row-col) | Done | Right-click cell -> Cut/Copy/Paste, Insert row/col, Delete row/col, Clear. Verified |
| UX-19 | **Text overflow** into adjacent empty cells (+ wrap toggle) | Done | Overflow across empty neighbours (align-aware); **Wrap text** toolbar toggle (`Style.wrap`, round-trips via `<alignment wrapText>`) with word-wrap + auto row height. Verified |
| UX-20 | Vertical alignment (top/middle/bottom) | Done | `Style.valign` (VAlign, round-trips via `<alignment vertical>`); toolbar menu; canvas positions text top/middle/bottom (single + wrapped). Verified |
| UX-21 | Merge cells | Done | `session_merge/unmerge_cells` + `session_merges`; editor renders a merge as one spanning cell (no internal gridlines), selection snaps to it, Merge toolbar button toggles. Verified |
| UX-22 | Status bar: sum / average / count of selection | Done | `session_range_stats`; bottom-bar readout for multi-cell selections. Verified |
| UX-23 | Ctrl+arrow to data edge, Home/End, PgUp/PgDn | Done | `session_edge` block-jump; Ctrl+Arrow, Ctrl+Home/End, Home, PgUp/PgDn (+shift-extend). Verified |
| UX-24 | Multi-range selection (Ctrl+click) | Done | `state.ranges` banks extra rectangles; Ctrl/Cmd+click commits the active range and starts a fresh one (Ctrl+drag extends it); each range gets the selection tint; formatting/clear fold over every range (`allRanges`), and the status bar aggregates Sum/Avg/Count across ranges. Plain click / arrow / header-select clear the extra ranges. Verified in-browser (3 disjoint cells: bold applied to all, Sum 23.49 / Avg 11.745 / Count 3) |
| UX-25 | Drag-fill / autofill handle | Done | Fill handle at the selection corner; drag to extend; `session_fill` tiles the source (value+style) and **shifts relative formula refs** (`shift_references`, absolute `$` held). Verified: fill of =(B2+C2) -> =(B3+C3)/=(B4+C4). |
| UX-26 | Hidden rows/columns (+ from file) | Done | `Sheet.hidden_rows/cols` (import/export `hidden="1"`); layout + editor treat hidden as 0px; Hide/Unhide in the cell context menu. Verified |
| UX-27 | Freeze panes in editor | Done | Editor geometry reworked to an explicit line-index list + per-cell quadrant clipping; 4-pane render (corner fixed, frozen row scrolls-x, frozen col scrolls-y, body both); divider lines; freeze toolbar menu (top row / first col / to selection / unfreeze); ensureVisible + clicks freeze-aware. No-freeze path unchanged. Verified |
| UX-28 | Find & replace | Done | `session_find` / `session_replace_all` (case toggle, undoable); Ctrl+F bar with next/prev, count, replace-all. Verified |
| UX-29 | Sheet tab reorder (drag) + tab color | Done | `session_move_sheet` + HTML5 drag-reorder (active sheet re-tracked). Tab color: model `Sheet.tab_color`, `<sheetPr><tabColor>` import/export (round-trip test), `session_tab_color`/`session_set_tab_color`, swatch strip in the tab context menu, colored underline stripe on the tab. Verified in-browser |

### P2 — polish / advanced

| ID | Item | Status | Notes |
|----|------|--------|-------|
| UX-40 | Conditional formatting | Todo | |
| UX-41 | Tables (structured ranges) | Todo | |
| UX-42 | Sort & filter | Done | **Filter done:** funnel toolbar button opens a checklist dropdown for the active column (sorted distinct values, Select-all, blanks as "(Blanks)"); unchecked values hide their rows (row 0 treated as header, never hidden) via `session_hide_rows`, Clear restores. Verified: unchecking "Gadget" hid row 3. **Sort done:** `session_sort_range` moves whole rows (values + styles + flags) sorted by a key column — blanks last, numbers before case-insensitive text; **relative formula references shift by the row delta** so per-row formulas (`=B2*C2`) re-anchor to their new row and recompute correctly (verified: sort by Qty asc kept the Total column right, no #REF!). One undo step. Toolbar sort menu (A→Z / Z→A) + cell context-menu items, keying off the active column. |
| UX-43 | Strikethrough | Done | `Style.strike` (round-trips `<strike/>`); toolbar toggle + strike-line render. Verified |
| UX-44 | Cell comments/notes | Todo | |
| UX-45 | Data validation | Todo | |
| UX-46 | PNG-render fills/font-colors/borders | Done | display list extended (CellBackground.fill, Text color/bold/italic, CellBorder); render paints them. Gated |
| UX-47 | Formula editing UX (autocomplete / ref insertion / validation) | Done | (1) **Function autocomplete** — typing `=SU…` opens a dropdown of matching functions with signatures (`function_catalog` wasm), ↑/↓ + Tab/Enter to accept, inserts `NAME(`. (2) **Reference insertion** — while editing a formula at a reference position (after `=`, an operator, `(`, `,`), clicking a cell inserts its A1 ref and dragging inserts a range (`A2:A4`), keeping the editor open. (3) **Validation** — `validate_formula` parses on commit; an unparseable formula is rejected (red cell outline + status error) and stays in edit mode instead of being silently stored as text. Verified in-browser. Editor now loads the wasm pkg with a build tag (`?b=`) so a rebuilt engine is never shadowed by a stale browser cache |

## Now / next

- **Shipped:** UX-01..04, 10..19, 22 — plus a big calc-engine expansion (IFERROR,
  AND/OR/NOT, COUNTIF/SUMIF/AVERAGEIF, CONCAT/text/INT/MOD/POWER/SQRT).
- **Next (dependency order):** UX-21 merge cells (+ render) → UX-27 freeze panes
  render → UX-20 vertical align → UX-23 keyboard nav (Ctrl+arrow/Home/PgUp) →
  UX-25 drag-fill → UX-28 find & replace → UX-46 PNG borders. Merged-cells and
  frozen-panes are the current fidelity trap (modelled + round-trip, editor
  ignores them).

See also [46-COMPETITIVE-PARITY-ANALYSIS.md](46-COMPETITIVE-PARITY-ANALYSIS.md)
(the exhaustive 141-item competitive inventory, CP-001..CP-141, that this tracker
distills), [14-EXECUTION-TRACKER.md](14-EXECUTION-TRACKER.md) (engine-level), and
[33-FIDELITY-LEDGER.md](33-FIDELITY-LEDGER.md).
