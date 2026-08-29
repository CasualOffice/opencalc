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

> **Status: semantic writer implemented (Phase 1B, P1B-001).**
> `casual-calc-export::write_workbook` emits a deterministic `.xlsx` from the
> model — values, formulas (from the AST via the pretty-printer), number formats
> (`cellXfs`), merges, frozen panes, and defined names. The **semantic fixed
> point** `import → write → import` is gated by tests.
>
> **Both remaining increments have since landed.** The byte-identical
> repackager is P1B-002: `WorkbookSession` keeps the bytes it opened and returns
> them from `save` while nothing has been edited, so a file opened and not
> edited comes back exactly — verified against packages this engine did not
> write. It lives in the session rather than the model because it is a fact
> about *this opening of this file*, not about the document, and putting the
> original in the `Workbook` would double every model snapshot for it.
> LibreOffice-Calc differential validation is P1B-003, in
> `tools/casual-calc-fidelity --validate-package`.

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

## Two package flavours: `.xlsx` and `.xlsm`

Added by `IO-08`; this section is the document catching up with it (`DOC-042`).
Both writers above emit **one of two package flavours**, chosen by
`casual_calc_export::PackageKind` (`crates/casual-calc-export/src/lib.rs:76`):

| `PackageKind` | Extension | Workbook content type |
| --- | --- | --- |
| `Workbook` (default) | `.xlsx` | `application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml` |
| `MacroEnabled` | `.xlsm` | `application/vnd.ms-excel.sheet.macroEnabled.main+xml` |

Note the second is **not** in the `openxmlformats-officedocument` namespace —
that content type, and the VBA part itself, are the only package-level
difference between the two flavours: same schema, same parts, same reader.

**The flavour is not a naming preference, and the container is where the loss
actually happened.** Retaining `vbaProject.bin` in the opaque side table is not
enough on its own: a plain declaration on a package that carries a VBA project
makes Excel report the file as damaged and repair it *by deleting the project*,
and a macro-enabled declaration on a package with no macros makes Excel warn
about content it will not find. So the flavour is **raised, not merely
accepted** — `PackageKind::for_workbook` consults
`Workbook::macro_project()` and overrides a caller who named `Workbook` for a
workbook that carries one (`write_workbook_as`, `:134-137`). A caller that
genuinely wants a plain `.xlsx` out of a macro workbook calls
`Workbook::remove_macro_project()` first, which is the route that reports the
loss rather than performing it silently.

`write_workbook` picks the flavour for its caller; `write_workbook_as` is for
the caller that wants to name it. Above the engine,
`SessionFormat::for_extension("xlsm")` resolves and the editor's Download
submenu is derived from `writable_extensions()`, so the flavour appears without
anybody remembering to add it.

**Known gap, tracked rather than papered over:** `SessionFormat::for_bytes`
cannot tell the two apart, because `casual_calc_io::detect` reads only the zip's
first local file header and both flavours start with `[Content_Types].xml`
(`IO-09`, Open). `.xlsm` bytes arriving with **no filename** therefore open as
`Xlsx`, and the first edit legitimately drops the macros — the loss is reported,
but the detection is lossy.

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
