# SKILLS.md — Domain Skills for OpenCalc

These are the **competencies** required to build OpenCalc — not tooling or
editor plugins. They define what an agent must understand to work responsibly
in each area. Grouped by domain.

## Working practice (per feature)

Every feature follows the same eight-step arc, mirroring
[docs/11-DESIGN-FIRST-PROCESS.md](docs/11-DESIGN-FIRST-PROCESS.md):

1. **Define the outcome** — what a user or host can do afterward that they can't now.
2. **Document the design** — a numbered `docs/` note; an ADR if a trigger fires.
3. **Compare competitors** — how Excel, LibreOffice Calc, OnlyOffice, Univer,
   IronCalc, Google Sheets handle it; record source + date checked.
4. **Identify UX and correctness risks** — wrong values, fidelity loss, perf cliffs.
5. **Define acceptance gates** — the tests/fixtures that prove it works.
6. **Update the execution tracker** — a row with a stable ID and a status.
7. **Implement** in small increments.
8. **Verify** against the gates, then update docs.

## 1. Spreadsheet formats

- **SpreadsheetML / OOXML (ECMA-376)** — the `.xlsx` OPC package: `workbook.xml`,
  `worksheets/sheetN.xml`, `sharedStrings.xml`, `styles.xml`, `calcChain.xml`,
  `_rels`, content types, defined names, tables, pivot caches, drawings/charts.
- **The shared string table** and inline strings; when each is used and why.
- **The styles part** — number formats (built-in + custom codes), fonts, fills,
  borders, cell `xf`/`cellXfs` indirection, `dxf` differential formats.
- **The calculation chain** — what `calcChain.xml` records, why it is a *hint*
  and can be rebuilt, and when Excel rejects a stale one.
- **OpenDocument Spreadsheet (`.ods`)** — the secondary import/export path.
- **CSV / TSV / PSV** — delimited text (comma/tab/pipe): lossy tabular
  interchange, delimiter/quoting/encoding ambiguities; not OPC packages.
- **Preservation** — the loss-aware discipline: dual-axis disposition taxonomy,
  compatibility report, retention byte-floor, opaque part side-table
  ([docs/34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md](docs/34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)).

## 2. Runtime engineering

- **Bounded, secure parsing** — zip-bomb, path-traversal, entity-expansion, and
  resource-exhaustion defenses; every limit explicit
  ([docs/21-PARSER-LIMITS.md](docs/21-PARSER-LIMITS.md)).
- **Deterministic modelling** — stable IDs, ordered definition maps,
  `deny_unknown_fields` + `skip_serializing_if` serde so snapshots stay
  byte-stable across additive schema changes.
- **The sparse cell grid** — representing a 1M-cell workbook without a dense
  array; row/column stores, block/tile partitioning, memory bounds.
- **Atomic transactions with inverses** — every edit reversible; reference
  rewriting on row/column insert & delete.

## 3. Calculation (the calc engine — Phase 2, designed now)

- **Formula language** — A1 and R1C1 references, absolute/relative/mixed,
  ranges, 3-D references, structured (table) references, operators, function
  calls, array/spill semantics.
- **Tokenizer & parser** — producing a stable AST; error tolerance; pretty-print
  round-trip.
- **The dependency graph** — cells as nodes, precedents/dependents as edges;
  ranges and volatile functions; incremental dirty propagation.
- **Recalculation** — topological ordering, cycle/iterative-calc handling,
  minimal-work recompute, and the <50 ms worst-case target
  ([docs/40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md](docs/40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md)).
- **Function semantics** — matching Excel's coercion, error values
  (`#REF!`, `#VALUE!`, `#DIV/0!`, `#N/A`, …), date system, and edge cases
  exactly enough to pass a differential oracle.

## 4. Grid layout & rendering

- **Grid geometry** — column widths, row heights, default vs explicit sizing,
  hidden rows/columns, outline grouping.
- **Merged cells, frozen panes, and split views.**
- **Viewport virtualization** — laying out and painting only the visible window
  of a 1M-cell sheet, at 60 fps, with incremental repaint on scroll/edit
  ([docs/42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md](docs/42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md)).
- **In-cell rich text shaping** via `parley`; number-format-driven display
  (dates, currency, scientific, fractions); alignment, wrap, shrink-to-fit,
  rotation.
- **The backend-neutral display list** and the CPU raster backend
  (`tiny-skia` + `skrifa`).

## 5. Product quality

- **Fidelity oracles** — LibreOffice Calc (open) and Excel as differential
  references for both computed values and rendered cells.
- **Determinism harnesses** — golden snapshots, golden display lists, golden
  recalc results.
- **Benchmark discipline** — versioned JSON reports, committed named-environment
  baselines, regression thresholds.
- **UX literacy** — MS Sheets 2026 and Google Sheets interaction models.
