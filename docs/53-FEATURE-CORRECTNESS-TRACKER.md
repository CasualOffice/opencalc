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
| **Hyperlinks** | **0** | 0 | 0 | ✅ | ❌ | **FC-07** |
| **Tables (ListObjects)** | **0** | 0 | 0 | ✅ | ❌ | **FC-08** |
| **Print setup** | **0** | 0 | 0 | ✅ | ❌ | **FC-09** |
| **Charts, drawings, images** | **0** | 0 | 0 | ✅ | ❌ | **FC-10** |
| Rich text runs | 0 | 0 | 0 | ✅ | ❌ | **FC-11** |

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
| FC-07 | Hyperlinks: insert, edit, follow, remove | P1 | 🔴 | model + round-trip done; zero UI, zero rendering |
| FC-08 | Tables: Ctrl+T, banded rows, header filter buttons, totals row | P1 | 🔴 | model + structured references done; zero UI |
| FC-09 | Print setup: margins, orientation, headers/footers, breaks | P2 | 🔴 | carried verbatim; a UI means modelling it properly first |
| FC-10 | Charts, drawings, images | P2 | 🔴 | retained byte for byte; not drawn, not editable |

### Modelled and round-tripping, but invisible

The file is right and the screen is wrong. From
[`depth.py`](../tools/fidelity-audit/depth.py): 70 fields are ROUND-TRIP only.

| ID | Feature | Sev | Status | Note |
| --- | --- | --- | --- | --- |
| FC-11 | Rich text runs — per-character formatting inside a cell | P1 | 🔴 | round-trips exactly; renders as uniform plain text |
| FC-12 | Gradient and pattern fills | P1 | 🔴 | modelled; canvas paints solid or nothing |
| FC-13 | Superscript / subscript, underline variants, legacy font effects | P2 | 🔴 | modelled; canvas draws plain |
| FC-14 | Shrink-to-fit, reading order, justify-last-line, relative indent | P2 | 🔴 | modelled; layout ignores |
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
