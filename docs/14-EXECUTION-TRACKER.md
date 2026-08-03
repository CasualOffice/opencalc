# 14 — Execution Tracker

**The single source of truth for the live state of all OpenCalc work.** Nothing
is worked on without a row here; nothing merges without its row updated. This is
the discipline: *track everything, update as it moves.*

## How to use this tracker

- **Every unit of work gets a row** with a **stable ID** the moment it starts
  (design, code, docs, fixtures, CI — all of it).
- The ID is the **cross-reference key** used everywhere else: PR titles,
  changelog entries, design-note "Tracker IDs" sections, and ADRs all cite it.
- **Update the status as it moves** — never let a row go stale. If a row is
  wrong, fix it in the same PR that made it wrong.
- IDs are **never reused.** A dropped item is marked `Dropped`, not deleted.

## ID scheme

`<PHASE>-<NNN>`, zero-padded, assigned in creation order within a phase:

| Prefix | Phase |
| --- | --- |
| `DOC-###` | Documentation-phase work (this repo's current state) |
| `F-###` | Phase 0 — Foundation |
| `P1A-###` | Phase 1A — Import & model |
| `P1B-###` | Phase 1B — Semantic writer |
| `P1C-###` | Phase 1C — Grid layout |
| `P1D-###` | Phase 1D — Grid render & virtualization |
| `P1E-###` | Phase 1E — Browser grid editor |
| `P2-###` | Phase 2 — Formula & calc engine |
| `P3-###` | Phase 3 — Spreadsheet features |
| `MNT-###` | Cross-cutting maintenance |

## Controlled status vocabulary

Use exactly these values — no ad-hoc statuses:

`Not started` · `Researching` · `Designing` · `Finalizing` · `Ready` ·
`In progress` · `Blocked` · `In review` · `Done` · `Dropped`

- **Ready** means: design finalized, ADRs accepted, acceptance gates defined —
  cleared to implement.
- **Blocked** must name the blocker (another ID, a decision, an upstream dep).

## Current rows — Documentation phase

| ID | Workstream | Status | Notes |
| --- | --- | --- | --- |
| DOC-001 | Root governance (README, AGENTS, CLAUDE, SKILLS, CONTRIBUTING, CHANGELOG) | Done | Authored 2026-08-04 |
| DOC-002 | docs index + process spine (00, 08, 11, 15, 16, 17) | Done | Authored 2026-08-04 |
| DOC-003 | Requirements, architecture, roadmap (01, 02, 06, 07, 18) | Done | Authored 2026-08-04 |
| DOC-004 | Layer division / workspace scaffold (19) | Done | Design-critical; DAG + seams fixed (ADR-003) |
| DOC-005 | Performance & capacity targets (30) | Done | 1M cells / 60 fps / <50 ms recalc |
| DOC-006 | Grid layout, virtualization & rendering (42) | Done | Design-critical; O(visible) layout |
| DOC-007 | Normalized workbook schema + reserved calc seams (22) | Done | Reserved seams (ADR-005) |
| DOC-008 | Formula & calc engine architecture (40) | Done | Held back to P2, designed now |
| DOC-009 | SpreadsheetML fidelity & preservation (34) | Done | Loss-aware discipline |
| DOC-010 | XLSX package reader (28) | Done | — |
| DOC-011 | Error registry + parser limits (20, 21) | Done | Security contracts |
| DOC-012 | Competitive analysis (12) | Done | Univer/OnlyOffice/LO Calc/Excel/Sheets/IronCalc |
| DOC-013 | Dual-host design: Tauri desktop (native) + web (WASM) | Done | Folded into 02/18/19/40; host capability trait |
| DOC-014 | Tauri desktop shell design note | Not started | Deferred design note; host-side glue only |
| DOC-015 | Reference/tooling docs (GOVERNANCE, SECURITY, LICENSE, .github, rust-toolchain, deny.toml) | Not started | Non-blocking; align with OpenDoc |

## Phase rows

Populated as each phase opens. Phase 0 (`F-###`) rows are added once the
documentation phase closes and the foundation design is finalized.

## Review note

Keep this file readable. When it grows large, split closed phases into an
archive doc (e.g. `14a-TRACKER-ARCHIVE-PHASE-0.md`) and keep only active +
recent rows here — but never drop IDs from the record.
