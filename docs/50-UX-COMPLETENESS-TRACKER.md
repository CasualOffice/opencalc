# 50 — UX Completeness Tracker

_Generated 2026-08-08 from a six-domain parallel audit of the editor against
Microsoft Excel + Google Sheets standard UX. Every "done" feature was
re-examined for actual completeness and standard behavior. Work items are
completed **one at a time to full Excel/Sheets parity** (implement → verify
in-browser → commit) before starting the next._

Status: 🔴 Todo · 🟡 Partial-in-progress · ✅ Done. Severity: **P0** broken /
data-loss / wrong-target · **P1** expected-daily · **P2** polish.

Sources: audits of selection/navigation, editing/clipboard, formatting,
structure/data, **formula-authoring**, and chrome/fidelity. Code citations are in
each audit; this file is the durable backlog.

---

## Already completed this session ✅

- Font pipeline (opendoc parity): substitution table (`casual-calc-layout`),
  bundled faces + skrifa glyph rendering (`casual-calc-render`), per-glyph
  coverage fallback, canvas `@font-face` + `font_css_stack`, per-workbook default
  font, canvas default size aligned to 11pt.
- Toolbar: menu bar (File/Edit/View/…); progressive group-collapse (no
  scrollbar/wrap); phantom collapsed-button fix; font/size auto-populate.
- Freeze: prominent **draggable** divider (change/remove by drag); header resize
  still works.
- Engine correctness: wildcard COUNTIF/SUMIF/AVERAGEIF(S); non-finite → #NUM!;
  missing builtin number-format IDs; Cmd+Shift+Z redo.
- Font picker: custom searchable combobox (UX-P07) — a native `datalist` is
  filtered by the input's own text, so the auto-populated box offered one font.
- **Import fidelity (round 2)**: shared formulas (`<f t="shared">`) expand from
  their master instead of importing as bare cached constants; theme + tint and
  legacy indexed colours resolve (the form every Excel built-in cell style
  uses); multi-area `sqref` keeps every area; comments bind through the OPC
  relationship graph rather than by part numbering.
- **Import fidelity**: `<col customWidth="true">` (LibreOffice / POI / ExcelJS
  spelling) no longer discards every column width; `customWidth` no longer gates
  whether a width is honored at all; `hidden`/`collapsed`/`wrapText` likewise;
  `<b val="0"/>` no longer reads as bold ON. The editor's auto row-height no
  longer overrides a height the workbook set, no longer un-hides hidden rows,
  and no longer inflates every styled row by 25%.
- **Text overflow control**: overflow / wrap / **clip** as one three-way choice
  on the toolbar and in Format ▸ Text overflow. Clip is an engine-side view
  setting (SpreadsheetML has no attribute for it — Excel always spills), noted
  in the fidelity ledger as not surviving a save.
- **Text overflow**: spill stays inside its own pane (a frozen cell can't borrow
  the scrolling body's columns, and vice versa), and a number too wide for its
  column renders `#######` instead of spilling or being clipped into a shorter,
  plausible-looking wrong number.
- **Frozen-pane painting**: selection spans are pane-relative (a body range no
  longer paints a sliding strip into the frozen band, and no longer vanishes
  when its start scrolls off); the selection outline / marching ants / fill
  preview clip per pane; a merge straddling a freeze line is painted as one
  slice per pane instead of one rect built from a pinned edge and a scrolling
  edge (which went negative-width and flipped back over the frozen band); merge
  geometry reads the drawn grid, so auto-row-height no longer makes merges drift
  from their own cells as you scroll. The in-cell editor follows its cell while
  scrolling and spans a merged block; a selection containing a merge unmerges it.

---

## P0 — broken / data-loss / wrong-target

| ID | Item | Domain | Status | Fix |
|----|------|--------|--------|-----|
| UX-F01 | **Formula bar is a dumb field** — no autocomplete, no ref-picking, no highlight; all formula intelligence is bound to the inline editor only | Formula | ✅ | One edit session keyed to `editSurface` (the in-cell overlay *or* the formula bar): autocomplete, click/drag reference picking, the invalid-formula outline, commit and revert all read the active surface. Text mirrors between the two, and focusing the bar mid-edit hands the same edit over. Reference highlighting (UX-F03) still to come |
| UX-M01 | **Merge silently hides data** — non-anchor values not cleared, no confirm; reappear on unmerge, round-trip keeps hidden values | Format | ✅ | `session_merge_hidden_count()` counts what a merge would bury; if any, a confirm names the number before anything happens. `session_merge_cells_discarding()` then merges and clears the covered values (keeping their styling) in one batch, so it is one undo step and unmerging no longer resurrects them |
| UX-N01 | **Scientific format renders garbage** (`0.00E+00` → `12345.00E+`) | Format | ✅ | Exponential branch in `numfmt.rs`: mantissa decimals from the pattern, exponent zero-padded to its placeholder count, `+` only when the pattern asks. Handles the carry when rounding pushes the mantissa to 10 (`9999` → `1.0E+04`), and skips an `E` inside a literal |
| UX-N02 | **`[Red]`/color codes in number formats dropped** (negatives not red) | Format | ✅ | `format_number_colored()` returns the chosen section's colour (Excel's eight named ones; `[Color n]` deliberately not guessed at), threaded through `display_color()` into `session_cells`' `fc`, where it overrides the style's font colour as in Excel. Verified by opening a file whose negatives carry `[Red]` |
| UX-N03 | **Text (`@`) format inert**; text values bypass number formatting | Format | ✅ | A cell formatted `@` keeps what was typed as text (`007` stays `007`, not `7`) — coercing it was a silent edit of the user's input. `format_text()` applies a code's text section to string values (`@" kg"`, and the 4th section of a multi-section code), wired into `display_text` |
| UX-D01 | **Sort includes the header row + single-key only** — corrupts tables | Data | ✅ | `session_sort_range_multi()` takes N key columns each with its own direction (later keys break ties in earlier ones); `looksLikeHeader()` defaults the "my data has a header row" checkbox by Excel's rule of thumb, and a header row is excluded by starting the sort one row lower. Custom-sort dialog (3 keys, columns named by their heading) on the toolbar, Data menu and cell menu; the one-click A→Z / Z→A keep working and now also spare the header |
| UX-C01 | **Right-click on row/col header** shows generic cell menu and acts on the *previous* selection (wrong target) | Chrome | ✅ | The contextmenu handler detects a header hit (cellAt returns null there, which is why it fell through), selects that band unless it is already inside a row/column selection, and opens a header menu whose verbs name the band: insert before/after, delete, clear, exact size, autofit, hide/unhide. Corner → select all |
| UX-E01 | **Alt+Enter multi-line entry impossible** (inline is `<input>`) | Editing | ✅ | `#inline-edit` is a `<textarea>` that grows past the cell while editing; Alt+Enter inserts a break (in-cell only — the formula bar is one line), Enter still commits. `wrapLines()` now treats `\n` as a hard break instead of letting `\s+` swallow it, and committing a multi-line value turns wrap on for that cell, as Excel does |

## P1 — expected daily

| ID | Item | Domain | Status | Fix |
|----|------|--------|--------|-----|
| UX-F02 | Arg/signature tooltip inside `()` with active arg bolded, persists | Formula | ✅ | `callAtCaret()` walks the text to the caret tracking a call stack, so nested calls resolve to the innermost and commas inside string literals or nested parens don't advance the argument. Yields to the name list while that is open; follows the caret on arrow/Home/End/click; a trailing `…` argument stays active past the named ones |
| UX-F03 | Colored reference highlighting (formula tokens + on-grid range-finder boxes) | Formula | 🟡 | **On-grid boxes done**: `formula_ref_spans()` (engine-side scanner, so a function name is never mistaken for a reference and string literals are skipped) drives one coloured outline per reference in `draw()`, clipped per pane. **Remaining**: tinting the reference *tokens inside the text* to match, which needs an overlay behind the input since a plain `<input>` cannot colour a substring |
| UX-F04 | Cross-sheet reference picking (click another sheet's cells mid-formula) | Formula | ✅ | A tab click mid-formula navigates without ending the edit (`switchSheet(i, keepEdit)`), picks there insert a qualified `Sheet!ref` (quoted when the name is not a bare word), and commit/cancel return to the cell the edit belongs to — tracked as `editHome`, since the write target can no longer be "wherever the selection is now" |
| UX-F05 | F4 anchor cycling ($A$1→A$1→$A1→A1) | Formula | ✅ | `cycleAnchors()` finds the reference under the caret from the UX-F03 spans and rewrites it A1 → $A$1 → A$1 → $A1 → A1; a range cycles both endpoints together and a sheet qualifier is left as written |
| UX-F06 | Keyboard point mode (arrows insert/extend a ref while editing) | Formula | ✅ | `pointStep()` — arrows build a reference when the caret sits where one may go (otherwise they move the text caret), Shift extends it into a range, the pointed cell is scrolled into view without disturbing the selection, and typing anything ends the mode leaving the reference in place |
| UX-F07 | Editing an existing formula re-highlights its refs | Formula | ✅ | `beginEdit` runs the range finder, so F2 on a stored formula outlines its inputs immediately — from either surface |
| UX-F08 | Error affordances: per-cell error corner marker + hover explanation + trace precedents/dependents | Formula | 🟡 | **Marker + explanation done**: `session_cells` flags error *values* (`er`) — not text that merely looks like `#VALUE!` — and `draw()` marks them; hovering names the error, explains it in the terms that caused it, and shows the formula. **Remaining**: trace precedents/dependents, which needs the dependency graph exposed |
| UX-E02 | Formula bar second-class: no Esc-revert, no autocomplete, no ref insert, Enter doesn't advance | Editing | ✅ | All four closed by UX-F01: Escape restores the cell's text, autocomplete and reference insert work from the bar, Enter commits and moves down, Tab commits and moves right, an invalid formula outlines the bar and keeps focus there |
| UX-E03 | Find: no highlight-all, match-entire-cell, wildcards/regex, Values look-in, all-sheets scope | Editing | 🟡 | **Done**: every match tinted on the grid (amber, distinct from the selection so the current hit still reads as current); match-entire-cell; **Values look-in** (`display_text` — a formula's text is `=SUM(…)` while its value is `43.48`, and only one is what the user sees); whole-workbook scope, following a hit to its sheet. **Remaining**: wildcards/regex |
| UX-E04 | Ctrl+D / Ctrl+R fill-down/right missing | Editing | ✅ | `fillWithin()` fills the selection from its own first row / first column via `session_fill`, so references shift as they do on a drag-fill |
| UX-E05 | Double-click fill handle to autofill to neighbor extent | Editing | ✅ | `autofillToNeighbour()` — the extent comes from the adjacent column's data edge (left column, else right), which is Excel's rule |
| UX-V01 | Data validation: list-only + **not enforced**; no number/date/text/custom rules, no input/error messages | Data | 🟡 | **Enforcement done**: `session_validation_error()` reports why an input is disallowed and `commit()` refuses it, naming the allowed values and outlining the field — a dropdown that accepted anything typed over it was a suggestion, not a validation. Gated on typed entry only, as in Excel; clearing a cell always passes. **Remaining**: rule kinds beyond a list (number/date/text-length/custom) and author-supplied input/error messages, both of which need model + import/export work |
| UX-V02 | Autofilter: single global column, checklist-only, fights manual hidden set; no per-header dropdowns/conditions/multi-column | Data | 🔴 | Real per-column filter model (separate from `hidden_rows`); header dropdowns; conditions |
| UX-V03 | Row-height / column-width numeric dialog missing | Data | ✅ | `sizeDialog()` on the row/column header menu (landed with UX-C01), seeded with the current size and applied across the selected band |
| UX-V04 | Outline / grouping: model support, zero UX | Data | 🔴 | `session_group_*`/`toggle_collapse`; outline bars; Alt+Shift+arrow |
| UX-V05 | Remove duplicates — absent | Data | ✅ | `session_remove_duplicates` compares rows on their *displayed* values (two cells reading `1.50` are the same row even if one is a formula), keeps the first of each, deletes bottom-up so indices stay valid, and returns the count. Confirms first, excludes a detected header, reports how many went |
| UX-FM1 | Conditional formatting: fill-only cell rules; no color scales / data bars / top-bottom / above-avg / duplicates; no Manage Rules; no priority/stop-if-true; font/border effects | Format | 🔴 | Extend `CfRule` + panel + render; Manage-Rules list |
| UX-FM2 | Format painter — absent | Format | ✅ | Toolbar button picks up the active cell's *resolved* style (`session_copy_style`), so number format, font, fill, borders, alignment and wrap travel together; the next selection is painted. Single click paints once, double-click keeps painting, Escape cancels |
| UX-FM3 | Custom number-format dialog + currency locale/symbol; expose negative/zero/text sections (engine supports) | Format | 🟡 | **Dialog done**: type any code with a live preview against the active cell's own value (`format_preview` / `format_preview_text` — the engine's real answer, not an approximation), seeded from the current format, with presets covering the sections, colours, scientific and text forms the engine gained this session. **Remaining**: a currency/locale picker that builds `[$SYM-locale]` for you |
| UX-FM4 | Align: no Justify / Fill / Center-Across; vertical Justify/Distributed folded away | Format | 🔴 | Extend `HAlign`/`VAlign` + toolbar + render |
| UX-FM5 | Merge variants: no Merge & Center, no Merge Across | Format | ✅ | The merge button is a four-verb menu: merge & centre, merge, merge across (each row separately), unmerge. Across counts what *all* the rows would bury and asks once before discarding, keeping UX-M01's rule intact |
| UX-FM6 | Color pickers: no theme colors, no RGB/HSL, no eyedropper; hex rejects 3-digit/rgb() | Format | 🔴 | Theme row from workbook theme; RGB/HSL inputs |
| UX-FM7 | Named cell styles (Good/Bad/Heading…) — absent | Format | 🔴 | Named-style registry + gallery |
| UX-FM8 | Indent increase/decrease — model field dead, no UI/API/render | Format | ✅ | `session_adjust_indent` (clamped to Excel's 0–250), two toolbar buttons, and the indent now reaches the editor at all — it was in the model but never emitted — shifting text off its leading edge in both the wrapped and single-line paths |
| UX-FM9 | Text rotation — not even in model | Format | ✅ | `Style::rotation` in OOXML's own `textRotation` encoding (0–90 CCW, 91–180 for value−90 CW, 255 stacked) so the round-trip is lossless and there is no conversion to get backwards; parsed, written (round-trip test), set from a toolbar menu, drawn under a canvas transform, and the row auto-grows to fit — without which the rotation is clipped to a 20 px row and achieves nothing |
| UX-G01 | **Drawn geometry vs engine geometry diverge for auto-grown rows** — `measure()` grows a row for wrapped text, but scroll anchoring (`session_row_at_px`), the frozen-band origin, the scrollbar extent, `ensureVisible` and the row-resize origin all read the engine's un-grown offsets. Symptoms: a hitch at row boundaries while wheel-scrolling, a short scrollbar extent, `ensureVisible` under-scrolling so the selection creeps below the viewport, and a resize drag offset by the accumulated growth | Nav | 🔴 | Narrowed to wrapped rows only now that auto-height respects file-set heights. Either push effective auto-height into the engine (`GridGeometry` already owns it) or keep a prefix-sum of effective heights in `measure()` and derive `fsr`/`subY`/`frozenH`/`contentH`/`rT` from it. Horizontal axis is unaffected — nothing grows `geo.colW` |
| UX-A01 | Status bar Min/Max never shown (engine computes them); no separate numeric count | Chrome | ✅ | `session_range_stats` already returned min/max; the bar now folds them across multi-ranges and shows a separate Numbers count when it differs from Count, so a stray text cell in a numeric column is visible |
| UX-A02 | Cell-mode indicator (Ready/Enter/Edit/Point) + persistent bottom status; Count always-on | Chrome | ✅ | A mode chip in the bottom bar. The word tracks the *gesture* that started the edit, not whether the cell was empty — typing over a filled cell is Enter, F2 into it is Edit — and reference picking (mouse or arrows) shows Point |
| UX-A03 | Canvas invisible to AT: no `role=grid`, no `aria-live` active-cell announce | Chrome | 🟡 | `role=grid` + label on the canvas, and a visually-hidden `aria-live` region announcing the active cell, the selection size and the cell's content as it moves (de-duplicated, since re-announcing an identical string is silence in some readers and noise in others). **Remaining**: this is an announcer, not a real accessibility tree — a DOM mirror of the visible cells would be needed for structural navigation |
| UX-A04 | Canvas has no focus ring | Chrome | ✅ | Inset ring on the grid, gated on keyboard navigation: Chrome counts a focusable `<canvas>` as `:focus-visible` even for a mouse click, which would ring the whole grid every time you clicked a cell |
| UX-A05 | Formula bar `fx` decorative; no Insert-Function; no multi-line expand | Chrome | 🔴 | Real `fx` button → function picker; auto-grow textarea |
| UX-A06 | Menu bar has no keyboard nav / mnemonics; items lack `role=menuitem` | Chrome | 🟡 | `role=menuitem`/`role=menu`, roving tabindex (the bar is one tab stop), Left/Right between menus, Down/Enter to open, Up/Down through items, Home/End, Left/Right *inside* an open menu moves to the neighbouring one, Escape returns focus to the bar. **Remaining**: Alt-key mnemonics |
| UX-A07 | Toolbar not a roving-tabindex composite (every control a tab stop) | Chrome | ✅ | One tab stop for the whole toolbar (was ~40), arrows/Home/End move within it. Text fields keep their own Left/Right so the caret still works, and every control — including those parked in a collapsed group's flyout — is reset, or opening a flyout would have added stops back |
| UX-A08 | Cell context menu missing Insert note / Format cells / Define name / Filter | Chrome | 🔴 | Add verbs to `cellMenu` |
| UX-NV1 | Zoom (Ctrl+wheel, control, 25–200%) — absent | Nav | ✅ | Applied to the **canvas transform**, not to the geometry: the grid keeps measuring in engine pixels and only the drawing and the viewport it must fill are scaled, so drawn and modelled geometry stay comparable (scaling widths/heights instead would re-create UX-G01). Pointer coordinates divide by zoom; the in-cell editor is a DOM element so it multiplies. Ctrl+wheel, Ctrl+0, View ▸ Zoom presets |
| UX-NV2 | Scrollbar track-click paging | Nav | 🔴 | mousedown on track pages toward click |
| UX-NV3 | Tab-run return on Enter (return to Tab-origin column, row+1) | Nav | 🔴 | Track tab-origin column |
| UX-NV4 | Enter/Tab wrap inside a multi-cell selection | Nav | 🔴 | **Blocked on a model split, not a keybinding.** `state.sel` is a *corner* of the selection (the block is `anchor`..`sel`), so moving it inside the block shrinks the block. Needs a distinct active-cell/focus separate from the selection corners, with the ~50 `state.sel` readers pointed at an `activeCell()` accessor |

## P2 — polish

| ID | Item | Domain | Status | Fix |
|----|------|--------|--------|-----|
| UX-P01 | Toolbar file group (New/Open/Download) redundant with File menu — declutter | Chrome | ✅ | Removed group; Open now in header + File menu; Download/New in File menu; new-sheet is the bottom "+" |
| UX-P02 | Ctrl+click deselect toggle; Shift+wheel horizontal; Alt+PageUp/Dn; clamp scroll to extent; Ctrl+Arrow into blank; End-mode; Go-To dialog | Nav | 🔴 | per audit-1 P2 list |
| UX-P03 | Name Box: accept `A:A`/`1:5`/`Sheet!ref`/comma-multi; NRxNC on keyboard extend; defined-names dropdown | Nav/Chrome | 🔴 | extend `gotoName` parser + dropdown |
| UX-P04 | Paste-special: arithmetic ops + full dialog (Ctrl+Alt+V) | Editing | 🔴 | extend `session_clip_paste_mode` + dialog |
| UX-P05 | Undo/redo labels; F2 caret-at-end; inline arrows commit+move (enter mode); Shift+Tab-left while editing; column-value autocomplete | Editing | 🔴 | per audit-2 P2 list |
| UX-P06 | Autofill: growth series, date stepping, Ctrl copy↔series toggle, fill-options popup | Editing | 🔴 | `mode` arg + popup |
| UX-P07 | Font picker searchable + rendered in-face; grow/shrink ladder beyond 72 | Format | ✅ | Custom combobox replaces `<input list=datalist>` (a datalist is filtered by the input's own text, so an auto-populated box offered exactly one font). List comes from the engine (`font_families()` ← `PICKER_FAMILIES`); rows preview in-face, substitutes say so in the tooltip. A▲/A▼ already stepped past 72 |
| UX-P08 | Borders: 13 placements incl. composite bottoms + diagonals; line-color swatch reflects pick | Format | 🔴 | add presets + diagonal model/render |
| UX-P09 | Date `[h]/[m]/[s]` elapsed time dropped; English-only month/day | Format | 🔴 | special-case brackets before drop |
| UX-P10 | numfmt/align submenu checkmarks; %/comma toggle back to General | Format/Chrome | 🔴 | `check` predicates; toggle logic |
| UX-P11 | Header right-click selects line (dup of UX-C01); Insert/Delete cells w/ shift; move/cut rows-cols; Unhide All; resize drag tooltip; autofit all-selected + wrap/merge | Data | 🔴 | per audit-4 P2 list |
| UX-P12 | Threaded comments (author/time/replies); right-click Insert comment | Data | 🔴 | extend comment model + popover |
| UX-P13 | Sheet tabs: overflow scroll + all-sheets menu; hide/unhide sheet; move-to; protect | Data | 🔴 | tab bar affordances + ops |
| UX-P14 | Text-to-columns | Data | 🔴 | reuse `casual-calc-io` delimiter split |
| UX-P15 | Import/export UX: progress + friendly errors; CSV encoding (BOM/UTF-16) fallback; CSV active-sheet-only warning; date/leading-zero fidelity | Chrome/Fidelity | 🔴 | modal + `read_delimited` detection |
| UX-P16 | Format Cells dialog (Ctrl+1) consolidating number/font/border/fill/align | Chrome | 🔴 | modal (shell exists) → `session_set_*` |
| UX-P17 | `prefers-reduced-motion`; freeze-line theming; ~~collapsed-flyout self-dismiss on input click~~ (fixed with UX-P07) | Chrome | 🟡 | media query; `freezeLine` token |

---

## Working order

1. **UX-P01** (toolbar declutter — user request, quick).
2. **UX-F01** formula-editor controller refactor → then the formula cluster UX-F02/03/06/07/E02 (highest user value; the flagged gap).
3. P0 correctness: UX-M01 merge, UX-N01/02/03 number formats, UX-D01 sort, UX-C01 header menu, UX-E01 multi-line.
4. P1 by domain value, then P2.

Each item: implement → verify in-browser → commit → flip status here before starting the next.
