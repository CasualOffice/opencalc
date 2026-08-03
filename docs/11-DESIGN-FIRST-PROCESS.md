# 11 — Design-First Process

OpenCalc is designed before it is built. This is not ceremony: the engine has
tightly coupled layers (model ↔ calc ↔ layout ↔ render) and hard scale targets,
and a wrong lower-layer decision is expensive to undo. The whole point of the
documentation phase is to **get the design right the first time** so later phases
slot in without a do-over.

## The eight steps

1. **Problem definition.** State the outcome: what a host or user can do
   afterward that they can't now. One paragraph.
2. **Research.** How do Excel, LibreOffice Calc, OnlyOffice, Univer, IronCalc,
   Formualizer, and Google Sheets handle it? Record each source and the date
   checked (research goes stale — see [16](16-DOCUMENTATION-MAINTENANCE.md)).
3. **Design note.** Write a numbered `docs/` note. Cover the model impact, the
   layer(s) touched, the seams, the failure modes, and the acceptance gates.
4. **Discussion & finalization.** Substantial designs are discussed and marked
   final before implementation. If an ADR trigger fires, the ADR must be
   **Accepted** first.
5. **Tracker update.** Add or move a row in
   [14-EXECUTION-TRACKER](14-EXECUTION-TRACKER.md) with a stable ID and status.
6. **Implementation.** Small, reviewable increments; one capability per PR.
7. **Verification.** Run the gates in [15](15-CI-AND-RELEASE-GATES.md); prove the
   acceptance criteria.
8. **Documentation.** Update the design note, ADRs, support matrix, and tracker
   so the written design and the code never diverge.

## ADR triggers

Write an ADR (record it in [08-ADR-REGISTER](08-ADR-REGISTER.md)) when a decision
touches any of:

- A **public API** or SDK surface.
- A **crate boundary** or the dependency DAG (the layer division).
- A **serialization format** — the normalized snapshot, the on-wire op model.
- **Transaction / edit semantics** — the op set, inverses, reference rewriting.
- **The model's representation** of the cell grid or the reserved calc seams.
- **The dependency-graph or recalculation model** (even though it's built in
  Phase 2, its shape is decided now).
- **Layout units**, the display-list contract, or a render backend.
- **Parser or security policy** — a limit, an admission rule.
- A **dependency choice** (a new crate, a font/shaping/raster library).
- The **collaboration op model** or any plugin/trust boundary.
- A **performance budget** that constrains a lower layer (see [30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)).

## "Held back, not un-designed"

The calc engine ships in Phase 2, but its design is finalized during the
documentation phase, because it dictates seams in the model (Phase 1A) and the
layout engine (Phase 1C). Any design that would force those earlier layers to be
rewritten when the calc engine arrives is rejected. The same rule applies to
collaboration and to the virtualization strategy.

## Design note template

```
# NN — Title

## Outcome
## Research (sources + dates)
## Design
### Model / schema impact
### Layers touched & seams
### Failure modes & limits
## Alternatives considered
## Acceptance gates (tests / fixtures)
## ADRs triggered
## Tracker IDs
```
