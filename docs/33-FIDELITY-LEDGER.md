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
| Cell: number | ● | ● | ✗ | ~ | — | P1C-001 |
| Cell: boolean | ● | ● | ✗ | ~ | — | P1C-001 |
| Cell: error value | ● | ● | ✗ | ~ | — | P1C-001 |
| Shared strings | ● | ● | ✗ | ~ | — | P1C-001 |
| Inline strings | ● | ● | ✗ | ~ | — | P1C-001 |
| Grid geometry / viewport | — | — | — | ● | — | P1C-001 |
| Grid raster (PNG) | — | — | — | ● | — | P1D-001 (gridlines + content fills; glyphs pending) |
| Formulas (AST) | ● | ● | ✗ | — | ✗ | P1B-001 |
| Formula cached value | ● | ● | — | ✗ | ✗ | P1B-001 |
| Number formats | ● | ● | ✗ | ✗ | — | P1B-001 |
| Styles: font/fill/border | ✗ | ✗ | ✗ | ✗ | — | P1A-003b |
| Merged ranges | ● | ● | ✗ | ✗ | — | P1B-001 |
| Column/row sizing | ✗ | ✗ | ✗ | ✗ | — | P1C |
| Frozen panes | ● | ● | ✗ | ✗ | — | P1B-001 |
| Defined names | ● | ● | ✗ | — | ✗ | P1B-001 |
| Sheet structure | ● | ● | ✗ | ✗ | — | P1B-001 |
| Whole-package (unedited) | — | ✗ | — | — | — | P1B (retention) |
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

## Maintenance

Any PR that advances a construct on any dimension updates its row here (and the
support matrix if the construct's overall status changes). A construct is never
marked `●` on a dimension without a gating test; where an oracle applies
(computed values, rendered cells), the gate is a differential diff against
LibreOffice Calc / Excel.
