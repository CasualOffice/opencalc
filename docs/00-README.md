# 00 — OpenCalc Documentation Index

`docs/` is the **source of truth** for OpenCalc's design. Code follows docs, not
the other way around. This index is the map.

## How to read this

- New here? Read [01-ORD](01-ORD.md) (what & why) → [02-ARCHITECTURE](02-ARCHITECTURE.md)
  (the shape) → [06-ROADMAP-AND-DELIVERY](06-ROADMAP-AND-DELIVERY.md) (the order).
- Building something? Follow [11-DESIGN-FIRST-PROCESS](11-DESIGN-FIRST-PROCESS.md)
  and update [14-EXECUTION-TRACKER](14-EXECUTION-TRACKER.md).
- Working the hard parts? [19-WORKSPACE-SCAFFOLD-DESIGN](19-WORKSPACE-SCAFFOLD-DESIGN.md)
  (layer division), [30-PERFORMANCE-AND-CAPACITY-TARGETS](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)
  and [42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md)
  (virtualization), [40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md)
  (the calc engine).

## Numbering discipline

- Numbers are **stable and never reused.** A retired doc keeps its number with a
  tombstone note; new docs take the next free number.
- Ranges are conventional, mirroring the OpenDoc layout so the two engines feel
  like siblings:
  - **00–19** — foundation, process, and top-level architecture.
  - **20–28** — stable contracts (schemas, cell store, transactions, limits,
    registries, package reader).
  - **30, 34, 36, 40, 42, 44** — the performance/fidelity/export/calc/grid/
    desktop architecture pillars.
  - **45–49, 54–57** — product surface: editor parity, UX map, design system,
    per-construct design notes (added as phases open).
  - **50–53** — measured gap trackers, generated from or audited against the
    code rather than hand-maintained.

## Index

### Foundation & process (00–19)

| # | Title | Purpose |
| --- | --- | --- |
| 00 | This index | Map of the design record |
| 01 | [Outcome & Requirements (ORD)](01-ORD.md) | What OpenCalc is for, and for whom |
| 02 | [Architecture](02-ARCHITECTURE.md) | Target architecture and principles |
| 06 | [Roadmap & Delivery](06-ROADMAP-AND-DELIVERY.md) | Phases, deliverables, exit gates |
| 07 | [Quality, Security & Compatibility](07-QUALITY-SECURITY-AND-COMPATIBILITY.md) | The non-negotiables |
| 08 | [ADR Register](08-ADR-REGISTER.md) | Accepted architecture decisions |
| 11 | [Design-First Process](11-DESIGN-FIRST-PROCESS.md) | How work is designed before it is built |
| 12 | [Competitive Analysis](12-COMPETITIVE-ANALYSIS.md) | Excel, LibreOffice, OnlyOffice, Univer, IronCalc, Formualizer, Google Sheets |
| 14 | [Execution Tracker](14-EXECUTION-TRACKER.md) | Live state of all work (tracked, always) |
| 15 | [CI & Release Gates](15-CI-AND-RELEASE-GATES.md) | The PR contract |
| 16 | [Documentation Maintenance](16-DOCUMENTATION-MAINTENANCE.md) | Keeping docs and code in sync |
| 17 | [Glossary](17-GLOSSARY.md) | Shared vocabulary |
| 18 | [Support Matrix](18-SUPPORT-MATRIX.md) | Target vs implemented, per platform & feature |
| 19 | [Workspace Scaffold & Layer Division](19-WORKSPACE-SCAFFOLD-DESIGN.md) | Crates, the dependency DAG, the seams |

### Contracts (20–28)

| # | Title | Purpose |
| --- | --- | --- |
| 20 | [Error-Code Registry](20-ERROR-CODE-REGISTRY.md) | Stable diagnostic codes |
| 21 | [Parser Limits](21-PARSER-LIMITS.md) | Security bounds on all admission |
| 22 | [Normalized Workbook Schema](22-NORMALIZED-SCHEMA.md) | The in-memory model + reserved calc seams |
| 23 | [Cell-Store Representation](23-CELL-STORE-REPRESENTATION.md) | The sparse grid internals (drives T1/T2) |
| 24 | [Transaction & Edit Semantics](24-TRANSACTION-AND-EDIT-SEMANTICS.md) | Op set, inverses, reference rewriting, collab seam |
| 28 | [XLSX Package Reader](28-XLSX-PACKAGE-READER.md) | SpreadsheetML OPC admission |
| 33 | [Fidelity Ledger](33-FIDELITY-LEDGER.md) | Per-construct status across model/round-trip/edit/render/calc |
| 29 | [Phase 0 Plan & Scaffold Specs](29-PHASE-0-PLAN.md) | Ordered F-### items + ready-to-instantiate build config |
| 31 | [Phase D Exit Report](31-PHASE-D-EXIT-REPORT.md) | Documentation phase closure |

### Architecture pillars (30, 34, 36, 40, 42, 44)

| # | Title | Purpose |
| --- | --- | --- |
| 30 | [Performance & Capacity Targets](30-PERFORMANCE-AND-CAPACITY-TARGETS.md) | 1M cells, 60 fps, <50 ms recalc |
| 34 | [SpreadsheetML Fidelity & Preservation](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md) | Loss-aware import |
| 36 | [Export & Round-Trip](36-EXPORT-AND-ROUNDTRIP-DESIGN.md) | Byte-identical repackager + semantic writer |
| 40 | [Formula & Calculation Engine](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md) | Parser, dependency graph, recalc (Phase 2, designed now) |
| 42 | [Grid Layout, Virtualization & Rendering](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md) | Laying out and painting a million cells |
| 44 | [Tauri Desktop Shell](44-TAURI-DESKTOP-SHELL-DESIGN.md) | The native desktop host — engine as native Rust |

### Product, UX & the SDK (45–49, 54–57)

| # | Title | Purpose |
| --- | --- | --- |
| 45 | [Editor Parity Tracker](45-EDITOR-PARITY-TRACKER.md) | The browser editor measured against Excel and Sheets |
| 46 | [Competitive Parity Analysis](46-COMPETITIVE-PARITY-ANALYSIS.md) | Where we stand feature-for-feature, and what that costs |
| 47 | [UX & Feature Map](47-UX-AND-FEATURE-MAP.md) | Every surface the editor exposes, and where it lives |
| 48 | [Feature Pipeline](48-FEATURE-PIPELINE.md) | What is queued, in what order, and why |
| 49 | [Design System](49-DESIGN-SYSTEM.md) | The editor's visual language — tokens, spacing, chrome |
| 54 | [Pivot Tables](54-PIVOT-TABLES.md) | Model, one-pass accumulation, refresh safety, `GETPIVOTDATA` |
| 55 | [SDK Embedding & Integration](55-SDK-EMBEDDING-AND-INTEGRATION-DESIGN.md) | The embeddable element: isolation, theming, commands, events, access, i18n |
| 56 | [Collaboration Concurrency](56-COLLABORATION-CONCURRENCY-DESIGN.md) | OT vs CRDT for Phase 5, and why the per-cell byte ceiling decides it (ADR-011) |
| 57 | [Collaboration Server Boundary](57-COLLABORATION-SERVER-BOUNDARY.md) | What the server owns and must not own; webhook callbacks not a file store, and horizontal scale without sticky sessions (ADR-012) |

### Measured gap trackers (50–53)

Generated or audited against the code rather than written by hand, so a row
cannot quietly disagree with what shipped.

| # | Title | Purpose |
| --- | --- | --- |
| 50 | [UX Completeness Tracker](50-UX-COMPLETENESS-TRACKER.md) | Editor gaps with severity — closed at all 63 rows |
| 51 | [Fidelity Gap Audit](51-FIDELITY-GAP-AUDIT.md) | The measured SpreadsheetML construct register |
| 52 | [Fidelity Tracker](52-FIDELITY-TRACKER.md) | FID-### / FN-### execution against that register |
| 53 | [Feature Correctness Tracker](53-FEATURE-CORRECTNESS-TRACKER.md) | FC-### — modelled fields that no surface could reach |

## Status

**Alpha — engine, editor and embeddable SDK are live.** Phases 0–1E done,
Phase 2 (calc) substantially done, Phase 3 shipped, and the browser SDK
published to npm as `@opencalc/sheet`. The Phase 5 concurrency model is
**decided and unbuilt** — server-mediated OT, not a CRDT (ADR-011,
[56](56-COLLABORATION-CONCURRENCY-DESIGN.md)).

`docs/` remains the source of truth for *design*: a substantial design is
settled here before it is implemented. It is no longer the source of truth for
*state* — for that see [14-EXECUTION-TRACKER](14-EXECUTION-TRACKER.md) and the
measured trackers above, and keep this section agreeing with
[AGENTS.md](../AGENTS.md) §Current state and the phase table in
[README.md](../README.md).
