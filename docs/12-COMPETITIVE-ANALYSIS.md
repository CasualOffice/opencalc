# 12 — Competitive analysis

**Checked 2026-08-29 against the editor served at `127.0.0.1:8123`; audited and
corrected 2026-08-30.** Against **Google Sheets, Microsoft Excel, OnlyOffice,
LibreOffice Calc and Univer**.

> **Read this first if you are briefing work from this document.**
>
> It is the most-briefed-from document in the repository, and by 2026-08-30 it
> had been caught wrong six times in a week — each time costing real effort,
> because a false finding sends a round of work at something that is already
> right. The corrections are `DOC-035`, `DOC-038`, `DOC-040`, `DOC-041`,
> `DOC-042`, `DOC-043`, `DOC-045` and `IO-06`, and they fall into two kinds
> worth telling apart:
>
> - **Findings that were never true.** Five chords in §4.1 that worked and
>   always had; §4.6's accessibility number, which a correct mirror produces
>   just as reliably as a broken one; §3.22's `?doc=` route, which never
>   existed; §3.17 and §6's claim that the layout crates paginate, which
>   `IO-03`'s cost estimate was built on.
> - **Findings that were true and have since been fixed.** Most of §8's
>   switching-blocker list, in a single day.
>
> Both kinds are corrected **in place, with the original struck rather than
> deleted**, so a reader can see what changed and check it. Every superseded
> claim names the row that closed it.
>
> **The operating rule this document now follows:** a measurement without a date
> is a claim about a tree that has moved, and a `file:line` is only evidence if
> somebody reads the line. Two of the six errors survived because a citation was
> quoted and never opened.

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
driven behaviours), [82](82-UX-VISUAL-AUDIT.md) (measured geometry — cited as
`docs/68` in the first version of this document, which was the number collision
`DOC-035` closed),
`docs/73-EXCEL-UX-PARITY-AUDIT.md` (a reading pass, explicitly marked
`[unverified]`), and `docs/14-EXECUTION-TRACKER.md`.

**Competitor claims are from working knowledge, not from a fresh test run**, and
where that knowledge is thin it says so inline rather than guessing. A wrong
claim about Excel would discredit the rest.

**Numbers this pass established, replacing older ones in the tree:**

| Quantity | Measured 2026-08-29 | Where | Re-derived 2026-08-30 |
| --- | --- | --- | --- |
| Engine functions | **429** | `crates/casual-calc-eval/src/functions/mod.rs` | **429**, and now *derived* rather than counted by hand — see below |
| WebAssembly bindings | 274 `#[wasm_bindgen]`, 239 `session_*` | `crates/casual-calc-wasm/src/` | **277** / **241** |
| Editor commands | **197** | `listCommands()` | not re-measured — needs a driven browser |
| Toolbar controls | **109** | measured at 1440×900 | not re-measured — needs a driven browser |
| Crates | 16 | `crates/` | **16** |
| Editor JavaScript | ~20,700 lines across 18 modules | `wc -l webapp/*.js` | **~19,900** across 18 |
| WASM payload | 7,811,572 bytes | `HEAD /pkg/casual_calc_wasm_bg.wasm` | **7,889,202 bytes** |
| Cold boot to `engine v0.0.0` | **334 ms** local | probe | not re-measured |

**Why the fourth column exists.** Everything in the second column was true on
2026-08-29 and four of the eight had moved by the next day, because they are
counts of a tree that is being worked on. A figure with no date is a figure that
will be quoted after it stops being true — which is exactly `DOC-038`, below.
The engine-function count is the one this is now safe for: `FUNCTIONS`
(`crates/casual-calc-eval/src/functions/mod.rs`) is the declared single source
of truth and `crates/casual-calc-eval/src/tests.rs` asserts every catalog entry
has a dispatch arm in `call_function`, so `FUNCTIONS.len()` and the number of
distinct dispatch names cannot drift apart — both are 429 today. The others are
`grep` counts with nothing holding them.

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
- ~~**No Go To.** `Ctrl+G` and `F5` both do nothing.~~ **Wrong, and it always
  was** (`DOC-045`): both chords focus the Name Box, which is this product's Go
  To and is genuinely good (§5). Asserted since `UX-KEY-04` by
  `tests/browser/editor.excel-keyboard-parity.spec.mjs`
  ("Ctrl+G and F5 both reach the name box").
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
- ~~`Ctrl+F3` does nothing, while the Help ▸ Keyboard shortcuts panel advertises
  `F3` for Name Manager.~~ **Both halves were wrong** (`DOC-045`): `Ctrl+F3`
  opens the Name Manager and always did, and the panel entry was corrected to
  `Ctrl+F3` by `UX-KEY-04`. What is true is only the first two bullets — the
  route in is good and there is nothing to manage once you are there.

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
  `docs/82` records `window.prompt` — *"Column A width (px)"*). A native browser
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
(`docs/82`: `{"col":3,"label":"Revenue","inkInArrowZone":6}`), and the table
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
- `docs/82` records Paste special as *"did not open — no modal or panel became
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

**`IO-05` closed most of what this section reported, and the five bullets it
carried are struck rather than deleted.** `File ▸ Print…` and `Ctrl+P` still open
a popup window and call `print()` [measured — one popup each], and the popup's
HTML still comes from `crates/casual-calc-wasm/src/objects.rs`. What it emits is
no longer a bare `<table>`:

- ~~Column widths are not emitted.~~ Emitted as `<col>` elements.
- ~~Merged cells are not emitted.~~ Emitted as `colspan`/`rowspan`
  (`objects.rs:1624`).
- ~~Cell borders do not print.~~ Per-cell borders, with the OOXML line-style
  token mapped to a CSS keyword and the width from
  `casual_calc_layout::border_width` (`print_border_css`, `:1595`).
- ~~Scale and fit-to-page are not applied.~~ Computed by
  `casual_calc_layout::print::effective_scale` and applied as `zoom` on the
  table — `zoom` and not `transform: scale()`, because a transform does not
  reflow and the browser would then paginate the unscaled box.
- ~~Header/footer field codes are stripped.~~ Substituted.

Still absent from the printout: charts, images, conditional formatting.

**No print preview, no Page Layout view, no page-break preview.** Excel has all
three; Sheets has an in-app preview with per-page controls; OnlyOffice and
LibreOffice both have page-break preview.

**Absent: PDF export** (`IO-03`, Open, P1). `grep -ri pdf crates/ webapp/`
returns only comments naming the future writer. All four competitors export PDF,
and it is the format a finished spreadsheet most often leaves the application as.

> **This paragraph used to say the tracker is right that `casual-calc-layout`
> and `casual-calc-render` already paginate, so PDF export is *"a writer over
> existing layout, not new layout"*. That is false, and `IO-03`'s cost estimate
> was built on it** (`IO-06`). Neither crate paginates:
> `crates/casual-calc-layout/src/print.rs` opens with *"Nothing here paginates.
> That is deliberate"*, and there are no page breaks, no headers/footers and no
> print scaling in either. `IO-05` built the **first piece** of a paginator —
> `casual_calc_layout::print`: paper sizes, the printable box, and the
> fit-to-page scale, which is the one thing CSS cannot express. The decision it
> recorded is that **the browser paginates and the engine supplies the
> numbers**, because a paginator here would be a second layout engine for the
> same grid — the fault `RND-10` removed — and it would still print *through*
> the browser. PDF export cannot take that route, so **`IO-03` needs a
> paginator, not a writer**, and it is the expensive direction of wrong.

### 3.18 Import and export

**Engine ✓ / editor ✗, plus two real refusals.**

- Openable: **`.xlsx`, `.ods`, `.csv`, `.tsv`, `.tab`, `.psv`** [measured via
  `openable_extensions()`].
- Downloadable: **same-format-as-opened, `.xlsx`, `.csv`, `.tsv`, `.psv`**
  [measured via `listCommands()`].
- ~~**`.ods` is missing from the download menu.**~~ **Closed by `IO-07`.** The
  finding was right and its cost estimate was not: the row is listed in §6 as
  *"host-side wiring"*, and it needed **three new WebAssembly bindings**, not a
  menu entry. The Download submenu is now derived from `writable_extensions()`
  the way Open is derived from `openable_extensions()`, so a new format appears
  without anybody remembering.
- ~~**`.xlsm` is refused, and converting one loses the macros.**~~ **Closed by
  `IO-08`**, and retaining the bytes turned out not to be the hard part — the
  *container* was. `SessionFormat::for_extension("xlsm")` resolves, and a macro
  workbook is written as the macro-enabled package it is, because a plain
  content-type declaration over a VBA project makes Excel report the file as
  damaged and repair it *by deleting the project* (see
  [36](36-EXPORT-AND-ROUNDTRIP-DESIGN.md) §"Two package flavours"). Converting
  one to `.xlsx` now names the loss per format rather than asking a question
  about a format the user did not pick. **Two residuals, both tracked:**
  `SessionFormat::for_bytes` still cannot tell the flavours apart, so `.xlsm`
  bytes with no filename open as `Xlsx` (`IO-09`, Open); and every macro fixture
  in the tree is one **this engine wrote**, so the tests prove the reader and
  writer agree and nothing about whether either matches Excel (`FID-37`, Open).
- **`.xls` is refused** deliberately (BIFF8 needs a real reader). This is the
  half of `IO-04` that is still open, and the row's own position is that it is
  worth doing only if real users still have such files — which is a measurement
  nobody has taken.
- No Google Sheets import, no Numbers, no HTML export, no JSON export.

### 3.19 Save

**Present but worse, and it is the first thing a user notices.**

**In a browser tab** there is no Save. `Ctrl+S` downloads a file called
**`opencalc.xlsx`** to the downloads folder and reports *"downloaded .xlsx"*
[measured]. There is no name prompt, no in-place save, no autosave, no
recent-files list. An hour's work with Excel habits produces `opencalc.xlsx`,
`opencalc (1).xlsx`, `opencalc (2).xlsx`.

**In the desktop shell this is now a real save** (`SAVE-02`, Phase A of
[83](83-SAVE-AUTOSAVE-AND-VERSION-HISTORY.md)): `Ctrl+S` writes back to the file
that was opened, through a temporary file in the target's own directory,
`sync_all`, permissions carried across, then a rename over — because a save that
fails partway must never leave the user with neither the old file nor the new
one. Read-only is checked explicitly, since on Unix a rename over a read-only
file succeeds and an atomic write would otherwise silently defeat the flag the
user set. `File ▸ Download` keeps writing a copy. The browser half is
`SAVE-03`/`SAVE-04`, still open.

`beforeunload` does guard against closing a dirty tab
(`webapp/editor.sheets.js`), which is the important half. But:

- Excel and OnlyOffice save in place.
- Sheets autosaves continuously and keeps **named version history**.
- **`HIST-01` (Open, P1) is right that this is the largest single feature gap
  against every competitor named.** Undo is the only route backwards and it dies
  with the tab.
- ~~The collaboration server is already an append-only op log with revision
  numbers and resume-from-revision — the history exists and nothing reads it as
  one.~~ **Measured, and it is not a history** (`SAVE-09`). There are **no
  timestamps anywhere**, no per-revision author, roughly 400–600 ops retained,
  Redis capped and TTL'd at 10k and one hour, the whole session evicted 30
  seconds after the last participant leaves, and nothing persisted. It is a
  correctly-scoped **resume buffer** that looks like a history from the outside,
  and reading it as one is what made `HIST-01` look cheaper than it is. History
  needs its own storage; [83](83-SAVE-AUTOSAVE-AND-VERSION-HISTORY.md) §6
  designs it as snapshots — forced rather than chosen, because revisions are
  positional (`ADR-011` leaves no room to rewrite history) and `COL-50` plus
  `TransformError::Unsupported` mean a log cannot be replayed reproducibly.

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

Above the line, when this was measured: **no Share button, no invite, no link,
no collaboration command of any kind.** `listCommands()` contained nothing
matching `share|collab|invite`; the presence strip was not visible and read
"Only you".

> **The route this section named never existed.** It said *"a session is joined
> by putting `?doc=` on the URL (`webapp/collab.js:150`)"*. Nothing in the
> editor has ever read `?doc=` off the **page** URL — line 150 puts `doc` on the
> **WebSocket** URL, from a key the caller already passed in. There was no
> user-reachable route at all, by query string or otherwise, so the sentence
> made the gap sound like an ergonomics problem when it was a total absence.
> This is the sharpest example of why this document dates and cites its
> measurements: a `file:line` was quoted, and it was a line about a different
> URL.

**`COL-53` closed it.** There is now `File ▸ Share…` with a dialog for the
collaboration server, the document key and the access token, and an invite link
carrying `?collab=` and `?doc=` — a real page route, which the dialog reads back
as its defaults. It is behind a **capability, off by default**: `canShare` is
`false` in every mode preset, so `File ▸ Share…` is absent from a plain editor
and `runCommand("file.share")` is refused until a host calls
`setCapabilities({ canShare: true })`. The dialog states the residual
convergence risk in plain words rather than burying it.

Against that: Sheets *is* sharing. Excel has Share on the title bar. OnlyOffice
co-edits by default when served. Univer's collaboration appears to sit in its
commercial tier — stated as **uncertain**; do not rely on it without checking.

`COL-46` — a `$`-anchored formula rebased across a concurrent insert diverging
between replicas with no error raised — was the P0 that made "turn sharing on"
unanswerable. It is **Done**, as is `COL-44` (concurrent move has no OT). What
is still open here, and is why `canShare` defaults to `false`:

- **`COL-50`** (Open, P1) — an insert meeting a delete does not converge for a
  formula **range**, and *both* answers are Excel's for the sequence that
  produced them. It needs no formula in flight: a document holding a range
  formula plus two ordinary concurrent structural edits is enough. Resolving it
  is a semantics decision about what a range means across a concurrent edit, not
  a transform bug to patch, and the property test pins the hole at exactly 68
  pairs so it cannot be narrowed away quietly.
- **`COL-47`** (Open, P1) — *half* fixed. The editor half is closed:
  `socket.onmessage` now catches a throwing `receive` and reports `desynced`,
  where before the document quietly stopped being shared and looked fine. The
  engine half reproduces: `ClientSession` still never drops a refused chunk, so
  one unmergeable submission blocks every later arrival and resending gets the
  identical refusal for ever.
- **`COL-52`** (Open, P2) — the server builds its sheet-name snapshot from a
  workbook the committed history has already been applied to.

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

> **This table was wrong about six of its twenty-three rows, and the correction
> is `DOC-045` plus `UX-KEY-04`.** The document claimed *"effects observed
> rather than assumed"*; for six chords they were not. Three (`Ctrl+G`, `F5`,
> `Ctrl+F3`) **always worked** and were reported dead. One (`Ctrl+Alt+V`)
> reporting *"clipboard is empty"* against an empty clipboard is **correct
> behaviour**, not a failure. One (`Ctrl+Shift+U`) was **worse** than reported:
> not dead but falling through to `Ctrl+U` and silently **underlining the
> selection** — the audit read it as dead because an underline on an empty cell
> is invisible, which is how a chord that modifies the document passes for one
> that does nothing. Only `Ctrl+Shift+;` was genuinely dead, and its mechanism
> is the useful one: the key delivers `{key: ":", shift: true}` and the handler
> tested `e.key === ";"`, so the whole time-stamp feature existed behind
> unreachable code. All six are now asserted in
> `tests/browser/editor.excel-keyboard-parity.spec.mjs` so they cannot be
> re-reported. `UX-KEY-04` also fixed the two rebindings and one further hole
> (`Ctrl+Shift+O`). The "Was" column below is the 2026-08-29 claim, kept so the
> correction is auditable rather than invisible.

| Chord | Excel does | OpenCalc does [measured 2026-08-30] | Verdict | Was |
| --- | --- | --- | --- | --- |
| `Ctrl+1` | Format Cells | Format cells dialog | ✅ | ✅ |
| `Ctrl+K` | Insert hyperlink | Insert link dialog | ✅ | ✅ |
| `Ctrl+T` | Create table | Create table dialog | ✅ | ✅ |
| `Ctrl+F` / `Ctrl+H` | Find / Replace | Find bar | ✅ | ✅ |
| `Shift+F11` | New sheet | sheets 1→2 | ✅ | ✅ |
| `Alt+=` | AutoSum | writes `=SUM(C2:C5)` | ✅ | ✅ |
| `Ctrl+;` | Today's date | writes the date | ✅ | ✅ |
| `Ctrl+9` / `Ctrl+0` | Hide row / column | row height 20→0 / col 64→0 | ✅ | ✅ |
| `Ctrl+Shift+L` | Toggle filter | *"filter on"* | ✅ | ✅ |
| `Ctrl+Shift+=` | Insert cells | inserts a row | ✅ | ✅ |
| `Ctrl+-` | Delete cells | deletes a row | ✅ | ✅ |
| `Shift+F2` | Insert/edit note | Comments panel | ✅ | ✅ |
| `Ctrl+P` | Print | opens the popup | ✅ (invisible in-page) | ✅ |
| `Ctrl+G` / `F5` | Go To | reaches the Name Box | ✅ **never was broken** | ❌ |
| `Ctrl+F3` | Name Manager | opens the Name Manager | ✅ **never was broken** | ❌ |
| `Ctrl+Alt+V` | Paste Special | *"clipboard is empty"* on an empty clipboard | ✅ **correct behaviour** | ❌ |
| `Ctrl+Shift+U` | Expand formula bar | expands the formula bar | ✅ (`UX-KEY-04`; was silently underlining) | ❌ |
| `Ctrl+Shift+;` | Current time | writes the time | ✅ (`UX-KEY-04`) | ❌ |
| `Ctrl+Shift+O` | Select cells with notes | selects every cell with a note | ✅ (`UX-KEY-04`) | ❌ |
| `Ctrl+Shift+F` | Format Cells ▸ Font | Format cells | ✅ (`UX-KEY-04`) | ❌ rebound |
| `Alt+↓` | In-column pick list | offers the column's entries | ✅ (`UX-KEY-04`) | ❌ rebound |
| `Ctrl+E` | Flash Fill | **nothing** | ❌ | ❌ |
| `Alt+F1` / `F11` | Chart | **nothing** | ❌ (`UX-KEY-05`) | ❌ |

**Twenty-one of twenty-three land; two do nothing; none is bound to something
else.** The 2026-08-29 tally of *"twelve of twenty-three … nine do nothing, two
are bound to something else"* understated the product by nine rows, and §3.12
and §9.6 inherited it. The two survivors are not alike, which is `UX-KEY-05`:
`Ctrl+E` (Flash Fill) has **no engine support** at all, while `Alt+F1` is
*insert a default chart from the selection* against an insert path that already
exists and seven chart types that already draw — an unbound feature, not a
missing one. `F11` (a chart **sheet**) genuinely has no model behind it.

Rebindings are the worse kind of miss, which is why the two that existed were
fixed first: "nothing happened" is at least legible, while "something else
happened" costs an undo and a moment of doubt — and `Ctrl+Shift+U` was the
extreme case, a chord that *silently changed formatting* and was recorded here
as inert.

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

- ~~**No zoom control anywhere in the chrome.**~~ **Closed** by `UX-CHROME-05`:
  a `−`/slider/`+` and a `100%` readout now sit at the right of the status bar,
  where Excel, Sheets, OnlyOffice and LibreOffice all put theirs. `docs/47`
  ranked this the #1 daily miss and it was the best cost-to-benefit item on the
  whole switching-blocker list. **Its sweep row is still weak** (`UX-MAP-04`):
  `"the zoom level is visible without opening a menu"` probes `/\d{2,3}\s*%/`
  over `.bottom-bar` and would be satisfied by a hard-coded `100%` that never
  changes; the row should set a zoom first and assert the readout followed.
  `tests/browser/editor.zoom-status.spec.mjs` does that properly, so the
  capability is gated even though the sweep row is not.
- **Zoom is clamped to 25–200%** against Excel's 10–400% (`docs/73`).
- **Paste Special has no menu-bar route** (§3.16).
- **Row height / column width have no menu-bar route** (`docs/47`: ❌).
- **The Help ▸ Keyboard shortcuts panel lists eight rows** [measured] out of a
  keyboard surface many times that size. It used to advertise `F3` for Name
  Manager, a chord that never worked; `UX-KEY-04` corrected the entry to
  `Ctrl+F3`, which does, and
  `tests/browser/editor.excel-keyboard-parity.spec.mjs` now asserts that **every
  chord the panel advertises does something** — so the panel can go on being too
  short, but it can no longer be wrong.
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
  Open, P1; `docs/82`).
- **Undo does not move the view to what it reversed** (`docs/47`: ❌) — undoing
  an off-screen edit is completely silent, which is how people lose confidence in
  undo.
- **Deleting a sheet does not ask** (`docs/47`: ❌). Excel and Sheets both
  confirm, because sheet deletion is not undoable in either.
- **A locked cell refuses after the user types, not before** (`docs/47`: ❌).
- **Pointer targets under 24 px in the Hyperlink dialog** —
  `{"el":"BUTTON","w":58,"h":21}` (`docs/82`).
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
- **Every toolbar control is reachable at 390px, and the finding this section
  used to carry was stale when it was written** (`DOC-041`). It reported *"only
  5 toolbar buttons remain visible"* as evidence the toolbar needs rethinking.
  `UX-MOB-01` had already added the `⋯` fold, so `scrollWidth == clientWidth`
  and **nothing was unreachable**: five survivors plus nine rows behind `⋯`.
  Group-collapse is the right behaviour at 390px, and wrapping the toolbar would
  cost grid height for no reachability gain. The real defect was that the five
  survivors and the nine folded rows were **30 px** — below the 44 px iOS and
  48 px Android minimums, and below the 24 px floor
  `docs/82` already enforces elsewhere. That is a hit-target problem
  (`UX-MOB-05`), not a layout one, and the distinction matters because the two
  have completely different fixes.
- Tapping selects a cell [measured]; a held finger raises the context menu
  (`webapp/editor.core.js:6601`).
- The grid gets 75% of the viewport [measured], which is fine.

What is missing is any mobile design at all: no bottom action bar, no larger
touch targets, no gesture story beyond tap-and-hold, no on-screen formula
keyboard. Excel and Sheets both ship dedicated mobile apps *and* responsive web;
OnlyOffice ships mobile apps. **This product is usable on a phone in the sense
that it renders, and not in the sense that anyone would choose to work in it.**

### 4.6 Accessibility

**Better than the category, and the "broken in another" half this section
claimed did not reproduce.**

The accessibility mirror is a real DOM tree — 805 `gridcell` elements, 36 `row`,
24 `columnheader`, 35 `rowheader`, with absolute `aria-rowindex` values
[measured] — plus a `menubar` with roving tabindex, a live region and a status
region. `A11Y-01`'s own note is right that this is *"better than the comparable
products"*: Sheets exposes a much thinner tree, and canvas-rendered grids
usually expose nothing.

**The measurement this section used to carry does not reproduce, and the real
defect was a different one** (`DOC-040`). It read: *"after moving the selection
to row 201 (scrollY 3360), the mirror's first `aria-rowindex` is still 1"*. That
probe takes `document.querySelector("[aria-rowindex]")`, which finds the
mirror's **column-header row** — `rebuildA11yGrid` gives that
`aria-rowindex="1"` deliberately, at every scroll position, by construction. So
the number is produced by a correct mirror and by a broken one alike: it would
have been satisfied by a mirror that never followed the viewport at all, and it
was *not* satisfied by one that always did. Measured on an unmodified tree at
`scrollY 3460`: `{firstVisibleRow: 173, mirrorFirstDataRow: 173}`. **The settled
mirror was correct all along.**

**What was real is *when*, and it was worse than the reported symptom.**
`scheduleA11yGrid` cleared and re-armed a 120 ms settle timer on every frame, so
a scroll that never settles never rebuilt: staleness was bounded by the length
of the gesture and nothing else. Measured over 1.5 s of continuous wheel
scrolling, the view was at row 380 and the mirror at row 160 — **220 rows
behind, and unbounded in principle.** A second defect sat in the same function:
`startGlide` never called `viewIsMoving()`, so a fling — the throw after the
finger has gone, where neither `wheel` nor `touchmove` fires — rebuilt the
mirror on all 40 of its frames.

`A11Y-01` fixed both (`webapp/editor.core.js`, `A11Y_MAX_STALE_MS = 250`, the
settle wait becoming `min(120, ceiling − age)`), and
`tests/browser/editor.a11y-viewport.spec.mjs` asserts all three properties
including the header row's `aria-rowindex="1"` at every scroll position, so the
mis-measurement cannot be made again.

**The lesson generalises, and it is why this document now dates its
measurements**: a probe that reads the first element matching a selector, in a
tree that has a deliberate constant at that position, measures the constant.
Any assertion taken *after* a scroll settles is blind to a staleness bug whose
whole nature is that it happens during motion.

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
4. **Then `Ctrl+S`**, and in a browser tab a file lands in the downloads folder
   called `opencalc.xlsx`. This is the moment the product stops feeling like a
   spreadsheet. **In the desktop shell this now saves the file** (`SAVE-02`).
5. ~~`F5` to jump somewhere — nothing happens.~~ **Never true**: `F5` focuses
   the Name Box (`DOC-045`).
6. ~~Look for the zoom control — there is none.~~ **Closed** (`UX-CHROME-05`):
   status bar, right-hand end.
7. ~~Look for Share — there is none.~~ **Closed** (`COL-53`), though a plain
   editor still has to be given the `canShare` capability before `File ▸ Share…`
   appears — so an evaluator on the default build still finds nothing, which is
   deliberate and is the honest reading of this step.
8. ~~Open a real Excel workbook with formula-based conditional formatting: the
   highlighting is gone.~~ **Closed** (`CF-01`, `CF-02`).
9. ~~Print it: an unstyled table with the wrong column widths.~~ **Closed**
   (`IO-05`) — widths, merges, borders and scaling now print. No PDF, still
   (`IO-03`).

Steps 1–3 were a genuinely good five minutes and better than this repository's
own self-assessment suggested. **Five of the six bad steps have closed in a day,
and one was never true** — which is the strongest single argument in this
document that the product's self-assessment ("bottom tier", "not usable at all")
was measuring the map rather than the territory. What remains of the bad five
minutes is step 4 in a browser tab, and step 7 on a default build.

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

| # | Capability | 2026-08-29 claim | State on 2026-08-30 |
| --- | --- | --- | --- |
| 1 | **ODF export** | engine writes ODF, no download entry | **Closed** (`IO-07`) — and it was *not* host-side wiring: it needed three new WebAssembly bindings |
| 2 | **Collaboration** | no Share/invite command; `?doc=` only | **Closed** (`COL-53`). The `?doc=` half was never true — see §3.22 |
| 3 | **Version history** | append-only op log with revisions, nothing reads it as history | **The premise was wrong** (`SAVE-09`): the op log is a resume buffer, not a history. `HIST-01` still open, and larger than this row implied |
| 4 | **Print layout** | `casual-calc-layout` + `render` paginate; print emits a bare table | **The premise was wrong** (`IO-06`): neither crate paginates. The printout half is closed by `IO-05`; `IO-03` needs a paginator |
| 5 | **Pictures** | decode + render + SDK access, no insert command | Still open — `RND-06/12/13/14`, `docs/47` |
| 6 | **`.xlsm`** | same OOXML package as `.xlsx`, refused by a name check | **Closed** (`IO-08`) — and the name check was not what stopped it; the *container* content type was |
| 7 | **Text shaping** | wired, not drawn | **Drawn** — `draw_glyphs` has a shaped arm under the `shaping` feature (`P1C-003`; the residual is font coverage, not drawing) |

> **The sentence this table used to end with was the expensive part, and it is
> withdrawn** (`DOC-042`, `IO-06`). It read: *"Every one of these is a host-side
> wiring job, not new engine work."* Three of the seven were not. `IO-07` needed
> three new engine bindings; rows 3 and 4 rested on capabilities that do not
> exist, so their fixes are new engine work of exactly the kind this category
> was invented to distinguish from; and row 6's fix was a package-level content
> type, not a name check. The *category* is real and worth naming — a
> capability the engine has and no command reaches is genuinely the cheapest
> class of gap. What is not safe is inferring the cost from membership in it.
> **Check the seam before quoting the estimate**: the question is not "does the
> engine do this" but "is there a binding that returns it", and those are
> different questions that this table conflated for seven rows.

Compare either with §3.7's conditional formatting, which needs a new model
variant, import, export, evaluation, UI and a fidelity test.

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
| `Ctrl+S` downloads a new file each time **in a browser tab** | A folder of `opencalc (n).xlsx`. Nobody chose this. Fixed in the desktop shell by `SAVE-02`; the browser half is `SAVE-03`/`SAVE-04`. |
| ~~`F5` / `Ctrl+G` do nothing~~ | **Withdrawn** — both always reached the Name Box (`DOC-045`). |
| ~~`Ctrl+Shift+F` opens Find instead of the font control~~ | **Fixed** by `UX-KEY-04`; it opens Format cells. |
| ~~`Alt+↓` moves the selection instead of opening the pick list~~ | **Fixed** by `UX-KEY-04`; it offers the column's entries. |
| ~~Paste Special reachable only by right-click~~ | **Withdrawn** — `Ctrl+Alt+V` opens the dialog; *"clipboard is empty"* against an empty clipboard is correct behaviour, not a miss (`DOC-045`). Paste Special still has no **menu-bar** route (§3.16), which is the real residual. |
| Currency five interactions deep | Every non-US user, on their first workbook. |
| No zoom in the chrome | Ranked the #1 daily miss by `docs/47`, and every competitor has it in the status bar. |
| Data validation forgets the rule when reopened | Retype from scratch, every edit. |
| Custom sort capped at 3 keys | Silent ceiling; the user discovers it only when they need the fourth. |
| Sheet deletion does not confirm | Irreversible in Excel and Sheets too — which is exactly why they both ask. |
| Menu item "Note", panel titled "Comments" | Splits Excel's own Note/Comment distinction. |
| Undo does not move the view to what it reversed | Undo that shows you nothing is undo you stop trusting. |

The pattern in the second table is that almost none of these were decisions.
That is the useful finding: the product is not losing on taste, it is losing on
unfinished edges, and unfinished edges are cheaper to fix than taste. Four of
the twelve rows have since been struck — three as fixed within a day of being
named, one as never true — which supports the finding and also warns about the
table: an "unfinished edge" is cheap to fix and therefore cheap to *mis-report*,
because nobody is surprised by one.

---

## 8. The switching-blocker list, ranked by what changes a decision

Ranked by whether it stops a spreadsheet user moving here today — **not** by how
hard it is to fix.

> **Ranked 2026-08-29; the state column is 2026-08-30.** Seven of the sixteen
> moved in a day, which is the useful thing to know about this list: it is a
> snapshot of a tree under active work, and briefing from it without checking
> the tracker sends effort at closed rows. The ordering is deliberately **not**
> re-sorted — a ranking is an argument about what matters, and re-sorting it
> silently would lose the argument along with the stale entries.

1. **The grid crashes at row 74,567 and the document is unrecoverable** (§3.2).
   No autosave, no history, no panic message. One diagnosed line
   (`axis.rs:218`, and `:169` alongside it). Nothing else on this list matters if
   this fires.
2. **There is no save** (§3.19). `Ctrl+S` produces a download. No autosave, no
   version history, no in-place save (`HIST-01`, P1). Sheets users will not give
   this up; Excel users will not either.
   **Partly closed:** the desktop shell saves in place (`SAVE-02`). The browser
   tab, autosave and history are still open (`SAVE-03`, `SAVE-04`, `SAVE-08`),
   and `SAVE-09` established that they need new storage rather than a reading of
   the op log.
3. **There is no way to share** (§3.22). A complete OT server with no button.
   And `COL-46` (P0) means two replicas of a `$`-anchored formula can diverge
   silently, so it cannot simply be switched on.
   **Closed** (`COL-53`, `COL-46`) — there is a Share dialog, behind a
   capability that is off by default because `COL-50` is still open.
4. ~~**Formula-based conditional formatting does not exist and is lost on
   import**~~ (§3.7). **Closed** — `CF-01` built formula rules and stopped
   `expression` rules being dropped on import; `CF-02` gave the editor a route
   to them.
5. **Printing does not reproduce the sheet, and there is no PDF** (§3.17,
   `IO-03` P1). ~~Column widths, merges, borders and scaling are all dropped.~~
   **The printout half is closed** (`IO-05`): widths, merges, per-cell borders
   and all three scale settings are emitted. PDF is still absent, and costs
   **more** than this entry assumed — `IO-06` established that nothing
   paginates. For many users the printout *is* the deliverable.
6. **Charts are seven types with no subtypes** (§3.10). No stacked, no combo, no
   secondary axis, no data labels. A stacked bar has no route at all.
7. **No macros, no scripting, no external data** (§3.23). Deliberate for macros,
   absent for the rest. Excludes an entire class of workbook outright.
8. **Pivot tables have no calculated fields, no Show Values As, no date
   grouping** (§3.9). The pivot exists; the analysis on top of it does not.
9. **The mobile experience is a desktop layout at phone size** (§4.5). 30 px
   targets, no touch design. **Partly closed** (`UX-MOB-01`, `UX-MOB-05` — the
   latter a P0, because a submenu opening off the right edge meant a tap could
   delete data). `UX-MOB-06` is what is left, and each part needs a design
   rather than a fix: no touch route to range selection, to a header-boundary
   resize, or to the sheet-tab menu; a status bar that does not fit at 390px;
   and no manifest, `theme-color`, `apple-touch-icon` or `color-scheme` at all.
10. ~~**`.xlsm` is refused and converting one drops the macros; `.ods` cannot be
    written from the UI**~~ (§3.18). **Both closed** (`IO-08`, `IO-07`).
    `IO-04`'s remaining half is `.xls`, which is a deliberate refusal.
11. **Comments are notes, not threads** (§3.21). No replies, no @mentions, no
    notification.
12. ~~**Nine Excel chords do nothing and two are rebound**~~ (§4.1). **Two do
    nothing and none is rebound**, and six of the eleven were never true —
    `DOC-045` and `UX-KEY-04`. The two survivors are `Ctrl+E` (Flash Fill, no
    engine support) and `Alt+F1`/`F11` (`UX-KEY-05`).
13. **Name Manager cannot manage names** (§3.12).
14. **No Flash Fill, no Subtotal, no Goal Seek, no sparklines, no pictures**
    (§3.1, §3.24, §3.25).
15. ~~**No zoom control in the chrome**~~ (§4.2). **Closed** (`UX-CHROME-05`) —
    a `−`/slider/`+` and a `100%` readout sit at the right of the status bar.
16. ~~**The accessibility mirror does not follow the viewport**~~ (§4.6).
    **Closed** (`A11Y-01`), and the reported symptom was a mis-measurement: the
    settled mirror was always correct, and the real defect was unbounded
    staleness *during* motion, plus a glide that rebuilt on every one of its
    frames.

Items 1 and 2 are what end an evaluation on the first day; 3 and 4 have since
closed. Items 15 and 16 were on the list because their cost-to-benefit ratio was
the best of anything here, and both closed within a day of being ranked — which
is the argument for keeping a ranking of this kind at all.

---

## 9. Where this pass contradicts a document in the tree

Each of these is a row to file, not a doc edit — `docs/14` §"Where a finding
goes" governs, and another agent owns that file.

**All six were filed and five are now closed.** Their outcomes are recorded
inline below rather than in a second list, because a contradiction that has been
resolved and a contradiction that has not look identical from outside and the
difference is the whole value of the section.

1. **`docs/47` has two false negatives, and they are its two largest items.**
   "Ctrl+click adds a second range" and "a banked multi-range is what operations
   act on" are marked ❌ and ranked **daily / l**. Both work: bold and Delete
   both act on a two-cell bank [measured, §3.3]. The probe at
   `tests/browser/ux-sweep.mjs:150-159` reads `selectionRectForTest()`, which
   returns the active rectangle and not the bank, so it can never observe a
   multi-range. The harness that exists to stop prose drift has drifted the same
   way the prose did, and the pipeline it generates is pointing large effort at
   two solved problems.
2. **`AGENTS.md` has drifted on two counts.** It said the engine dispatches
   364 functions; it dispatches **429**. And it said the workspace is fifteen
   crates; `crates/` holds **16**.
   **Half closed** (`DOC-038`). The function count was withdrawn rather than
   corrected — `AGENTS.md` now says *"several hundred functions dispatch"* and
   states in the same paragraph that no count is given on purpose, because a
   figure nobody can enumerate can only be carried, not maintained. That is the
   right resolution for a figure with no gate, and it is worth being explicit
   that **429 is now derivable**: `FUNCTIONS`
   (`crates/casual-calc-eval/src/functions/mod.rs`) is the declared single
   source of truth and `crates/casual-calc-eval/src/tests.rs` asserts every
   catalog entry has a dispatch arm, so the two cannot drift apart. Anyone who
   wants the number should read `FUNCTIONS.len()`, not a document.
   **The crate count is still wrong in `AGENTS.md` §"Current state"** — it says
   fifteen and there are sixteen — and that file is outside this document's
   reach; `DOC-046` is the row.
3. **`docs/73` #9 implies partial-selection sorting destroys data.** Measured, it
   auto-expands to the contiguous block and preserves rows (§3.5). The concern
   the row raises does not occur. `docs/73`'s own `[unverified]` marking is doing
   its job — this is the reproduction it asked for.
4. **The UX visual audit was numbered 68, and so was
   `docs/68-CLIPBOARD-HTML-PASTE.md`.** Two documents shared a number in a
   repository whose tracker exists because ids were reused, and
   `python3 tools/check-doc-index.py` was already red on an unmodified tree.
   **Closed.** The audit is [82](82-UX-VISUAL-AUDIT.md), and — the half a
   renumber alone would have missed — `tests/browser/ux-visual-audit.mjs` writes
   `docs/82-UX-VISUAL-AUDIT.md`, so **regenerating no longer recreates the
   collision** (`DOC-035`). A gate on the documents alone could never have been
   the whole fix, because the generator recreated the fault with nobody having
   touched a document.
5. **The Page Setup dialog offers scale / fit-to-width / fit-to-height and the
   print path applies none of them** (§3.17). Under `docs/14`'s rule this is a
   control that states a contract the code does not keep, so it is a row and the
   dialog keeps its controls.
   **Closed by `IO-05`** — the code was fixed and the controls kept, which is
   what that rule is for.
6. ~~**`Help ▸ Keyboard shortcuts` advertises `F3` for Name Manager; neither
   `F3` nor `Ctrl+F3` does anything.**~~ **Half of this was never true**
   (`DOC-045`): `Ctrl+F3` opened the Name Manager and always did. The advertised
   `F3` was real and is **closed** by `UX-KEY-04`, which corrected the panel
   entry and added a test asserting every chord the panel advertises does
   something.

**What this section is now evidence for.** Six contradictions were filed against
other documents; **two of them were the *filing* being wrong**, not the document
(items 2 and 6, in part, plus five of §4.1's rows). A pass that only ever finds
other documents at fault has not checked itself, and the corrective in both
cases was the same: read the code the claim cites, not the claim.

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
- ~~**Wire what the engine already has** (§6): ODS in the download menu, a Share
  button, a picture insert, `.xlsm` past the name check. Host wiring, no new
  engine work, four separate small tasks.~~ **Three of the four are done**
  (`IO-07`, `COL-53`, `IO-08`); picture insert remains. **And "no new engine
  work" was wrong for two of them** — see the note under §6. The category
  survives; the estimate attached to it does not.
- **A real save story.** In-browser persistence (IndexedDB), a document name,
  autosave, and version history. ~~read off the op log that already exists~~ —
  **the op log is a resume buffer, not a history** (`SAVE-09`), so history needs
  its own storage and its own snapshots. [83](83-SAVE-AUTOSAVE-AND-VERSION-HISTORY.md)
  designs the whole thing in three phases; Phase A (`SAVE-02`) has shipped.
  Large, entirely understood, and it removes blocker #2.
- **A PDF writer** (`IO-03`). ~~over the existing pagination~~ — **there is no
  existing pagination** (`IO-06`), so this needs a paginator first and is the
  most under-estimated item in this document. Decide Rust-side versus browser
  print first — `docs/19`'s host-seam rule points at Rust-side, and
  `casual_calc_layout::print` is the first piece (`IO-05`).
- ~~**Print that reproduces the sheet.**~~ **Done** (`IO-05`).
- ~~**Formula-based conditional formatting** and icon sets.~~ **Formula rules are
  done** (`CF-01`, `CF-02`); icon sets remain.
- **Chart subtypes** — stacked, 100%-stacked, combo, secondary axis, data labels.
  The display list and headless renderer are in place (`RND-10`, `RND-11`).
- **Pivot depth** — calculated fields, Show Values As, date grouping.
- ~~**The nine dead chords and the two rebindings** (§4.1), plus a zoom control
  in the status bar~~ — **both done** (`UX-KEY-04`, `UX-CHROME-05`), and six of
  the eleven chords were never dead. What remains here is a command palette and
  the Paste Special / row-height / column-width menu routes. Individually tiny;
  collectively this is what "feels like Excel" is made of.
- **A real dialog for row height and column width** (`UX-DLG-02`), retiring the
  `window.prompt` at `webapp/editor.dialogs.js`. Still open, and it is the last
  native dialog left in `webapp/` — the audit now stubs `prompt`/`confirm`/
  `alert` at page init, so there can be no others hiding the way this one did.
- ~~**Make the accessibility mirror follow the viewport** (`A11Y-01`).~~
  **Done**, and the symptom that put it on the list was not the defect — see
  §4.6.
- **Data validation reopening with its rule**; **Name Manager that manages**;
  **more than 3 sort keys**; **Remove Duplicates with a column chooser**;
  **confirm before deleting a sheet**.
- **Threaded comments** with replies and resolve-history.
- **A mobile layout** — 44 px targets, a bottom action bar, a touch selection
  model. Large, and every design question has a published answer to copy.

### 10.2 Genuinely unsolved here

These need a decision before they need work, and picking wrong is expensive.

- **Concurrent editing correctness.** ~~`COL-46` (P0) — `$`-anchored formulas
  diverge across a concurrent insert. `COL-44` — a concurrent move has no
  transform.~~ **Both answered and closed.** What is genuinely unsolved is
  **`COL-50`**: an insert meeting a delete does not converge for a formula
  **range**, and each of the two answers is the one Excel gives for the sequence
  that produced it — `apply` grows a range an insert lands inside and clamps one
  a delete overlaps, and those two rules do not commute. Resolving it means
  choosing a rule that *does* commute, which is a semantics decision about what
  a range means across a concurrent edit, not a transform bug to patch. It needs
  no formula in flight, so two people doing unremarkable things reach it. This
  is the one blocker on the list that is a research problem rather than a
  schedule, and it is why `canShare` defaults to `false`.
- **What a document *is* when there is no server.** Version history, autosave and
  change attribution all need a storage answer, and today's common case is
  local-only with no backend (`HIST-01` names this as the open question, and it
  is policy, not mechanism).
- **The extensibility position.** No macros is the right security default and it
  excludes a real class of user (§3.23). A sandboxed scripting story — what
  language, what capability set, how it is bounded, whether it survives
  `unsafe_code = "forbid"` and the no-network rule — has not been designed. Every
  competitor has an answer; none of their answers is obviously portable here.
- **IME and complex text.** Composition input (§3.1) is the live half.
  ~~plus `P1C-003`'s shaped text that is wired and not drawn~~ — **shaped text
  is drawn**: `draw_glyphs` takes the shaped path whenever one face covers the
  whole run, and `shape::run` returns glyphs in visual order, asserted for
  Hebrew. What `P1C-003` still names is **font coverage**, which is a different
  problem with a different answer: the bundled families cover Latin and Hebrew
  and not Arabic, Devanagari, Thai or CJK, and `needs_shaping()` /
  `shaping_available()` exist so a host can be told rather than guess. Shaping
  is also off in the WebAssembly build by decision (ADR-018), because the
  browser shapes the text itself. Together with IME these decide whether the
  product works for CJK and Indic users at all.
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
cd tests/browser && node ux-visual-audit.mjs --write   # docs/82
```

`ux-sweep.mjs --only "<substring>"` runs one row. Before trusting a ❌ from it,
check what the probe actually reads — §9 item 1 is what happens when nobody does.
