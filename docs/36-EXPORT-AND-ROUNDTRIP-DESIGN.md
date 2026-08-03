# 36 — Export & Round-Trip Design

How OpenCalc writes `.xlsx` back out — the counterpart to import
([34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)). Owning crate:
`casual-calc-export`. This is where the **round-trip guarantees** in
[07](07-QUALITY-SECURITY-AND-COMPATIBILITY.md) are actually delivered. Phase 1B.

There are **two writers**, chosen by whether the workbook was edited:

## 1. Byte-identical repackager (unedited workbooks)

- Input: the `RetainedSource` captured at import in **retention mode**
  ([34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)).
- Behavior: re-emit each retained part **unchanged** and re-zip, so an unedited
  workbook reconstructs **bit-for-bit**.
- Determinism: parts written in a fixed order, a fixed (zeroed) ZIP timestamp,
  and a fixed compression method — reproducible bytes regardless of host or run.
- This is the "exact no-op return": `open → save` with no edits changes nothing.

## 2. Semantic writer (edited workbooks)

- Input: the normalized `Workbook` model (possibly edited).
- Output: a valid, **deterministic** `.xlsx` that opens without repair in Excel
  and LibreOffice Calc.
- Emits, from the model: `workbook.xml` (sheets, defined names, calc settings),
  each `worksheets/sheetN.xml` (cells, values, formulas, merges, panes, col/row
  props), `sharedStrings.xml`, `styles.xml` (number formats, fonts, fills,
  borders, `cellXfs`), and the parts for any modeled feature.
- **Formulas:** written from the AST via the `casual-calc-formula` pretty-printer;
  the cached value is written as `<v>` so a reader that doesn't recalc still shows
  results. `calcChain.xml` is **regenerated or omitted** — the dependency graph is
  authoritative, the chain is a rebuildable hint
  ([40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md)).
- **Determinism:** fixed part order, fixed ZIP timestamp, and IDs / relationships
  / style indices **re-minted in document order** — so the same model always
  produces the same bytes (golden-testable).
- **Unsupported constructs are skipped, never emitted malformed.**

## Opaque part merge-back

Both writers re-attach the **opaque parts** the model never consumed (charts,
pivot caches, VBA, `customXml`, signatures) from the retained side table
([34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)), without double-emitting
relationships the model already owns. This is what keeps an edited workbook from
silently losing its charts.

## Export dispositions

Mirroring the preservation ledger, the semantic writer classifies each source
region as one of `unchanged-copy | regenerate | merge | omit | block`, so what
the writer does is auditable against what import recorded. A region marked
`block` (e.g. something a security limit refused to retain) is reported, not
silently dropped.

## The round-trip fixed point (the exit gate)

The Phase 1B exit gate is a **model fixed point**:

```
import(retention) → semantic model → write → reopen → identical model
```

and, separately, the byte floor:

```
import(retention) → byte-identical repackager → identical bytes   (unedited)
```

Plus: every written file opens without repair in LibreOffice Calc. These are the
gates in [15](15-CI-AND-RELEASE-GATES.md).

## What is deferred

- Streaming export for very large sheets (write `sheetN.xml` without buffering
  the whole part) — a Phase 1B/1D performance concern, same spirit as the
  streaming reader ([28](28-XLSX-PACKAGE-READER.md)).
- `.ods` and CSV writers live in `casual-calc-ods` / the `casual-calc-io`
  adapters, not here; they follow the same deterministic discipline.
