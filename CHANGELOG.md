# Changelog

All notable changes to OpenCalc are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/); semantic versioning applies once
a published crate line begins. Until then, everything lives under **Unreleased**,
grouped by date.

Each entry should cite the driving tracker ID (see
[docs/14-EXECUTION-TRACKER.md](docs/14-EXECUTION-TRACKER.md)) and, where relevant,
the design doc or ADR that motivated it.

## Unreleased

### 2026-08-05 — Interactive editor & fidelity fixes

**Added**

- Interactive **canvas grid editor** (`webapp/editor.html`, W-002): a real
  spreadsheet editor on the demo page — row/column headers, click-to-select,
  keyboard navigation, inline + formula-bar editing, `New`/`Open`/`Save`/`Undo`/
  `Redo`, and wheel virtualization. The WASM engine owns the workbook and supplies
  layout + display text; the browser canvas draws the grid and text (crisp,
  font-free). A settings gear (top-left) tunes theme (auto/light/dark), accent
  color, and scroll speed (default 0.40), persisted to `localStorage`.
- `casual-calc-wasm`: a session-based editor API (`session_open`/`session_new`,
  `session_cells`, `session_cell_input`, `session_set_cell`, `session_undo`/`redo`,
  `session_save`) over a thread-local `WorkbookSession`.

**Fixed**

- `casual-calc-layout` number display: `General` format now rounds to 15
  significant digits (Excel precision), so a `SUM` of `13.5 + 10 + 19.98` shows
  `43.48` instead of the floating-point tail `43.480000000000004`.

### 2026-08-05 — Host facade & WebAssembly demo

**Added**

- `casual-calc-sdk` (SDK-001): the host-facing engine facade. `WorkbookSession`
  composes the whole pipeline into one surface — `open`/`blank`, `edit` (apply an
  op then recalc, with undo/redo history), `recalculate`, `layout`, `render_png`,
  `save`, and `compatibility_report` — and re-exports the vocabulary a host needs
  so embedders depend on one crate. This is what the Tauri desktop shell and
  headless services embed. Full open→edit→recalc→save→reopen→render lifecycle
  gated. 4 tests.

- `casual-calc-wasm` (W-001): the `wasm-bindgen` bridge — a thin transport over
  the host-agnostic engine. `version`, `eval_formula` (parse + evaluate a
  self-contained formula), `render_xlsx` (import → recalc → layout viewport →
  PNG), and `describe_xlsx`. The whole read→calc→render pipeline runs in the
  browser; verified end-to-end (`=SUM(1,2,3)*IF(2>1,10,0)` → 60, sample `.xlsx`
  renders to pixels). `wasm-opt` is disabled (bulk-memory compatibility).
- Marketing site + demo (SITE-001): `webapp/` — a landing page, a live formula
  playground, and an open-`.xlsx`→render demo. `.github/workflows/pages.yml`
  builds the engine with `wasm-pack` and deploys the site to GitHub Pages.

### 2026-08-04 — Editing: the transaction layer

**Added**

- `casual-calc-transaction` (E-001): the atomic, invertible edit operation set.
  `Operation` (`SetCell`, `SetValue`, `SetStyle`, `ClearCell`, `Batch`); `apply`
  returns the **inverse** operation, so undo/redo is inverse replay; `Batch` is
  all-or-nothing with rollback; a `History` provides undo/redo stacks. An
  edit→recalc integration is gated (editing a precedent recomputes dependents).
  This is the fifth and final fidelity dimension — the ledger's Edit column is
  now `●` for cell content. Structural ops (row/column insert-delete with
  reference rewriting) are the next increment. 7 tests.

### 2026-08-04 — Phase 2: the calculation engine

**Added**

- `casual-calc-eval` (P2-001): the calc engine. Evaluates the formula ASTs the
  model stores — memoized recursive evaluation with circular-reference detection
  — supporting arithmetic/comparison/concat/unary operators, cell and range
  references (same- and cross-sheet), defined names, and a starter function
  library (`SUM`, `AVERAGE`, `MIN`, `MAX`, `COUNT`, `IF`, `ABS`, `ROUND`).
  `recalculate(workbook)` recomputes every formula cell's cached value (a correct
  full recalc); deterministic. Nothing depends on `-eval` except the (future)
  host bridges, so the model/layout/render layers still build without it. The
  fidelity ledger's Calc column is now `●` for formulas. 7 tests. Incremental
  dependency graph + <50 ms budget and a broader oracle-diffed function library
  are later increments.

### 2026-08-04 — Phase 1D: grid render (pixels)

**Added**

- `casual-calc-render` (P1D-001): the CPU raster backend. `render_png` /
  `render_pixmap` execute a viewport's `DisplayList` onto a `tiny-skia` pixmap —
  white ground, light gridlines at the visible row/column boundaries, and a
  subtle fill per content cell — with a twips→pixel transform at a given DPI.
  Deterministic; compiles to `wasm32-unknown-unknown`. Glyph text (bundled font +
  `skrifa`) is the next increment; cells are shown as highlighted rectangles for
  now. 3 tests.

### 2026-08-04 — Phase 1C: grid layout & virtualization

**Added**

- `casual-calc-layout` (P1C-002): number-format-aware display text. A `numfmt`
  interpreter renders a cached numeric value + format code to its display string
  — `General`, fixed decimals (`0.00`), thousands grouping (`#,##0`), percent
  (`0%`), and date/time formats (Excel serial → civil `YYYY-MM-DD` / `HH:MM:SS`).
  `display_text` applies the cell's style number format. Deferred: negative/zero/
  text sections, currency/literal runs, and token-exact date layout.

- `casual-calc-layout` (P1C-001): the virtualization core. An `Axis` cumulative
  offset index maps line indices ↔ twip positions (`offset`/`line_at`, gated as
  inverses); `GridGeometry` holds the column/row axes; a serializable,
  backend-neutral `DisplayList`/`PaintItem` is the render contract; and
  `layout_viewport`/`layout_full` lay out the grid. The **virtualization
  invariant** — a viewport's output equals the full layout restricted to that
  window — is gated. Layout reads only the model's cached values (no calc
  engine). Added `CellStore::row_band` for O(visible) row-band scans. The
  fidelity ledger's Render column is now `~` for laid-out cell values (glyph
  shaping + oracle come in Phase 1D).

### 2026-08-04 — Phase 1B: the semantic writer

**Added**

- `casual-calc-export` (P1B-001): the semantic SpreadsheetML writer.
  `write_workbook` serializes a `Workbook` to a deterministic `.xlsx` — cell
  values, formulas (from the AST via the pretty-printer), number formats
  (`cellXfs` + custom `numFmts`), merged ranges, frozen panes, and defined names.
  The **semantic fixed point** `import → write → import` yields an equal model
  (gated). `casual-calc-import` now pre-interns `cellXfs` in order so the style
  table is canonical, letting styles round-trip deterministically. The fidelity
  ledger's Round-trip column is now `●` (semantic) for the covered constructs.

### 2026-08-04 — Phase 1A: semantic import begins

**Added**

- `casual-calc-model` + `casual-calc-import` (P1A-004): defined names, merged
  ranges, and frozen panes. The model gained `CellRange`, `SheetView` (frozen
  rows/cols) with `Sheet.merges`/`view`, and `DefinedName` (a parsed `Expr` with
  workbook/sheet scope). Import parses worksheet `mergeCells` and the
  `sheetView` `pane`, and workbook `definedNames` (resolving `localSheetId` to the
  assigned sheet id; unparseable refers-to is `Degraded`).
- `casual-calc-model` + `casual-calc-import` (P1A-003): number formats. The model
  gained an interned, deduplicated `Style`/`StyleTable` (`Workbook.styles`,
  `intern_style`; `validate()` checks style references). Import parses
  `styles.xml` — custom `numFmts` and `cellXfs` — resolves each `xf`'s number
  format (custom code or a built-in `numFmtId` from the ECMA-376 subset), and maps
  a cell's `s` index to a `StyleId`. Font/fill/border are deferred to P1A-003b.
- `casual-calc-formula` (P1A-002): the formula language — tokenizer,
  precedence-climbing parser, a serializable `Expr` AST, A1 reference algebra
  (`$` anchors, sheet qualification, ranges), and a canonical pretty-printer with
  a gated `parse(print(e)) == e` round-trip. Subset per docs/40; no evaluation
  (that is Phase 2).
- `casual-calc-model`: a formula AST **arena** (`Workbook.formulas`,
  `store_formula`/`formula`); `Cell.formula` now resolves to a real AST, and
  `validate()` checks formula handles. Model now depends on `casual-calc-formula`
  for the `Expr` type (the only upward edge; no cycle).
- `casual-calc-import`: worksheet `<f>` formulas are parsed into the arena with
  the cached value preserved and reported `Mapped`; a formula that fails to parse
  is `Degraded` (cached value kept). Added a living **Fidelity Ledger**
  (`docs/33`) tracking each construct across model/round-trip/edit/render/calc.

**Existing (P1A-001)**

- `casual-calc-model`: a `StringTable` — deterministic, deduplicated string
  interning; `Workbook` gains `strings` and `intern_string`, and `validate()`
  now checks that cell string references resolve. Empty-workbook snapshots stay
  byte-identical.
- `casual-calc-import` (P1A-001): SpreadsheetML → model for **cell values** —
  numbers, booleans, shared strings, inline strings, and error values — plus A1
  reference parsing and a dual-axis `CompatibilityReport` (Mapped/Degraded/
  Omitted × Preserved/NotRetained/NotApplicable). Formula cells keep their cached
  value and are recorded as `Omitted` pending the AST (a later increment).
  Import is deterministic (fixed workbook id, sequential sheet ids, insertion-
  ordered interning). 6 tests over in-memory `.xlsx`.

### 2026-08-04 — Phase 0 foundation begins

**Added**

- Fixture corpus (F-006): a deterministic generator (`fixtures/tools/generate.py`),
  a committed `generated/minimal.xlsx` with a SHA-256 `manifest.json`, a test that
  the fixture parses through the real reader, and a CI `repository-policy` job
  (checksum verification + merge-conflict-marker rejection).
- Fuzz workspace (F-008): a separate cargo workspace with a `bounded_package`
  target (admission never panics on arbitrary bytes — 200k runs clean) and a CI
  `fuzz-build` job on pinned nightly that asserts `fuzz/Cargo.lock` is unchanged.

**Fixed**

- `docs` CI gate: the benchmark tool's usage line (`--env <label>`) tripped
  rustdoc's "unclosed HTML tag" under `-D warnings`; wrapped it in a code fence.

- `casual-calc-benchmark` (F-007): reproducible micro-benchmark harness emitting
  versioned JSON (median/p95 ns, output checksum + determinism flag, per-case
  regression tolerance), a `--smoke` mode, a committed `dev-reference` baseline,
  and a CI `benchmark-smoke` job that validates the report shape with `jq`.
- `casual-calc-ooxml` (F-011): SpreadsheetML package discovery — opens a
  `.xlsx`, follows the OPC graph (root rels → workbook part → `<sheets>` →
  workbook rels) to resolve the workbook part and each worksheet's part, under
  per-part XML element/depth limits. Reaches the Phase 0 "opens a trivial .xlsx"
  goal. Depends on `casual-calc-package` + `quick-xml`; 7 tests.
- `deny.toml`: `allow-wildcard-paths = true` so intra-workspace path deps aren't
  flagged as wildcard versions.
- `casual-calc-model` (F-010): the normalized workbook shell — non-zero hex
  `Id` + `IdGenerator` + typed id newtypes, `CellValue`/`ErrorValue`, a compact
  `Cell` carrying the **reserved calc seams** (`formula` handle, cached `value`,
  `CellFlags` spill bits), the sparse ordered `CellStore` (blank cells cost
  nothing), `Sheet`, and `Workbook` with deterministic, byte-stable JSON
  snapshot I/O (`deny_unknown_fields` + `skip_serializing_if`). The empty-workbook
  byte-stable round-trip (a Phase 0 exit-gate condition) is gated by a test.
  8 tests.
- `casual-calc-package` (F-009): bounded ZIP/OPC package admission — the
  format-neutral substrate for `.xlsx` and `.ods`. `Package::open` enforces
  input-size, entry-count, expansion-ratio, total-expansion, and path-safety
  limits (`PackageLimits`); `read_part` decompresses on demand under a size cap.
  Hostile inputs (zip bomb, path traversal, oversized, too-many-entries) are
  rejected cleanly with stable `OC-PKG-*` codes and covered by 10 tests. Compiles
  to `wasm32-unknown-unknown` via pure-Rust deflate. Clarified that CSV/TSV/PSV
  are delimited-text adapters that do not pass through the package layer.

- Cargo workspace skeleton (F-001): 15 library crates (`casual-calc-*`) and 2
  tool crates, with workspace-inherited manifests. `cargo check --workspace` is
  green; `unsafe_code` is forbidden workspace-wide.
- Toolchain and policy (F-002/003): `rust-toolchain.toml` (channel 1.96.0,
  wasm32 target), workspace lints + release profile, and `deny.toml`
  supply-chain policy (`cargo deny check` passes).
- CI workflow (F-004/005): `format`, `lint`, `test`, `docs`, `wasm`,
  `dependency-policy`, and a `platform` matrix (macOS/Windows + a 1.88.0 MSRV
  check); README CI badge. Benchmark/fuzz/repo-policy/browser jobs are added with
  their harnesses.

### 2026-08-04 — Documentation foundation

**Added**

- Repository governance and agent contract: `README.md`, `AGENTS.md`, `CLAUDE.md`,
  `SKILLS.md`, `CONTRIBUTING.md`.
- The `docs/` design record: master index (`00`), requirements (`01`),
  architecture (`02`), roadmap and phased delivery (`06`), quality/security/
  compatibility (`07`), ADR register (`08`), design-first process (`11`),
  competitive analysis (`12`), execution tracker (`14`), CI and release gates
  (`15`), documentation maintenance (`16`), glossary (`17`), support matrix
  (`18`), workspace/layer-division scaffold (`19`), error-code registry (`20`),
  parser limits (`21`), normalized workbook schema (`22`), XLSX package reader
  (`28`), performance and capacity targets (`30`), SpreadsheetML fidelity and
  preservation architecture (`34`), formula and calculation engine architecture
  (`40`), and grid layout/virtualization/rendering architecture (`42`).

- Tauri desktop shell design note (`44`) — the native desktop host that drives
  the engine as native Rust (calc runs native, not WASM), the host capability
  trait, and the command surface.
- Repository scaffolding: `LICENSE` (Apache-2.0), `SECURITY.md`, `GOVERNANCE.md`,
  `CODE_OF_CONDUCT.md`, and `.github/` PR + issue templates.
- Cell-store representation (`23`) — the sparse row-blocked tile design, per-cell
  byte budget, and structural-edit behavior behind the 1M-cell / 60 fps targets.
- Transaction & edit semantics (`24`) — the closed operation set, atomic
  inverses, reference rewriting on structural edits, dirty-set emission, and the
  collaboration seam.
- Export & round-trip design (`36`) — the byte-identical repackager and the
  deterministic semantic writer, and the Phase 1B round-trip fixed-point gate.
- Phase 0 plan & scaffold specs (`29`) — the ordered `F-###` work items and the
  ready-to-instantiate build config (workspace `Cargo.toml`, `rust-toolchain.toml`,
  `deny.toml`, CI workflow, fixtures/benchmark layout).
- Phase D exit report (`31`) — the documentation phase closed against its
  roadmap exit gate (**passed** 2026-08-04).

**Changed**

- Consistency audit across the whole doc set: aligned the architecture-pillar
  index, added pending ADRs (dual-host capability trait, edit/op-schema),
  corrected an ADR attribution, standardized spill-flag and product-name
  terminology, and marked the MSRV provisional (pinned at Phase 0).

**Notes**

- OpenCalc is in the **documentation phase**. No engine code exists yet. The
  design record commits to the full architecture — including the layer division
  and the virtualization strategy — up front, so later phases (notably the
  Phase 2 calc engine) slot in without a do-over.
