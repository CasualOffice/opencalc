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

## Phase rows — Host bridges & site

| ID | Workstream | Status | Notes |
| --- | --- | --- | --- |
| SDK-001 | Host facade (`casual-calc-sdk`) | Done | `WorkbookSession`: open/blank, `edit` (op→recalc), undo/redo, `recalculate`, `layout`, `render_png`, `save`, `compatibility_report`. Composes all layers into one surface; re-exports the host vocabulary. Full lifecycle gated. 4 tests |
| W-001 | WASM bridge (`casual-calc-wasm`) | Done | `wasm-bindgen` bridge: `version`, `eval_formula`, `render_xlsx` (import→recalc→layout→render PNG), `describe_xlsx`. Verified in-browser. `wasm-opt` disabled (bulk-memory) |
| SITE-001 | Marketing site + WASM demo + Pages deploy | Done | `webapp/` (landing + formula playground + open-xlsx render); `.github/workflows/pages.yml` builds wasm-pack + deploys to GitHub Pages |
| W-002 | Interactive canvas grid editor (demo page) | Done | `webapp/editor.html`: canvas grid, headers, **range selection** (drag + shift), inline + formula-bar editing, **formatting toolbar** (Bold + fill swatches), **keyboard shortcuts** (Ctrl+B/Z/Y/S/C/V, Delete, shift+arrows), **copy/paste TSV**, New/Open/Save/Undo/Redo. WASM range ops (`session_toggle_bold`/`set_fill`/`clear_range`/`copy_tsv`/`paste_tsv`) as atomic batches. Settings gear: theme/accent/scroll-speed. Verified in-browser |
| W-003 | Editor UX pass: variable sizing, fluid scroll, resize, borders, sheet tabs, icon toolbar | Done | Variable column/row sizing + **fluid pixel scrolling** + **drag-to-resize** (P1C-004); **cell borders** draw + toggle (P1A-003c); **bottom sheet-tab bar** (switch + `+` add via `session_add_sheet`); chrome split into header + toolbar bars with **inline SVG icons**. Verified in-browser |
| TAURI-001 | Tauri desktop shell (native) | Not started | `docs/44`; consumes `casual-calc-sdk` |

## Phase rows — Editing (transaction layer)

Atomic, invertible operations; all model mutation flows through here
([24](24-TRANSACTION-AND-EDIT-SEMANTICS.md)).

| ID | Workstream | Status | Notes |
| --- | --- | --- | --- |
| E-001 | Cell-level op set + inverses + undo/redo (`casual-calc-transaction`) | Done | `Operation` (SetCell/SetValue/SetStyle/ClearCell/Batch); `apply` returns the inverse; atomic Batch with rollback; `History` undo/redo. Edit→recalc integration gated. 7 tests |
| E-002 | Structural ops: insert/delete rows & columns + reference rewriting | Not started | Needs formula reference algebra; the subtle correctness surface (doc 24) |
| E-003 | Selection model (`casual-calc-selection`) + fill/paste | Not started | — |

## Phase rows — Phase 2 (Formula & calc engine)

Evaluate the formula ASTs; recompute cached values
([40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md)).

| ID | Workstream | Status | Notes |
| --- | --- | --- | --- |
| P2-001 | Evaluator + full recalc (`casual-calc-eval`) | Done | Memoized recursive evaluation with circular detection; arithmetic/comparison/concat, unary, cell + range refs (same/cross-sheet), defined names; functions SUM/AVERAGE/MIN/MAX/COUNT/IF/ABS/ROUND; `recalculate(workbook)` full recalc; deterministic. Nothing depends on `-eval` except (future) host bridges. 7 tests |
| P2-002 | Incremental dependency graph + dirty propagation | In progress | First increment landed: `casual-calc-eval::recalculate_incremental` builds a per-pass precedent graph (cell + range edges; defined-name users conservatively always-dirty), BFS the changed cells' transitive dependents, and re-evaluates only those (Evaluator dirty-set mode reads clean cells from cache). `casual-calc-sdk::edit` routes value edits here, skips recalc for pure style/geometry edits, and keeps full recalc for structural (ref-shifting) edits and undo/redo. Correctness pinned by differential tests (incremental == full over chains/ranges + 40 pseudo-random edits). **Remaining:** persistent cross-edit graph + range-bucketed edges to hit the <50 ms / 1M-cell worst-case ([30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)) |
| P2-003 | Broader function library + oracle diff (LibreOffice Calc) | Not started | Function-by-function fidelity |
| P2-004 | Volatile functions, spill/dynamic arrays, iterative calc | Not started | — |

## Phase rows — Phase 1D (Grid render)

CPU raster backend: display list → pixels
([42](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md)).

| ID | Workstream | Status | Notes |
| --- | --- | --- | --- |
| P1D-001 | CPU raster: display list → PNG (`casual-calc-render`) | Done | `tiny-skia` pixmap: white ground, gridlines at visible boundaries, content-cell fills; twips→px transform; `render_png`/`render_pixmap`; deterministic; wasm-clean. Glyph text is P1D-002. 3 tests |
| P1D-002 | Glyph text via bundled font + `skrifa` | Not started | Needs a bundled font asset; turns Render ● for cell text |
| P1D-003 | Viewport virtualization on scroll + hit-testing (pixel→cell) | Not started | Incremental repaint; selection/editing input |

## Phase rows — Phase 1C (Grid layout)

Grid geometry, viewport virtualization, and the display list
([42](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md)).

| ID | Workstream | Status | Notes |
| --- | --- | --- | --- |
| P1C-001 | Offset index + viewport virtualization + display list | Done | `casual-calc-layout`: `Axis` cumulative offset index (offset/line_at, inverse-gated), `GridGeometry`, backend-neutral `DisplayList`/`PaintItem`, `layout_viewport`/`layout_full`; model `CellStore::row_band` for O(visible) scans. Invariant gated: viewport == full restricted to window. Reads cached values only (no calc). 8 tests |
| P1C-002 | Number-format-aware display text | Done | `numfmt` interpreter: General, fixed decimals, thousands grouping, percent, and date/time (serial→civil date). `display_text` applies the cell's style number format. 5 tests. Deferred: negative/zero/text sections, currency/literals, token-exact date layout |
| P1C-003 | In-cell text shaping (`parley`) + merged-cell/frozen-pane layout | Not started | Glyph runs; needed for visual fidelity |
| P1C-004 | Import column/row sizing → geometry | Done | `AxisSizing` (default + per-line twips) on `Sheet.columns`/`rows`; import parses `<cols>`, `<row @ht>`, `<sheetFormatPr>` defaults; export writes them back (coalesced spans); `GridGeometry::for_sheet` feeds layout/render; wasm `session_col_px`/`session_row_px` drive the editor's variable-width grid (cumulative offsets, hit-test, scroll). Round-trip gated by the semantic fixed point. Verified in-browser: wide/narrow columns + tall rows render. Editor adds **interactive drag-to-resize** (undoable `SetColumnWidth`/`SetRowHeight` ops, live preview, double-click-to-reset) and **fluid pixel scrolling** (absolute `scrollX/scrollY`, sub-cell offsets, clipped body/headers). Hidden/outline still pending |

## Phase rows — Phase 1B (Semantic writer)

Model → valid `.xlsx`; the semantic fixed point `import → write → import`
([36](36-EXPORT-AND-ROUNDTRIP-DESIGN.md)).

| ID | Workstream | Status | Notes |
| --- | --- | --- | --- |
| P1B-001 | Semantic writer (`casual-calc-export`) | Done | Deterministic model → .xlsx: values, formulas (from AST), number formats, merges, frozen panes, defined names. Semantic fixed-point gated (import→write→import equal); import pre-interns cellXfs in order for canonical style round-trip. 3 round-trip tests |
| P1B-002 | Byte-identical repackager (retention mode) | Not started | Needs P1A-006 retained source; whole-package byte floor |
| P1B-003 | Opens without repair in LibreOffice Calc | Not started | Differential validation (`tools/casual-calc-fidelity`) |

## Phase rows — Phase 1A (Semantic import & modeling)

Import SpreadsheetML → normalized model + compatibility report; formulas parsed
& preserved but **not evaluated** ([06](06-ROADMAP-AND-DELIVERY.md),
[34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)).

| ID | Workstream | Status | Notes |
| --- | --- | --- | --- |
| P1A-001 | Model string table + cell-value import (shared/inline strings, number, bool, error) | Done | `StringTable` in model; `casual-calc-import` maps values + dual-axis `CompatibilityReport`; A1 parsing; deterministic; 16 tests (10 model + 6 import) |
| P1A-002 | Formula import → AST (`casual-calc-formula` tokenizer/parser) | Done | Lexer + Pratt parser + serde AST + pretty-printer (round-trip gated); model gains a formula arena (model→formula dep); import parses `<f>` → AST, cached value kept; unparseable → Degraded. 26 tests (formula 9 + model 10 + import 7) |
| P1A-003 | Number formats + cell-style linkage import | Done | Model `Style`/`StyleTable` (interned, deduped); import parses styles.xml numFmts + cellXfs (custom + built-in numFmtId subset); cell `s` → StyleId; number format modeled. Font/fill/border deferred to P1A-003b. 18 tests (model 10 + import 8) |
| P1A-003b | Styles: font (bold/italic/color) + solid fill | Done | Model `Style` gains bold/italic/font_color/fill_color; import parses `styles.xml` fonts + fills + xf links; editor renders bold/color/fill; `session_set_style` edit. Borders + export of fonts/fills are follow-ups. Import test gates it |
| P1A-003c | Styles: cell borders | Done | `Style.border: Option<Borders>` (per-edge line-style token + optional color); import parses `<borders>` + `cellXfs/@borderId`; export writes an interned/deduped `<borders>` with `applyBorder` → **round-trips** (fixed-point test carries a box border). Editor draws borders (thickness/dash by token) + **All-borders toggle** (`Ctrl+Shift+7`, one undo step); border-only empty cells now render. PNG-render borders pending (display-list contract). Also: editor chrome split into header + toolbar bars |
| P1A-004 | Defined names, merged ranges, sheet views | Done | Model: `CellRange`, `SheetView` (frozen panes), `Sheet.merges`/`view`, `DefinedName` (parsed Expr, workbook/sheet scope). Import parses mergeCells, sheetView pane, and workbook definedNames (localSheetId resolved). 19 tests (model 10 + import 9) |
| P1A-005 | Proper part discovery via content-types + all rels (not conventional paths) | Not started | sharedStrings currently found by conventional path |
| P1A-006 | Retention mode + retained-source / opaque parts | Not started | — |

## Phase rows — Formats (open / text adapters)

Lightweight, dependency-free text formats in `casual-calc-io`; ODS and the
format registry follow.

| ID | Work | Status | Notes / evidence |
|----|------|--------|------------------|
| IO-001 | Delimited text (CSV / TSV / PSV) reader + writer | Done | `casual-calc-io`: RFC 4180-style `read_delimited`/`write_delimited` (quoting, CRLF, field typing number/bool/text); `parse → write → parse` fixed point + quoting/delimiter tests (5). SDK `from_workbook`; WASM `session_open_delimited`/`session_save_delimited`; editor **Open** routes `.csv/.tsv/.psv` by extension and a **Download-as menu** exports xlsx/csv/tsv/psv (numbers use General formatting — no binary-float tails). Verified in-browser (quoted comma field preserved on import; clean `43.48` on export). ODS + format registry pending |

## Phase rows — Feature pipeline (docs/48)

The dependency-ordered parity roadmap synthesized in
[48](48-FEATURE-PIPELINE.md). Earlier milestone work (data validation,
conditional formatting, cell comments, named ranges, rich clipboard, autofill,
sort) landed ahead of this tracker section; rows below are added as each item
is (re)started under the pipeline.

| ID | Workstream | Status | Notes |
| --- | --- | --- | --- |
| M1-4 | Split Clear into Contents / Formats / All; Delete key keeps formatting | Done | Clear Contents (keep style), Delete→clear-contents, and Clear All were already in place. Added the missing `session_clear_formats` (drops style via SetStyle{None}, keeps value+formula, recalc-skipped) + a "Clear formats" context-menu item between the two. Also closed the hide/unhide direct-mutation gap from M1-1 (now undoable). |
| M1-3 | Sheet rename rewrites cross-sheet formula refs + triggers recalc | Done | `casual-calc-formula::rename_sheet_references` walks the AST rewriting `Old!A1` → `New!A1` (case-insensitive, bare refs untouched). The transaction `RenameSheet` op applies it workbook-wide after the name swap; its existing inverse reverses the rewrite. sdk classifies RenameSheet as Full recalc. 6 tests (4 formula-rewrite + rename-rewrite-and-undo in transaction). Verified in-browser (rename keeps totals, no #REF). Delete already yields #REF via name resolution + full recalc; baking #REF into the AST on delete (to survive same-name recreation) is a deferred refinement given the name-based model. |
| M1-1 | Route all sheet & structural mutations through commit_edit (undoable + dirties doc) | Done | Keystone. Transaction crate gains InsertSheet/RemoveSheet/RenameSheet/MoveSheet/SetTabColor invertible ops (8 new tests, self-inverse + History undo/redo); sdk `recalc_plan` classifies them (sheet add/remove/rename → Full recalc for name resolution, move/tab-color → Skip). WASM sheet ops (add/rename/delete/move/duplicate), freeze, tab-color, merge/unmerge, and resize-all moved off direct `workbook_mut()` onto `session.edit` (freeze/merge/resize-all reuse SetSheetMetadata via a metadata-snapshot helper). editor.js `doUndo`/`doRedo` now `renderTabs()` (re-clamps the active sheet). Verified in-browser: add→undo removes the sheet, redo restores it. Single/range/clear col-row width were already routed. Hide/unhide `hidden_edit` was still mutating `workbook_mut()` directly — routed through SetSheetMetadata alongside M1-4. Data-feature ops (validation/CF/comments/names) undoability is a follow-up. |

## Review note

Keep this file readable. When it grows large, split closed phases into an
archive doc (e.g. `14a-TRACKER-ARCHIVE-PHASE-0.md`) and keep only active +
recent rows here — but never drop IDs from the record.
