# 53 — Feature Correctness Tracker

_Every feature the app exposes, and whether it is implemented **through the
model** or patched around it. Rows are generated from the code by
[`tools/feature-audit/inventory.py`](../tools/feature-audit/inventory.py), not
written from memory._

The fidelity phase ([51](51-FIDELITY-GAP-AUDIT.md),
[52](52-FIDELITY-TRACKER.md)) took the file layer to 95.9% structural and 0
destructive gaps. It answered *"does a workbook survive a round trip"*. This
tracker answers a different question the first one cannot:

> **Does the app do the right thing, through the model, for every feature it
> already claims to have?**

## The five questions

A feature can pass any one of these and fail the rest. "It works" in a demo
usually means only the first.

| | Question | Failure looks like |
| --- | --- | --- |
| **UI** | Is it reachable — a command, button, dialog or panel? | The construct is supported but nobody can use it |
| **MODEL** | Does it change the workbook model rather than editor-local state? | Works until reload, then gone |
| **UNDO** | Does the change go through `Operation`, so undo reverses it? | Ctrl+Z silently does nothing, or undoes the *wrong* thing |
| **ROUND-TRIP** | Does the change survive a save and reopen? | Silent data loss — the class 51 was built to find |
| **RENDER** | Does anything draw it? | The file is right and the screen is wrong |

## Current state

Measured 2026-08-09. `FNS` is the exported wasm surface for the area; `MUT` how
many mutate; `UNDO` how many are reversible.

| Area | FNS | MUT | UNDO | RT | Render | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| Cell values and formulas | 3 | 3 | 3 | ✅ | ✅ | correct |
| Number formats | 2 | — | — | ✅ | ✅ | correct (styling path) |
| Fonts and text style | 9 | — | — | ✅ | ✅ | correct (styling path) |
| Fill and colours | 3 | — | — | ✅ | ✅ | correct (styling path) |
| Borders | 3 | 1 | 1 | ✅ | ✅ | correct |
| Alignment, wrap, indent, rotation | 5 | — | — | ✅ | ✅ | correct (styling path) |
| Merge | 5 | 3 | 3 | ✅ | ✅ | correct |
| Freeze panes | 1 | 1 | 1 | ✅ | ✅ | correct |
| Rows/columns: size, hide, outline | 17 | 12 | 12 | ✅ | n/a | correct |
| Insert/delete rows, columns, cells | 5 | 5 | 5 | ✅ | n/a | correct |
| Sheets: add, rename, reorder, tab colour | 7 | 5 | 5 | ✅ | n/a | correct |
| Autofilter | 7 | 5 | 5 | ✅ | n/a | correct |
| Sort | 2 | 1 | 1 | ✅ | n/a | correct |
| Defined names | 2 | 1 | 1 | ✅ | n/a | correct |
| Clipboard and paste special | 5 | 2 | 2 | n/a | n/a | correct |
| Fill handle and series | 2 | 1 | 1 | n/a | n/a | correct |
| Find and replace | 4 | 2 | 2 | n/a | n/a | correct |
| Conditional formatting | 6 | 5 | 5 | ✅ | ✅ | correct |
| Comments and threads | 6 | 3 | 3 | ✅ | ✅ | correct |
| Data validation | 5 | 3 | 3 | ✅ | ❌ | undo correct; FC-16 render |
| Named cell styles | 2 | 1 | 0* | ✅ | ❌ | correct (*definition registration only) |
| Sheet visibility | 2 | 1 | 1 | ✅ | n/a | correct |
| Sheet protection | 2 | 1 | 1 | ✅ | ❌ | correct |
| Hyperlinks | 3 | 1 | 1 | ✅ | ✅ | correct |
| Tables (ListObjects) | 5 | 3 | 3 | ✅ | ✅ | correct |
| **Print setup** | **0** | 0 | 0 | ✅ | ❌ | **FC-09** |
| **Charts, drawings, images** | **0** | 0 | 0 | ✅ | ❌ | **FC-10** |
| Rich text runs | — | — | — | ✅ | ✅ | correct (render) |

## Rows

Status: 🔴 Todo · 🟡 Partial-in-progress · ✅ Done.

### Undo is broken, not missing — the worst kind

These fourteen mutations write straight to `workbook_mut()` and never construct
an `Operation`. That is not "undo is unimplemented": undo *appears* to work,
because the button is enabled and the history has entries. Pressing it after
editing a comment reverses **the last cell edit instead**, which is worse than
doing nothing — the user loses work they did not ask to lose, in a place they
were not looking.

| ID | Feature | Sev | Status | Note |
| --- | --- | --- | --- | --- |
| FC-01 | Conditional formatting: `add_cf`, `clear_cf`, `delete_cf_rule`, `reorder_cf_rule`, `set_cf_stop` | P0 | ✅ | Fixed by widening the mechanism, not by patching the call sites: `SheetMetadata` — which already had a proven inverse for the positional state — now also carries validations, conditional formats, comments, visibility and protection, so all thirteen mutations route through the existing `edit_sheet_metadata` and become reversible at once. Adding five new `Operation` variants would have meant five new inverses to get right. `session_set_sheet_visibility` keeps its "at least one visible sheet" check *outside* the closure, since an operation closure has nowhere to report an error to. **The regression test was verified by reintroducing the bug**: with the bypass restored it fails with the real symptom — `undo after a comment edit destroyed the preceding cell edit` — and passes once fixed. A guard never seen to fail is a comment. |
| FC-02 | Comments and threads: `set_comment`, `reply_comment`, `resolve_comment` | P0 | ✅ | Fixed by widening the mechanism, not by patching the call sites: `SheetMetadata` — which already had a proven inverse for the positional state — now also carries validations, conditional formats, comments, visibility and protection, so all thirteen mutations route through the existing `edit_sheet_metadata` and become reversible at once. Adding five new `Operation` variants would have meant five new inverses to get right. `session_set_sheet_visibility` keeps its "at least one visible sheet" check *outside* the closure, since an operation closure has nowhere to report an error to. **The regression test was verified by reintroducing the bug**: with the bypass restored it fails with the real symptom — `undo after a comment edit destroyed the preceding cell edit` — and passes once fixed. A guard never seen to fail is a comment. |
| FC-03 | Data validation: `set_validation`, `set_list_validation`, `clear_validation` | P0 | ✅ | Fixed by widening the mechanism, not by patching the call sites: `SheetMetadata` — which already had a proven inverse for the positional state — now also carries validations, conditional formats, comments, visibility and protection, so all thirteen mutations route through the existing `edit_sheet_metadata` and become reversible at once. Adding five new `Operation` variants would have meant five new inverses to get right. `session_set_sheet_visibility` keeps its "at least one visible sheet" check *outside* the closure, since an operation closure has nowhere to report an error to. **The regression test was verified by reintroducing the bug**: with the bypass restored it fails with the real symptom — `undo after a comment edit destroyed the preceding cell edit` — and passes once fixed. A guard never seen to fail is a comment. |
| FC-04 | Named cell styles: `apply_cell_style` | P0 | ✅ | Already correct in its user-visible effect: the styling goes through `apply_style_range`, which builds an `EditOperation::Batch`. What escapes the log is the *registration* of a built-in style definition in `Workbook::cell_styles` — inert, idempotent, and re-registered on the next use. Left alone deliberately rather than contorted into an operation for a number's sake; the audit tool still counts it, which is the honest reading. |
| FC-05 | Sheet visibility: `set_sheet_visibility` | P0 | ✅ | Fixed by widening the mechanism, not by patching the call sites: `SheetMetadata` — which already had a proven inverse for the positional state — now also carries validations, conditional formats, comments, visibility and protection, so all thirteen mutations route through the existing `edit_sheet_metadata` and become reversible at once. Adding five new `Operation` variants would have meant five new inverses to get right. `session_set_sheet_visibility` keeps its "at least one visible sheet" check *outside* the closure, since an operation closure has nowhere to report an error to. **The regression test was verified by reintroducing the bug**: with the bypass restored it fails with the real symptom — `undo after a comment edit destroyed the preceding cell edit` — and passes once fixed. A guard never seen to fail is a comment. |
| FC-06 | Sheet protection: `set_sheet_protected` | P0 | ✅ | Fixed by widening the mechanism, not by patching the call sites: `SheetMetadata` — which already had a proven inverse for the positional state — now also carries validations, conditional formats, comments, visibility and protection, so all thirteen mutations route through the existing `edit_sheet_metadata` and become reversible at once. Adding five new `Operation` variants would have meant five new inverses to get right. `session_set_sheet_visibility` keeps its "at least one visible sheet" check *outside* the closure, since an operation closure has nowhere to report an error to. **The regression test was verified by reintroducing the bug**: with the bypass restored it fails with the real symptom — `undo after a comment edit destroyed the preceding cell edit` — and passes once fixed. A guard never seen to fail is a comment. |

The fix is the same in every case: extend `Operation` so the change is
expressible, or route it through the existing `SetSheetMetadata` capture/install
pair that sheet-level state already uses. Not a per-call patch.

### Supported in the file, unreachable in the app

Constructs that round-trip correctly and have **no way to create or edit them**.
Opening a workbook preserves them; the user cannot make one.

| ID | Feature | Sev | Status | Note |
| --- | --- | --- | --- | --- |
| FC-07 | Hyperlinks: insert, edit, follow, remove | P1 | ✅ | Insert/edit dialog, underline + accent cue on the grid, hover tooltip, click to follow, and remove — all through `SheetMetadata`, so the whole thing is undoable. **The dialog offers both destinations rather than a mode switch**, because the schema treats `r:id` and `location` as independent and a link may carry both ("open that document at this anchor"); making the user pick one would model the feature more narrowly than the format does. Following is guarded three ways so it cannot hijack ordinary work: not while picking a formula reference, not with a modifier held (Ctrl-click selects a linked cell without leaving), and not on a cell that is already selected — that is a drag or a rename starting, not a request to navigate. External targets open with `noopener`, or the opened page can reach back through `window.opener` and navigate this one. Internal anchors resolve through `parseNameRange`, the same parser the name box uses, so a link accepts exactly the references a user can type. Verified in-browser end to end including undo removing the link and redo restoring it. |
| FC-08 | Tables: Ctrl+T, banded rows, header filter buttons, totals row | P1 | ✅ | Ctrl+T and a context-menu command create a table; header shading and banded rows are painted; "Convert to range" removes one. All through `SheetMetadata`, so creating or removing a table is a single undo step. **The header question is asked, not guessed**: whether the first row is a header decides the column *names*, and a structured reference resolves by name — a wrong guess leaves every `Table[Column]` pointing at the wrong column while still resolving, which is silent. Empty headers become `Column1…`, duplicates get a suffix, and the table name is made unique across the workbook, all for the same reason: a duplicate name resolves to whichever came first and reads the wrong data. A single-cell selection expands to the surrounding block via `session_block_bounds` rather than asking the user to select the table first. Bands count from the first *data* row, so a header does not shift the stripe pattern by one. Band colours are theme-derived and deliberately faint — a band strong enough to compete with a cell's own fill makes the user's formatting harder to see, not easier. Verified in-browser: Ctrl+T on one cell, then `=SUM(Table[Amount])` resolving to 450 against the table the UI had just made. **Completed further**: header filter buttons (the existing autofilter button renderer generalised to take a list of header regions rather than the sheet's single filter, so tables and the sheet filter share one implementation), a totals-row toggle that moves the table's bottom edge — leaving the range alone would make the last data row read as the totals row — and **auto-expand**, so typing directly below or beside a table takes the cell in. That last one is what makes a table worth having: the range, the banding and every structured reference follow the data instead of needing to be re-pointed by hand. Expansion is limited to exactly one row below or one column right; anything further would swallow unrelated data. A new column gets a generated name, or a structured reference to it has nothing to resolve against. **Still missing**: the table style gallery and calculated columns. |
| FC-09 | Print setup: margins, orientation, headers/footers, breaks | P2 | 🔴 | carried verbatim; a UI means modelling it properly first |
| FC-10 | Charts, drawings, images | P2 | 🔴 | retained byte for byte; not drawn, not editable |

### Modelled and round-tripping, but invisible

The file is right and the screen is wrong. From
[`depth.py`](../tools/fidelity-audit/depth.py): 70 fields are ROUND-TRIP only.

| ID | Feature | Sev | Status | Note |
| --- | --- | --- | --- | --- |
| FC-11 | Rich text runs — per-character formatting inside a cell | P1 | ✅ | The canvas draws per-run formatting: bold, italic, strike, underline, colour, size, typeface and super/subscript. Runs travel in the existing cell payload and only when the string actually has them — a `runs` key on every cell would bloat a screenful for the overwhelming majority that are plain. **A run inherits rather than replaces**: `<rPr>` carries only what differs, so treating an absent property as a reset would drop the cell's own font on every partially-formatted string. **Measurement is per run too** — each has its own font, so measuring the concatenation with the cell's font gives a width that is wrong wherever they differ, and the overflow scan then borrows the wrong number of neighbouring columns; alignment drifts with it. Super/subscript is drawn smaller and offset by a fraction of the cell size, so it tracks a resized font. Verified by importing a real `.xlsx` with three differing runs rather than by injecting state. LIVE depth 58.7% → 61.3%; fidelity unchanged at 95.9% / P0 0. |
| FC-12 | Gradient and pattern fills | P1 | ✅ | Gradients and patterns paint. **A pattern's *background* fills the cell and its foreground draws the motif on top** — painting only the foreground as a solid is what made every patterned cell a flat block of the wrong colour. The motifs are approximated by density rather than matched hatch for hatch: the alternative is eighteen bitmaps for something almost no sheet uses, and a wrong-density hatch still reads as a hatch, whereas a solid block reads as a fill the user did not choose. A gradient replaces the pattern rather than joining it, matching `<fill>`, which holds one or the other. |
| FC-13 | Superscript / subscript, underline variants, legacy font effects | P2 | ✅ | Super/subscript and the underline variants now draw on cell fonts, as they already did on runs. The vertical shift is applied **before measuring**, so the spill scan and alignment use the size actually drawn rather than the nominal one — measuring first and shrinking after would centre the text as though it were full size. A double or accounting underline is a second rule below the first; drawing one line for all four kinds is what made a ledger's accounting underline read as an ordinary one. The payload carries the *kind* beside the existing boolean rather than replacing it, so every current reader of `u` keeps working. `sup` is a separate key from `va`, which is already vertical alignment — two different properties with confusingly similar names in the format. Verified by importing a workbook with all four. |
| FC-14 | Shrink-to-fit, reading order, justify-last-line, relative indent | P2 | 🟡 | Shrink-to-fit scales the font down until the text fits its own cell. **It also had to be excluded from the overflow scan**, alongside `clip`: both mean "stay inside this cell", so borrowing a neighbour first and then shrinking would leave the text scaled down *and* overhanging — worse than either behaviour alone. Reading order, justify-last-line and relative indent remain modelled but unrendered; they need bidi layout rather than a paint change. |
| FC-15 | `quotePrefix` indicator in the grid | P2 | 🔴 | stored; no visual cue that a value is forced text |

## Working rules

Carried from [52](52-FIDELITY-TRACKER.md), because they are what made it
converge:

1. **One row at a time, to completion.** No part-done rows.
2. **Fix the mechanism, not the call site.** Six separate patches to make six
   commands undoable is the same mistake at larger scale. Extend `Operation`
   once.
3. **Verify before claiming.** Re-run `inventory.py`; the number in the note is
   the measured one.
4. **Record what was actually wrong**, not what changed. The diff shows the
   what; the note is the only place the why survives.
5. **A trap found is a test written.** With its reason in the assertion message.

## A note on this tracker's own tooling

The first version of `inventory.py` read each exported function's body and
reported every toolbar styling command — bold, fill, alignment, number format —
as un-undoable. They are all fine: they delegate to `apply_style_range`, which
builds an `EditOperation::Batch`. The tool could not see one level of
indirection and produced a confident, wrong answer.

It now follows delegation into private helpers. Worth recording because it is
the same failure this tracker exists to catch, committed by the instrument
rather than the code — and because a measurement nobody checks is just an
assertion with a number attached.
