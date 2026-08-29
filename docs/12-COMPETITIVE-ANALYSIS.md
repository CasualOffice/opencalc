# 12 — Competitive analysis

**Checked 2026-08-29 against the editor served at `127.0.0.1:8123`.** Against
**Google Sheets, Microsoft Excel, OnlyOffice, LibreOffice Calc and Univer**.

The product goal in this repository is *"a viable alternative to Excel"*. The
owner's verdict on the current editor is *"bottom tier"*, *"third tier"*, *"not
usable at all"*. This document exists to say precisely which parts of that are
true, which are not, and what the difference costs a user.

The unit of comparison here is **a task somebody actually does** — sort, filter,
pivot, print, share — not a toolbar. For each one:

- **Present and equal or better** — say what makes it so.
- **Present but worse** — say how much worse, in clicks, chords, reopened
  dialogs, or results the user has to check by hand.
- **Absent** — and whether the **engine** can already do it while the **editor**
  cannot reach it. That distinction changes the cost of the fix by an order of
  magnitude and it has now come up seven times (§6).

And for every place OpenCalc does something differently from Excel or Sheets,
one question is answered explicitly: **does the difference help the user, or does
it just tax them for choosing this product?** A divergence nobody chose is not
uniqueness, it is an unfinished feature. Muscle memory is a real cost: somebody
arriving from Excel has decades of it, and each rebound chord or reordered dialog
is a small tax that accumulates.

## 1. How this was measured, and how far to trust it

Every OpenCalc claim below carries a measurement, a `file:line`, or a tracker id.
Nothing is asserted from reading alone — this repository has been burned by
prose maps in both directions, and §9 lists three places where *this* pass
contradicts a document currently in the tree.

The probes drove the real editor through Playwright against a session started
with `session_new()` (not the demo workbook — `seed()` at
`webapp/editor.selection.js:1701` writes its own styled sample, and a probe that
skips `session_new` measures the sample). Existing measured sources were read
first and built on rather than redone: `docs/47-UX-AND-FEATURE-MAP.md` (46
driven behaviours), `docs/68-UX-VISUAL-AUDIT.md` (measured geometry),
`docs/73-EXCEL-UX-PARITY-AUDIT.md` (a reading pass, explicitly marked
`[unverified]`), and `docs/14-EXECUTION-TRACKER.md`.

**Competitor claims are from working knowledge, not from a fresh test run**, and
where that knowledge is thin it says so inline rather than guessing. A wrong
claim about Excel would discredit the rest.

**Numbers this pass established, replacing older ones in the tree:**

| Quantity | Measured | Where |
| --- | --- | --- |
| Engine functions | **429** | `crates/casual-calc-eval/src/functions/mod.rs:59` — `AGENTS.md:38` still says 364 |
| WebAssembly bindings | **274** `#[wasm_bindgen]`, 239 `session_*` | `crates/casual-calc-wasm/src/` |
| Editor commands | **197** | `listCommands()` |
| Toolbar controls | **109** | measured at 1440×900 |
| Crates | 16 | `crates/` |
| Editor JavaScript | ~20,700 lines across 18 modules | `wc -l webapp/*.js` |
| WASM payload | **7,811,572 bytes** | `HEAD /pkg/casual_calc_wasm_bg.wasm` |
| Cold boot to `engine v0.0.0` | **334 ms** local | probe |

---

## 2. The five products, in one line each

| Product | What it is | Why it is here |
| --- | --- | --- |
| **Microsoft Excel** | The semantics oracle and the incumbent. | Defines "correct" for formulas, formats, coercion, errors, dynamic arrays. Defines muscle memory. |
| **Google Sheets** | The mainstream web bar. | Defines what a browser spreadsheet is allowed to feel like: instant, shared, undoable, never lost. |
| **OnlyOffice** | C++ OOXML engine, desktop + web, co-editing. | Proof that faithful OOXML round-trip and live editing coexist in one engine. The closest thing to what OpenCalc is trying to be. |
| **LibreOffice Calc** | The open desktop suite; ODF-native. | The automatable differential oracle for the fidelity harness, and the benchmark for "free and complete". |
| **Univer** | TypeScript office framework, canvas render, plugin seams. | The closest competitor to OpenCalc's **embed/SDK** story. Open core with a commercial tier; specifics of what is OSS versus paid are stated as uncertain below. |

---

## 3. Task-by-task

Each row is a thing a user does. `E`/`GS`/`OO`/`LO` are Excel, Google Sheets,
OnlyOffice, LibreOffice Calc. "Engine ✓ / editor ✗" means the capability exists
below the WebAssembly boundary and no command reaches it.

### 3.1 Enter and edit data

**Equal or better.** The typing loop is the strongest part of this product and
the part most likely to be underestimated from the outside.

- **Formula autocomplete with signatures.** Typing `=SU` offers 8 entries, each
  with its full argument list — `SUBSTITUTE(text, old, new, [instance])`,
  `SUMIFS(sum_range, range1, criteria1, …)` [measured]. Excel offers names only
  until you open the paren; Sheets shows a signature panel. This is at Sheets'
  level and ahead of Excel's.
- **Argument hint while typing.** After `=SUM(`, `#sig-tip` shows
  `SUM(number1, …)` [measured]. Parity.
- **Range finder.** Editing `=SUM(B2:B5)+C2` tints `B2:B5` blue
  (`rgb(26,115,232)`) and `C2` orange in a `.ref-mirror` over the textarea, and
  strokes matching boxes on the canvas (`webapp/editor.paint.js:644`,
  `webapp/editor.core.js:2588`) [measured]. Parity with all four incumbents.
- **Insert Function dialog** with a live search box over all 429 functions
  [measured]. Parity.
- **Status-bar aggregates.** A 4-cell range reports
  `Sum: 75 Avg: 18.75 Min: 5 Max: 40 Count: 4` — all six at once, and folded
  across disjoint ranges [measured]. Excel shows a configurable subset; Sheets
  shows one at a time behind a menu. **Better.**

**Present but worse.**

- **A single cell gets no aggregates at all** — `#sel-stats` is empty
  [measured]. Excel populates Average/Count/Sum for one numeric cell. Small, but
  it is the "did I select the right thing" glance, several times a minute.
- **No IME composition path.** Dispatching `compositionstart` at the grid opens
  no inline editor [measured]; `docs/73` records the same. A CJK or Indic user
  cannot *begin* a cell entry by typing — they must press F2 first. That is not
  a polish item, it is a whole class of user who cannot use the default path.
- **No in-column autocomplete** of values already in the column
  (`docs/47`: ❌). Excel and Sheets both do this and it is the single most-used
  typing shortcut in a list of repeated text.

**Absent.**

- **Flash Fill / Smart Fill.** `Ctrl+E` does nothing [measured]; `docs/47`: ❌.
  Excel (Flash Fill) and Sheets (Smart Fill) both derive a column from one
  example. This is the highest-leverage missing editing feature.

### 3.2 Navigate a sheet

**Present but worse — and one of these destroys the document.**

- **The grid crashes at row 74,567.** `session_row_offset_px(0, 74566)` traps
  the WebAssembly module with `RuntimeError: unreachable`, and **every
  subsequent engine call fails; the document is unrecoverable** [measured].
  Mechanism, confirmed rather than guessed:
  `crates/casual-calc-wasm/src/axis.rs:218` computes
  `rows.offset(row) as i32 * 96 / 1440`. At the default 20 px row height row
  74,566 is 22,369,800 twips, and `× 96` is 2,147,500,800 — past
  `i32::MAX` (2,147,483,647). Row 74,565 gives 2,147,472,000 and returns
  `1491300` fine [measured]. The boundary scales exactly inversely with row
  height, as the mechanism predicts: at 40 px rows it moves to 37,283
  [measured] — the same 1,491,3xx px offset. `session_col_offset_px`
  (`axis.rs:169`) has the identical cast, but the default column width puts its
  overflow at column ~23,301, beyond XFD, so columns never reach it [measured:
  column 16383 is fine].
  **`overflow-checks = true` in `[profile.release]` (`Cargo.toml:70`), so this
  panics in the shipped build too — it is not a debug artefact.** There is no
  panic hook: the console carries no message, so the user gets a frozen
  application and an empty log [measured].
  Excel, Sheets, OnlyOffice and LibreOffice all address 1,048,576 rows. This
  product addresses 74,566 before it dies, against a repository target of 1M
  cells (`docs/30`). It is reachable by typing `A1048576` into the Name Box and
  by `Ctrl+End` on a sheet whose data reaches the bottom — both trap [measured].
- **`Ctrl+Arrow` cannot leave the used range.** From A1 on a 5×3 block,
  `Ctrl+Down` twice then `Ctrl+Right` twice lands on C5 and stops [measured].
  In Excel, `Ctrl+Down` past the last row goes to row 1,048,576 — which is how
  users learn how big a sheet is, and how they get to the bottom to append. Here
  the sheet has no visible bottom. **This bug is currently the only thing
  stopping a keyboard user from reaching the crash above.**
- **No Go To.** `Ctrl+G` and `F5` both do nothing [measured]. The Name Box
  substitutes and is genuinely good (§5), but `F5` is thirty years of muscle
  memory and it silently does nothing at all — the worst possible response,
  because the user cannot tell whether they mis-pressed.
- **No Go To Special** (`docs/73`). Excel's "select all blanks / all formulas /
  all constants" is the fastest route to several everyday cleanups; Sheets has
  no equivalent either, so this is Excel-only.
- **Arrowing past a hidden row does not skip it** (`docs/47`: ❌).

**Equal or better.**

- `Ctrl+End`, `Ctrl+Arrow` within data, per-sheet scroll and selection memory,
  and a Name Box that accepts whole-column bands, sheet-qualified refs and
  comma-separated multi-ranges (`docs/47`: ✅ ×5).

### 3.3 Select

**Equal.** And `docs/47` is wrong about this — see §9.

Ctrl+click banking works and **operations act on the bank**: Ctrl+clicking A2
then C4 gives `Count: 2` in the status bar, `toolbar.bold` sets `b:1` on *both*
cells, and `Delete` clears both [measured]. `docs/47` marks
"Ctrl+click adds a second range" and "a banked multi-range is what operations act
on" as ❌ and ranks both **daily / large**; they are the two biggest items in
that document's fix pipeline and both already work.

Column/row header drag-to-reorder, drag-across-headers span selection, and
double-click-to-autofit are all present (`docs/47`: ✅).

**Present but worse:** dragging the selection border to move a range is absent
(`docs/47`: ❌) — in Excel and Sheets that is the primary way to move a block,
and its absence forces cut-and-paste, which in this editor has its own history
(`UX-CUT-03`, `UX-CUT-04`, both now closed).

### 3.4 Autofill

**Equal.** Dragging the fill handle fills; a date increments by a day; `Item 1`
continues the number series (`docs/47`: ✅ ×3, the last two closing defects
`docs/73` had recorded as broken). A fill-options popup exists and `Edit ▸ Fill`
exposes fill-down/right, series, growth, copy, formats-only and values-only —
which is Excel's `Fill` submenu, reachable from a menu rather than only by
dragging. Sheets has no equivalent menu; **this is better than Sheets.**

**Present but worse:** fill-handle drag does not auto-scroll (`docs/73`), so a
fill is capped at what fits on screen — in a 5,000-row table that is unusable
and the user must fall back to Fill Down.

### 3.5 Sort

**Equal, with a smaller cap.**

- Sorting from a single cell inside a block **auto-expands to the whole
  contiguous range and keeps rows intact**, detecting the header row: sorting
  from C2 on a 5×3 block reordered whole records and left `Region|Rep|Units` on
  top [measured]. `docs/73` #9 warned this was the classic data-destroying case
  with no "expand the selection" prompt. **Measured, it does not destroy data.**
  Excel prompts; this expands silently. *Does the divergence help?* Yes — it
  removes a modal from a very common action and does the safe thing. Worth
  keeping.
- The filter dropdown offers `Sort A→Z` / `Sort Z→A` inline [measured], which
  is where Excel and Sheets put it.

**Present but worse:** the custom sort dialog caps at **3 keys** — "Sort by" plus
two "Then by" [measured]. Excel allows 64; Sheets adds columns without limit.
Three covers most work and is a real ceiling for the rest.

**Absent:** sort by colour or by cell icon (Excel, Sheets and LibreOffice all
have it). Sort left-to-right (Excel, LibreOffice).

### 3.6 Filter

**Equal or better.**

The dropdown carries `Sort A→Z`, `Sort Z→A`, `Filter by condition…`, a
**search box** over the values, and the value checklist [measured] — everything
`docs/73` recorded as missing has landed. Values sort numerically, not
lexicographically (`docs/47`: ✅), which was a named defect.

**Better than Excel and OnlyOffice:** a per-user **"Just for me"** filter —
*"Others keep seeing every row"* [measured], built to `docs/71` under `COL-32`.
Sheets has Filter Views and Excel 365 has Sheet Views; having it inline in the
dropdown rather than behind a separate menu is a genuinely better placement.

**Absent:** filter by colour (Excel, Sheets, LibreOffice).

### 3.7 Conditional formatting

**Present but materially worse, and it loses data on import.**

- The rule list offers 15 predicates: the six comparisons, between/not between,
  text-contains, top/bottom N and N%, above/below average, duplicated,
  appears-only-once, 2- and 3-stop colour scales, and data bars [measured].
  That is a good set and covers most everyday use.
- **There is no custom-formula rule** [measured]. This is the single most
  important omission in the whole conditional-formatting story: "highlight the
  row where column F is overdue" is a formula rule, and Excel, Sheets,
  OnlyOffice and LibreOffice all have one. Without it, whole-row highlighting —
  the commonest real use — is impossible.
- **There are no icon sets** [measured].
- **On import, `expression` rules, icon sets and date-period rules are dropped.**
  `crates/casual-calc-import/src/lib.rs:1329` records every unmodellable rule as
  `Omitted` + `NotRetained` — so it is *counted and named* in the compatibility
  report, which is what `docs/34` requires, and it is **still lost**. An Excel
  workbook with formula-based highlighting opens here with the highlighting gone
  and re-saves without it. That is a switching blocker regardless of how honestly
  it is reported.
- The rules manager is view-and-delete: with no rules it shows *"No conditional
  formatting on this sheet"* and a Close button [measured]. No edit, no
  reorder, no stop-if-true. Excel's Manage Rules does all three.

### 3.8 Data validation

**Present and close to equal.** Eight types — List of values, Whole number,
Number, Date, Time, Text length, **Custom formula**, Any value — with eight
operators, and the stop/warning/information split with a custom title and
message [measured]. That is Excel's model, and the three-level severity is
something Sheets does not have.

**Present but worse:** reopening the dialog does not show the rule that is
already there (`docs/47`: ❌). Every edit to an existing rule is therefore a
retype from scratch. That is a pure tax: it costs the user everything they had
already entered, and the engine holds the rule.

### 3.9 Pivot tables

**Present, well past prototype, and behind on the analytical half.**

- A real drag-field dialog — Filters / Columns / Rows / Values, grand-total row
  and column toggles, 10 styles, Delete and Refresh [measured] — over a
  1,386-line pivot engine. `Alt+F5` / `Ctrl+Alt+F5` refresh one or all.
- **Absent: calculated fields**, **Show Values As** (% of total, running total,
  rank), **date grouping** into months/quarters/years, **slicers** and
  **timelines**. Excel has all five; Sheets has calculated fields and slicers.
  Date grouping is the one people hit first — a pivot over a date column here
  produces one row per distinct day.
- **Export is partial** (`PIV-02`, `Partial`): a pivot created here now reaches
  the file as a real `pivotCacheDefinition` + `pivotTableDefinition` with
  `refreshOnLoad="1"`, which is the honest route, but it is a recent fix and the
  row is not closed.

### 3.10 Charts

**Present but far behind.**

Seven types, no subtypes: Column, Bar, Line, Area, Pie, Doughnut, Scatter
[measured; `crates/casual-calc-model/src/chart.rs:35` has the same seven plus
`Unsupported`, which is retained-and-not-drawn rather than dropped — the right
call]. The dialog offers title, axis titles, legend position, named series with
ranges, and category labels [measured].

Against that: Excel has roughly 17 families with stacked and 100%-stacked
subtypes, combo charts, secondary axes, trendlines, error bars and data labels;
Sheets has around 30 chart types. Missing here and reached for constantly:
**stacked columns/bars, combo charts, secondary axis, data labels, trendlines**.

*Does the narrowness help?* No — it is unfinished, not chosen. A user who needs
a stacked bar has no route at all, and the chart they imported from Excel is
retained but not drawn.

**Absent entirely: sparklines** (`docs/47`: ❌). Excel, Sheets and LibreOffice
have them.

### 3.11 Find and replace

**Present and close to equal.** The find bar carries case, whole-cell, search-in-
values, **all sheets**, and wildcards [measured] — a fuller option set than
Sheets', and every match is highlighted live in the grid, which Excel does not do
(§5).

**Present but worse:** Replace All does not honour the all-sheets option the Find
used (`docs/47`: ❌) — so the count reports across sheets while only one changes,
which is worse than not offering the option. No Find All result list (`docs/73`);
Excel's is how you audit a replace before committing it.

### 3.12 Named ranges

**Split: creation is better, management is absent.**

- The Name Box **defines** a name from the selection and accepts five reference
  syntaxes (`docs/47`: ✅ ×2). Excel's Name Box only navigates and defines a
  simple name; this is **better**.
- **Name Manager is a popmenu that says "No named ranges yet"** with no controls
  [measured]. `docs/73` records it as view-and-delete only. There is no way to
  rename a name, change its scope, edit its range, or add a comment. Excel's Name
  Manager does all of that and is where any real workbook's names are maintained.
- `Ctrl+F3` does nothing [measured], while the Help ▸ Keyboard shortcuts panel
  advertises `F3` for Name Manager — an advertised chord that misses.

### 3.13 Freeze panes, grouping, outlines

**Equal or better.** Freeze from the menu *and* by dragging the divider
(`UX-FREEZE-01`, Done) — Excel has no drag; Sheets does. Correct per-quadrant
clipping (`docs/73`, verified). Grouping with expand/collapse-all and
show-level-1/2, re-indexed on insert and delete.

**Absent:** `View ▸ Split` (`docs/73`), which Excel and LibreOffice both have.

### 3.14 Number formats and cell formatting

**Present, deep, and awkwardly placed.**

- 14 preset formats in the toolbar with a live preview, plus a custom-format
  dialog with a **currency picker** covering 8 currencies, red-negative and
  accounting presets, and section syntax help [measured].
- `docs/47` marks "a currency other than $ can be chosen" ❌ — that is true *of
  the toolbar*, and the capability is there, four levels deep:
  Format ▸ Custom number format… ▸ pick currency ▸ Insert currency format ▸
  Apply. Excel puts a currency dropdown next to the currency button; Sheets puts
  it in Format ▸ Number ▸ Currency. **Present but worse: five interactions where
  the incumbents take two**, for something a non-US user needs on their first
  workbook.
- `Ctrl+1` opens Format Cells [measured] — the one chord an Excel user presses
  most, and it is bound correctly.
- **Row height and column width are not on the menu bar** (`docs/47`: ❌). They
  are on the row and column header context menus as `Row height…` /
  `Column width…` [measured], which is where Excel puts them too — so this is
  reachable, and `docs/47`'s row is about the menu bar specifically.
- **Both ask with a raw `window.prompt`** (`UX-DLG-02`, Open, P1;
  `docs/68` records `window.prompt` — *"Column A width (px)"*). A native browser
  prompt in the middle of an application is the loudest possible signal that
  something is unfinished, and it cannot be styled, validated or cancelled
  consistently.
- Cell styles: 10 named styles (Normal/Good/Bad/Neutral/Title/Heading 1–4/Total)
  applied *and tagged by name* [measured] — the OOXML model done properly. No
  user-defined styles; Excel has those and LibreOffice leans on them heavily.
- **Format Cells does not leave a font colour alone if untouched**
  (`docs/47`: ❌) — a dialog that changes something you did not ask it to change
  is a trust problem, not a formatting one.
- **No font preview on hover** (`docs/47`: ❌). Excel and Sheets both live-preview.

### 3.15 Tables (structured ranges)

**Present, with two sharp edges.** `Ctrl+T` opens a real Create Table dialog with
a range, a "My table has headers" checkbox and 10 styles [measured] — which
closes `docs/73` #11's claim that headers were hard-coded.

Still open from `docs/73`, unverified: `[@Column]` structured references
reported to evaluate to `#VALUE!`; totals-row insertion overwriting whatever is
below; editing a header not renaming the column. `UX-TABLE-02` (Open, P1) is
measured and real — *the header label is drawn underneath its own filter arrow*
(`docs/68`: `{"col":3,"label":"Revenue","inkInArrowZone":6}`), and the table
outline draws no border at all where the engine resolves one.

### 3.16 Paste, paste special, clipboard

**Present and close to equal, badly placed.** The cell context menu carries
Paste special with Values only / Formulas only / Formats only / Transpose and a
full `Paste special…` dialog [measured]. HTML paste recovers font, size, wrap,
vertical alignment, merges and `mso-number-format` from Excel and Sheets
(`docs/73`) — **better than Sheets' inbound paste**.

**Present but worse:**

- **There is no `edit.paste-special` command at all** [measured — it is absent
  from `listCommands()`], so Paste Special is reachable *only* by right-click.
  It is not on the Edit menu and `Ctrl+Alt+V` reports "clipboard is empty"
  rather than opening the dialog [measured]. Excel binds `Ctrl+Alt+V`; Sheets
  binds `Ctrl+Shift+V`. An Excel user pressing `Ctrl+Alt+V` gets a status message
  that does not explain itself.
- `docs/68` records Paste special as *"did not open — no modal or panel became
  visible"* on its own path.
- Transpose is exclusive with the paste-type choice; outbound clipboard HTML
  carries far less than the inbound parser reads; copy drops manually hidden rows
  where Excel keeps them (`docs/73`).

### 3.17 Print and page setup

**Page setup is equal to Excel's. What actually prints is not.**

`File ▸ Page setup…` offers orientation, five paper sizes, scale percent /
fit-to-width / fit-to-height, four margins, print gridlines, row/column headings,
centre across and down, a print area with "Set from selection", repeat-rows-at-top,
and header/footer fields documented with `&L &C &R &P` [measured]. That is a
genuinely complete Page Setup and better than Google Sheets'.

Printing then hands almost none of it to the output. `File ▸ Print…` and `Ctrl+P`
both open a popup window and call `print()` [measured — one popup each]. The
popup's HTML (`crates/casual-calc-wasm/src/objects.rs:1471`) is a bare `<table>`:

- **Column widths are not emitted.** A sheet with column A set to 200 px prints
  with `table-layout:fixed` and no `<col>` element [measured] — the printed
  layout does not match the screen.
- **Merged cells are not emitted** — no `colspan`/`rowspan` anywhere in the
  generator.
- **Cell borders do not print.** The only border rule is a blanket
  `td,th{border:1px solid #b0b0b0}` when gridlines or headings are on
  (`objects.rs:1574`).
- **Scale and fit-to-page are not applied.** The emitted CSS is only
  `@page{size:letter;margin:…}` [measured] — the three scaling controls in the
  dialog change the saved file and not the printout.
- **Header/footer field codes are stripped, not substituted** (`strip_hf_codes`),
  so `&P` cannot print a page number. The dialog's own placeholder text
  advertises `&P`.
- No charts, no images, no conditional formatting.

What it *does* carry: bold, fill colour and number format
(`<td style="font-weight:bold;background-color:#FFFF00;">`, `25.00%`)
[measured], the print area, repeat-rows via `<thead>`, and manual row breaks.

**No print preview, no Page Layout view, no page-break preview.** Excel has all
three; Sheets has an in-app preview with per-page controls; OnlyOffice and
LibreOffice both have page-break preview.

**Absent: PDF export** (`IO-03`, Open, P1). `grep -ri pdf crates/ webapp/`
returns nothing. All four competitors export PDF, and it is the format a finished
spreadsheet most often leaves the application as. The tracker is right that
`casual-calc-layout` and `casual-calc-render` already paginate — this is a
writer over existing layout, not new layout.

### 3.18 Import and export

**Engine ✓ / editor ✗, plus two real refusals.**

- Openable: **`.xlsx`, `.ods`, `.csv`, `.tsv`, `.tab`, `.psv`** [measured via
  `openable_extensions()`].
- Downloadable: **same-format-as-opened, `.xlsx`, `.csv`, `.tsv`, `.psv`**
  [measured via `listCommands()`].
- **`.ods` is missing from the download menu.** `format_for_extension("ods")`
  returns `"ods"` and `casual_calc_sdk::SessionFormat::Ods` has a writer
  (`crates/casual-calc-sdk/src/lib.rs:1575` → `casual_calc_ods::export_ods`)
  [measured]. So the engine writes ODF and the editor offers no way to ask for
  it. A LibreOffice user can open their file here and cannot get it back in
  their own format unless they opened it as `.ods` in the first place.
- **`.xlsm` is refused, and converting one loses the macros** (`IO-04`, Open,
  P1). It is the same OOXML package as `.xlsx` with a `vbaProject.bin` part; what
  stops it is a name check.
- **`.xls` is refused** deliberately (BIFF8 needs a real reader).
- No Google Sheets import, no Numbers, no HTML export, no JSON export.

### 3.19 Save

**Present but worse, and it is the first thing a user notices.**

There is no Save. `Ctrl+S` downloads a file called **`opencalc.xlsx`** to the
downloads folder and reports *"downloaded .xlsx"* [measured]. There is no name
prompt, no in-place save, no autosave, no recent-files list. An hour's work with
Excel habits produces `opencalc.xlsx`, `opencalc (1).xlsx`, `opencalc (2).xlsx`.

`beforeunload` does guard against closing a dirty tab
(`webapp/editor.sheets.js`), which is the important half. But:

- Excel and OnlyOffice save in place.
- Sheets autosaves continuously and keeps **named version history**.
- **`HIST-01` (Open, P1) is right that this is the largest single feature gap
  against every competitor named.** Undo is the only route backwards and it dies
  with the tab. The collaboration server is already an append-only op log with
  revision numbers and resume-from-revision — the history exists and nothing
  reads it as one.

*Does the divergence help?* No. Downloading is what a browser app does when it
has no storage story yet; it is not a design choice and it should not be
presented as one.

### 3.20 Protection

**Present but thin.** Cell-level Locked and Hide-formula, plus Protect this sheet
[measured — a toggle, no dialog]. Column and row resizing now honours protection
(`UX-PROT-01`, Done).

Against Excel: no password, no per-action permission list (allow sorting / allow
filtering / allow formatting), no workbook-structure protection, no per-range
permissions with different users. Sheets has protected ranges with per-editor
permissions. **Absent: digital signatures** (`SIGN-01`, Open, P2) — and the more
urgent half of that row is that opening a signed workbook and re-saving here
**silently invalidates the signature**.

### 3.21 Comments

**Present but worse.** A per-cell thread with a body, an author name, and Save /
Resolve / Delete [measured], reachable by `Shift+F2` [measured]. Resolve is the
right verb and it is there.

But it is one note per cell, not a thread: **no replies, no @mentions, no
assignment, no notification, no comment list**. Excel has threaded comments with
replies and @mentions; Sheets has replies, assignment and email notification, and
comments are one of the main reasons teams use Sheets at all. Naming the menu
item "Note" while the panel is titled "Comments" also splits Excel's own
distinction (a Note is unthreaded, a Comment is threaded) in a way that will
confuse anyone who knows it.

### 3.22 Sharing and co-editing

**The deepest engine/editor gap in the product.**

Below the line: a clustered OT server with a leader per document, epoch-fenced
appends, relay from any node, resume, presence and host callbacks, exercised by
two real browsers in CI (`docs/56`/`57`/`59`, ADR-011/012/014/017 all Accepted).
That is more collaboration engineering than most spreadsheet projects ever do.

Above the line, in the editor: **no Share button, no invite, no link, no
collaboration command of any kind.** `listCommands()` contains nothing matching
`share|collab|invite`; the presence strip is not visible and reads "Only you"
[measured]. A session is joined by putting `?doc=` on the URL
(`webapp/collab.js:150`). So the only way a user starts collaborating is by
hand-editing a query string.

Against that: Sheets *is* sharing. Excel has Share on the title bar. OnlyOffice
co-edits by default when served. Univer's collaboration appears to sit in its
commercial tier — stated as **uncertain**; do not rely on it without checking.

`COL-46` (Open, **P0**) is worth naming here because it bears directly on
whether co-editing can be turned on at all: a `$`-anchored formula rebased across
a concurrent insert **diverges between replicas with no error raised**.
`COL-44` (concurrent move has no OT) and `COL-47` (a refused chunk leaves the
client permanently deaf and silent) are open alongside it.

### 3.23 Extensibility, scripting, connected data

**Absent across the board.** No macro language, no scripting, no formula-level
extension point, no add-in surface, no external data connection.

- Excel: VBA, Office Scripts, Power Query, Power Pivot, external connections.
- Sheets: Apps Script, `IMPORTRANGE`, `IMPORTHTML`, `GOOGLEFINANCE`,
  Connected Sheets / BigQuery.
- LibreOffice: Basic and Python macros.
- OnlyOffice: a JavaScript plugin API.
- Univer: a plugin architecture is its central design idea.

No macro execution is a **deliberate** security position (`AGENTS.md` §Engineering
priorities: "no macro execution; no automatic network fetches"), and it is the
right default. *Does it help the user?* For opening an untrusted file, yes,
clearly. For a user whose workbook **is** a macro, it is a hard no — and the
honest framing is that this product does not serve that user at all today, rather
than that macros are unnecessary.

### 3.24 What-if and analysis tools

**Absent.** No Goal Seek, no Solver, no Scenario Manager, no Data Table, no
Subtotal (`docs/47`: ❌), no quick-analysis affordance (`docs/47`: ❌), no
Analyze Data / Explore. `grep -rniE 'goal.?seek|scenario|solver'` finds nothing
in `crates/` outside financial function names.

Excel has all of these; LibreOffice has Goal Seek, Solver, Scenarios and
Subtotals; Sheets has Explore. Goal Seek and Subtotal are the two an ordinary
business user actually reaches for.

**Present and better than Excel:** `Data ▸ Column stats…` gives count, populated,
empty, unique, composition and most-common values for a column [measured].
Sheets has this; Excel does not.

### 3.25 Objects on the sheet

**Absent.** No picture insert (`docs/47`: ❌), no shapes, no text boxes, no
form controls, no checkboxes.

Engine ✓ / editor ✗ in part: the headless renderer decodes and draws PNG
(`RND-06`, `RND-12`, `RND-13` all Done) and the SDK can ask for pictures
(`RND-14`, Done) — so imported images render, and the editor has no way to add
one. Sheets' checkbox and dropdown chips are among its most-used features and
have no analogue here.

---

## 4. Cross-cutting UX

### 4.1 Keyboard parity — the accumulating tax

Measured, one chord per fresh session, effects observed rather than assumed.

| Chord | Excel does | OpenCalc does [measured] | Verdict |
| --- | --- | --- | --- |
| `Ctrl+1` | Format Cells | Format cells dialog | ✅ |
| `Ctrl+K` | Insert hyperlink | Insert link dialog | ✅ |
| `Ctrl+T` | Create table | Create table dialog | ✅ |
| `Ctrl+F` / `Ctrl+H` | Find / Replace | Find bar | ✅ |
| `Shift+F11` | New sheet | sheets 1→2 | ✅ |
| `Alt+=` | AutoSum | writes `=SUM(C2:C5)` | ✅ |
| `Ctrl+;` | Today's date | writes `2026-08-29` | ✅ |
| `Ctrl+9` / `Ctrl+0` | Hide row / column | row height 20→0 / col 64→0 | ✅ |
| `Ctrl+Shift+L` | Toggle filter | *"filter on"* | ✅ |
| `Ctrl+Shift+=` | Insert cells | inserts a row | ✅ |
| `Ctrl+-` | Delete cells | deletes a row | ✅ |
| `Shift+F2` | Insert/edit note | Comments panel | ✅ |
| `Ctrl+G` / `F5` | Go To | **nothing** | ❌ |
| `Ctrl+P` | Print | opens the popup | ✅ (invisible in-page) |
| `Ctrl+Shift+;` | Current time | **nothing** | ❌ |
| `Ctrl+E` | Flash Fill | **nothing** | ❌ |
| `Alt+F1` / `F11` | Chart | **nothing** | ❌ |
| `Ctrl+Shift+O` | Select cells with notes | **nothing** | ❌ |
| `Ctrl+Shift+U` | Expand formula bar | **nothing** | ❌ |
| `Ctrl+F3` | Name Manager | **nothing** (Help advertises `F3`) | ❌ |
| `Ctrl+Alt+V` | Paste Special | *"clipboard is empty"* | ❌ |
| `Ctrl+Shift+F` | Format Cells ▸ Font | opens the **find bar** | ❌ rebound |
| `Alt+↓` | In-column pick list | moves the selection down one | ❌ rebound |

Twelve of twenty-three land; nine do nothing; two are bound to something else.
The two rebindings are the worse kind, because "nothing happened" is at least
legible while "something else happened" costs an undo and a moment of doubt.

Genuinely at parity beyond the table (`docs/73`, verified): Tab-runs and
Enter-returns-to-the-starting-column — the thing most editors get wrong;
`Ctrl+Space` / `Shift+Space` / `Ctrl+Shift+Space`; commit semantics across
Enter/Tab/Esc/click-away; `F4` anchor cycling; End mode (`UX-END-01`, Done);
`Alt+PgUp/PgDn` horizontal paging (`UX-PAGE-01`, Done).

Also measured: **undo is at least 300 steps deep** with no cap reached and a
`session_undo_label()` of `"cell edit"` [measured]. Excel caps at 100.

### 4.2 Discoverability

The menu bar is Sheets-shaped — File, Edit, View, Insert, Format, Data, Tools,
Help — with 197 commands behind it and a working `⋯` overflow. Context menus are
the strongest surface in the product: the cell menu carries Cut/Copy/Paste, a
Paste-special submenu, Insert/Delete cells with shift direction, Insert/Delete
row and column, Hide, Clear, Sort, Format cells…, Insert comment, Insert link…,
Create table…, Define name… and Filter [measured — 41 items]. That is comparable
to Excel's and better than Sheets'.

Where discoverability fails, it fails specifically:

- **No zoom control anywhere in the chrome.** The only zoom is inside
  `View ▸ Zoom` — the five `50%…200%` labels exist solely inside that hidden
  submenu [measured]. Excel, Sheets, OnlyOffice and LibreOffice all put a zoom
  readout and slider in the status bar. `docs/47` ranks this the #1 daily miss.
- **Zoom is clamped to 25–200%** against Excel's 10–400% (`docs/73`).
- **Paste Special has no menu-bar route** (§3.16).
- **Row height / column width have no menu-bar route** (`docs/47`: ❌).
- **The Help ▸ Keyboard shortcuts panel lists eight rows** [measured] out of a
  keyboard surface many times that size, and one of the eight (`F3` for Name
  Manager) does not work.
- **No command palette.** Sheets has one (`Alt+/`); Excel has "Tell me". With 197
  commands behind eight menus, a palette would recover most of what the menu
  depth costs, and it is the cheapest discoverability win available.

### 4.3 Feedback and affordances

Good: a persistent status line that names refusals in plain words
(*"nothing above or to the left to total"*, *"nothing to filter"*,
*"clipboard is empty"*) [measured]; live find-match highlighting across the grid;
marching ants that honour `prefers-reduced-motion`; hidden-band handles you can
click to unhide (`UX-HIDE-01`); a fill-options popup; number formats with live
preview.

Bad, and each is a tracker row:

- **A native `window.prompt` for row height and column width** (`UX-DLG-02`,
  Open, P1; `docs/68`).
- **Undo does not move the view to what it reversed** (`docs/47`: ❌) — undoing
  an off-screen edit is completely silent, which is how people lose confidence in
  undo.
- **Deleting a sheet does not ask** (`docs/47`: ❌). Excel and Sheets both
  confirm, because sheet deletion is not undoable in either.
- **A locked cell refuses after the user types, not before** (`docs/47`: ❌).
- **Pointer targets under 24 px in the Hyperlink dialog** —
  `{"el":"BUTTON","w":58,"h":21}` (`docs/68`).
- **A table header label drawn under its own filter arrow** (`UX-TABLE-02`, Open,
  P1).

### 4.4 Information density and rendering

Density is close to Excel's and denser than Sheets'. The grid canvas takes
1440×685 of a 1440×900 window [measured] — a 76% content share, which is good;
Sheets' toolbar-plus-header chrome takes noticeably more.

Rendering performance is genuinely strong and should not be understated.
Measured on a 20,000-cell sheet (2,000 rows × 10 columns), single-threaded in the
browser:

- seeding 20,000 cells through the transaction path: **13 ms**
- scroll+select frame time over 60 steps: **p50 16.6 ms, p95 17.5 ms, max
  17.7 ms** — rAF-bound, i.e. a clean 60 fps
- `=SUM(A1:J2000)` over all 20,000 cells: **4.5 ms**, correct value 199,990,000

Against Sheets, which visibly stalls on large sheets in a browser, and Excel,
which is native, this is the right side of the line. The 60 fps target in
`docs/30` is being met at this size.

The cost is the payload: **7.8 MB of WebAssembly**. Boot to `engine v0.0.0` was
334 ms on localhost [measured], but that number does not survive a real network.

Contrast work has been done properly: gridlines were raised from 1.13:1 to
1.44:1 light / 1.66:1 dark, with `prefers-contrast: more` clearing 3:1
(`UX-A11Y-01`, Done) — measured against Sheets' 1.32:1 and Excel's ~1.45:1.

### 4.5 Mobile and touch

**Third-tier, concretely.** Measured at 390×844 with touch:

- No horizontal overflow, and the menu bar correctly collapses to
  File/Edit/View/Insert/Format/Data + `⋯` [measured] — the layout does not break.
- **Only 5 toolbar buttons remain visible, and every one is 30 px** [measured] —
  below the 44 px iOS and 48 px Android minimums, and below the 24 px floor
  `docs/68` already enforces elsewhere.
- Tapping selects a cell [measured]; a held finger raises the context menu
  (`webapp/editor.core.js:6601`).
- The grid gets 75% of the viewport [measured], which is fine.

What is missing is any mobile design at all: no bottom action bar, no larger
touch targets, no gesture story beyond tap-and-hold, no on-screen formula
keyboard. Excel and Sheets both ship dedicated mobile apps *and* responsive web;
OnlyOffice ships mobile apps. **This product is usable on a phone in the sense
that it renders, and not in the sense that anyone would choose to work in it.**

### 4.6 Accessibility

**Better than the category in one respect and broken in another.**

The accessibility mirror is a real DOM tree — 805 `gridcell` elements, 36 `row`,
24 `columnheader`, 35 `rowheader`, with absolute `aria-rowindex` values
[measured] — plus a `menubar` with roving tabindex, a live region and a status
region. `A11Y-01`'s own note is right that this is *"better than the comparable
products"*: Sheets exposes a much thinner tree, and canvas-rendered grids
usually expose nothing.

**And it does not follow the viewport.** After moving the selection to row 201
(scrollY 3360), the mirror's first `aria-rowindex` is still `1` [measured —
reproduces `A11Y-01`, Open, P1]. A screen-reader user is being read a screen that
is no longer there. An excellent mirror pointed at the wrong rows is worth less
than a mediocre one pointed at the right ones.

Also open: the missing IME path (§3.1) is an accessibility-shaped hole, not a
polish item. There is no high-contrast theme switch beyond
`prefers-contrast: more`, and no keyboard route to the canvas-drawn chart or
image objects.

### 4.7 The first five minutes, arriving from Excel

What actually happens, in order, measured:

1. **334 ms to a usable grid** with a sample workbook already in it. Good — no
   splash, no sign-in, no template gallery. Better than Sheets' first load.
2. The chrome is legible: a Sheets-style menu bar and an Excel-style toolbar.
   Bold, alignment, borders, number formats and merge are all where a hand
   expects them.
3. Typing works. `Ctrl+1` opens Format Cells. `Alt+=` totals a column. `Ctrl+;`
   stamps the date. The first ten things an Excel user tries mostly land.
4. **Then `Ctrl+S`**, and a file lands in the downloads folder called
   `opencalc.xlsx`. This is the moment the product stops feeling like a
   spreadsheet.
5. `F5` to jump somewhere — nothing happens.
6. Look for the zoom control — there is none.
7. Look for Share — there is none.
8. Open a real Excel workbook with formula-based conditional formatting: the
   highlighting is gone.
9. Print it: an unstyled table with the wrong column widths.

Steps 1–3 are a genuinely good five minutes and better than this repository's own
self-assessment suggests. Steps 4–9 are where "third tier" comes from, and none
of them is about the toolbar.

---

## 5. Where OpenCalc genuinely leads

Not manufactured; each of these is a real advantage and several are things the
incumbents cannot easily copy.

1. **`unsafe_code = "forbid"` workspace-wide** (`Cargo.toml:46`) with
   **`overflow-checks = true` in release** (`Cargo.toml:70`) — deliberately
   against Cargo's default, on the stated ground that a wrapped index is a wrong
   cell. Excel, OnlyOffice and LibreOffice are all C/C++ engines parsing
   untrusted files. This is a category difference in the security posture, not a
   marketing point. (It is also what turned §3.2's overflow into a loud crash
   rather than a silently wrong screen — the policy working as designed, on a
   bug that should not exist.)
2. **Loss-aware preservation.** Nothing the model cannot represent is dropped
   quietly: it is counted and named in a compatibility report (`docs/34`). The
   conditional-formatting loss in §3.7 is real data loss *and* it is reported,
   which is more than any of the four competitors do. "Lossless" is a banned word
   in this repository unless the fidelity dimension is named.
3. **A byte-identical round-trip floor for unedited files.** No competitor
   promises this. Open a workbook, save it, get the same bytes.
4. **Determinism as a contract** — identical input and version give identical
   model, values, layout and bytes, and it is gated in CI.
5. **Fuzz and fidelity gates.** 11 fuzz targets covering the XLSX package, ODS,
   OOXML XML, the formula parser, number formats, snapshots, the wire operation
   format, TP1 transform and token verification; a differential fidelity harness
   against LibreOffice (`tools/casual-calc-fidelity`). Plus 21 CI jobs and some
   forty named gate steps [`tools/check-doc-claims.py`: *"61 documents, 21 CI
   jobs, every named gate is real"*], including "no document claims a gate CI
   does not run" and "every code path a document names exists" — gates about the
   gates. Nothing in the four competitors' public engineering resembles this.
6. **The embed/SDK story.** `@opencalc/sheet`, `@opencalc/engine`,
   `@opencalc/react`, a types package and Next/React examples; `?hide=` chrome
   regions sharing one vocabulary with `<opencalc-sheet>`. Univer is the only
   named competitor competing here at all, and Excel/Sheets/LibreOffice are not
   embeddable in this sense. **This is the clearest place where the product is
   ahead of four of its five competitors.** Caveat: `0.0.0` and no type
   declarations shipped (`SDK-009`).
7. **Self-hosting with no vendor.** Docker stack, a collaboration server, no
   account, no telemetry, no network fetch. Against Sheets that is the whole
   pitch; against Excel it is the compliance pitch.
8. **One core, two hosts.** The same Rust engine behind WASM and Tauri. Univer is
   TypeScript-first; OnlyOffice and LibreOffice have desktop and web builds that
   are not the same code in the same sense.
9. **Explicit, gated scale targets** — 1M cells / 60 fps / <50 ms recalc
   (`docs/30`), and the frame and recalc halves are being met at 20,000 cells
   (§4.4). No competitor states a number.
10. **Genuinely better than the incumbents at specific interactions,** all
    measured or verified: all six status-bar aggregates at once folded across
    disjoint ranges; the Name Box that *defines* names and takes five reference
    syntaxes; the per-user "Just for me" filter inline in the filter dropdown;
    live highlighting of every find match; the HTML paste path; clickable
    hidden-band handles; drag-to-freeze; Column stats; the `Edit ▸ Fill` submenu;
    an accessibility mirror deeper than anything in the category (when it points
    at the right rows).

---

## 6. Engine capable, editor cannot reach it

This pattern has now come up seven times and it deserves to be named as a
category, because it is the cheapest class of gap in the product and it keeps
being counted as a missing feature.

| # | Capability | Engine | Editor | Evidence |
| --- | --- | --- | --- | --- |
| 1 | **ODF export** | `SessionFormat::Ods` → `export_ods` | no download entry | `sdk/src/lib.rs:1575`; measured |
| 2 | **Collaboration** | clustered OT server, presence, resume, relay | no Share/invite command at all; `?doc=` only | measured; `webapp/collab.js:150` |
| 3 | **Version history** | append-only op log with revisions | nothing reads it as history | `HIST-01` |
| 4 | **Print layout** | `casual-calc-layout` + `render` paginate | print emits a bare HTML table | `IO-03`; measured |
| 5 | **Pictures** | decode + render + SDK access | no insert command | `RND-06/12/13/14`; `docs/47` |
| 6 | **`.xlsm`** | same OOXML package as `.xlsx` | refused by a name check | `IO-04` |
| 7 | **Text shaping** | wired | not drawn | `P1C-003`, Partial |

Every one of these is a host-side wiring job, not new engine work. Compare the
cost of that with §3.7's conditional formatting, which needs a new model variant,
import, export, evaluation, UI and a fidelity test.

---

## 7. Divergence ledger: does it help, or does it tax?

The owner's standard, applied one row at a time.

**Divergences that help, and should be kept.**

| Divergence | Why it helps |
| --- | --- |
| Sort auto-expands to the contiguous block instead of prompting | Removes a modal from a daily action and does the safe thing. Measured: rows stay intact and the header is detected. |
| The Name Box defines names and takes five syntaxes | Strictly more than Excel's, with no behaviour removed. |
| All six aggregates at once, folded across disjoint ranges | Strictly more information in the same space. |
| "Just for me" filter inline in the dropdown | Sheets/Excel bury the same idea behind a separate menu. |
| No macro execution by default | The correct security default for opening untrusted files. Stated in `AGENTS.md`; it is a decision, not a gap — but see §3.23 for who it excludes. |
| Compatibility report instead of silent drops | The user learns what was lost. Nobody else does this. |
| `Ctrl+Alt+0` for 100% zoom instead of `Ctrl+0` | Deliberate and correct: `Ctrl+0` is Excel's hide-column and is bound that way (`editor.core.js` comment). The label was fixed to match. Good practice — the divergence is *from Sheets*, toward Excel. |

**Divergences that only tax, and are unfinished features wearing a difference's
clothes.**

| Divergence | What it costs |
| --- | --- |
| `Ctrl+S` downloads a new file each time | A folder of `opencalc (n).xlsx`. Nobody chose this. |
| `F5` / `Ctrl+G` do nothing | Thirty years of muscle memory, answered with silence. |
| `Ctrl+Shift+F` opens Find instead of the font control | Worse than nothing: costs an Escape and a moment of doubt. |
| `Alt+↓` moves the selection instead of opening the pick list | Same class. |
| Paste Special reachable only by right-click | An Excel user's `Ctrl+Alt+V` gets "clipboard is empty". |
| Currency five interactions deep | Every non-US user, on their first workbook. |
| No zoom in the chrome | Ranked the #1 daily miss by `docs/47`, and every competitor has it in the status bar. |
| Data validation forgets the rule when reopened | Retype from scratch, every edit. |
| Custom sort capped at 3 keys | Silent ceiling; the user discovers it only when they need the fourth. |
| Sheet deletion does not confirm | Irreversible in Excel and Sheets too — which is exactly why they both ask. |
| Menu item "Note", panel titled "Comments" | Splits Excel's own Note/Comment distinction. |
| Undo does not move the view to what it reversed | Undo that shows you nothing is undo you stop trusting. |

The pattern in the second table is that almost none of these were decisions.
That is the useful finding: the product is not losing on taste, it is losing on
unfinished edges, and unfinished edges are cheaper to fix than taste.

---

## 8. The switching-blocker list, ranked by what changes a decision

Ranked by whether it stops a spreadsheet user moving here today — **not** by how
hard it is to fix.

1. **The grid crashes at row 74,567 and the document is unrecoverable** (§3.2).
   No autosave, no history, no panic message. One diagnosed line
   (`axis.rs:218`, and `:169` alongside it). Nothing else on this list matters if
   this fires.
2. **There is no save** (§3.19). `Ctrl+S` produces a download. No autosave, no
   version history, no in-place save (`HIST-01`, P1). Sheets users will not give
   this up; Excel users will not either.
3. **There is no way to share** (§3.22). A complete OT server with no button.
   And `COL-46` (P0) means two replicas of a `$`-anchored formula can diverge
   silently, so it cannot simply be switched on.
4. **Formula-based conditional formatting does not exist and is lost on import**
   (§3.7). Whole-row highlighting is impossible, and an Excel workbook that has
   it arrives here without it.
5. **Printing does not reproduce the sheet, and there is no PDF** (§3.17,
   `IO-03` P1). Column widths, merges, borders and scaling are all dropped. For
   many users the printout *is* the deliverable.
6. **Charts are seven types with no subtypes** (§3.10). No stacked, no combo, no
   secondary axis, no data labels. A stacked bar has no route at all.
7. **No macros, no scripting, no external data** (§3.23). Deliberate for macros,
   absent for the rest. Excludes an entire class of workbook outright.
8. **Pivot tables have no calculated fields, no Show Values As, no date
   grouping** (§3.9). The pivot exists; the analysis on top of it does not.
9. **The mobile experience is a desktop layout at phone size** (§4.5). 30 px
   targets, no touch design.
10. **`.xlsm` is refused and converting one drops the macros; `.ods` cannot be
    written from the UI** (§3.18, `IO-04`).
11. **Comments are notes, not threads** (§3.21). No replies, no @mentions, no
    notification.
12. **Nine Excel chords do nothing and two are rebound** (§4.1). Individually
    minor; collectively the thing that makes the product feel foreign all day.
13. **Name Manager cannot manage names** (§3.12).
14. **No Flash Fill, no Subtotal, no Goal Seek, no sparklines, no pictures**
    (§3.1, §3.24, §3.25).
15. **No zoom control in the chrome** (§4.2). Trivial to fix, ranked #1 daily
    miss by the measured map.
16. **The accessibility mirror does not follow the viewport** (`A11Y-01`, §4.6).
    Blocks a class of user completely, which is why it is on this list at all
    despite being one bug.

Items 1, 2, 3 and 4 are the four that end an evaluation on the first day. Items
15 and 16 are on the list because their cost-to-benefit ratio is the best of
anything here.

---

## 9. Where this pass contradicts a document in the tree

Each of these is a row to file, not a doc edit — `docs/14` §"Where a finding
goes" governs, and another agent owns that file.

1. **`docs/47` has two false negatives, and they are its two largest items.**
   "Ctrl+click adds a second range" and "a banked multi-range is what operations
   act on" are marked ❌ and ranked **daily / l**. Both work: bold and Delete
   both act on a two-cell bank [measured, §3.3]. The probe at
   `tests/browser/ux-sweep.mjs:150-159` reads `selectionRectForTest()`, which
   returns the active rectangle and not the bank, so it can never observe a
   multi-range. The harness that exists to stop prose drift has drifted the same
   way the prose did, and the pipeline it generates is pointing large effort at
   two solved problems.
2. **`AGENTS.md` has drifted on two counts.** Line 38 says the engine dispatches
   364 functions; it dispatches **429**
   (`crates/casual-calc-eval/src/functions/mod.rs:59`). Line 116 says the
   workspace is fifteen crates; `crates/` holds **16** (`Cargo.toml:4-19`).
   Neither is load-bearing on its own, and both are the shape of drift that
   `docs/14`'s own preamble was written about.
3. **`docs/73` #9 implies partial-selection sorting destroys data.** Measured, it
   auto-expands to the contiguous block and preserves rows (§3.5). The concern
   the row raises does not occur. `docs/73`'s own `[unverified]` marking is doing
   its job — this is the reproduction it asked for.
4. **`docs/68-UX-VISUAL-AUDIT.md` is numbered 68, and so is
   `docs/68-CLIPBOARD-HTML-PASTE.md`.** Two documents share a number in a
   repository whose tracker exists because ids were reused. **This is already a
   red gate**, on an unmodified tree: `python3 tools/check-doc-index.py` fails
   with *"docs/00-README.md:108: a second row claims number 68 (the first is
   line 107); numbers are never reused"* [measured]. The brief for this document
   also pointed at `docs/82-UX-VISUAL-AUDIT.md`, which does not exist — the
   collision is already costing readers.
5. **The Page Setup dialog offers scale / fit-to-width / fit-to-height and the
   print path applies none of them** (§3.17). Under `docs/14`'s rule this is a
   control that states a contract the code does not keep, so it is a row and the
   dialog keeps its controls.
6. **`Help ▸ Keyboard shortcuts` advertises `F3` for Name Manager; neither `F3`
   nor `Ctrl+F3` does anything** [measured].

---

## 10. What parity would take

Split by what is actually unknown, because those two piles have very different
schedules.

### 10.1 A lot of work, but fully understood

Nothing here needs a decision — only time. Roughly ordered by value per unit of
work.

- **The `i32` overflow in `session_row_offset_px` / `session_col_offset_px`**
  (`axis.rs:169`, `:218`). Widen to `i64` and clamp. One line each, plus a test
  that selects row 1,048,575 and is watched to fail first. This is the single
  highest-value change in the document.
- **Wire what the engine already has** (§6): ODS in the download menu, a Share
  button, a picture insert, `.xlsm` past the name check. Host wiring, no new
  engine work, four separate small tasks.
- **A real save story.** In-browser persistence (OPFS or IndexedDB), a document
  name, autosave, and version history read off the op log that already exists
  (`HIST-01`). Large, entirely understood, and it removes blockers #2 and part of
  #3 at once.
- **A PDF writer over the existing pagination** (`IO-03`). Decide Rust-side
  versus browser print first — `docs/19`'s host-seam rule points at Rust-side.
- **Print that reproduces the sheet.** Emit `<col>` widths, `colspan`/`rowspan`,
  real cell borders, and the scale settings the dialog already collects.
- **Formula-based conditional formatting and icon sets.** New model variants,
  import, export, evaluation and UI. Understood end to end; the import site
  already knows exactly what it is dropping (`import/src/lib.rs:1329`).
- **Chart subtypes** — stacked, 100%-stacked, combo, secondary axis, data labels.
  The display list and headless renderer are in place (`RND-10`, `RND-11`).
- **Pivot depth** — calculated fields, Show Values As, date grouping.
- **The nine dead chords and the two rebindings** (§4.1), plus a zoom control in
  the status bar, a command palette, and the Paste Special / row-height /
  column-width menu routes. Individually tiny; collectively this is what
  "feels like Excel" is made of.
- **A real dialog for row height and column width** (`UX-DLG-02`), retiring the
  `window.prompt`.
- **Make the accessibility mirror follow the viewport** (`A11Y-01`).
- **Data validation reopening with its rule**; **Name Manager that manages**;
  **more than 3 sort keys**; **Remove Duplicates with a column chooser**;
  **confirm before deleting a sheet**.
- **Threaded comments** with replies and resolve-history.
- **A mobile layout** — 44 px targets, a bottom action bar, a touch selection
  model. Large, and every design question has a published answer to copy.

### 10.2 Genuinely unsolved here

These need a decision before they need work, and picking wrong is expensive.

- **Concurrent editing correctness.** `COL-46` (P0) — `$`-anchored formulas
  diverge across a concurrent insert, silently, which is the worst class this
  system has. `COL-44` — a concurrent move has no transform. Until both are
  answered, sharing cannot be switched on even though the server is built. This
  is the one blocker on the list that is a research problem rather than a
  schedule.
- **What a document *is* when there is no server.** Version history, autosave and
  change attribution all need a storage answer, and today's common case is
  local-only with no backend (`HIST-01` names this as the open question, and it
  is policy, not mechanism).
- **The extensibility position.** No macros is the right security default and it
  excludes a real class of user (§3.23). A sandboxed scripting story — what
  language, what capability set, how it is bounded, whether it survives
  `unsafe_code = "forbid"` and the no-network rule — has not been designed. Every
  competitor has an answer; none of their answers is obviously portable here.
- **IME and complex text.** Composition input (§3.1) plus `P1C-003`'s shaped text
  that is wired and not drawn. Together these decide whether the product works
  for CJK and Indic users at all, and the canvas rendering model is what makes it
  hard.
- **What the desktop app is.** `UX-DESK-01` (Open, P1): *"still looks like a web
  application, instead of a real desktop application"*. Not a hide-the-header
  patch — `#settings-panel` lives inside `.app-header`. The question underneath
  is how much the two hosts are allowed to diverge, and that is an architecture
  decision.
- **Whether the 7.8 MB WASM payload is acceptable** on a real network, and what
  the answer implies for streaming, splitting or a server-rendered first paint.
  Nobody has measured this off localhost.
- **Fidelity beyond the fixture corpus.** The differential harness against
  LibreOffice is real, but which oracle owns which disagreement is still decided
  case by case, and the corpus is not yet large enough to say "Excel-compatible"
  without qualification.

---

## 11. Reproducing this

The editor must already be served (`python3 webapp/serve.py 8123`); do not start
a second one. Then, from `tests/browser` so that `@playwright/test` resolves:

```js
import { chromium } from "@playwright/test";
const b = await chromium.launch();
const page = await b.newPage({ viewport: { width: 1440, height: 900 } });
await page.goto("http://127.0.0.1:8123/editor.html");
await page.waitForFunction(
  () => /^engine v/.test(document.querySelector("#tb-status")?.textContent || ""),
  null, { timeout: 30000 });
// session_new() FIRST — seed() writes a styled sample workbook and a probe
// that skips this measures the sample, not the product.
await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
```

The single-line reproduction for §3.2, the most important measurement here:

```js
await page.evaluate(() => window.opencalcEditor.wasmApi().session_row_offset_px(0, 74565));
// -> 1491300
await page.evaluate(() => window.opencalcEditor.wasmApi().session_row_offset_px(0, 74566));
// -> RuntimeError: unreachable — and every later engine call fails
```

The two generated maps regenerate against the same served tree:

```
cd tests/browser && node ux-sweep.mjs --write          # docs/47
cd tests/browser && node ux-visual-audit.mjs --write   # docs/68
```

`ux-sweep.mjs --only "<substring>"` runs one row. Before trusting a ❌ from it,
check what the probe actually reads — §9 item 1 is what happens when nobody does.
