# 73 — Excel UX parity: what a user notices, ranked

**Status:** audit, 2026-08-16. Input to [14](14-EXECUTION-TRACKER.md); rows are
created from it, not from memory of it.

## Why this exists

The user's words, after using the editor: *"many UX is not complete and not as
par as excel."* That is a claim about the whole surface, and the honest answer to
it is a list, not a reassurance.

The trigger is worth recording because it decides how much to trust this
document. [UX-GRID-02](14-EXECUTION-TRACKER.md) — the leading column that appears
to change width as you scroll — was found by **using the editor for ten minutes**,
not by reading it. Everything shipped before that came from defect reports and
code audits. So the standing assumption is that driving it finds things reading
it does not, and this audit is deliberately a *reading* pass: it produces
candidates, and the expensive step is still to run each one.

## How it was produced, and how much to trust it

Four read-only agents, one per surface, each told that a false "missing" claim is
worse than no claim and each required to cite `file:line`. Their reports are
**candidates, not findings** — the rule in CLAUDE.md is that a worker's report is
not evidence.

Three claims were verified personally against the source, and are marked
**[verified]**. The rest are marked **[unverified]** and must be reproduced
before anyone acts on them. That distinction is the point of the document; do not
quietly drop it.

Nothing here has been reproduced *in a browser* yet. Given how UX-GRID-02 was
found, expect the running to reorder this list.

---

## P1 — silent corruption or a daily action that is broken

### 1. Inserting or deleting rows/columns never moves a table **[verified]**

`crates/casual-calc-transaction/src/structural.rs` shifts merges, the sheet's
`auto_filter`, `filter_hidden`, sizing, hidden lines, the freeze boundary and
outline levels. **`sheet.tables` appears nowhere in the file.** The only
`tables.iter_mut()` in the entire workspace are in
`crates/casual-calc-wasm/src/lib.rs:1803` and `:1889` — the table operations
themselves, never the structural path.

So inserting one row above a table leaves its range, banding, filter buttons and
column list on the old row numbers. `SUM(Table1[Amount])` then reads the header
text row and drops the last record. **No error, no report, a wrong number.**
Deleting a column inside a table desynchronises `columns[]` from the cells.

This is the most damaging single defect the audit found, and it is the one to fix
first. It is also a test-shaped hole: `structural.rs`'s own module comment lists
what must be shifted "or a merge, custom height, or freeze silently" breaks —
tables were never added to that list or its tests.

### 2. Arrowing right or down out of a merged cell does nothing **[verified]**

`select()` snaps any coordinate inside a merge back to the merge's top-left
anchor (`webapp/editor.js:2586`), and the arrow step is unconditionally ±1 from
`state.sel` (`webapp/editor.js:10156`). With `B2:D2` merged and the cursor on
`B2`, ArrowRight computes `(1,2)`, `mergeAt` snaps it back to `(1,1)`, and **the
selection never moves.** Left and up work, because the anchor *is* the top-left —
so it fails asymmetrically, which is what makes it read as a frozen keyboard
rather than as a merge rule. Tab has it too.

In Excel a merge is never a wall: you land on the first cell past its edge.

### 3. Ctrl+Shift+End / Ctrl+Shift+Home throw the selection away **[verified]**

Both branches call `select()` — which collapses to a single cell — and never test
`e.shiftKey` (`webapp/editor.js:10066-10067`). The sibling handlers two dozen
lines above *do* branch on it (`10041-10043` for Ctrl+Arrow, `10166-10167` for
plain Home/End), so this is an omission rather than a decision. "Select
everything from here down" is a daily action, and it silently does the opposite.

### 4. Cut → Paste rewrites the moved formulas' references **[unverified]**

Reported at `crates/casual-calc-wasm/src/lib.rs:8731-8744`: the paste path shifts
references by the per-cell delta regardless of whether the operation was a copy
or a cut, and the `cut` flag only decides whether the source is cleared. Excel
*moves* cut cells verbatim — `=A1+1` stays `=A1+1` — and additionally repoints
every other formula that referenced the moved cells.

### 5. Esc after Cut cancels the marquee but not the cut **[unverified]**

`stopMarch()` (`webapp/editor.js:7264`) touches no engine state, and no
`session_clip_clear` binding is reported to exist. So Cut → Esc → paste elsewhere
still empties the source the user believed they had spared.

### 6. Internal paste silently skips blank source cells **[unverified]**

`clip_capture` only records cells that exist, so pasting a range with gaps leaves
stale values in the target. Excel's "skip blanks" is opt-in. The external TSV
path *does* write empty fields, so the same visual paste behaves differently
depending on where it came from.

### 7. Replace All ignores every option except Match case **[unverified]**

`session_replace_all` is reported to know only `match_case` and to do a plain
substring replace over the active sheet. With "match entire cell" ticked, find
`1` → replace `2` would rewrite every `1` inside every number and formula; with
"search every sheet" ticked, the count covers all sheets while only one changes.

### 8. Sorting moves filter-hidden rows, against a stale hidden set **[unverified]**

Sorting a filtered list is reported to permute every row in the span and leave
`filter_hidden` indexed by position, so rows that should be visible disappear and
excluded rows appear.

### 9. Sorting a partial selection has no "expand the selection" warning **[unverified]**

Excel guards this case specifically, because it is the classic
data-destroying spreadsheet mistake: sorting one column of a table in isolation
breaks every row.

### 10. `[@Column]` structured references evaluate to `#VALUE!` **[unverified]**

`resolve_structured` is reported to handle `#All`/`#Data`/`#Headers`/`#Totals`
and a bare column name, but not the row-wise `@` form — which is *the* standard
in-table formula. Files authored in Excel would show `#VALUE!` throughout.
Calculated columns are reported to be imported and exported but never applied.

### 11. Ctrl+T never asks about headers **[unverified]**

`has_headers` is reported hard-coded true, so a headerless block silently
promotes its first record to headers — putting that record outside every
aggregate.

### 12. Most conditional-format rule types are dropped on open and then on save **[unverified]**

Formula-based rules and icon sets are reported to have no model variant at all,
and unmapped rule types to hit `_ => None` **without a retention entry** — which,
if true, is the `Omitted` + `NotRetained`-without-a-count shape
[34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md) forbids, and belongs with the
FID rows rather than here.

### 13. Double-click to edit selects the whole value **[unverified]**

Reported at `webapp/editor.js:9987-9989`: the click coordinates are never mapped
to a character offset and `beginEdit` selects all, so the next keystroke wipes
the cell. F2 is correct, which makes double-click the odd one out.

### 14. Dates do not autofill as a date series **[unverified]**

A single date is reported to tile rather than step by a day, and `1-Jan, 1-Feb`
to step by the raw serial difference (+31 → `4-Mar`) rather than by month.

### 15. Ctrl+D / Ctrl+R refuse a single-cell selection **[unverified]**

Which is the common case — you do not pre-select a block.

---

## P2 — noticeably worse than Excel

Grouped, since these move together:

**Keyboard.** ~~Closed by `UX-KEY-02`.~~ Of the five defects recorded here,
three were real and are fixed: **Ctrl+Enter** now fills the selection with the
entry (through `session_fill`, so relative references adjust and it is one undo
step), **Ctrl+H** opens Replace with the caret in the replacement field, and
**Ctrl+Shift+L** toggles the filter instead of left-aligning.

The other two were already stale when this audit was written, and are recorded
as such rather than deleted, because a fixed-looking claim and a never-true one
are worth telling apart. Alt+PageDown/PageUp is **not** nested in the Ctrl
branch — it is handled at `editor.core.js:5930`, ahead of `if (mod)` on 5940.
Shift+Enter moves **up**, which is Excel's behaviour: `enterStep(back)` passes
`back ? -1 : 1` to `stepFrom`. Every one of the five is now covered by
`editor.excel-shortcuts.spec.mjs`, so the next reader gets a red test rather
than a paragraph to re-verify by hand.

**Selection.** Go To with a range leaves the active cell at the *bottom-right*,
so the view scrolls to the wrong corner and the first thing typed lands there.
Ctrl+Arrow can never leave the used range (and recomputes the used bounds by
scanning every cell on every keypress). Merged ranges are not absorbed into an
extended selection. Shift+drag does not extend, because the shift branch never
sets `state.dragging`.

**Fill.** Ctrl+drag hard-wires *copy* rather than inverting the guess (the
comment claims inversion; the code does not). No `Item 1` → `Item 2` pattern
series. Fill-handle drag does not auto-scroll, so a drag-fill is capped at the
viewport. Ctrl+D turns a month name into a series where Excel copies it.

**Undo.** Three cases split one user action into several undos — AutoSum across
columns, an Alt+Enter entry, and Delete over a multi-range selection. Undo also
does not move the selection to what it reversed, so undoing an off-screen edit is
silent.

**Editing.** The in-cell editor never grows past the column width. No column
AutoComplete of repeated text. **No IME/composition path** — a CJK user cannot
begin a cell entry by typing, only by pressing F2 first, which is an
accessibility-shaped hole rather than a polish item.

**Tables and filters.** The filter dropdown has no sort, no colour, no
condition presets (Top 10, Above Average, date periods). `SUBTOTAL(1–11)`
includes filter-hidden rows. Turning on the Totals row overwrites whatever was
below the table. Editing a header does not rename the column, so the model name
and the visible header disagree permanently.

**Clipboard.** Paste Special makes Transpose exclusive with the paste-type
choice. No paste-options popup. Outbound clipboard HTML carries far less than the
inbound parser reads — no merges, borders or widths — so round-trip fidelity is
one-way. Copy always drops manually hidden rows and columns, where Excel includes
them.

**Missing outright.** Go To Special. Find All with a result list.

---

## P3

Plain End is Sheets-style rather than Excel's End-mode. No clamp at the last
row/column. Backspace clears a whole multi-cell selection. Enter/Tab do not walk
a multi-range selection. Ctrl+A has an extra step and moves the active cell. The
`3R x 5C` readout never clears. Zoom is clamped to 25–200% against Excel's
10–400%. No View ▸ Split. Double-clicking a boundary autofits one line rather
than the selection. Filter values sort lexicographically, so "10" precedes "9".
Status-bar aggregates vanish for a single cell. Name Manager and Manage Rules are
view-and-delete only.

---

## Where it is already at parity, or ahead

This matters as much as the gaps: it is what stops the list reading as "nothing
works", and it is the positioning material.

**At parity, verified by the audit rather than assumed:** Tab runs and
Enter-returns-to-the-starting-column — the thing most editors get wrong;
Ctrl+Space / Shift+Space / Ctrl+Shift+Space; Ctrl+click multi-range including
click-to-deselect; the active cell staying put while Shift+Arrow extends; commit
semantics across Enter/Tab/Esc/click-away including a refused commit swallowing
the click; F4 anchor cycling; the fill-options popup; data validation's
stop/warning/information split; freeze panes with correct per-quadrant clipping;
grouping and outline re-indexing on insert/delete; the totals-row function list
writing both the OOXML attribute and a real `SUBTOTAL`; Format Painter including
sticky double-click; the border picker; number formats with a live preview;
marching ants honouring `prefers-reduced-motion`; and context menus comparable to
Excel's.

**Ahead of Excel:** the Name Box accepts whole-column bands, sheet-qualified
refs, comma-separated multi-ranges and *defines* a name from the selection;
hidden-band handles you can click to unhide; all six status-bar aggregates at
once, folded across disjoint ranges; live highlighting of every find match in the
grid; and an HTML paste path that recovers font, size, wrap, vertical alignment,
merges and `mso-number-format` from Excel and Sheets.

---

## What to do with this

1. The three **[verified]** P1s are ready to become tracker rows and be fixed.
   Table shifting is first: it is the only one that produces a wrong number in a
   saved file rather than an annoyance on screen.
2. Every **[unverified]** claim needs reproducing before it is scheduled. Some
   will be wrong. Finding out which is cheap next to fixing the wrong thing.
3. The conditional-formatting item belongs with the fidelity ledger, not here, if
   it survives verification — it is round-trip loss, not UX.
4. This audit read the code. The next one should **drive the editor**, because
   that is what found the defect that prompted it.
