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
- **The calc engine is built.** 364 functions dispatch, dynamic arrays spill
  and refuse rather than overwrite, `LET` and `LAMBDA` carry first-class
  function values, and recalculation is incremental over a precedent graph kept
  across edits — a cell-reference edit is flat from ten thousand cells to a
  hundred thousand. Automatic or manual mode comes from the file's own
  `<calcPr>`. See
  [docs/40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md](docs/40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md)
  and [docs/66-INCREMENTAL-RECALC-GRAPH.md](docs/66-INCREMENTAL-RECALC-GRAPH.md).
  What remains is budget rather than capability: the <50 ms target is asserted
  for warm incremental recalc only (`PERF-07`), and range precedents are still
  scanned linearly (`PERF-06`).
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

**Alpha — the engine, the editor and the embeddable SDK are live.** Phases 0
through 1E are done, Phase 2 (calc) is substantially done, and Phase 3
(spreadsheet features) has shipped. The workspace is fifteen crates; the browser
editor runs the same core through WebAssembly.

What is still open, and therefore where the work is:

- **Phase 2 tail** — the persistent incremental dependency graph and the <50 ms
  worst-case recalc budget.
- **Phase 4** — the SDK is published (`@opencalc/sheet` and friends, released
  by an `sdk-v*` tag); what remains is a *stable* API, since `0.0.x` is a
  preview.
- **Phase 5** — **built and running, both halves.** `casual-calc-transaction`
  carries `transform` (TP1 as a property), the session protocol
  (`PROTOCOL_VERSION` 5), snapshots and idempotent submissions;
  `server/casual-calc-collab-server` is a workspace member serving `/collab`,
  with a leader per document, epoch-fenced appends, relay from any node,
  resume, presence and host callbacks, plus a standalone mode needing no
  external services. Two browsers drive the real binary in CI.
  ADR-011, ADR-012, ADR-014 and ADR-017 are all **Accepted** — the concurrency
  model is decided, not open. See [56](docs/56-COLLABORATION-CONCURRENCY-DESIGN.md),
  [57](docs/57-COLLABORATION-SERVER-BOUNDARY.md),
  [59](docs/59-COLLABORATION-SERVICE-STACK.md).
  The boundary still holds and is checked by CI: **nothing in `crates/` may
  depend on the server**.
  What is *not* done is operational — no metrics, no published image, no chart
  ([14](docs/14-EXECUTION-TRACKER.md), `DEP-06..08`).

The design-first rule has not relaxed now that code exists: a substantial
design is discussed and written down before it is implemented, not alongside.
Do not update this section from a commit message — update it when a phase
actually changes state, and keep it agreeing with the phase table in
[README.md](README.md) and [06](docs/06-ROADMAP-AND-DELIVERY.md).
