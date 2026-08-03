# 28 — XLSX Package Reader

The SpreadsheetML OPC admission contract. Two crates:

- **`casual-calc-package`** — the format-neutral bounded ZIP/OPC substrate
  (shared design with OpenDoc's `casual-doc-package`).
- **`casual-calc-ooxml`** — the SpreadsheetML reader on top of it.

All admission is bounded ([21](21-PARSER-LIMITS.md)); untrusted input is rejected
cleanly, never crashed on.

## `casual-calc-package` — bounded OPC substrate

> **Status: implemented (Phase 0, F-009).** `Package::open(bytes, PackageLimits)`
> admits an archive under the limits below and exposes `read_part`, `entries`,
> `entry_names`, and `contains`. Hostile inputs (zip bomb, path traversal,
> oversized, too-many-entries) are covered by tests and rejected with stable
> `OC-PKG-*` codes ([20](20-ERROR-CODE-REGISTRY.md)). Compiles to
> `wasm32-unknown-unknown` (pure-Rust deflate).

- Wraps ZIP reading with **non-bypassable limits**: max input size, max entry
  count, max total expanded size, max expansion ratio (zip-bomb defense), max
  path length, path-traversal rejection.
- Provides content-types resolution and **on-demand, bounded part reads** — a
  part is never fully decompressed until asked for, and its expanded size is
  capped.
- Produces an **immutable source snapshot** so import can re-read parts and
  export (retention mode) can re-emit them byte-for-byte.
- Knows nothing spreadsheet-specific; the same crate could admit any OPC package.

## `casual-calc-ooxml` — SpreadsheetML shape

Reads the OPC graph and exposes the SpreadsheetML structure without mapping to
the model (that's `casual-calc-import`):

### Parts

| Part | Role |
| --- | --- |
| `[Content_Types].xml` | Part content-type map |
| `_rels/.rels`, `xl/_rels/workbook.xml.rels`, per-sheet `_rels` | Relationship graph |
| `xl/workbook.xml` | Sheets, defined names, workbook views, calc properties |
| `xl/worksheets/sheetN.xml` | Cell grid, dimensions, sheet views (panes), merges, col/row props |
| `xl/sharedStrings.xml` | Deduplicated string table |
| `xl/styles.xml` | Number formats, fonts, fills, borders, `cellXfs`, `dxf` |
| `xl/calcChain.xml` | Recorded calc order (hint; rebuildable) |
| `xl/tables/tableN.xml` | Structured-ref tables (Phase 3) |
| `xl/pivotCache/*`, `xl/pivotTables/*` | Pivots (preserved 1A; Phase 3) |
| `xl/drawings/*`, `xl/charts/*` | Drawings/charts (preserved 1A; Phase 3) |
| `xl/media/*` | Embedded images |
| `xl/vbaProject.bin` | VBA (preserved opaque; **never executed**) |
| `docProps/*` | Core/app/custom properties |

### Reader responsibilities

- Discover the workbook part and the sheet-part relationships; establish sheet
  order from `xl/workbook.xml`.
- Resolve relationships (`r:id`) to target parts.
- Handle **markup compatibility** (`mc:AlternateContent`, `mc:Ignorable`) so
  extension namespaces degrade gracefully.
- Stream part XML with `quick-xml` under element-count and depth caps
  ([21](21-PARSER-LIMITS.md)).
- Never evaluate, never fetch external targets, never run VBA.

## Streaming decode (large-sheet aware)

`xl/worksheets/sheetN.xml` can be enormous (a 1M-cell sheet). The reader:

- Streams rows/cells rather than building a full DOM.
- Resolves shared-string indices against the (also streamed/interned) string
  table.
- Feeds the importer a **cell event stream** it maps into the sparse
  `CellStore` ([22](22-NORMALIZED-SCHEMA.md)) — so peak memory tracks the model
  size, not a transient DOM of the whole sheet.

## Output

An `ooxml`-level source model: the immutable part snapshot + the resolved graph +
typed access to each part's element stream. `casual-calc-import` consumes this to
produce the `Workbook` model, the compatibility report, and the retained source
([34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)).
