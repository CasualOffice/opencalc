# 17 — Glossary

Shared vocabulary for OpenCalc. When a term is ambiguous across spreadsheet
products, this doc fixes the meaning we use.

## Format & package

- **OPC** — Open Packaging Conventions: the ZIP-based container `.xlsx` (and
  `.docx`) use. Parts + relationships + content types.
- **SpreadsheetML** — the OOXML markup for spreadsheets (ECMA-376, Part 1).
- **Part** — a file inside the OPC package (e.g. `xl/worksheets/sheet1.xml`).
- **Shared string table** — `xl/sharedStrings.xml`; deduplicated cell strings
  referenced by index.
- **Styles part** — `xl/styles.xml`; number formats, fonts, fills, borders, and
  the `cellXfs` records cells point into.
- **Calc chain** — `xl/calcChain.xml`; Excel's recorded calculation order. A
  *hint*, rebuildable from the dependency graph.

## Model

- **Workbook** — the top-level document: sheets + workbook-scoped definitions.
- **Sheet / worksheet** — one grid of cells plus sheet-scoped state (panes,
  merges, dimensions, defined names).
- **Cell** — a value at a `(row, column)` address; may hold a literal, a shared
  string, or a formula with a cached value.
- **Sparse grid** — the cell store keyed by address, not a dense array; empty
  cells cost nothing.
- **Address / reference** — A1 (`B7`, `$B$7`, `Sheet2!B7`) or R1C1; absolute,
  relative, mixed; ranges (`A1:C9`); 3-D (`Sheet1:Sheet3!A1`); structured
  (`Table1[Amount]`).
- **Defined name** — a named reference or formula, workbook- or sheet-scoped.
- **Number format** — a format code (built-in or custom) that maps a stored
  numeric/date value to its displayed text.
- **Merged range** — a rectangle of cells displayed as one.
- **Reserved calc seams** — model fields present from Phase 1A that the Phase 2
  calc engine fills: the formula AST, the cached value, and dependency edges.

## Calculation (Phase 2)

- **Formula AST** — the parsed tree of a formula; produced at import, evaluated
  by the calc engine.
- **Dependency graph** — nodes (cells/ranges) and edges (precedent → dependent).
- **Precedent / dependent** — cells a formula reads / cells that read it.
- **Recalculation** — recomputing values after an edit; **full** vs
  **incremental** (only the dirty sub-graph).
- **Dirty propagation** — marking a cell and its transitive dependents stale.
- **Volatile function** — one that must recompute every calc (`NOW`, `RAND`, …).
- **Spill / dynamic array** — a formula whose result populates a range of cells.
  The origin cell is the **spill-anchor**; the cells it fills are **spill-child**
  cells (the `CellFlags` names used across docs 22/23/40).
- **Iterative calculation** — bounded re-evaluation of intentional cycles.
- **Error value** — `#REF!`, `#VALUE!`, `#DIV/0!`, `#N/A`, `#NAME?`, `#NULL!`,
  `#NUM!`, `#SPILL!`, etc.

## Layout & render

- **Grid geometry** — the column widths / row heights that place cells in space.
- **Frozen panes** — rows/columns pinned during scroll.
- **Viewport / visible window** — the range of cells currently on screen.
- **Virtualization** — laying out and painting only the visible window (plus a
  small overscan), so cost is O(visible), not O(workbook).
- **Display list** — the backend-neutral, serializable list of paint items
  layout emits; the render seam.
- **Tile** — a fixed block of the grid used as a unit of layout/paint caching.
- **Twip** — 1/1440 inch; the internal layout unit (device pixels appear only at
  raster time).

## Fidelity

- **Disposition** — the recorded fate of an imported construct: a **model
  outcome** (`Mapped` / `Degraded` / `Omitted`) and a **retention outcome**
  (`Preserved` / `NotRetained` / `Blocked` / `Rejected` / `NotApplicable`).
- **Compatibility report** — the aggregated dispositions returned by import.
- **Retention floor** — the byte-identical guarantee for unedited packages.
- **Opaque part** — an admitted part the semantic model doesn't consume, carried
  verbatim for write-back.
- **Oracle** — an external reference (LibreOffice Calc / Excel) used to check
  computed values or rendered output.

## Hosts & integration

**"Host" carries two senses in this repository and both are load-bearing**, so
they are separated here rather than merged. Which one is meant is decided by
the document: the architecture and engine documents mean the first, the
collaboration and WOPI documents mean the second.

- **Host** *(engine sense)* — the program the engine is compiled into and which
  owns policy: the browser editor (`casual-calc-wasm`), the desktop shell
  (`desktop/`), the collaboration server (`server/`). AGENTS.md's rule is about
  this one: *the engine computes; the host decides fonts, I/O, network,
  persistence and collaboration transport.* Its seams are
  [78](78-HOST-CAPABILITY-SEAMS.md).
- **Host** *(storage sense)* — the system that keeps the document of record and
  mints the tokens: a WOPI host such as Nextcloud or SharePoint
  ([74](74-WOPI-INTEGRATION.md)), or the party a collaboration deployment posts
  finished bytes back to ([57](57-COLLABORATION-SERVER-BOUNDARY.md)).
- **Integrator** — the party embedding OpenCalc in their product. The preferred
  word for the *people*, where "host" is the preferred word for the *program*.
  **"Embedder" and "SDK consumer" are drift**: use "integrator".
- **Embed / embedding** — putting the editor inside somebody else's page
  through the SDK ([55](55-SDK-EMBEDDING-AND-INTEGRATION-DESIGN.md)). Distinct
  from *hosting*, which is compiling the engine into a program.

## Process

- **ADR** — Architecture Decision Record ([08](08-ADR-REGISTER.md)).
- **Tracker ID** — the stable `<PHASE>-<NNN>` key for a unit of work
  ([14](14-EXECUTION-TRACKER.md)).
- **Exit gate** — the condition a phase must meet to be `Done`
  ([06](06-ROADMAP-AND-DELIVERY.md)).
