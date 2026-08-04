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
| DOC-012 | Competitive analysis (12) | Done | Univer/OnlyOffice/LO Calc/Excel/Sheets/IronCalc/Formualizer |
| DOC-013 | Dual-host design: Tauri desktop (native) + web (WASM) | Done | Folded into 02/18/19/40; host capability trait |
| DOC-014 | Tauri desktop shell design note (44) | Done | Native host; capability trait; command surface |
| DOC-015 | Repo scaffolding (LICENSE, SECURITY, GOVERNANCE, CODE_OF_CONDUCT, .github templates) | Done | Apache-2.0; PR + issue templates |
| DOC-016 | CI workflow YAML + rust-toolchain.toml + deny.toml | Ready | Specs written in doc 29; instantiated in Phase 0 (F-002/003/004) |
| DOC-017 | Cell-store representation (23) | Done | Sparse row-blocked tiles; per-cell budget (ADR-004) |
| DOC-018 | Transaction & edit semantics (24) | Done | Op set, inverses, reference rewriting, collab seam |
| DOC-019 | Doc-set consistency audit + fixes | Done | 11 findings applied 2026-08-04 |
| DOC-020 | Export & round-trip design (36) | Done | Byte-identical repackager + semantic writer |
| DOC-021 | Phase 0 plan + scaffold specs (29) | Done | F-### breakdown + ready-to-instantiate config |
| DOC-022 | Phase D exit report (31) | Done | Exit gate PASSED 2026-08-04 |

**Documentation phase (Phase D): CLOSED — exit gate passed 2026-08-04**
([31-PHASE-D-EXIT-REPORT](31-PHASE-D-EXIT-REPORT.md)).

## Phase rows — Phase 0 (Foundation)

**Phase 0: CLOSED — exit gate PASSED 2026-08-04.** All items below `Done`; all 12
CI jobs green (fmt, lint, test, docs, wasm, benchmark-smoke, fuzz-build,
repository-policy, dependency-policy, platform ×3 incl. MSRV). Detailed in
[29-PHASE-0-PLAN](29-PHASE-0-PLAN.md).

| ID | Workstream | Status | Notes |
| --- | --- | --- | --- |
| F-001 | Workspace skeleton (root Cargo.toml + crate dirs) | Done | 15 crates + 2 tools; `cargo check --workspace` green (2026-08-04) |
| F-002 | rust-toolchain.toml, workspace lints, release profile | Done | channel 1.96.0; clippy `all` priority -1 |
| F-003 | deny.toml supply-chain policy | Done | `cargo deny check` bans/licenses/sources ok |
| F-004 | CI workflow (gate jobs) | Done | format/lint/test/docs/wasm/dependency-policy/platform(+MSRV); benchmark/fuzz/repo/browser jobs deferred to their items |
| F-005 | CI badge wired | Done | README badge → ci.yml |
| F-006 | Fixture corpus + manifest.json + generator | Done | Deterministic generator; committed minimal.xlsx + sha256 manifest; parses through the real reader (test); repository-policy CI job |
| F-007 | Benchmark harness + baseline | Done | tools/casual-calc-benchmark: versioned JSON, median/p95 + determinism check; committed dev-reference baseline; CI benchmark-smoke job with jq validation |
| F-008 | Fuzz workspace (pinned nightly) | Done | Separate workspace; bounded_package target (200k runs, no crash); fuzz-build CI job asserts lockfile unchanged |
| F-012 | Fix docs CI (rustdoc `<label>` HTML) | Done | Benchmark usage wrapped in code fence; doc gate green |
| F-009 | casual-calc-package: bounded OPC admission | Done | limits + path safety + capped part reads; 10 tests incl. zip-bomb/traversal; wasm-clean; codes OC-PKG-0001..0006 |
| F-010 | casual-calc-model shell + snapshot I/O + reserved seams | Done | Ids, CellValue, Cell (reserved seams), sparse CellStore, Sheet, Workbook; deterministic snapshots; empty-workbook byte-stable round-trip gated; 8 tests |
| F-011 | Minimal casual-calc-ooxml (open + discover workbook) | Done | Opens a trivial .xlsx; resolves workbook + sheet parts via OPC rels; bounded XML; 8 tests; codes OC-XML/OC-IMP |

## Phase rows — Phase 1A (Semantic import & modeling)

Import SpreadsheetML → normalized model + compatibility report; formulas parsed
& preserved but **not evaluated** ([06](06-ROADMAP-AND-DELIVERY.md),
[34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)).

| ID | Workstream | Status | Notes |
| --- | --- | --- | --- |
| P1A-001 | Model string table + cell-value import (shared/inline strings, number, bool, error) | Done | `StringTable` in model; `casual-calc-import` maps values + dual-axis `CompatibilityReport`; A1 parsing; deterministic; 16 tests (10 model + 6 import) |
| P1A-002 | Formula import → AST (`casual-calc-formula` tokenizer/parser) | Done | Lexer + Pratt parser + serde AST + pretty-printer (round-trip gated); model gains a formula arena (model→formula dep); import parses `<f>` → AST, cached value kept; unparseable → Degraded. 26 tests (formula 9 + model 10 + import 7) |
| P1A-003 | Number formats + cell-style linkage import | Done | Model `Style`/`StyleTable` (interned, deduped); import parses styles.xml numFmts + cellXfs (custom + built-in numFmtId subset); cell `s` → StyleId; number format modeled. Font/fill/border deferred to P1A-003b. 18 tests (model 10 + import 8) |
| P1A-003b | Styles: font, fill, border, alignment | Not started | Extends `Style`; needed for render fidelity |
| P1A-004 | Defined names, merged ranges, sheet views | Not started | — |
| P1A-005 | Proper part discovery via content-types + all rels (not conventional paths) | Not started | sharedStrings currently found by conventional path |
| P1A-006 | Retention mode + retained-source / opaque parts | Not started | — |

## Review note

Keep this file readable. When it grows large, split closed phases into an
archive doc (e.g. `14a-TRACKER-ARCHIVE-PHASE-0.md`) and keep only active +
recent rows here — but never drop IDs from the record.
