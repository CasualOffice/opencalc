# 07 — Quality, Security & Compatibility

The non-negotiables. These outrank features and performance (see the ordered
priorities in [AGENTS.md](../AGENTS.md)).

## Quality

- **Correctness first.** A change that can produce a wrong cell value, a wrong
  computed result, or a corrupt file does not ship — no matter how useful.
- **Determinism.** Identical input + engine version ⇒ identical model, computed
  values, layout, display list, and output bytes. Enforced by golden tests on
  snapshots, display lists, and recalc results.
- **Fidelity is measured, not asserted.** Computed values and rendered cells are
  diffed against a LibreOffice Calc / Excel oracle. We never claim parity we
  haven't measured, and we never say "lossless" without naming the fidelity
  dimension.
- **Every behavior change carries tests.** Round-trip, determinism, and (Phase 2)
  recalc goldens are mandatory, not optional.

## Fidelity dimensions

Fidelity is not one thing. Each dimension is owned by a phase and tested
separately:

| Dimension | Question | Owner |
| --- | --- | --- |
| Package | Do the OPC parts survive admission and repackaging? | Phase 0/1B |
| Structural | Sheets, cells, ranges, defined names correctly modeled? | Phase 1A |
| Semantic | Styles, number formats, formulas (as text/AST) correct? | Phase 1A |
| Preservation | Is unmodeled content kept or reported, never dropped? | Phase 1A |
| Edit | Do transactions/inverses and reference rewriting behave? | Phase 1A+ |
| Computational | Do formulas evaluate to the oracle's values? | Phase 2 |
| Visual | Do rendered cells match the oracle? | Phase 1C/1D |
| Behavioral | Recalc order, volatility, spill, iterative calc | Phase 2 |
| Diagnostic | Is the compatibility report accurate and complete? | Phase 1A |

## Security

- **No macro / VBA execution.** Ever. VBA parts are preserved as opaque, never run.
- **No automatic external access.** External references, linked images, and
  remote resources are never fetched automatically; the host decides.
- **Bounded admission.** Every parser enforces explicit limits — package size,
  entry count, expansion ratio, path length, XML element/depth caps, cell/sheet
  ceilings ([21](21-PARSER-LIMITS.md)). Untrusted input is rejected cleanly, not
  crashed on.
- **Cancellable jobs.** Long admission/calc jobs are bounded and cancellable so a
  hostile file can't wedge the host.
- **`unsafe` forbidden** workspace-wide (planned), matching OpenDoc.
- **Supply chain** gated by `cargo deny` + `cargo audit`
  ([15](15-CI-AND-RELEASE-GATES.md)).

## Compatibility

- **Round-trip guarantee.** An unedited workbook reconstructs byte-identically;
  an edited one writes deterministic canonical OOXML that opens without repair in
  Excel and LibreOffice Calc.
- **Preservation guarantee.** Constructs the semantic model doesn't represent are
  preserved verbatim (retention mode) or reported (semantic mode) — never
  silently dropped ([34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)).
- **Versioned contracts.** Schema, ops, SDK, and file-compat profile version
  independently ([02](02-ARCHITECTURE.md)); the support matrix
  ([18](18-SUPPORT-MATRIX.md)) tracks target vs implemented per feature.
- **No feature is "supported" until its gate passes.** A target in the support
  matrix is aspirational until its CI/oracle gate is green.
