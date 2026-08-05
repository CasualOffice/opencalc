# 46 — Competitive Parity Analysis

The **authoritative, fidelity-grounded parity backlog** for OpenCalc's editor and
engine, measured against Microsoft Excel (desktop + web), Google Sheets,
OnlyOffice, and Univer. Its purpose is to stop the team re-deriving "what's
missing": every gap between what a real spreadsheet does and what OpenCalc does
today is catalogued once here, with a stable ID, the fidelity dimension(s) it
touches, an effort and priority, and the concrete model/API change it needs. It
complements [45-EDITOR-PARITY-TRACKER.md](45-EDITOR-PARITY-TRACKER.md) (the
session-level task board — statuses there are the live truth) and
[33-FIDELITY-LEDGER.md](33-FIDELITY-LEDGER.md) (per-construct fidelity matrix).
This doc is the *complete competitive surface*; 45 is the *working subset*.

## How to read this

- **Status** is verified against code as of 2026-08-05:
  `webapp/editor.{js,html,css}`, `crates/casual-calc-model/src/{style,sheet,cell,value}.rs`,
  `crates/casual-calc-wasm/src/lib.rs`, `crates/casual-calc-eval/src/functions.rs`.
  - **Done** — usable end-to-end in the editor (or fully modelled + round-tripped
    where the item is model-only).
  - **Partial** — some layers present (e.g. modelled + round-trips) but not
    usable/visible in the editor, or a narrow subset only.
  - **Todo** — not present in any usable form.
- **Fidelity dims** use the [33](33-FIDELITY-LEDGER.md) vocabulary:
  **Model** / **Round-trip** / **Edit** / **Render** / **Calc**. A cell in the
  editor is *usable* only when Edit **and** Render are green; a file *survives*
  only when Model **and** Round-trip are green.
- **Effort**: S (≤1 day), M (a few days), L (a week+ or cross-layer).
- **Priority**: **P0** broken-basics, **P1** expected-of-any-sheet, **P2** polish.

## What OpenCalc has today (baseline — do not re-list as missing)

Verified present and usable:

- **Grid**: canvas render, variable column/row sizing from real `.xlsx` widths,
  fluid sub-cell pixel scroll (wheel), headers with selected-band tint.
- **Selection**: single cell, drag range, shift-extend, whole row/col via header,
  whole sheet via corner (`selKind` model), Ctrl+A. No scroll-jump on header
  select (UX-01 fixed).
- **Navigation**: arrows, Enter (down), Tab / Shift+Tab, shift+arrow extend.
- **Sizing**: drag-resize (undoable), double-click col boundary = autofit-to-widest,
  double-click row boundary = reset-to-default, resize-all / resize-band by
  selection scope.
- **Editing**: inline edit, formula-bar edit, F2, type-to-replace, Delete/Backspace
  clears, undo/redo, copy/paste as **TSV values**.
- **Formatting**: bold / italic / underline, font color, solid fill color,
  horizontal align L/C/R, number formats (menu + currency + percent buttons),
  borders (all / outer / clear). Toolbar reflects the focus cell's format.
- **Number formats**: General, fixed decimals, thousands, percent, currency,
  basic date; literal runs. (Negative/text/color sections deferred — P1C-002.)
- **Sheets**: bottom tab bar, switch, add, rename (dblclick), duplicate, delete,
  tab right-click context menu. Last-sheet protected.
- **Files**: open/save `.xlsx`; open/save `.csv/.tsv/.psv`. `.xlsx` round-trips
  values, formulas (from AST), number formats, fonts/fills, borders, merges,
  frozen panes, defined names at the **semantic fixed point** (`import→write→import`).
- **Calc**: full recalc; arithmetic, comparison, string concat, unary, cell +
  range refs (same/cross-sheet), `$`-absolute refs, defined names; functions
  **SUM, AVERAGE, COUNT, MIN, MAX, IF, ABS, ROUND** (8 total).
- **Chrome**: icon toolbar, light/dark + accent theming, settings panel, sheet
  tabs, read-only cell-ref indicator, status text.

Modelled + round-tripped **but not yet editable/rendered in the editor** (Partial
— the important "fidelity trap" set): **merged ranges**, **frozen panes**,
**defined names**, **per-edge border style/color** (only all/outer/clear exposed).

## Gap catalog

Columns: **ID | Area | Item | Reference behavior | Status | Fidelity dims |
Effort | Priority | Model/API changes needed.**

### Navigation

| ID | Area | Item | Reference behavior | Status | Fidelity dims | Effort | Priority | Model/API changes |
|----|------|------|--------------------|--------|---------------|--------|----------|-------------------|
| CP-001 | Nav | Arrow-key cell movement | Arrows move focus one cell | **Done** | Edit | — | P0 | — |
| CP-002 | Nav | Enter/Tab advance + Tab-run return | Enter↓, Tab→, Enter after a Tab run returns to start column of next row | **Partial** | Edit | S | P1 | Editor: track tab-run origin column |
| CP-003 | Nav | Ctrl+Arrow to data edge | Jump to next non-empty/edge block boundary | **Todo** | Edit | M | P1 | WASM `session_edge_in_direction`; scans `CellStore` |
| CP-004 | Nav | Home / Ctrl+Home / Ctrl+End | Home→col A of row; Ctrl+Home→A1; Ctrl+End→last used cell | **Todo** | Edit | S | P1 | Reuse `session_used_bounds` |
| CP-005 | Nav | PgUp / PgDn (+Alt = horizontal) | Page the viewport by its height/width | **Todo** | Edit | S | P1 | Editor-only (page = visible rows) |
| CP-006 | Nav | Name box (type `A1` / `B2:D9` / name → select+scroll) | Editable ref box selects/navigates | **Todo** | Edit | S | P1 | Make `cell-ref` an input; A1 parse (reuse formula ref parser) |
| CP-007 | Nav | Go-to (Ctrl+G / F5), go-to-special | Dialog to jump to ref/name; select blanks/formulas/etc | **Todo** | Edit | M | P2 | Name-box first; special needs cell-kind scan |
| CP-008 | Nav | Custom OS-consistent scrollbars (V+H) | Always-visible draggable scrollbars sized to used extent | **Todo** | Render | M | **P0** | Overlay DOM scrollbars; thumb from `session_used_bounds` (UX-04) |
| CP-009 | Nav | Fluid smooth scroll | Pixel-smooth wheel scroll | **Done** | Render | — | P1 | — |
| CP-010 | Nav | Freeze-panes navigation + render | Frozen rows/cols stay pinned while scrolling | **Partial** | Model, Round-trip, **Edit ✗, Render ✗** | L | P1 | `SheetView` modelled; need edit op + editor split-viewport render (UX-27) |
| CP-011 | Nav | Zoom (50–200%) | Scale grid; persists per sheet | **Todo** | Render | M | P2 | Optional model `SheetView.zoom`; layout scale factor |
| CP-012 | Nav | Scroll-lock / split windows | Independent scroll regions | **Todo** | Render | L | P2 | Beyond freeze; low value |

### Selection

| ID | Area | Item | Reference behavior | Status | Fidelity dims | Effort | Priority | Model/API changes |
|----|------|------|--------------------|--------|---------------|--------|----------|-------------------|
| CP-013 | Selection | Cell / drag-range / shift-extend | Click, drag, shift-click select ranges | **Done** | Edit | — | P0 | — |
| CP-014 | Selection | Whole row/col via header, whole sheet via corner | Header click selects line; corner selects all | **Done** | Edit | — | P0 | `selKind` model |
| CP-015 | Selection | No scroll-jump on header/large select | Viewport stays put | **Done** | Edit | — | P0 | UX-01 fixed |
| CP-016 | Selection | Header drag to select a band (A:C, 2:5) | Drag across headers selects contiguous cols/rows | **Partial** | Edit | S | P1 | `selectColumn/Row(_,exp)` exists; wire header mousemove drag-extend |
| CP-017 | Selection | Multi-range (Ctrl+click / Ctrl+drag) | Discontiguous selection; ops apply to union | **Todo** | Edit | L | P1 | `selKind:"multi"` list of rects; every format/clear/copy path iterates rects (UX-24) |
| CP-018 | Selection | Ctrl+Shift+Arrow extend-to-edge | Extend selection to data block edge | **Todo** | Edit | S | P1 | Pairs with CP-003 |
| CP-019 | Selection | Ctrl+Space / Shift+Space (select col/row of active) | Keyboard row/col select | **Todo** | Edit | S | P2 | Editor-only |
| CP-020 | Selection | Selection persists across sheet switch | Each sheet remembers selection | **Todo** | Edit | S | P2 | Per-sheet selection state (currently `resetView` zeroes it) |

### Sizing

| ID | Area | Item | Reference behavior | Status | Fidelity dims | Effort | Priority | Model/API changes |
|----|------|------|--------------------|--------|---------------|--------|----------|-------------------|
| CP-021 | Sizing | Drag-resize column/row | Live drag with preview, undoable | **Done** | Edit, Render | — | P1 | — |
| CP-022 | Sizing | Double-click col boundary = autofit width | Fit to widest cell | **Done** | Edit | — | P1 | measures widest via canvas |
| CP-023 | Sizing | Resize applies to all selected lines | Resizing one of N selected resizes all | **Done** | Edit | — | P1 | band/all scope |
| CP-024 | Sizing | Autofit **row height** to content | Double-click row boundary fits tallest cell | **Partial** | Edit, Render | M | P1 | Currently resets to default; needs measured wrapped height (depends on CP-034 wrap) |
| CP-025 | Sizing | Hide / unhide rows & columns | Size-0 lines, shown collapsed; unhide restores | **Todo** | Model, Round-trip, Edit, Render | M | P1 | `AxisSizing` hidden set or `hidden` flag; import `<col hidden>`/`<row hidden>`; editor skip + marker (UX-26) |
| CP-026 | Sizing | Outline / group rows & columns (+/- collapse) | Grouping bars with collapse | **Todo** | Model, Round-trip, Edit, Render | L | P2 | Model outline levels on axis; import `outlineLevel` |
| CP-027 | Sizing | Default width/height set for whole sheet | Set sheet-wide default | **Done** | Model, Edit | — | P2 | `AxisSizing.default` + `session_set_all_*` |

### Structural operations

| ID | Area | Item | Reference behavior | Status | Fidelity dims | Effort | Priority | Model/API changes |
|----|------|------|--------------------|--------|---------------|--------|----------|-------------------|
| CP-028 | Structure | Insert / delete rows & columns | Grid shifts; **formula refs rewrite**; ranges/merges adjust | **Todo** | Model, Edit, **Calc**, Render | L | **P0** | `EditOperation::{Insert,Delete}{Rows,Columns}` (E-002, designed doc 24); AST reference shift/clamp; WASM + context-menu UI (UX-17) |
| CP-029 | Structure | Insert / delete cells with shift-direction | Shift cells up/left/down/right | **Todo** | Model, Edit, Calc | M | P1 | Builds on CP-028 op algebra |
| CP-030 | Structure | Move / cut rows/columns (drag or cut-paste) | Relocate lines, refs follow | **Todo** | Model, Edit, Calc | L | P1 | Move op = delete+insert with ref-preserving rewrite |
| CP-031 | Structure | Clear formats only / clear contents only | Separate "clear formats" vs "clear all" | **Partial** | Edit | S | P1 | `session_clear_range` clears everything; add `session_clear_formats` (style→None, keep value) |
| CP-032 | Structure | Insert/delete affects merges, defined names, sizing | All structural metadata shifts consistently | **Todo** | Model, Edit, Calc | M | P1 | Part of CP-028 correctness surface |

### Editing

| ID | Area | Item | Reference behavior | Status | Fidelity dims | Effort | Priority | Model/API changes |
|----|------|------|--------------------|--------|---------------|--------|----------|-------------------|
| CP-033 | Editing | Inline + formula-bar edit, F2, type-to-replace | Standard cell entry | **Done** | Edit | — | P0 | — |
| CP-034 | Editing | Text overflow into adjacent empty cells | Long unwrapped text spills over empty neighbors | **Todo** | Render | M | **P0** | Render change: measure text, paint across empty neighbors; engine must report neighbor-empty (UX-19) |
| CP-035 | Editing | Wrap text (+ auto row height) | Wrapped multi-line cells | **Todo** | Model, Round-trip, Edit, Render | M | P1 | `Style.wrap_text: bool`; import `wrapText`; multi-line canvas draw; feeds CP-024 |
| CP-036 | Editing | Autofill / drag-fill handle (series, copy, patterns) | Drag fill handle extends series/formulas | **Todo** | Edit, Calc | L | P1 | `casual-calc-selection` fill engine (E-003); series detection + ref-adjust (UX-25) |
| CP-037 | Editing | Copy/paste **values** (TSV) | Clipboard interchange | **Done** | Edit | — | P1 | `session_copy_tsv`/`paste_tsv` |
| CP-038 | Editing | Copy/paste **formats** | Paste carries fills/fonts/borders/numfmt | **Todo** | Edit | M | P1 | Internal clipboard model (styles), not just text; HTML clipboard for cross-app |
| CP-039 | Editing | Copy/paste **formulas** with ref adjustment | Relative refs shift on paste | **Todo** | Edit, Calc | M | P1 | Copy currently exports *display text* only; need formula-aware clipboard + ref-rewrite |
| CP-040 | Editing | Cut (Ctrl+X) + move semantics | Cut marks source, paste clears it, refs follow | **Todo** | Edit, Calc | M | P1 | Cut = copy + source clear on paste; ref move rewrite |
| CP-041 | Editing | Paste special (values/formats/transpose/ops) | Dialog: paste subset or arithmetic | **Todo** | Edit | M | P2 | Depends on CP-038/039 clipboard model |
| CP-042 | Editing | Multi-line entry (Alt+Enter) | Hard line breaks within a cell | **Todo** | Model, Round-trip, Render | S | P2 | Newline in string + wrap render |
| CP-043 | Editing | Formula editing aids (autocomplete, colored refs, range highlight, arg tooltip) | Live function/name autocomplete, ref highlighting | **Todo** | Edit | L | P1 | Editor tokenizes input via `casual-calc-formula`; overlay highlights |
| CP-044 | Editing | In-cell reference picking (click cell while editing formula) | Clicking inserts a ref into the formula | **Todo** | Edit | M | P1 | Editor: route grid clicks into edit buffer |
| CP-045 | Editing | Undo / redo | Full history | **Done** | Edit | — | P0 | `History` (E-001) |
| CP-046 | Editing | Auto-complete from column values (text) | Suggests existing column entries | **Todo** | Edit | S | P2 | Editor scan of column strings |

### Formatting

| ID | Area | Item | Reference behavior | Status | Fidelity dims | Effort | Priority | Model/API changes |
|----|------|------|--------------------|--------|---------------|--------|----------|-------------------|
| CP-047 | Format | Bold / italic / underline | Toggles + Ctrl+B/I/U | **Done** | Model, Round-trip, Edit, Render(~) | — | P1 | — |
| CP-048 | Format | Strikethrough | Toggle | **Todo** | Model, Round-trip, Edit, Render | S | P2 | `Style.strike: bool`; import/export `<font><strike>` (UX-43) |
| CP-049 | Format | Font family | Family picker per cell | **Todo** | Model, Round-trip, Edit, Render | M | **P0** | `Style.font_name: Option<String>`; import/export `<font><name>`; canvas font (UX-16) |
| CP-050 | Format | Font size | Size picker per cell | **Todo** | Model, Round-trip, Edit, Render | M | **P0** | `Style.font_size: Option<f32>`; import/export `<sz>`; affects row autofit (UX-16) |
| CP-051 | Format | Font color | Text color picker | **Done** | Model, Round-trip, Edit, Render | — | P1 | `font_color` |
| CP-052 | Format | Fill color (solid) | Cell background | **Done** | Model, Round-trip, Edit, Render | — | P1 | `fill_color` |
| CP-053 | Format | Fill patterns / gradients | Patterned/gradient fills | **Todo** | Model, Round-trip, Render | M | P2 | `Style.fill` beyond solid; import `<patternFill>`/`<gradientFill>` |
| CP-054 | Format | Horizontal align L/C/R | Align buttons | **Done** | Model, Round-trip, Edit, Render | — | P1 | `HAlign` |
| CP-055 | Format | Horizontal justify / fill / centerAcross | Extra horizontal modes | **Partial** | Model, Render | S | P2 | `HAlign` maps centerContinuous→Center on import; no justify/fill; extend enum |
| CP-056 | Format | Vertical align (top/middle/bottom) | Vertical align buttons | **Todo** | Model, Round-trip, Edit, Render | S | P1 | `Style.valign: Option<VAlign>`; import/export `<alignment vertical>`; render baseline (UX-20) |
| CP-057 | Format | Indent | Increase/decrease indent | **Todo** | Model, Round-trip, Edit, Render | S | P2 | `Style.indent: u8`; `<alignment indent>` |
| CP-058 | Format | Text rotation | Angled/vertical text | **Todo** | Model, Round-trip, Render | M | P2 | `Style.rotation`; `<alignment textRotation>`; canvas transform |
| CP-059 | Format | Number formats (menu + $ + %) | Preset + custom format codes | **Done** | Model, Round-trip, Edit, Render | — | P1 | numfmt interpreter |
| CP-060 | Format | Number-format negative/zero/text sections + colors | `#,##0;[Red](#,##0);"-"` semantics | **Partial** | Render | M | P1 | P1C-002 deferred multi-section + color; extend numfmt interpreter |
| CP-061 | Format | More number-format presets + custom-code dialog | Accounting, scientific, fraction, date/time variants, custom entry | **Partial** | Edit, Render | S | P1 | Model already stores arbitrary code; add UI + interpreter coverage |
| CP-062 | Format | Borders: all / outer / clear | Preset borders | **Done** | Model, Round-trip, Edit, Render(~) | — | P1 | — |
| CP-063 | Format | Borders: per-side + line style + color picker | Choose edge, weight, dash, color | **Partial** | Edit, Render | M | P1 | Model fully supports per-edge style+color; expose granular WASM + border palette/style UI |
| CP-064 | Format | Merge cells (merge/unmerge, merge-across) | Merge selection into one cell | **Partial** | Model, Round-trip, **Edit ✗, Render ✗** | L | **P0** | `Sheet.merges` modelled + round-trips; need `Merge/Unmerge` edit op, merged layout/render, anchor-value semantics, toolbar (UX-21) |
| CP-065 | Format | Conditional formatting | Rules → dynamic style (data bars, scales, cell rules) | **Todo** | Model, Round-trip, Edit, Render, Calc | L | P2 | New model `ConditionalFormat`; rule eval; import `<conditionalFormatting>` (UX-40) |
| CP-066 | Format | Clear formatting (format-only) | Removes format, keeps value | **Todo** | Edit | S | P1 | Same as CP-031 `session_clear_formats` |
| CP-067 | Format | Format painter | Copy format, brush onto range | **Todo** | Edit | S | P2 | Editor: capture style id, apply to target; needs CP-038 style clipboard |
| CP-068 | Format | Cell styles / named styles (Heading, Good/Bad) | Named reusable style presets | **Todo** | Model, Round-trip, Edit | M | P2 | Named-style table (OOXML `cellStyleXfs`) |
| CP-069 | Format | Theme colors / palette awareness | Colors resolve against workbook theme | **Todo** | Model, Round-trip, Render | M | P2 | Theme part modelled (ledger shows theme table); resolve indexed/theme colors |

### Formulas & calculation

| ID | Area | Item | Reference behavior | Status | Fidelity dims | Effort | Priority | Model/API changes |
|----|------|------|--------------------|--------|---------------|--------|----------|-------------------|
| CP-070 | Calc | Arithmetic / comparison / concat / unary | Operator evaluation | **Done** | Calc | — | P1 | — |
| CP-071 | Calc | Cell + range refs, cross-sheet, `$`-absolute | `A1`, `A1:B9`, `Sheet2!$B$7` | **Done** | Model, Calc | — | P1 | parser + eval |
| CP-072 | Calc | Defined names in formulas | Named ranges resolve in calc | **Partial** | Model, Round-trip, Calc, **Edit ✗** | M | P1 | Resolves in eval + round-trips; no create/edit/manage UI or op |
| CP-073 | Calc | Function library breadth | Excel/Sheets ship 400+ functions | **Partial (8 fns)** | Calc | L | **P0** | Only SUM/AVERAGE/COUNT/MIN/MAX/IF/ABS/ROUND; big gap (P2-003) |
| CP-074 | Calc | Aggregation family (COUNTA, COUNTIF(S), SUMIF(S), AVERAGEIF(S)) | Conditional aggregates | **Todo** | Calc | M | **P0** | Extend `functions.rs`; criteria parsing |
| CP-075 | Calc | Lookup family (VLOOKUP, HLOOKUP, INDEX, MATCH, XLOOKUP, OFFSET, INDIRECT) | Table lookups | **Todo** | Calc | L | P1 | Extend eval; INDIRECT/OFFSET need ref-returning eval path |
| CP-076 | Calc | Text functions (LEFT/RIGHT/MID/LEN/TRIM/UPPER/LOWER/CONCAT/TEXTJOIN/SUBSTITUTE/TEXT) | String manipulation | **Todo** | Calc | M | P1 | Extend eval; TEXT reuses numfmt interpreter |
| CP-077 | Calc | Date/time functions (TODAY/NOW/DATE/YEAR/MONTH/DAY/EDATE/NETWORKDAYS) | Date math | **Todo** | Calc | M | P1 | Serial-date helpers exist in numfmt; NOW/TODAY are volatile (needs CP-082) |
| CP-078 | Calc | Logical (AND/OR/NOT/XOR/IFERROR/IFNA/IFS/SWITCH) | Boolean + error handling | **Todo** | Calc | S | **P0** | Extend eval; IFERROR core for real sheets |
| CP-079 | Calc | Math/stat (SUMPRODUCT/MOD/POWER/SQRT/ROUNDUP/ROUNDDOWN/INT/RAND/MEDIAN/STDEV) | Numeric functions | **Todo** | Calc | M | P1 | Extend eval; RAND volatile |
| CP-080 | Calc | Incremental recalc (dirty graph, <50 ms) | Only affected cells recompute | **Todo** | Calc | L | P1 | Full recalc only today; dependency graph (P2-002) |
| CP-081 | Calc | Dynamic arrays / spill (FILTER/SORT/UNIQUE/SEQUENCE, spill ranges) | Formulas spill into neighbors | **Todo** | Model, Edit, Render, Calc | L | P2 | `CellFlags` SPILL_ANCHOR/CHILD reserved; needs array eval + spill layout (P2-004) |
| CP-082 | Calc | Volatile functions + iterative calc | NOW/RAND recompute; circular-with-iteration | **Todo** | Calc | M | P2 | Volatile set + recalc trigger; iterative settings (P2-004) |
| CP-083 | Calc | Array formulas (Ctrl+Shift+Enter, legacy CSE) | Range-entered array formulas | **Todo** | Model, Edit, Calc | M | P2 | Array formula range in model; eval broadcast |
| CP-084 | Calc | Error values + propagation | `#REF!`,`#VALUE!`,`#DIV/0!` etc propagate | **Done** | Model, Calc | — | P1 | `ErrorValue` |
| CP-085 | Calc | Formula error surfacing UI (trace, error tooltip, green corner) | Editor flags/explains errors | **Todo** | Render | S | P2 | Editor decoration on error cells |

### Data features

| ID | Area | Item | Reference behavior | Status | Fidelity dims | Effort | Priority | Model/API changes |
|----|------|------|--------------------|--------|---------------|--------|----------|-------------------|
| CP-086 | Data | Sort (range / by column, multi-key) | Reorder rows by key(s) | **Done (single-key)** | Edit, Calc | M | P1 | `session_sort_range`: row permutation moving values+styles+flags, relative formula refs shifted by row delta (per-row formulas re-anchor); blanks last, numbers<text. Toolbar + context-menu controls. Multi-key sort still open |
| CP-087 | Data | Filter / autofilter | Header dropdowns hide non-matching rows | **Todo** | Model, Round-trip, Edit, Render | L | P1 | `AutoFilter` model; import `<autoFilter>`; hidden-row render (CP-025) |
| CP-088 | Data | Tables (structured ranges + refs) | Named table, banded rows, `Table[Col]` refs | **Todo** | Model, Round-trip, Edit, Render, Calc | L | P2 | Table model; import `<table>`; structured-ref parsing (UX-41) |
| CP-089 | Data | Data validation (dropdowns, ranges, rules) | Restrict input; in-cell dropdown | **Todo** | Model, Round-trip, Edit, Render | L | P2 | `DataValidation` model; import `<dataValidation>` (UX-45) |
| CP-090 | Data | Find & replace (+ regex, scope, in-formula) | Locate/replace across sheet/workbook | **Todo** | Edit | M | P1 | WASM search over `CellStore`; editor dialog + highlight (UX-28) |
| CP-091 | Data | Named-range manager (create/edit/delete/scope) | Manage defined names | **Todo** | Model, Round-trip, Edit | M | P1 | Names modelled; add edit ops + UI (pairs CP-072) |
| CP-092 | Data | Remove duplicates | Dedupe rows by columns | **Todo** | Edit | S | P2 | Selection op |
| CP-093 | Data | Text-to-columns (split) | Split a column by delimiter | **Todo** | Edit | S | P2 | Reuse `casual-calc-io` delimiter logic in-sheet |
| CP-094 | Data | Comments / notes | Threaded comments + legacy notes | **Todo** | Model, Round-trip, Edit, Render | M | P2 | Note table modelled in schema; import `<comments>`; render marker (UX-44) |
| CP-095 | Data | Hyperlinks | Clickable cell links | **Todo** | Model, Round-trip, Edit, Render | S | P2 | `Style`/cell hyperlink; import `<hyperlinks>` |
| CP-096 | Data | Charts | Insert/edit charts | **Todo (preserve-only)** | Model, Round-trip, Edit, Render, Calc | L | P2 | P3 scope; preserve on round-trip only |
| CP-097 | Data | Pivot tables | Pivot from source range | **Todo (preserve-only)** | all | L | P2 | P3 scope |
| CP-098 | Data | Images / shapes / drawings | Embedded objects | **Todo** | Model, Round-trip, Render | L | P2 | Drawing part; preserve-first |

### Chrome / UX

| ID | Area | Item | Reference behavior | Status | Fidelity dims | Effort | Priority | Model/API changes |
|----|------|------|--------------------|--------|---------------|--------|----------|-------------------|
| CP-099 | Chrome | Icon toolbar | Grouped formatting toolbar | **Done** | Render | — | P1 | — |
| CP-100 | Chrome | Right-click **cell** context menu | Cut/copy/paste/clear, insert/delete row-col, format | **Todo** | Edit | M | **P0** | Editor menu (reuse sheet-tab menu pattern); pairs CP-028 (UX-18) |
| CP-101 | Chrome | Right-click **row/column header** menu | Insert/delete/hide/resize/clear on the line | **Todo** | Edit | S | P1 | Pairs CP-028/CP-025 |
| CP-102 | Chrome | Sheet-tab context menu (rename/dup/delete) | Tab right-click actions | **Done** | Edit | — | P1 | — |
| CP-103 | Chrome | Sheet-tab reorder (drag) | Drag to reorder tabs | **Todo** | Edit | S | P1 | `session_move_sheet(from,to)` (UX-29) |
| CP-104 | Chrome | Sheet-tab color | Color a tab | **Todo** | Model, Round-trip, Edit, Render | S | P2 | `Sheet.tab_color`; import `<sheetPr><tabColor>` |
| CP-105 | Chrome | Hide / unhide / very-hidden sheets | Sheet visibility state | **Todo** | Model, Round-trip, Edit | S | P2 | `Sheet.visibility`; import `<sheet state>` |
| CP-106 | Chrome | Status bar aggregates (sum/avg/count/min/max) | Live selection stats | **Todo** | Render | S | **P0** | Editor computes over selection via `session_cells` (UX-22) |
| CP-107 | Chrome | Name box (see CP-006) | Active-cell ref is editable | **Todo** | Edit | S | P1 | `cell-ref` is a read-only `<span>` today |
| CP-108 | Chrome | Formula bar: multi-line expand, fx button, insert-function | Resizable formula editor + function picker | **Partial** | Edit | M | P2 | Single-line input exists; add expand + fx dialog |
| CP-109 | Chrome | Gridline / heading show-hide toggles | View toggles | **Todo** | Render | S | P2 | Editor view flags (+ round-trip `<sheetView>` gridlines) |
| CP-110 | Chrome | Light/dark + accent theming | Themed chrome | **Done** | Render | — | P2 | settings panel |
| CP-111 | Chrome | Keyboard-shortcut parity (Excel-flavored) | Consistent, discoverable shortcuts | **Partial** | Edit | M | P1 | Has B/I/U/Z/Y/S/C/V/A + align; missing X, F, nav (CP-003/04/05), F4, etc |
| CP-112 | Chrome | Print / page setup / print area | Page layout + export to print | **Todo** | Model, Round-trip, Render | L | P2 | Page-setup model; import `<pageSetup>`/`<printOptions>` |
| CP-113 | Chrome | Accessibility (ARIA grid, screen-reader nav) | Announce active cell/selection | **Partial** | Render | M | P2 | Canvas grid is opaque to AT; add live region / a11y layer |

### Collaboration

| ID | Area | Item | Reference behavior | Status | Fidelity dims | Effort | Priority | Model/API changes |
|----|------|------|--------------------|--------|---------------|--------|----------|-------------------|
| CP-114 | Collab | Real-time co-editing | Concurrent multi-user edits merge | **Todo** | Edit | L | P2 | Collab seam designed (doc 24); needs op transport/CRDT + server |
| CP-115 | Collab | Presence / remote cursors | See others' selections | **Todo** | Render | M | P2 | Depends CP-114 |
| CP-116 | Collab | Comment threads / @mentions | Discussion on cells | **Todo** | Model, Edit | M | P2 | Depends CP-094 |
| CP-117 | Collab | Version history / restore | Named revisions, restore | **Todo** | — | L | P2 | Server-side; snapshot I/O exists in model |
| CP-118 | Collab | Sharing / permissions | Access control | **Todo** | — | L | P2 | Host/server concern |

### Import / export fidelity

| ID | Area | Item | Reference behavior | Status | Fidelity dims | Effort | Priority | Model/API changes |
|----|------|------|--------------------|--------|---------------|--------|----------|-------------------|
| CP-119 | Fidelity | `.xlsx` open/save (values/formulas/numfmt/style) | Faithful round-trip | **Done** | Model, Round-trip | — | P0 | semantic fixed point (P1B-001) |
| CP-120 | Fidelity | Fonts / fills / borders round-trip | Preserved on save | **Done** | Model, Round-trip | — | P1 | P1A-003b/c |
| CP-121 | Fidelity | Merged ranges **render on open** | Opened merges display merged | **Partial** | Model, Round-trip, **Render ✗, Edit ✗** | M | **P0** | Modelled + round-trips but editor ignores `Sheet.merges`; see CP-064 |
| CP-122 | Fidelity | Frozen panes **render on open** | Opened freeze displays pinned | **Partial** | Model, Round-trip, **Render ✗, Edit ✗** | M | P1 | See CP-010 |
| CP-123 | Fidelity | CSV / TSV / PSV open+save | Delimited interchange | **Done** | Round-trip | — | P1 | IO-001 |
| CP-124 | Fidelity | ODS open/save | OpenDocument spreadsheet | **Todo** | Model, Round-trip | L | P2 | `casual-calc-ods` is a 4-line shell |
| CP-125 | Fidelity | Byte-identical repackage (unedited) | Reopen unchanged file byte-for-byte | **Todo** | Round-trip | L | P2 | Retention mode (P1B-002); needs retained source (P1A-006) |
| CP-126 | Fidelity | Rich text runs (mixed format in one cell) | Per-character formatting | **Todo** | Model, Round-trip, Edit, Render | L | P2 | `CellValue` has no run model; add rich-text string |
| CP-127 | Fidelity | Hyperlinks round-trip | Preserved links | **Todo** | Model, Round-trip | S | P2 | Pairs CP-095 |
| CP-128 | Fidelity | Conditional formatting round-trip | Preserved CF rules | **Todo** | Model, Round-trip | M | P2 | Pairs CP-065 |
| CP-129 | Fidelity | Data validation round-trip | Preserved rules | **Todo** | Model, Round-trip | M | P2 | Pairs CP-089 |
| CP-130 | Fidelity | Comments / notes round-trip | Preserved comments | **Todo** | Model, Round-trip | M | P2 | Pairs CP-094 |
| CP-131 | Fidelity | Charts / images / pivots preserve | Survive round-trip unmodified | **Todo** | Round-trip | L | P2 | Opaque-part retention (P1A-006) |
| CP-132 | Fidelity | Print settings / page setup round-trip | Preserved page layout | **Todo** | Model, Round-trip | M | P2 | Pairs CP-112 |
| CP-133 | Fidelity | Proper part discovery (content-types/rels, not conventional paths) | Robust to non-standard packaging | **Partial** | Model, Round-trip | M | P1 | sharedStrings found by convention today (P1A-005) |
| CP-134 | Fidelity | Number-format multi-section + color-in-format | Negative/text sections + `[Red]` | **Partial** | Render | M | P1 | See CP-060 |
| CP-135 | Fidelity | PNG landing-preview: borders/fills/glyphs | Preview matches oracle | **Partial** | Render | M | P2 | Render backend has fills/gridlines; borders + glyph text pending (P1D-002, UX-46) |

### Performance

| ID | Area | Item | Reference behavior | Status | Fidelity dims | Effort | Priority | Model/API changes |
|----|------|------|--------------------|--------|---------------|--------|----------|-------------------|
| CP-136 | Perf | O(visible) layout / virtualization | Only visible cells laid out | **Done** | Render | — | P0 | P1C-001 |
| CP-137 | Perf | 1M-cell workbook target | Smooth at capacity | **Partial** | Render | L | P1 | Designed (doc 30); not yet load-tested in editor |
| CP-138 | Perf | 60 fps scroll (incremental repaint) | No full redraw per frame | **Partial** | Render | M | P1 | Editor redraws whole canvas each frame; dirty-region repaint (P1D-003) |
| CP-139 | Perf | Incremental recalc <50 ms | Sub-frame recompute on edit | **Todo** | Calc | L | P1 | See CP-080 (P2-002) |
| CP-140 | Perf | Off-main-thread engine (worker) | WASM calc off UI thread | **Todo** | — | L | P2 | Editor runs WASM on main thread; move to Web Worker |
| CP-141 | Perf | Large paste / fill batching | Bulk edits stay responsive | **Partial** | Edit | M | P2 | Paste uses one `Batch`; very large pastes unbounded |

## Recommended build sequence

Ordered by dependency and by *broken-basics-first*. Each step names the gate it
unblocks. Rationale is dependency-driven: model fields precede their UI; the
structural op algebra precedes everything that reshapes the grid; the clipboard
model precedes paste-special/format-painter; the calc dependency graph precedes
any serious function work being usable at scale.

1. **Custom scrollbars (CP-008)** — the most visible P0; a canvas grid with no
   scrollbars does not read as a real sheet. Editor-only, no model change.
2. **Text overflow into empty neighbors (CP-034)** — P0 render gap; every sheet
   overflows unwrapped text. Render-only, unblocks readable data.
3. **Status-bar aggregates (CP-106)** — cheap, high-signal P0; pure editor,
   reuses `session_cells`.
4. **Font family + size model fields (CP-049, CP-050)** — add `Style.font_name`
   / `font_size` with import/export/render *before* any font UI; this is the
   canonical "model field before font UI" dependency. Also unblocks correct row
   autofit (CP-024) and wrap height (CP-035).
5. **Structural insert/delete rows & columns (CP-028)** — the P0 correctness
   centerpiece (E-002, designed in doc 24). Requires the AST reference
   shift/clamp algebra; once landed it unblocks cell-shift (CP-029), move
   (CP-030), and correct adjustment of merges/names/sizing (CP-032). Build the
   op + WASM here; wire UI in step 6.
6. **Cell + header right-click context menus (CP-100, CP-101)** — the delivery
   surface for CP-028/CP-025/CP-031; pairs structurally with insert/delete.
7. **Merge cells: edit + render (CP-064 / CP-121)** — P0 fidelity trap: files
   with merges already round-trip but display wrong. Needs merged-cell layout in
   the editor + a `Merge/Unmerge` op. Do after CP-028 so merges shift correctly.
8. **Vertical alignment (CP-056) + wrap text (CP-035)** — small model additions
   (`valign`, `wrap_text`) that complete the everyday formatting set and feed
   row autofit (CP-024).
9. **Core calc function expansion (CP-078 IFERROR/AND/OR, CP-074 COUNTIF/SUMIF,
   CP-076 text, CP-077 date)** — the 8-function library is the biggest calc gap;
   prioritize logical + conditional-aggregate + text/date, since these are what
   real workbooks use. Sequence the **incremental dependency graph (CP-080)**
   alongside so the growing library stays within the <50 ms budget.
10. **Formula-aware clipboard + Ctrl+X (CP-038, CP-039, CP-040)** — replace the
    display-text-only copy with a real style/formula clipboard that adjusts
    relative refs; this unblocks paste-special (CP-041) and format painter
    (CP-067), and depends on the CP-028 reference-rewrite algebra.

**Deferred but high-value (post-top-10):** name box + go-to (CP-006/007),
find & replace (CP-090), freeze-panes render (CP-010/122), autofill handle
(CP-036), multi-range selection (CP-017), Ctrl+Arrow navigation (CP-003), named-
range manager (CP-091), sort/filter (CP-086/087). **Explicitly P2/P3:**
conditional formatting, tables, data validation, charts/pivots, ODS,
collaboration, byte-identical retention.

## Maintenance

This is the *complete* competitive surface; keep IDs stable and never delete a
row (mark superseded items). When an item ships, flip its status here and update
the matching row in [45](45-EDITOR-PARITY-TRACKER.md) and the construct's row in
[33-FIDELITY-LEDGER.md](33-FIDELITY-LEDGER.md); the three must not drift.
