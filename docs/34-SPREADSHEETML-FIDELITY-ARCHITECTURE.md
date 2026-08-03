# 34 — SpreadsheetML Fidelity & Preservation Architecture

The loss-aware contract. OpenCalc **never drops content silently.** Anything the
semantic model doesn't represent is either preserved verbatim or reported through
the compatibility report — and which of the two is a policy the host chooses.
This is the format-neutral "crown jewel" inherited from OpenDoc, specialized to
SpreadsheetML.

The word **"lossless" is banned** unless the exact fidelity dimension is named
([07](07-QUALITY-SECURITY-AND-COMPATIBILITY.md) lists the dimensions).

## The import pipeline

```
.xlsx bytes
 → bounded OPC admission                 (casual-calc-package, doc 21)
 → immutable source snapshot             (casual-calc-ooxml, doc 28)
 → OPC graph: content-types, _rels, parts, markup-compat (mc:*)
 → streaming typed decode of each part   (quick-xml)
 → mapping → { Workbook model,
              provenance,
              preservation ledger,
              compatibility report,
              retained source + opaque parts }
 → atomic ImportBundle
```

## Dual-axis disposition taxonomy

Every construct the importer encounters gets **two** recorded outcomes — this is
the mechanism that makes "no silent loss" checkable:

**Model outcome** — how well the semantic model captured it:

| Value | Meaning |
| --- | --- |
| `Mapped` | Fully represented in the model |
| `Degraded` | Partially represented; some nuance lost |
| `Omitted` | Not represented in the model at all |

**Retention outcome** — whether the original bytes are kept for write-back:

| Value | Meaning |
| --- | --- |
| `Preserved` | Original bytes retained and re-emittable |
| `NotRetained` | Not kept (semantic mode drop) |
| `Blocked` | Retention refused by policy (e.g. a security limit) |
| `Rejected` | Admission refused it |
| `NotApplicable` | Nothing to retain (fully mapped, regenerated on write) |

A construct that is `Omitted` + `NotRetained` is the *only* way data leaves the
system, and it is always **counted and reported** — never silent.

## Compatibility report

- A `CompatibilityReport` = aggregated `CompatibilityEntry` values, bucketed by
  feature (element local-name or part name), with counts and the dual-axis
  dispositions.
- Bounded (a `MAX_REPORT_FEATURES` cap with an `(overflow)` bucket) and
  deterministically sorted.
- Whole admitted parts the semantic model never consumed (VBA project, pivot
  caches, digital signatures, `customXml`, drawings/charts before Phase 3) are
  recorded as part-level dispositions.

## Import modes (the host's preservation policy)

| Mode | Unmapped construct becomes | Round-trip guarantee |
| --- | --- | --- |
| **Retention** | `Preserved` (original part bytes kept) | Unedited workbook reconstructs **byte-identically** |
| **Semantic** | `Omitted` + `NotRetained`, reported | Model → valid `.xlsx`, unmapped content dropped-but-reported |
| **Inspect** | original bytes kept under stricter policy | For analysis; not for editing |

Retention mode gives the **byte-identical floor**: the export byte-identical
repackager ([export design](06-ROADMAP-AND-DELIVERY.md) Phase 1B) re-zips the
retained parts unchanged.

## The opaque side table

Admitted parts the model doesn't consume are carried verbatim in a
`RetainedParts` side table (part bytes + relationships + content-type). The
semantic writer merges them back on export so they aren't orphaned — without
double-emitting relationships the model already owns.

## Provenance & preservation ledger (designed; built with import)

- **Provenance map:** each model node can point back to its source location
  (part + element path), for diagnostics and precise write-back.
- **Preservation ledger:** per-entry record of anchor, applicable limits,
  invalidation conditions, and export disposition (`unchanged-copy` /
  `regenerate` / `merge` / `omit` / `block`). The report + retained-source +
  opaque-table pieces are the concrete first implementation; the full typed
  ledger is elaborated as import matures.

## SpreadsheetML-specific fidelity concerns

- **Shared vs inline strings:** both imported; the model marks which so the
  writer can round-trip the original choice in retention mode.
- **`calcChain.xml`:** treated as a rebuildable hint, not authoritative data;
  preserved in retention mode, regenerated (or omitted) by the semantic writer,
  since the dependency graph is the source of truth ([40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md)).
- **Styles indirection:** `cellXfs`/`dxf` indices are remapped deterministically
  on semantic write; retention mode keeps the original `styles.xml` bytes.
- **Formulas:** parsed to AST (Phase 1A) *and* their cached `<v>` retained; a
  cell that is `Mapped` semantically is still `Preserved` byte-wise in retention
  mode.
- **Charts, pivots, drawings, VBA:** `Omitted` from the model until their phase,
  but `Preserved` (opaque) from Phase 1A so nothing is lost meanwhile. VBA is
  **never executed** regardless of mode.

## Export dispositions

The semantic writer classifies each source region as one of:
`unchanged-copy | regenerate | merge | omit | block`, mirroring the ledger — so
what the writer does is auditable against what import recorded.

## Why this is format-neutral

The taxonomy, the report, the retention floor, and the opaque table are the same
design OpenDoc uses for `.docx`/`.odt`. Adding `.ods` or CSV to OpenCalc reuses
it wholesale — only the mapping tables differ.
