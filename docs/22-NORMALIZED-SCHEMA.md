# 22 — Normalized Workbook Schema

The in-memory model `casual-calc-model` — the authoritative representation a
workbook is imported into, edited through, calculated over, laid out from, and
written back from. This is a **contract** doc: the schema versions independently
([02](02-ARCHITECTURE.md)) and changes append-only where possible so golden
snapshots stay byte-stable.

> **Status: shell implemented (Phase 0, F-010).** `casual-calc-model` provides
> `Id`/`IdGenerator` and typed id newtypes, `CellValue`/`ErrorValue`, `Cell` with
> the reserved calc seams (`formula: Option<FormulaHandle>`, cached `value`,
> `CellFlags` spill bits), the sparse `CellStore` (ordered API; block layout is a
> later perf impl behind it — see [23](23-CELL-STORE-REPRESENTATION.md)), `Sheet`,
> and `Workbook` with deterministic, byte-stable JSON snapshot I/O
> (`deny_unknown_fields` + `skip_serializing_if`). The empty-workbook byte-stable
> round-trip is gated by a test. Definition tables (styles, numbering, defined
> names, notes, theme) and per-sheet dependency edges are still to be filled in as
> import matures.

Two properties dominate the design:

1. **Sparse & compact** to meet the 1M-cell target (ADR-004, [30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)).
2. **Reserved calc seams** so the Phase 2 calc engine adds behavior, not schema
   (ADR-005). *This is the doc that makes "held back, not un-designed" real.*

## Envelope

```
Workbook
├── schema_version: u32
├── workbook_id: Id
├── sheets: Vec<SheetId>            // order = tab order
├── sheet_by_id: Map<SheetId, Sheet>
├── definitions: WorkbookDefinitions
│   ├── shared_strings: SharedStringTable
│   ├── styles: StyleTable          // fonts, fills, borders, cellXfs
│   ├── number_formats: NumberFormatTable
│   ├── defined_names: DefinedNameTable   // workbook-scoped
│   ├── themes / colors
│   └── calc_settings: CalcSettings // iterative calc, calc mode (reserved)
├── properties: DocumentProperties  // docProps core/app/custom
└── retained: RetainedSource        // preservation (see doc 34)
```

Serialization: deterministic JSON, `camelCase`, `deny_unknown_fields`,
`skip_serializing_if` on every optional so additive fields keep old snapshots
byte-identical. Definition tables are ordered maps that reject duplicate keys on
deserialize and iterate in stable key order.

## IDs

- `Id(u128)` — non-zero, serialized as 32-char lowercase hex; deterministic
  generator; namespaced (`from_parts(namespace, counter)`), matching OpenDoc's
  `NodeId`.
- Typed newtypes over `Id`: `SheetId`, `StyleId`, `NumberFormatId`,
  `DefinedNameId`, `StringId`, `TableId`, `MediaId`.

## Sheet

```
Sheet
├── id: SheetId
├── name: String
├── dimension: CellRange              // used bounds (a hint, revalidated)
├── cells: CellStore                  // the sparse grid — see below
├── columns: AxisSpans                // widths / hidden / outline, sparse
├── rows: AxisSpans                   // heights / hidden / outline, sparse
├── merges: Vec<CellRange>
├── views: SheetView                  // frozen panes, splits, active cell, zoom
├── defined_names: DefinedNameTable   // sheet-scoped
├── tables: Vec<TableId>              // structured-ref tables (Phase 3)
├── conditional_formats: ...          // (Phase 3)
├── data_validations: ...             // (Phase 3)
└── dependency_edges: SheetDepEdges   // RESERVED calc seam — see below
```

## The sparse cell grid (`CellStore`)

The heart of the 1M-cell target. Cells are **keyed by address**, never a dense
array. Design intent (final representation chosen in Phase 0, then benchmarked):

- Addressed by `(row: u32, col: u32)`; empty cells are simply absent.
- Backing structure is block-partitioned (row-blocked sparse tiles) so both
  range scans (layout, calc) and point access (edit) are efficient, and so a
  sheet with clustered data stays compact. The full internal design — block
  layout, the per-cell byte budget, and structural-edit behavior — is
  [23-CELL-STORE-REPRESENTATION](23-CELL-STORE-REPRESENTATION.md).
- **Per-cell record is compact and fixed-shape** — no fat boxed enum per cell:

```
Cell
├── value: CellValue          // the CACHED/literal value (see below)
├── style: StyleId            // interned; 1M cells → 1 style costs 1
├── formula: Option<FormulaHandle>   // RESERVED calc seam — AST by handle
└── flags: CellFlags          // dirty, spill-anchor, spill-child, etc. (reserved bits)
```

```
CellValue =
  Empty
| Number(f64)
| Bool(bool)
| SharedString(StringId)      // interned
| InlineString(StringId)      // interned, but marked inline for write-back
| Error(ErrorValue)           // #REF! #VALUE! #DIV/0! #N/A ...
```

- Strings are interned in the `SharedStringTable`; the cell holds a `StringId`.
- `value` is the **cached value**: for a literal cell it *is* the value; for a
  formula cell it is the last computed result (from `<v>` at import, or from the
  calc engine at runtime). **Layout and render only ever read `value`** — they
  never invoke the calc engine ([42](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md)).

## Reserved calc seams (ADR-005)

These fields exist from Phase 1A and are populated/consumed by Phase 2 without
any schema break:

| Seam | Present from | Filled at import (1A) | Used by calc (2) |
| --- | --- | --- | --- |
| `Cell.formula: Option<FormulaHandle>` | 1A | AST parsed from `<f>` via `casual-calc-formula` | evaluated |
| `Cell.value` (cached) | 1A | `<v>` cached result copied in | overwritten by recompute |
| `Sheet.dependency_edges` | 1A | left empty | built & maintained |
| `WorkbookDefinitions.calc_settings` | 1A | imported (calc mode, iterative caps) | honored |
| `CellFlags` dirty/spill bits | 1A | clean | set on edit / spill |

`FormulaHandle` indexes a per-sheet or per-workbook **AST arena** (formulas are
interned too — a filled-down column of the same relative formula shares one AST
with a shift, not N copies). The AST type is owned by `casual-calc-formula`
([19](19-WORKSPACE-SCAFFOLD-DESIGN.md)) so import and the transaction layer can
build and rewrite it without depending on the calc engine.

> **Why this matters:** because the model already carries the formula AST, the
> cached value, and a place for dependency edges, the calc engine is *additive
> code* in Phase 2 — no cell record changes, no snapshot format change, no
> re-import. This is the concrete mechanism behind "design it right the first
> time."

## Axis spans (`AxisSpans`)

Row heights and column widths as **run-length spans**, not per-line records:

```
AxisSpans
├── default_size: Twips
└── spans: Vec<Span { start, end, size?, hidden?, outline_level? }>
```

A uniform 1M-row sheet is one implicit default + a handful of override spans —
essential for T1 memory and for the O(log n) offset index in
[42](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md).

## Styles & number formats

- `StyleTable` interns fonts, fills, borders, alignment, and the `cellXfs`
  records cells reference by `StyleId`. Cells store an index, never inline style.
- `NumberFormatTable` holds built-in and custom format codes by `NumberFormatId`;
  a style references one. Format-code *interpretation* (value → display text)
  lives in `casual-calc-layout`, not the model.

## Defined names & references

- `DefinedNameTable`: name → parsed reference/formula AST + scope
  (workbook or sheet). Stored as AST (via `-formula`) so the calc engine and
  reference rewriting use them directly.

## Invariants (enforced by `validate()`)

- Non-zero IDs; no duplicate keys in any definition table.
- Cell addresses within the SpreadsheetML maximum (2^20 rows × 2^14 cols).
- Interned references resolve (every `StringId`/`StyleId`/`FormulaHandle` exists).
- Merge ranges disjoint; frozen-pane coordinates in range.
- Snapshot size within the model limit ([21](21-PARSER-LIMITS.md)).

## Migration

Schema changes are additive where possible. A deterministic total migration
function upgrades an older snapshot version to the current one; migrations are
golden-tested. Breaking changes require an ADR and a version bump.
