# AGENTS.md — Agent Contract for OpenCalc

This file is the entry contract for **every** coding agent that works in this
repository. Read it in full before doing anything else.

## Repository boundary

- **This repository (`sheets/`, product name OpenCalc) is the target.** All work
  happens here.
- **`../opendoc-fixes/` is reference-only.** OpenDoc is the sibling `.docx`
  engine OpenCalc is modelled on. Read it to learn the format-neutral spine and
  the process. **Do not modify it.**
- Other siblings (`../opendoc`, `../design-system`, `../docs`) are context, not
  work surfaces.

## Mission

Build a **production-grade** spreadsheet engine. Not an MVP, not a prototype, not
a side project. The bar is a deterministic, embeddable, loss-aware `.xlsx` engine
that reads, models, preserves, writes, calculates, lays out, and renders real
workbooks at scale — with fidelity measured against LibreOffice Calc and Excel.

## Prime directive: design it right the first time

OpenCalc is being designed **fully and correctly up front** so that later phases
slot in without rework. The order of *construction* is phased (see
[docs/06-ROADMAP-AND-DELIVERY.md](docs/06-ROADMAP-AND-DELIVERY.md)), but the
*design* is not deferred:

- **Layer division is settled before code.** Crate boundaries, the dependency
  DAG, and the seams between layers are designed in
  [docs/19-WORKSPACE-SCAFFOLD-DESIGN.md](docs/19-WORKSPACE-SCAFFOLD-DESIGN.md)
  and must not require a do-over when the calc engine or collaboration land.
- **The calc engine is held back, not un-designed.** Formulas are parsed and
  preserved from Phase 1A; the model reserves every seam the dependency graph
  and recalculation need (see
  [docs/22-NORMALIZED-SCHEMA.md](docs/22-NORMALIZED-SCHEMA.md) §"Reserved calc
  seams" and [docs/40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md](docs/40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md)).
  "Held back" means *built later*, never *decided later*.
- **Virtualization is a first-class design axis**, not a late optimization. The
  1M-cell / 60 fps / <50 ms-recalc targets shape the model, the layout engine,
  and the render seam from the start
  ([docs/30-PERFORMANCE-AND-CAPACITY-TARGETS.md](docs/30-PERFORMANCE-AND-CAPACITY-TARGETS.md)).

If a design decision would force a later rewrite of a lower layer, it is wrong —
stop and redesign before writing code.

## Required workflow

For any non-trivial change, in order:

1. **Read the docs.** Start with `docs/00-README.md`; read the design notes and
   ADRs that touch your area.
2. **Design first.** Write or update a numbered design note in `docs/` before
   implementing. If the change trips an ADR trigger (see
   [docs/11-DESIGN-FIRST-PROCESS.md](docs/11-DESIGN-FIRST-PROCESS.md)), write an
   ADR and get it accepted.
3. **Discuss and finalize substantial designs** before building them.
4. **Update the execution tracker** ([docs/14-EXECUTION-TRACKER.md](docs/14-EXECUTION-TRACKER.md)):
   add or move the row, using the controlled status vocabulary.
5. **Implement in small, reviewable increments.** One coherent capability per PR.
6. **Add tests with every behavior change.** Determinism and round-trip/fidelity
   tests are not optional.
7. **Update docs and ADRs** so the written design and the code never diverge.
8. **Keep CI current.** If a gate should exist and doesn't yet, document the gap
   and add the future gate to the CI-and-release-gates doc.

## Engineering priorities (ordered)

When two goals conflict, the earlier one wins:

1. **Correctness & safety** — never produce wrong cell values or corrupt files.
2. **Determinism** — identical input + version ⇒ identical model, values,
   layout, and bytes.
3. **Security & resource bounds** — every parser is bounded; untrusted files
   cannot exhaust memory/CPU; no macro execution; no automatic network fetches.
4. **Compatibility & round-trip fidelity** — preserve intent and unknown data;
   round-trip unedited workbooks byte-faithfully.
5. **Performance & scale** — meet the 1M-cell / 60 fps / <50 ms-recalc targets.
6. **API stability** — public surfaces narrower than internals; versioned.
7. **UX** — grid interaction modelled on MS Sheets 2026 / Google Sheets.
8. **Maintainability.**

## Design rules

- **Mutation only via commands/transactions.** No layer reaches in and mutates
  the workbook model directly; edits go through `casual-calc-transaction` /
  the edit op set, which returns inverses for undo.
- **The host owns policy.** The engine computes; the host decides fonts, I/O,
  network, persistence, and collaboration transport.
- **No DOM (or canvas) as source of truth.** The engine state is authoritative;
  any view is a projection of the display list.
- **No silent data loss.** Anything the semantic model doesn't represent is
  preserved verbatim or reported through the compatibility report — never
  dropped quietly. See [docs/34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md](docs/34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md).
- **The word "lossless" is banned** unless the exact fidelity dimension is named.

## Verification rules

- Run the relevant checks (format, lint, test, docs, wasm) before claiming a task
  done. See [CONTRIBUTING.md](CONTRIBUTING.md) for the exact commands.
- If a verification gate does not exist yet, say so explicitly and add it to the
  future-gates list in [docs/15-CI-AND-RELEASE-GATES.md](docs/15-CI-AND-RELEASE-GATES.md).
- Report outcomes faithfully. If something is preserved-but-not-modelled, or
  parsed-but-not-calculated, say exactly that.

## Current state

**Documentation phase — no engine code exists yet.** Today the only valid work
is authoring and refining the design record in `docs/` and the root governance
files. The first code milestone is Phase 0 (workspace + CI + fixtures + bounded
XLSX reader), and it does not begin until its design docs are finalized.
