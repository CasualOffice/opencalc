# 12 — Competitive Analysis

What we study, why, and what we take. Every claim here should carry a **source**
and a **date checked** ([16](16-DOCUMENTATION-MAINTENANCE.md)); the notes below
are the initial survey (checked **2026-08-04**) and must be re-verified before
being relied on for a specific decision.

We study three kinds of prior art: **semantics oracles** (what "correct" means),
**open engines** (implementation patterns we can borrow), and **UX references**
(how the grid should feel).

## Semantics & fidelity oracles

### Microsoft Excel — the semantics oracle
- **Role:** the definition of correct for formulas, number formats, coercion,
  error values, date systems, spill/dynamic arrays, and rendered cells.
- **Take:** function semantics, `#SPILL!`/dynamic-array behavior, 15-significant-
  digit precision, 1900/1904 date systems. We match Excel where Excel and the
  ECMA-376 spec differ (Excel wins for real-world files).
- **UX reference:** **MS Sheets 2026** for grid interaction, selection, fill,
  formula editing, and the ribbon/command surface.

### LibreOffice Calc — the open fidelity oracle
- **Role:** our automatable differential oracle (as LibreOffice is OpenDoc's for
  `.docx`) — open-source, scriptable, headless-capable, so it can gate CI for
  both computed values and rendered cells.
- **Take:** a runnable reference for the fidelity harness
  (`tools/casual-calc-fidelity`); a second opinion where Excel is unavailable.
- **Caveat:** Calc and Excel disagree on edge cases; the corpus records which
  oracle owns which case.

### OnlyOffice — C++ engine + collaborative editor
- **Role:** a modern, high-fidelity OOXML spreadsheet implementation with
  real-time collaboration.
- **Take:** collaboration op-model ideas (Phase 5); evidence that faithful OOXML
  round-trip + live editing is achievable in a single engine.

## Open Rust engines (implementation patterns)

### IronCalc — open-source Rust spreadsheet engine
- **Role:** the closest analog to OpenCalc's positioning — Rust core, WASM,
  browser-first, embeddable, language bindings.
- **Take:** validation that a Rust+WASM calc engine hits interactive latency;
  patterns for the function library and the browser bridge. **Differences we
  intend:** OpenCalc's loss-aware preservation/round-trip discipline, the
  explicit 1M-cell/60fps/50ms gated targets, and the Tauri-native + WASM dual
  host.

### Formualizer — layered Rust calc workspace
- **Role:** a permissively-licensed engine with a clean layered crate split:
  `common` → `parse` → `eval` (dependency graph) → `workbook` → `sheetport`,
  400+ functions, Rust/Python/WASM bindings, undo/redo.
- **Take:** strong confirmation of our **parse/eval split** ([19](19-WORKSPACE-SCAFFOLD-DESIGN.md),
  [40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md)); a reference for
  dependency-graph + span evaluation and function coverage breadth.

### calamine / umya-spreadsheet / rust_xlsxwriter — focused IO libs
- **Role:** `calamine` (fast pure-Rust reader), `umya-spreadsheet` (read/modify/
  write), `rust_xlsxwriter` (writer).
- **Take:** reference implementations for XLSX part parsing and writing details;
  none provides a live calc engine + virtualized render + preservation model, so
  they inform our IO layer but not the whole engine.

## Web-native architecture

### Univer — TypeScript office-suite framework
- **Role:** a modern, plugin-based web spreadsheet/doc framework with canvas
  rendering and a formula engine.
- **Take:** canvas grid **virtualization** and rendering patterns for the web
  host; plugin/extension seams to consider for Phase 4+. **Difference:** Univer
  is TS-first; OpenCalc keeps the engine in Rust (native + WASM) with the browser
  as one host, not the home.

### Google Sheets — UX reference
- **Role:** the mainstream bar for grid interaction, collaboration feel, and
  formula-editing UX at scale.
- **Take:** selection/fill/autocomplete/named-range UX; large-sheet scroll feel;
  collaboration expectations (Phase 5).

## Where OpenCalc deliberately differs

| Axis | Most prior art | OpenCalc |
| --- | --- | --- |
| Source of truth | DOM/canvas or server | Rust engine state; view is a projection |
| Preservation | best-effort, silent drops common | dual-axis disposition; nothing dropped silently ([34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)) |
| Round-trip | often lossy | byte-identical floor for unedited files |
| Hosts | web *or* native | one core, **Tauri-native + WASM** bridges ([02](02-ARCHITECTURE.md)) |
| Scale targets | implicit | explicit & gated: 1M cells / 60 fps / <50 ms ([30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)) |
| Determinism | rarely a contract | identical model/values/layout/bytes, gated |

## Open questions to resolve with research

- Excel vs LibreOffice divergences that matter for the corpus (dates, rounding,
  `TEXT`/format edge cases) — enumerate before Phase 2.
- Dynamic-array/spill semantics differences across Excel versions.
- Canvas rendering approach on the web host (2D canvas vs WebGL/WebGPU) informed
  by Univer's experience — decide with the render backend ADR.
