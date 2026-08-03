# 01 — Outcome & Requirements (ORD)

What OpenCalc is for, who it serves, and the requirements that define "done."

## Problem

Working with `.xlsx` today forces a trade-off:

- A **full office suite** (Excel, LibreOffice) has real fidelity and a real calc
  engine, but you can't embed it in your product.
- A **converter/library** (readers and writers) round-trips bytes but has no live
  model, no calculation, and often drops what it doesn't understand.
- A **web spreadsheet** treats the DOM as truth and pushes calculation to a
  server, so the model isn't portable and offline/embedded use is hard.

There is no **embeddable, deterministic, loss-aware** spreadsheet *engine* that
reads real workbooks, holds them in an editable model, calculates them, and
renders a live grid — native, in the browser (WASM), and headless — the way
OpenDoc is that engine for `.docx`.

## Outcome

A host application can:

1. **Open** a real-world `.xlsx` (and later `.ods`, CSV) under strict resource
   bounds, getting a normalized, editable workbook model plus an honest
   compatibility report.
2. **Read and query** cells, formulas, styles, number formats, defined names,
   merges, and sheet structure.
3. **Edit** through atomic, reversible operations (set cells, insert/delete rows
   & columns with reference rewriting, format, merge).
4. **Calculate** — evaluate formulas with a dependency graph and incremental
   recalculation matching Excel/LibreOffice semantics *(Phase 2)*.
5. **Lay out and render** a live cell grid — including a 1M-cell sheet — as a
   backend-neutral display list and to pixels, virtualized for 60 fps.
6. **Write back** to a valid `.xlsx` — byte-identical for an unedited workbook,
   deterministic canonical OOXML for an edited one — losing nothing silently.
7. Do all of the above **in Rust, in WASM, and headless**, with no mandatory DOM,
   server, or UI framework.

## Users

- **Product engineers** embedding a spreadsheet in an app (native or web) without
  shipping a browser or a server round-trip.
- **Backend/data engineers** who need faithful, bounded, headless `.xlsx`
  read/modify/write and correct calculation.
- **The Casual Sheets product** — OpenCalc is its engine.

## Requirements

### Functional

- FR-1 Read `.xlsx` into a normalized model; report every unmodeled construct.
- FR-2 Preserve unknown/unmodeled content verbatim or by explicit report.
- FR-3 Write `.xlsx`: byte-identical (unedited) and semantic (edited) paths.
- FR-4 Edit via atomic, invertible operations; correct reference rewriting.
- FR-5 Parse formulas to a stable AST at import (evaluate in Phase 2).
- FR-6 Calculate with a dependency graph + incremental recalc (Phase 2).
- FR-7 Lay out the grid (geometry, merges, frozen panes, in-cell rich text,
  number-format display) into a display list; render to pixels.
- FR-8 Virtualize layout/paint to the visible window over a 1M-cell sheet.
- FR-9 Import/export `.ods` and CSV via the format-neutral adapter registry.

### Non-functional

- NFR-1 **Determinism** — identical input + version ⇒ identical model, values,
  layout, bytes.
- NFR-2 **Security** — bounded admission; no macros; no auto network fetch
  ([21](21-PARSER-LIMITS.md)).
- NFR-3 **Scale** — 1,000,000+ populated cells within a bounded memory budget.
- NFR-4 **Latency** — <50 ms worst-case incremental recalc; 60 fps grid scroll
  ([30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)).
- NFR-5 **Fidelity** — computed values and rendered cells verifiable against a
  LibreOffice Calc / Excel oracle.
- NFR-6 **Portability** — native + `wasm32-unknown-unknown` + headless.
- NFR-7 **Embeddability** — no mandatory DOM/server/framework; host owns policy.

## Non-goals (for now)

- Not a UI product; OpenCalc is an engine. (The WASM grid editor is a
  developer surface, not a shipped app.)
- No macro/VBA execution — ever, by policy.
- No server-side collaboration service in the core (the op model for
  collaboration is Phase 5; transport is the host's).
- Full chart/pivot *rendering* fidelity is a later phase; charts/pivots are
  **preserved** from Phase 1A.

## Definition of done (engine-level)

A workbook can be opened, edited, calculated, laid out at scale, and written back
— deterministically, within the security bounds, meeting the performance targets,
and passing the fidelity oracle — with nothing lost silently.
