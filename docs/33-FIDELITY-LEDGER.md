# 33 — Fidelity Ledger

A **living matrix** tracking, per SpreadsheetML construct, how far it has come
along each **fidelity dimension** ([07](07-QUALITY-SECURITY-AND-COMPATIBILITY.md)
defines the dimensions; this doc tracks status). Updated in the same PR that
moves a construct — like the execution tracker
([14](14-EXECUTION-TRACKER.md)), but organized by *fidelity* rather than by task.

## Dimensions

| Column | Fidelity dimension | Question |
| --- | --- | --- |
| **Model** | structural + semantic | Is the construct represented in the normalized model? |
| **Round-trip** | package + preservation | Does it survive import → write → reopen (byte-identical when unedited)? |
| **Edit** | edit | Can it be created/changed/removed via a transaction with a correct inverse? |
| **Render** | visual | Does it lay out and paint to match the oracle? |
| **Calc** | computational + behavioral | Does it evaluate to the oracle's value (formulas/derived)? |

## Status legend

- `—` not applicable to this construct
- `✗` not started
- `~` partial (in the model / partially handled; not yet gated)
- `●` implemented **and gated** (tests + oracle where applicable)

> **Round-trip note:** `●` in the Round-trip column currently means the
> **semantic fixed point** `import → write → import` yields an equal model
> (gated in `casual-calc-export`). The stronger **byte-identical** guarantee for
> unedited packages (the retention-mode repackager) is separate and still `✗`
> (the "Whole-package (unedited)" row).

A cell moves to `●` only when its gate is green (see
[15](15-CI-AND-RELEASE-GATES.md), [18](18-SUPPORT-MATRIX.md) "Required release
evidence").

## Ledger

| Construct | Model | Round-trip | Edit | Render | Calc | Driving tracker |
| --- | --- | --- | --- | --- | --- | --- |
| Cell: number | ● | ● | ● | ~ | — | E-001 |
| Cell: boolean | ● | ● | ● | ~ | — | E-001 |
| Cell: error value | ● | ● | ● | ~ | — | E-001 |
| Shared strings | ● | ● | ● | ~ | — | E-001 |
| Inline strings | ● | ● | ● | ~ | — | E-001 |
| Grid geometry / viewport | — | — | — | ● | — | P1C-001 |
| Grid raster (PNG) | — | — | — | ● | — | P1D-001 (gridlines + content fills; glyphs pending) |
| Formulas (AST) | ● | ● | ✗ | — | ● | P2-001 |
| Formula cached value | ● | ● | — | ~ | ● | P2-001 |
| Number formats (incl. currency/literals) | ● | ● | ● | ● | — | E-001; numfmt literal runs; editor menu + $ / % |
| Styles: bold/italic/underline/font color/fill | ● | ● | ● | ~ | — | P1A-003b/d (editor toggles + pickers) |
| Styles: text rotation | ● | ● | ● | ● | — | OOXML `textRotation` kept in its own encoding; angled, vertical and stacked all render, and rows auto-grow to fit |
| Styles: text overflow (overflow / wrap / clip) | ● | ~ | ● | ● | — | Wrap is OOXML's `wrapText` and round-trips. **Clip has no SpreadsheetML attribute** — Excel always spills into empty neighbours — so it is an engine-side view choice that is *not* written to `.xlsx` and is lost on save |
| Styles: theme / indexed colors | ● | ~ | — | ● | — | `theme1.xml` palette + tint + legacy indexed table resolved on import (the form Excel's built-in cell styles use); export rewrites them as literal rgb, so the theme *linkage* is not yet round-tripped |
| Shared formulas (`<f t="shared">`) | ● | ~ | — | — | ● | Followers expanded from their master via the AST shifter; the writer emits each expanded formula rather than re-sharing |
| Styles: horizontal + vertical alignment | ● | ● | ● | ● | — | P1A-003d (`<alignment>`; editor renders + sets both axes) |
| Styles: borders | ● | ● | ● | ~ | — | P1A-003c (per-edge style+color, interned `<borders>`; editor draws + toggles; PNG borders pending) |
| Merged ranges | ● | ● | ● | ● | — | P1B-001 (import/export) + editor render & merge/unmerge |
| Column/row sizing | ● | ● | ● | ● | — | P1C-004 (widths/heights + defaults; render + editor honor them; drag-to-resize, undoable) |
| Frozen panes | ● | ● | ● | ● | — | P1B-001 (import/export) + editor render & freeze control (UX-27) |
| Defined names | ● | ● | ✗ | — | ✗ | P1B-001 |
| Date epoch (1904) | ● | ● | — | ● | — | `workbookPr/@date1904`. Serials live in the file's own system, so the flag is read, written, and threaded into date rendering. Excel's phantom 1900 leap day is reproduced rather than corrected |
| Sheet visibility | ● | ● | ● | ● | — | `<sheet state="hidden"|"veryHidden">`; veryHidden is preserved distinctly rather than flattened |
| Sheet structure | ● | ● | ✗ | ✗ | — | P1B-001 |
| Whole-package (unedited) | — | ✗ | — | — | — | P1B (retention) |
| Conditional formatting | ● | ● | ● | ◐ | — | cellIs, containsText, colorScale, dataBar, top10, aboveAverage, duplicate/uniqueValues, plus `priority` and `stopIfTrue`. Dxf carries fill, font colour and bold; other dxf effects (borders, number format, italic/underline) are not yet modelled |
| Named cell styles | ● | ● | ● | ● | — | `cellStyleXfs` + `cellStyles` (name, `builtinId`) and the `xf/@xfId` link per cell. Normal is emitted in slot 0 on write, since unlinked cells resolve `xfId="0"` to it |
| Borders | ● | ● | ● | ● | — | All four edges plus the diagonal, with `diagonalUp`/`diagonalDown`. Styles render at their real weight — `double` as two parallel lines, not one thick one |
| Alignment | ● | ● | ● | ◐ | — | All OOXML `horizontal` and `vertical` tokens model and round-trip exactly. Render: left/center/right/fill/centerContinuous/justify/distributed and vertical top/center/bottom/justify/distributed are drawn; justification stretches word gaps rather than shaping glyphs, so it is a close approximation and not oracle-exact |
| Outline / grouping | ● | ● | ● | ● | — | `outlineLevel` per row/column, `collapsed`, and `<outlinePr>` summary placement. Collapsed detail lines write `hidden="1"` — OOXML has no separate marker — with the `collapsed` flag on the summary line recording that a group did the hiding |
| Autofilter | ● | ● | ● | ● | — | `<autoFilter>` with `<filters>` (incl. `blank`) and `<customFilters>` (one or two comparisons, AND/OR). Rules are per column offset (`colId`), so the mapping survives a round-trip. Filtered rows write `hidden="1"` like any other hidden row — OOXML has no separate marker — and are re-derived from the rules on load. Not modelled: `sortState`, filter-by-colour, top-10, dynamic (date-period) filters |
| Tables (ListObjects) | ✗ | ✗ | ✗ | ✗ | ✗ | **Not handled at all** — `xl/tables/table*.xml` and the worksheet's `<tableParts>` are neither read nor written, so a table's name, style, header/total rows and its own autofilter are dropped on save, and structured references (`Table1[Sales]`) do not parse. See [18](18-SUPPORT-MATRIX.md); Phase 3 |
| Charts | ✗ (preserve-only) | ✗ | ✗ | ✗ | — | P3 |
| Pivot tables | ✗ (preserve-only) | ✗ | ✗ | ✗ | ✗ | P3 |
| VBA / macros | preserve-only | ✗ | — | — | never | — |

## How this maps to the phases

- **Model** advances during Phase 1A (import) and P1A-003/004.
- **Round-trip** advances in Phase 1B (the writer + byte-identical repackager,
  [36](36-EXPORT-AND-ROUNDTRIP-DESIGN.md)).
- **Edit** advances as `casual-calc-transaction` ops land
  ([24](24-TRANSACTION-AND-EDIT-SEMANTICS.md)).
- **Render** advances in Phase 1C/1D
  ([42](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md)). `~` means the construct
  is placed into the display list (geometry + text string) but not yet glyph-
  shaped, and not yet checked against a visual oracle; `●` there requires the
  render backend (Phase 1D) and an oracle diff.
- **Calc** advances in Phase 2 ([40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md)).
  `●` currently means a correct **full** recalc over the supported operator/
  function subset; an incremental dependency graph and the <50 ms budget, plus
  oracle-diffed function-by-function coverage, are later increments.

## Maintenance

Any PR that advances a construct on any dimension updates its row here (and the
support matrix if the construct's overall status changes). A construct is never
marked `●` on a dimension without a gating test; where an oracle applies
(computed values, rendered cells), the gate is a differential diff against
LibreOffice Calc / Excel.
