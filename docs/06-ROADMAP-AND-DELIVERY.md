# 06 — Roadmap & Delivery

OpenCalc is delivered in **capability-gated phases.** A phase is `Done` only when
its **exit gate** passes — not when the code merely exists. Phases are
independently gated: passing import does not imply layout; passing layout does
not imply calculation.

The design of every phase is settled during the documentation phase. The
*construction* order below holds the calc engine back to Phase 2, but its design
(and its seams in earlier layers) is fixed now — see
[40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md) and
[22](22-NORMALIZED-SCHEMA.md).

## Delivery strategy

- **Design-first, no do-overs.** Layer division ([19](19-WORKSPACE-SCAFFOLD-DESIGN.md))
  and virtualization ([30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md),
  [42](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md)) are correct before code.
- **Determinism and fidelity are exit-gate conditions**, not later hardening.
- **Track everything.** Each deliverable is one or more tracker rows
  ([14](14-EXECUTION-TRACKER.md)); the phase closes only when they're all `Done`.

---

## Phase D — Documentation (current)

**Deliver:** the full design record — governance, architecture, roadmap,
contracts (schema, limits, package reader, fidelity), and the two design-critical
pillars (layer division, virtualization) plus the finalized calc-engine design.

**Exit gate:** every doc in [00-README](00-README.md) exists and is internally
consistent; the layer division and virtualization designs are finalized (ADRs
Accepted); no open ADR trigger for the model or calc seams.

---

## Phase 0 — Foundation

**Deliver:** the Cargo workspace with the crate skeleton from
[19](19-WORKSPACE-SCAFFOLD-DESIGN.md); the CI gate matrix
([15](15-CI-AND-RELEASE-GATES.md)); a checksummed fixture corpus (synthetic +
rights-reviewed real-producer `.xlsx`); the benchmark harness with a committed
named-environment baseline; the bounded XLSX package reader
([28](28-XLSX-PACKAGE-READER.md)); and the minimal normalized model shell with
its reserved calc seams ([22](22-NORMALIZED-SCHEMA.md)).

**Exit gate:** CI green on all platforms; a hostile fixture (zip bomb, deep
nesting, oversized parts) is rejected within limits; the model round-trips an
empty workbook snapshot byte-stably.

---

## Phase 1A — Semantic import & workbook modeling

**Deliver:** import of `workbook.xml`, worksheets, `sharedStrings.xml`,
`styles.xml` (number formats, fonts, fills, borders, `cellXfs`), defined names,
merged ranges, sheet views (panes, dimensions), and cell values into the
normalized model; the **dual-axis disposition taxonomy**, the **compatibility
report**, retained-source and opaque-part side tables
([34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)); deterministic JSON snapshots.
**Formulas are parsed to an AST and preserved in the reserved seam — not
evaluated.**

**Exit gate:** the fixture corpus imports; the compatibility report accounts for
every unmodeled construct; snapshots are golden-stable; formula ASTs round-trip
through pretty-print.

---

## Phase 1B — Semantic writer

**Deliver:** `casual-calc-export` — the byte-identical repackager (unedited
workbook reconstructs bit-for-bit) and the semantic writer (edited model →
valid, deterministic `.xlsx` that opens cleanly in LibreOffice Calc / Excel),
including opaque-part merge-back.

**Exit gate:** `import(retention) → write → reopen` is a model fixed point;
unedited packages reconstruct byte-identically; written files open without repair
in LibreOffice Calc.

---

## Phase 1C — Grid layout

**Deliver:** `casual-calc-layout` — column/row geometry (default + explicit,
hidden, outline), merged-cell layout, frozen panes/splits, in-cell rich text
shaping (`parley`), number-format-driven display (dates, currency, scientific,
fractions), alignment/wrap/shrink/rotation, and the backend-neutral display list.
Layout reads **cached cell values**, so it works before the calc engine exists.

**Exit gate:** golden display lists for the layout fixtures; the number-format
display matches the oracle on a format corpus; merged/frozen fixtures are correct.

---

## Phase 1D — Grid render & virtualization

**Deliver:** `casual-calc-render` (CPU raster, tiny-skia + skrifa); the
**viewport virtualization** path so layout+paint touch only the visible window of
a 1M-cell sheet; incremental repaint on scroll and edit; hit-testing
(pixel ↔ cell).

**Exit gate:** the virtualized-viewport output equals the full-layout output for
fixtures; the scroll/paint benchmark meets the 60 fps budget on the baseline env;
a 1M-cell fixture stays within the memory budget ([30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)).

---

## Phase 1E — Browser grid editor (WASM)

**Deliver:** `casual-calc-wasm` + a zero-server webapp harness: open `.xlsx`,
render the grid, edit cells, select ranges, fill, format, undo/redo, save back.
Values remain static (calc engine not yet wired).

**Exit gate:** browser-smoke (Playwright) loads a fixture, edits, and saves; the
editor scrolls a 1M-cell fixture at target frame rate.

---

## Phase 2 — Formula & calculation engine

**Deliver:** `casual-calc-eval` — the dependency graph over the model's reserved
seams; incremental recalculation (dirty propagation + topological order); cycle
detection and bounded iterative calc; volatile functions; spill/dynamic arrays;
and the built-in function library (math, text, lookup, logical, date/time,
statistical, financial, information). Edits now trigger recompute; layout reads
freshly computed cached values.

**Exit gate:** computed values match the LibreOffice Calc / Excel oracle across
the formula corpus; worst-case incremental recalc is <50 ms on the baseline env
([30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)); recalc is deterministic
(golden results).

---

## Phase 3 — Spreadsheet features

**Deliver:** conditional formatting, data validation, tables & structured
references, autofilter & sort, and preserved-then-rendered charts and pivot
tables.

**Exit gate:** each feature has import→model→render→export fidelity against the
oracle; nothing previously preserved regresses to dropped.

---

## Phase 4 — SDK beta / embedding

**Deliver:** the stable `casual-calc-sdk` surface; native and WASM packaging;
embedding docs and examples.

**Exit gate:** the SDK version is frozen for beta; API-stability tests pass.

---

## Phase 5 — Collaboration / web migration

**Deliver:** the operation model for shared editing (host-supplied transport);
web migration of the editor.

**Exit gate:** concurrent-edit convergence tests pass; no determinism regression.

---

## Phase 6 — 1.0

**Deliver:** stable SDK, published crate line, support guarantees, semver.

**Exit gate:** the support matrix ([18](18-SUPPORT-MATRIX.md)) shows the 1.0
feature set as *implemented* with all release-evidence gates green.

---

## Risk register (top items)

| Risk | Mitigation |
| --- | --- |
| Calc-engine design forces model rewrite | Reserved seams fixed now (ADR-005); Phase 2 adds behavior, not schema |
| Virtualization retrofit is too late | Virtualization designed in Phase 1C/1D from the model up (ADR-009) |
| Fidelity drift vs Excel semantics | Differential oracle gates from Phase 1A (values) and 1C (render) |
| Scale target missed | Sparse model + tile/viewport benchmarks gate Phases 1D and 2 |
| Silent data loss | Disposition taxonomy + compatibility report gate Phase 1A |
