# 45 — Editor Parity Tracker & Competitive UX Analysis

The single source of truth for the **web editor's** journey to product-grade
parity with Excel / Google Sheets. Every requested UX item lives here with a
status, so work survives context resets and we stop re-deriving the backlog.

Status vocabulary: **Done** · **WIP** (in progress this session) · **Todo** ·
**Bug** (regression/known-broken). Priority: **P0** (broken basics) · **P1**
(expected of any sheet) · **P2** (polish / advanced).

Update this file the moment an item's status changes.

## Competitive UX baseline (what "a real sheet" has)

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
| UX-20 | Vertical alignment (top/middle/bottom) | Todo | `Style.valign` |
| UX-21 | Merge cells | Done | `session_merge/unmerge_cells` + `session_merges`; editor renders a merge as one spanning cell (no internal gridlines), selection snaps to it, Merge toolbar button toggles. Verified |
| UX-22 | Status bar: sum / average / count of selection | Done | `session_range_stats`; bottom-bar readout for multi-cell selections. Verified |
| UX-23 | Ctrl+arrow to data edge, Home/End, PgUp/PgDn | Todo | Excel nav parity |
| UX-24 | Multi-range selection (Ctrl+click) | Todo | |
| UX-25 | Drag-fill / autofill handle | Todo | |
| UX-26 | Hidden rows/columns (+ from file) | Todo | model flag; import `hidden` |
| UX-27 | Freeze panes in editor (model has `SheetView`) | Todo | render frozen bands |
| UX-28 | Find & replace | Todo | |
| UX-29 | Sheet tab reorder (drag) + tab color | Todo | |

### P2 — polish / advanced

| ID | Item | Status | Notes |
|----|------|--------|-------|
| UX-40 | Conditional formatting | Todo | |
| UX-41 | Tables (structured ranges) | Todo | |
| UX-42 | Sort & filter | Todo | |
| UX-43 | Strikethrough, more font styles | Todo | |
| UX-44 | Cell comments/notes | Todo | |
| UX-45 | Data validation | Todo | |
| UX-46 | PNG-render borders/colors (landing preview) | Todo | display-list contract change |

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
