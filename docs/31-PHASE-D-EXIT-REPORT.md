# 31 — Phase D (Documentation) Exit Report

This report closes the **documentation phase** against its exit gate in
[06-ROADMAP-AND-DELIVERY](06-ROADMAP-AND-DELIVERY.md). Phase D delivered the full
design record for OpenCalc — governance, architecture, contracts, and the two
design-critical pillars (layer division and virtualization) — with the calc
engine fully designed but held to Phase 2.

Date: 2026-08-04.

## Exit-gate check

| Gate condition | Status |
| --- | --- |
| Every doc in [00-README](00-README.md) exists and is indexed | ✅ 26 numbered docs + governance |
| Cross-references resolve; no dangling doc links | ✅ verified |
| Layer division finalized (crate DAG + seams) | ✅ [19](19-WORKSPACE-SCAFFOLD-DESIGN.md), ADR-003 |
| Virtualization strategy finalized | ✅ [30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md), [42](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md), ADR-009 |
| Model reserves the calc seams (no P2 schema break) | ✅ [22](22-NORMALIZED-SCHEMA.md), [23](23-CELL-STORE-REPRESENTATION.md), ADR-004/005 |
| Loss-aware preservation designed | ✅ [34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md), ADR-007 |
| Round-trip / export designed | ✅ [36](36-EXPORT-AND-ROUNDTRIP-DESIGN.md) |
| Dual host (Tauri native + web WASM) designed | ✅ [02](02-ARCHITECTURE.md), [44](44-TAURI-DESKTOP-SHELL-DESIGN.md) |
| Consistency audit run and findings applied | ✅ [14](14-EXECUTION-TRACKER.md) DOC-019 |
| No open ADR trigger for the model or calc seams | ✅ (open items are Phase-0/2 ADRs, listed below) |

**Verdict: Phase D exit gate passed.** The design is internally consistent and
commits the whole architecture up front, per the "design it right the first
time" mandate ([AGENTS.md](../AGENTS.md)).

## What is designed (inventory)

- **Process & governance:** 01 ORD, 02 architecture, 06 roadmap, 07 quality/
  security, 08 ADRs, 11 design-first, 12 competitive, 14 tracker, 15 CI gates,
  16 doc-maintenance, 17 glossary, 18 support matrix.
- **Layer division:** 19 (crate DAG, seams, boundary invariants).
- **Model & edit contracts:** 22 schema (reserved calc seams), 23 cell store,
  24 transaction/edit semantics.
- **Format contracts:** 20 errors, 21 limits, 28 package reader, 34 fidelity/
  preservation, 36 export/round-trip.
- **Architecture pillars:** 30 performance/capacity, 40 calc engine, 42 grid
  layout/virtualization/render, 44 Tauri desktop shell.

## ADR status

**Accepted (10):** ADR-001..010 — product identity, phased/held-back delivery,
layer division, sparse model, reserved calc seams, bounded OPC, preservation,
display-list backend, virtualization, deterministic snapshots.

**Pending — to be Accepted at their phase** (carried forward, see
[08](08-ADR-REGISTER.md)):

- Dual-host capability trait (Phase 0).
- Edit / operation schema (before Phase 1A editing).
- MSRV / toolchain pin (Phase 0).
- Cell-store constants — shape Accepted, constants at Phase 0.
- Dependency-graph representation; recalc-ordering strategy (before Phase 2).
- Collaboration op model (Phase 5).

None of these block Phase 0; each is scoped to the phase that needs it, and none
can force a lower-layer rewrite (that's the point of fixing the seams now).

## Open decisions carried into implementation

Each design note lists its own "open decisions." The material ones for the near
term (Phase 0 → 1B):

- Exact cell-store constants (`BLOCK_H`, second-axis blocking, `Cell` byte
  ceiling) — decided by benchmark ([23](23-CELL-STORE-REPRESENTATION.md)).
- Streaming reader/writer buffering strategy for very large sheets
  ([28](28-XLSX-PACKAGE-READER.md), [36](36-EXPORT-AND-ROUNDTRIP-DESIGN.md)).
- The exact package/XML/cell limit defaults ([21](21-PARSER-LIMITS.md)).

## Known gaps (intentionally later)

- Per-function-family calc specs (Phase 2 detail; the engine architecture is set
  in [40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md)).
- Selection-model detail doc (folded into [24](24-TRANSACTION-AND-EDIT-SEMANTICS.md)
  for now; may get its own note when the editor phase opens).
- Per-construct feature notes (conditional formatting, validation, charts,
  pivots) — added as Phase 3 opens.

## Handoff to Phase 0

The next phase is **Phase 0 — Foundation**. Its concrete, tracked plan and the
ready-to-instantiate build scaffolding (workspace `Cargo.toml`, `rust-toolchain.toml`,
`deny.toml`, CI workflow) are specified in
[29-PHASE-0-PLAN](29-PHASE-0-PLAN.md). Phase 0 is the first phase that creates
build files; it still writes no engine logic beyond the bounded reader and the
minimal model shell, and every step is gated by [15](15-CI-AND-RELEASE-GATES.md).
