# 51 — Fidelity Gap Audit

_Measured 2026-08-09 against ECMA-376 Part 1, 5th edition. Regenerate with
[`tools/fidelity-audit`](../tools/fidelity-audit)._

Every construct we have missed so far — tables, the 1904 date epoch, the run of
five "P2 polish" rows that turned out to be silent data loss — was missed the
same way: the checklist was written from memory, so the gaps nobody thought of
stayed invisible. This audit exists to remove memory from the loop. Its "what
should exist" side is the [vendored schema](../schemas/ooxml/README.md); its
"what we actually do" side is a measured round trip.

## Method, and one method that did not work

**Detector A — schema inventory (what should exist).** Walk `sml.xsd` from the
roots we claim to model and enumerate every reachable element and its declared
attributes. Authoritative and corpus-independent: a construct shows up whether
or not any test file happens to use it.

**Detector B — round-trip differ (what we actually keep).** Build a probe
workbook, import it, write it back, and compare the XML that went in against the
XML that came out. Every element or attribute present in the source and absent
from the output is a **measured** loss.

Two details decide whether B tells the truth:

- The probe sets **every attribute the schema declares** for each element it
  places. Element placement is hand-written, because valid OOXML needs correct
  nesting, but the attribute set is taken from the schema, so no attribute can
  be left out by oversight.
- Every value is deliberately **not the schema's declared default**. A writer is
  correct to omit a default, so a probe supplying one cannot distinguish
  preservation from omission. The first run of this audit reported
  `row/@hidden`, `sheet/@state`, `c/@t`, `dataValidation/@operator` and
  `border/@outline` as lost purely because the probe had supplied defaults. All
  five work correctly. Attribute defaults are now read from the schema.

**A third detector was built and thrown away.** The original plan included a
static scan pairing the element names the importer matches on against the tags
the exporter writes. It cannot work on this codebase, and the measurement proved
it: the importer delegates (`b"col" => read_col(&e, …)`, so the attributes are
read inside a helper the scan never enters) and the exporter interpolates
(`"<col min=\"{}\"{width_attr}"`, so `width` never appears beside `<col`). The
scan scored `col/@width` and `row/@ht` as unhandled when both round-trip
correctly. A tool that under-reports silently is worse than no tool, because it
manufactures exactly the false confidence this audit is meant to remove. Only
detectors A and B are kept.

## Scores

Coverage of the semantic core, by part. A container element counts as handled
when we handle what it contains — we dispatch on `<mergeCell>`, never on its
`<mergeCells>` wrapper, and counting the wrapper as a gap would inflate the
register with things that already work.

| Part | Elements handled | Unhandled |
| --- | --- | --- |
| `styleSheet` | 41 / 54 | 13 |
| `comments` | 18 / 32 | 14 |
| `sst` | 14 / 24 | 10 |
| `worksheet` | 48 / 126 | 78 |
| `table` | 9 / 22 | 13 |
| `workbook` | 6 / 30 | 24 |

Measured round trip of the probe workbook: **68 elements in, 61 out** (10 lost),
**141 attributes in, 94 out** (32 lost on elements that themselves survived).

## What we drop — ranked by what it costs

Severity is judged by **what reaches the file**, not by how it looks on screen.
That distinction is the whole reason the previous tracker mis-rated five rows.

### P0 — silent data loss

| Construct | What happens |
| --- | --- |
| **Hyperlinks** (`hyperlink`, `hyperlinks`) | Dropped entirely, along with the sheet rels part holding the targets. Every link in a workbook is gone after one save, leaving the display text behind so nothing looks wrong. |
| **Tables / ListObjects** (`table`, `tableColumns`, `tableColumn`, `tableStyleInfo`, `tableParts`, `calculatedColumnFormula`, `totalsRowFormula`) | The whole construct is dropped. Worse, **every structured-reference formula becomes a frozen constant**: `=SUM(Sales[Sales])` is written back as `<v>300</v>`. Edit the data afterwards and the cell never updates. |
| **Rich text runs** (`r`/`rPr` inside `CT_Rst`) | A cell holding mixed formatting is flattened to plain text: `"Hello"` bold red + `" world"` imports as `"Hello world"`, and the formatting cannot be recovered. Affects `sst`, `comments` and inline strings alike. |
| **Gradient fills** (`gradientFill`, `stop`) | A cell with a gradient background loses its fill entirely — `fillId` falls back to 0. |
| **`vertAlign`** | Superscript and subscript are dropped from every run and font. |
| **`u/@val`** | Double, single-accounting and double-accounting underlines all collapse to a plain underline. |
| **`xf/@quotePrefix`** | The marker that forces a numeric-looking value to stay text is lost, so `'0123` can silently become a number. |
| **External references** (`externalReference(s)`) | Links to other workbooks are dropped; formulas referencing them break. |

### P1 — visible loss, no corruption

| Construct | What happens |
| --- | --- |
| **Print setup** (`pageMargins`, `pageSetup`, `printOptions`, `headerFooter`, `oddHeader`/`oddFooter` and the even/first variants, `rowBreaks`, `colBreaks`, `brk`, `pageSetUpPr`) | Everything about printing is dropped: margins, orientation, scaling, headers, footers, page breaks. |
| **Inside borders** (`horizontal`, `vertical` in `CT_Border`) | Excel's inside-horizontal/vertical borders are neither modelled nor written. |
| **`bgColor`** | The second colour of a pattern fill is dropped, so any non-solid pattern loses half its definition. |
| **`col/@style`** | A column's default style is dropped, so formatting applied to a whole column is lost. |
| **`row/@s` + `row/@customFormat`** | Likewise for a row-level style. |
| **`sheetView`**: `rightToLeft`, `showFormulas`, `showZeros`, `tabSelected` | RTL sheets come back left-to-right. |
| **`sheetFormatPr`**: `zeroHeight`, `thickTop`, `thickBottom`, `outlineLevelRow`, `outlineLevelCol`, `customHeight` | Sheet-wide row defaults are lost. |
| **`alignment`**: `shrinkToFit`, `readingOrder`, `relativeIndent`, `justifyLastLine` | Shrink-to-fit and reading order are common in real files. |
| **`dataValidation`**: `showDropDown`, `errorStyle`, `imeMode` | A validation's error severity degrades to the default. |
| **Font metadata** (`family`, `scheme`, `charset`) | Theme-linked font resolution degrades. |
| **`indexedColors` / `rgbColor`** | The legacy palette is dropped; we resolve `indexed` on read but never write the palette back. |
| **Drawings, images, charts, OLE** (`drawing`, `picture`, `oleObject(s)`, `control(s)`) | Scoped preserve-only, but **not currently preserved** — they are dropped. |

### P2 — genuinely cosmetic or rare

`dimension`, `selection`, `ignoredError(s)`, `cellWatch(es)`, `sortState` /
`sortCondition`, `protectedRange(s)`, `scenario(s)`, `customSheetView(s)`,
`dataConsolidate`, smart tags, web-publish items, `phoneticPr` / `rPh` / `rFont`
(East Asian phonetic guides), `col/@bestFit`, `col/@phonetic`, `row/@spans`,
`row/@thickTop`/`thickBot`, `definedName/@hidden`, `xf/@pivotButton`,
`border/@outline`, filter refinements (`top10`, `dynamicFilter`, `colorFilter`,
`iconFilter`, `dateGroupItem`), conditional-format `cfvo` / `iconSet`.

### Calculation settings — `calcPr`

Its own case: `calcPr` (`iterate`, `iterateCount`, `iterateDelta`,
`fullCalcOnLoad`, `calcMode`) is dropped. Harmless while the calc engine is held
back, and a correctness problem the moment it lands — a workbook that requires
iterative calculation would be recalculated without it.

## Known limitations of this audit

- The vendored schema is **Strict**, not Transitional. Real files are almost
  always Transitional, a superset. The inventory is therefore conservative: a
  Transitional-only construct cannot be reported as a gap. See
  [`schemas/ooxml/README.md`](../schemas/ooxml/README.md).
- Detector B measures one probe workbook. Its attribute coverage per element is
  schema-complete, but element *placement* is hand-written, so an element the
  probe never places is scored only by detector A.
- Parts outside the semantic core (charts, pivot caches, VBA, doc properties,
  printer settings, custom XML) are not walked at all.
- "Handled" means the construct survives a round trip. It does **not** mean the
  editor exposes it, nor that the renderer draws it.
