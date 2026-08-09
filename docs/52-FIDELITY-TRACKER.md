# 52 — Fidelity Execution Tracker

_The durable backlog for closing the gaps measured in
[51 — Fidelity Gap Audit](51-FIDELITY-GAP-AUDIT.md). Rows are created from
measurements, not recollection, and are worked **one at a time to completion**
(implement → verify → commit) as the UX tracker
([50](50-UX-COMPLETENESS-TRACKER.md)) was._

Status: 🔴 Todo · 🟡 Partial-in-progress · ✅ Done.

Severity is judged by **what reaches the file**, not by how it looks on screen.
That distinction is why five rows on the last tracker were mis-rated as polish
when they were silent data loss.

- **P0** — silent data loss: the file is opened, saved, and something is
  irrecoverably gone with nothing on screen to say so.
- **P1** — visible loss: the user can see it went missing.
- **P2** — cosmetic, rare, or legacy.

## Product goal

**A viable alternative to Microsoft Excel. Nothing below that quality bar is
acceptable.** This is the standard every row here is measured against, and it
has three consequences worth stating plainly so the numbers below are not
mistaken for the finish line:

1. **>90% structural is a milestone, not the goal.** The remaining 10% still
   loses somebody's work. The goal is that a real workbook opens, edits and
   saves without the user discovering later that something went missing.
2. **Constructs currently out of scope come back into it.** Charts, pivot
   tables and drawings are excluded from today's denominator
   ([51](51-FIDELITY-GAP-AUDIT.md) names each exclusion). An Excel alternative
   cannot drop a chart on save, so they are deferred, not dismissed.
3. **Coverage is not correctness.** A `YIELD` that returns a plausible wrong
   number scores the same as a right one on the coverage tool and is worse than
   not having it — a missing function is visible, a wrong one is not. Financial
   and statistical rows are gated on the spec's worked examples as test vectors,
   not on my recollection of the formulas.

## Targets

| Axis | Baseline | Current | Target | Measured by |
| --- | --- | --- | --- | --- |
| Structural | 54.8% | 54.8% | **>90%** | `tools/fidelity-audit/score.py` |
| Functions | 22.8% | 32.3% | **100%** | `tools/fidelity-audit/functions.py` |

Both numbers come from a tool, so a row cannot be called done on an opinion.
Re-run both after every row and record the new figure in the note.

## Structural

| ID | Construct | Sev | Status | Note |
| --- | --- | --- | --- | --- |
| FID-01 | Hyperlinks: `hyperlink`, `hyperlinks`, and the sheet rels that hold the targets | P0 | 🔴 | model + read/write + rels part |
| FID-02 | Rich text runs: `r`/`rPr` in `CT_Rst` — per-character formatting in a cell | P0 | 🔴 | `sst`, `comments`, inline strings; touches the renderer |
| FID-03 | Tables / ListObjects: `xl/tables/*`, `tableParts`, `tableColumn(s)`, `tableStyleInfo`, `calculatedColumnFormula`, `totalsRowFormula` | P0 | 🔴 | also unblocks structured references |
| FID-04 | `xf/@quotePrefix` — the marker forcing a numeric-looking value to stay text | P0 | 🔴 | one attribute, silent corruption without it |
| FID-05 | `vertAlign` — superscript and subscript | P0 | 🔴 | font + run level |
| FID-06 | `u/@val` — double / accounting underline variants | P0 | 🔴 | currently collapses to a bool |
| FID-07 | Gradient fills: `gradientFill`, `stop` | P0 | 🔴 | cell loses its background entirely today |
| FID-08 | External references: `externalReference(s)` | P0 | 🔴 | formulas referencing other workbooks break |
| FID-09 | Print setup: `pageMargins`, `pageSetup`, `printOptions`, `headerFooter` + odd/even/first, `rowBreaks`, `colBreaks`, `brk`, `pageSetUpPr` | P1 | 🔴 | ~13 elements, mostly carry-through |
| FID-10 | Inside borders: `horizontal`, `vertical` in `CT_Border` | P1 | 🔴 | neither modelled nor written |
| FID-11 | `bgColor` — the second colour of a pattern fill | P1 | 🔴 | non-solid patterns lose half their definition |
| FID-12 | Row and column default styles: `col/@style`, `row/@s`, `row/@customFormat` | P1 | 🔴 | whole-column formatting is lost |
| FID-13 | `sheetView`: `rightToLeft`, `showFormulas`, `showZeros`, `tabSelected` | P1 | 🔴 | RTL sheets come back LTR |
| FID-14 | `sheetFormatPr`: `zeroHeight`, `thickTop`, `thickBottom`, `outlineLevelRow`, `outlineLevelCol`, `customHeight` | P1 | 🔴 | sheet-wide row defaults |
| FID-15 | `alignment`: `shrinkToFit`, `readingOrder`, `relativeIndent`, `justifyLastLine` | P1 | 🔴 | shrink-to-fit is common |
| FID-16 | `dataValidation`: `showDropDown`, `errorStyle`, `imeMode` | P1 | 🔴 | error severity degrades to default |
| FID-17 | Font metadata: `family`, `scheme`, `charset` | P1 | 🔴 | theme-linked font resolution degrades |
| FID-18 | `indexedColors` / `rgbColor` — the legacy palette | P1 | 🔴 | read on import, never written back |
| FID-19 | Drawings, images, charts, OLE — scoped preserve-only but **not preserved** | P1 | 🔴 | needs the retention path, not the model |
| FID-20 | `workbook`: `calcPr`, `bookViews`/`workbookView`, `fileVersion`, `fileSharing`, `workbookProtection` | P1 | 🔴 | the weakest part at 28.6% |
| FID-21 | `<f>` attributes: array formulas (`t="array"`, `ref`) and data tables (`t="dataTable"`, `dt2D`, `dtr`, `r1`, `r2`, `del1`, `del2`), plus `ca`/`aca` | P1 | 🔴 | belongs with the calc phase |
| FID-22 | Filter refinements: `top10`, `dynamicFilter`, `colorFilter`, `iconFilter`, `dateGroupItem`, `sortState`, `sortCondition` | P2 | 🔴 | autofilter completeness |
| FID-23 | Conditional formatting: `cfvo`, `iconSet`, `colorScale` | P2 | 🔴 | rule types beyond the ones modelled |
| FID-24 | `tableStyles`, `tableStyle`, `tableStyleElement` in `styleSheet` | P2 | 🔴 | custom table styles; pairs with FID-03 |
| FID-25 | Remaining P2: `dimension`, `selection`, `ignoredError(s)`, `protectedRange(s)`, `col/@bestFit`, `col/@phonetic`, `row/@spans`, `row/@thickTop`/`thickBot`, `definedName/@hidden`, `xf/@pivotButton`, `border/@outline` | P2 | 🔴 | cheap carry-through, good score-per-effort |

## Function library

Batched by cluster. Each batch is implemented against the semantics in
ECMA-376 §18.17.7, with tests for the cases where a spreadsheet deviates from
the obvious implementation — those are the ones that bite.

| ID | Cluster | Count | Status | Note |
| --- | --- | --- | --- | --- |
| FN-01 | Math and trigonometry | 41 | ✅ | Trig with its reciprocals and hyperbolics, inverse/log family, EVEN/ODD/MROUND/QUOTIENT, combinatorics, GCD/LCM, SUMSQ, SERIESSUM, PI. Three real traps: **ATAN2 takes x then y**, the reverse of every maths library, so forwarding in order mirrors every angle about the diagonal while still looking plausible; domain failures are **#NUM!, not NaN** (`ASIN(2)`, `LN(-1)`) and zero denominators are #DIV/0!, since a NaN in a cell compares and formats as nonsense; and **COMBIN accumulates term by term**, because `n!/(k!(n-k)!)` overflows f64 long before the answer does and returns #NUM! for COMBIN(100,2)=4950. Also EVEN(0)=0 and ODD(0)=1 against the general round-away rule, and FACT(171) is #NUM! not infinity. 9 tests. 22.8% → 32.3% |
| FN-02 | Text | 17 | 🔴 | CHAR, CODE, CLEAN, DOLLAR, FIXED, NUMBERVALUE, UNICHAR, UNICODE, T, plus the byte-oriented `*B` variants and the East Asian ASC/JIS/DBCS/PHONETIC/BAHTTEXT |
| FN-03 | Date and time | 16 | 🔴 | NOW, TODAY, TIME, TIMEVALUE, DATEVALUE, HOUR, MINUTE, SECOND, DATEDIF, DAYS360, WEEKNUM, ISOWEEKNUM, NETWORKDAYS(.INTL), WORKDAY(.INTL). Needs a clock seam so tests stay deterministic |
| FN-04 | Logical and information | 8 | 🔴 | TRUE, FALSE, N, TYPE, ISREF, ERROR.TYPE, CELL, INFO |
| FN-05 | Lookup and reference | 9 | 🔴 | ADDRESS, AREAS, INDIRECT, OFFSET, LOOKUP, TRANSPOSE, HYPERLINK, GETPIVOTDATA, RTD |
| FN-06 | Engineering | 39 | 🔴 | Base conversion (BIN/OCT/DEC/HEX), bit operations, complex numbers (IM*), Bessel, ERF/ERFC, CONVERT, DELTA, GESTEP |
| FN-07 | Financial | 53 | 🔴 | Annuities, depreciation, bonds, coupons, yields. **Day-count conventions decide correctness**; needs the spec's own worked examples as test vectors |
| FN-08 | Statistical and database | 92 | 🔴 | Descriptive statistics, distributions and their inverses, regression, the D* family. The distribution inverses need real numerics, not closed forms |

## Working rules

Carried over from the UX tracker, because they are what made it converge:

1. **One row at a time, to completion.** No part-done rows left behind.
2. **Verify before claiming.** Structural rows re-run the audit; function rows
   re-run the coverage tool. The number in the note is the measured one.
3. **Record what was actually wrong**, not what was changed. A row's note is the
   only place the reasoning survives — the diff shows the what, never the why.
4. **A trap found is a test written.** Every case where a spreadsheet deviates
   from the obvious implementation gets a test that pins it, with the reason in
   the assertion message.
