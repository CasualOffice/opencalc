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

## Release signal

`tools/fidelity-audit/status.py` prints all of this; it reads the status column
below, so the tracker and the signal cannot drift apart.

**Coverage alone is a poor gate.** Going 54.8% → 70% by implementing obscure
attributes matters far less than eliminating the last construct that silently
deletes somebody's work, and a single percentage cannot tell those apart. So the
destructive-loss counts sit beside it and are what actually gate a milestone.

| Signal | Baseline | Current |
| --- | --- | --- |
| Structural coverage | 54.8% | 64.5% |
| Function coverage | 22.8% | 32.3% |
| **P0 destructive remaining** | 8 | 2 |
| P1 visible-loss remaining | 13 | 13 |
| P2 compatibility remaining | 4 | 4 |

## Milestones

| Milestone | Structural | P0 | P1 | Functions | Also |
| --- | --- | --- | --- | --- | --- |
| **Alpha fidelity** | 75%+ | 0 | — | 50%+ | |
| **Beta / daily use** | 90%+ | 0 | ~0 | 75%+ | |
| **Excel alternative** | 98–100% | 0 | 0 | 100% | charts preserve + render · pivots preserve then model · drawings preserve + edit · formula correctness validated against a spec corpus |

The ordering is deliberate: **P0 goes to zero before coverage is chased.** A
release that scores 90% while still dropping one construct on save is worse than
one that scores 75% and loses nothing, because the first kind of failure is
invisible until someone reopens the file.

## Structural

| ID | Construct | Sev | Status | Note |
| --- | --- | --- | --- | --- |
| FID-01 | Hyperlinks: `hyperlink`, `hyperlinks`, and the sheet rels that hold the targets | P0 | ✅ | Modelled as `Sheet::hyperlinks`, read and written with the sheet rels that carry the targets. A link is external, internal, or both, matching the schema where `r:id` and `location` are independent — one type with two optional destinations rather than an enum, since a link can legitimately carry both ("open that document at this anchor"). Relationship ids stay out of the model: they are a packaging detail with no meaning once the file is open, so the target is resolved on import and re-minted on export. **Three things this turned up.** `Relationship` had no `TargetMode`, which is the only thing separating a URL from a path inside the zip — resolving an external target as a part path mangles it silently. The sheet rels part was written *only* when a sheet had comments, so a sheet with links and no notes had nowhere to put its targets. And `<hyperlink>` is childless, so it arrives as `Event::Empty`; the worksheet walk keeps separate `Start` and `Empty` arms, and handling it only in `Start` would have read a workbook full of links as having none — the same trap that has now bitten four times. Targets are deduplicated: fifty cells linking to one address write one relationship. 2 tests. Structural 54.8% → 55.8% |
| FID-02 | Rich text runs: `r`/`rPr` in `CT_Rst` — per-character formatting in a cell | P0 | ✅ | The string table now carries runs beside the text rather than instead of it: `get()` still returns the flattened characters, so rendering, search and CSV export are untouched, and `runs()` gives the formatting to the few callers that want it. **Interning is keyed on text *and* runs** — two cells reading "Total" are the same string only if they are formatted alike, and keying on text alone would hand the second one the first one's formatting. A file that wraps unformatted text in a lone `<r>` collapses back to a plain string, or it would write runs for a string that has none and stop deduplicating against the identical plain entry. `RunFont` is its own type rather than a reuse of `Style`: `<rPr>` holds only font properties, and sharing `Style` would invite code that sets a run's fill and silently loses it. `<rPr>` children are written in CT_RPrElt's declared order, which Excel enforces — the same children shuffled is a package it refuses to open. Closes superscript/subscript and the underline variants **for runs**; the cell-level `<font>` in styles.xml is a separate parser and stays open as FID-05/06/17. 3 tests. Structural 55.8% → 64.5% |
| FID-03 | Tables / ListObjects: `xl/tables/*`, `tableParts`, `tableColumn(s)`, `tableStyleInfo`, `calculatedColumnFormula`, `totalsRowFormula` | P0 | 🔴 | also unblocks structured references |
| FID-04 | `xf/@quotePrefix` — the marker forcing a numeric-looking value to stay text | P0 | ✅ | Modelled as `Style::quote_prefix`, read from `xf/@quotePrefix` and written back. Typing `'0123` now sets the marker rather than merely forcing text this once: without it the cell saves as a plain string and Excel re-reads it as the number 123 on the next open, which is the silent half of the corruption. The formula bar shows the apostrophe back, or opening the cell and pressing Enter would commit the bare text and drop the marker again. **Found while implementing**: `applyProtection` was never written, so the `<protection>` child carrying `locked`/`hidden` was being emitted and then ignored by Excel — the flag is what makes the child count, so cell protection had been round-tripping into a file that quietly disregards it. 2 tests |
| FID-05 | `vertAlign` — superscript and subscript | P0 | ✅ | Closed together with FID-02, since all three are the same properties on the other parser — the cell-level `<font>` in styles.xml rather than a run's `<rPr>`. `Style::underline` changed from `bool` to `Option<Underline>`: a bool cannot hold `u/@val`, so a ledger formatted with accounting underlines came back with ordinary ones. The toolbar toggle stays binary and flips between none and single, which is what Excel's own button does — it does not cycle the variants — so a cell carrying a double underline reads as underlined and switches off. `<u/>` with no `val` means single while `val="none"` means not underlined at all, and reading the element's presence as truth would underline the second; a test pins both. `vertAlign`, `family`, `scheme` and `charset` added to `Style` and to the font dedup key, so two fonts differing only in those stay distinct. **Correction**: FID-02's note claimed Excel enforces `CT_RPrElt`'s child order. It does not — the type is an `xsd:choice`, so order is conventional. We still match the declared order, but to keep our output diffable against a file Excel wrote, not because it is required. 2 tests |
| FID-06 | `u/@val` — double / accounting underline variants | P0 | ✅ | Closed together with FID-02, since all three are the same properties on the other parser — the cell-level `<font>` in styles.xml rather than a run's `<rPr>`. `Style::underline` changed from `bool` to `Option<Underline>`: a bool cannot hold `u/@val`, so a ledger formatted with accounting underlines came back with ordinary ones. The toolbar toggle stays binary and flips between none and single, which is what Excel's own button does — it does not cycle the variants — so a cell carrying a double underline reads as underlined and switches off. `<u/>` with no `val` means single while `val="none"` means not underlined at all, and reading the element's presence as truth would underline the second; a test pins both. `vertAlign`, `family`, `scheme` and `charset` added to `Style` and to the font dedup key, so two fonts differing only in those stay distinct. **Correction**: FID-02's note claimed Excel enforces `CT_RPrElt`'s child order. It does not — the type is an `xsd:choice`, so order is conventional. We still match the declared order, but to keep our output diffable against a file Excel wrote, not because it is required. 2 tests |
| FID-07 | Gradient fills: `gradientFill`, `stop` | P0 | ✅ | Fills are modelled properly rather than as a single solid colour: `fill_pattern` keeps a non-solid `patternType`, `fill_bg_color` its second colour, and `fill_gradient` the whole `<gradientFill>` with its stops. FID-07 and FID-11 were closed together because they are one model — splitting them would have meant reworking the fill twice. **The deduplication key was the subtle part**: it now includes the pattern, the background and the gradient, so two cells whose foregrounds match but whose patterns differ stay distinct; keying on the foreground alone would have handed the second one the first one's pattern, which is a *new* corruption introduced by the fix rather than one it removes. A test pins a patterned and a solid cell sharing one foreground. Gradient geometry is `xsd:double` in the schema but is stored as integer millionths, since `Style` is `Hash + Eq` for deduplication and a float is neither — finer than any renderer resolves, and it re-reads to the same integer so the round trip settles. `patternType="none"` is dropped rather than kept, or every unfilled cell would carry a style. 1 test |
| FID-08 | External references: `externalReference(s)` | P0 | 🔴 | formulas referencing other workbooks break |
| FID-09 | Print setup: `pageMargins`, `pageSetup`, `printOptions`, `headerFooter` + odd/even/first, `rowBreaks`, `colBreaks`, `brk`, `pageSetUpPr` | P1 | 🔴 | ~13 elements, mostly carry-through |
| FID-10 | Inside borders: `horizontal`, `vertical` in `CT_Border` | P1 | 🔴 | neither modelled nor written |
| FID-11 | `bgColor` — the second colour of a pattern fill | P1 | ✅ | Fills are modelled properly rather than as a single solid colour: `fill_pattern` keeps a non-solid `patternType`, `fill_bg_color` its second colour, and `fill_gradient` the whole `<gradientFill>` with its stops. FID-07 and FID-11 were closed together because they are one model — splitting them would have meant reworking the fill twice. **The deduplication key was the subtle part**: it now includes the pattern, the background and the gradient, so two cells whose foregrounds match but whose patterns differ stay distinct; keying on the foreground alone would have handed the second one the first one's pattern, which is a *new* corruption introduced by the fix rather than one it removes. A test pins a patterned and a solid cell sharing one foreground. Gradient geometry is `xsd:double` in the schema but is stored as integer millionths, since `Style` is `Hash + Eq` for deduplication and a float is neither — finer than any renderer resolves, and it re-reads to the same integer so the round trip settles. `patternType="none"` is dropped rather than kept, or every unfilled cell would carry a style. 1 test |
| FID-12 | Row and column default styles: `col/@style`, `row/@s`, `row/@customFormat` | P1 | 🔴 | whole-column formatting is lost |
| FID-13 | `sheetView`: `rightToLeft`, `showFormulas`, `showZeros`, `tabSelected` | P1 | 🔴 | RTL sheets come back LTR |
| FID-14 | `sheetFormatPr`: `zeroHeight`, `thickTop`, `thickBottom`, `outlineLevelRow`, `outlineLevelCol`, `customHeight` | P1 | 🔴 | sheet-wide row defaults |
| FID-15 | `alignment`: `shrinkToFit`, `readingOrder`, `relativeIndent`, `justifyLastLine` | P1 | 🔴 | shrink-to-fit is common |
| FID-16 | `dataValidation`: `showDropDown`, `errorStyle`, `imeMode` | P1 | 🔴 | error severity degrades to default |
| FID-17 | Font metadata: `family`, `scheme`, `charset` | P1 | ✅ | Closed together with FID-02, since all three are the same properties on the other parser — the cell-level `<font>` in styles.xml rather than a run's `<rPr>`. `Style::underline` changed from `bool` to `Option<Underline>`: a bool cannot hold `u/@val`, so a ledger formatted with accounting underlines came back with ordinary ones. The toolbar toggle stays binary and flips between none and single, which is what Excel's own button does — it does not cycle the variants — so a cell carrying a double underline reads as underlined and switches off. `<u/>` with no `val` means single while `val="none"` means not underlined at all, and reading the element's presence as truth would underline the second; a test pins both. `vertAlign`, `family`, `scheme` and `charset` added to `Style` and to the font dedup key, so two fonts differing only in those stay distinct. **Correction**: FID-02's note claimed Excel enforces `CT_RPrElt`'s child order. It does not — the type is an `xsd:choice`, so order is conventional. We still match the declared order, but to keep our output diffable against a file Excel wrote, not because it is required. 2 tests |
| FID-18 | `indexedColors` / `rgbColor` — the legacy palette | P1 | 🔴 | read on import, never written back |
| FID-19 | Drawings, images, charts, OLE — scoped preserve-only but **not preserved** | P1 | 🔴 | needs the retention path, not the model |
| FID-20 | `workbook`: `calcPr`, `bookViews`/`workbookView`, `fileVersion`, `fileSharing`, `workbookProtection` | P1 | 🔴 | the weakest part at 28.6% |
| FID-21 | `<f>` attributes: array formulas (`t="array"`, `ref`) and data tables (`t="dataTable"`, `dt2D`, `dtr`, `r1`, `r2`, `del1`, `del2`), plus `ca`/`aca` | P1 | 🔴 | belongs with the calc phase |
| FID-22 | Filter refinements: `top10`, `dynamicFilter`, `colorFilter`, `iconFilter`, `dateGroupItem`, `sortState`, `sortCondition` | P2 | 🔴 | autofilter completeness |
| FID-23 | Conditional formatting: `cfvo`, `iconSet`, `colorScale` | P2 | 🔴 | rule types beyond the ones modelled |
| FID-24 | `tableStyles`, `tableStyle`, `tableStyleElement` in `styleSheet` | P2 | 🔴 | custom table styles; pairs with FID-03 |
| FID-25 | Remaining P2: `dimension`, `selection`, `ignoredError(s)`, `protectedRange(s)`, `col/@bestFit`, `col/@phonetic`, `row/@spans`, `row/@thickTop`/`thickBot`, `definedName/@hidden`, `xf/@pivotButton`, `border/@outline` | P2 | 🔴 | cheap carry-through, good score-per-effort |

## Function library

Batched by cluster and **ordered by practical workbook impact**, not by how
much of the spec each covers. Reference semantics — `INDIRECT`, `OFFSET`,
`LOOKUP`, `ADDRESS`, `TRANSPOSE` — appear in ordinary workbooks constantly,
while Bessel functions and complex arithmetic almost never do, so engineering
goes last despite being 39 functions of easy wins.

Each batch is implemented against the semantics in ECMA-376 §18.17.7, with tests
for the cases where a spreadsheet deviates from the obvious implementation.
Those are the ones that bite: wrapping the Rust standard library and moving on
is how a plausible wrong answer ships.

| ID | Cluster | Count | Status | Note |
| --- | --- | --- | --- | --- |
| FN-01 | Math and trigonometry | 41 | ✅ | Trig with its reciprocals and hyperbolics, inverse/log family, EVEN/ODD/MROUND/QUOTIENT, combinatorics, GCD/LCM, SUMSQ, SERIESSUM, PI. Three real traps: **ATAN2 takes x then y**, the reverse of every maths library, so forwarding in order mirrors every angle about the diagonal while still looking plausible; domain failures are **#NUM!, not NaN** (`ASIN(2)`, `LN(-1)`) and zero denominators are #DIV/0!, since a NaN in a cell compares and formats as nonsense; and **COMBIN accumulates term by term**, because `n!/(k!(n-k)!)` overflows f64 long before the answer does and returns #NUM! for COMBIN(100,2)=4950. Also EVEN(0)=0 and ODD(0)=1 against the general round-away rule, and FACT(171) is #NUM! not infinity. 9 tests. 22.8% → 32.3% |
| FN-04 | Logical and information | 8 | 🔴 | TRUE, FALSE, N, TYPE, ISREF, ERROR.TYPE, CELL, INFO |
| FN-03 | Date and time | 16 | 🔴 | NOW, TODAY, TIME, TIMEVALUE, DATEVALUE, HOUR, MINUTE, SECOND, DATEDIF, DAYS360, WEEKNUM, ISOWEEKNUM, NETWORKDAYS(.INTL), WORKDAY(.INTL). Needs a clock seam so tests stay deterministic |
| FN-05 | Lookup and reference | 9 | 🔴 | ADDRESS, AREAS, INDIRECT, OFFSET, LOOKUP, TRANSPOSE, HYPERLINK, GETPIVOTDATA, RTD |
| FN-02 | Text | 17 | 🔴 | CHAR, CODE, CLEAN, DOLLAR, FIXED, NUMBERVALUE, UNICHAR, UNICODE, T, plus the byte-oriented `*B` variants and the East Asian ASC/JIS/DBCS/PHONETIC/BAHTTEXT |
| FN-08 | Statistical and database | 92 | 🔴 | Descriptive statistics, distributions and their inverses, regression, the D* family. The distribution inverses need real numerics, not closed forms |
| FN-07 | Financial | 53 | 🔴 | Annuities, depreciation, bonds, coupons, yields. **Day-count conventions decide correctness**; needs the spec's own worked examples as test vectors |
| FN-06 | Engineering | 39 | 🔴 | Base conversion (BIN/OCT/DEC/HEX), bit operations, complex numbers (IM*), Bessel, ERF/ERFC, CONVERT, DELTA, GESTEP |

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
