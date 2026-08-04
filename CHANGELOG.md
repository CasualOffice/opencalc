# Changelog

All notable changes to OpenCalc are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/); semantic versioning applies once
a published crate line begins. Until then, everything lives under **Unreleased**,
grouped by date.

Each entry should cite the driving tracker ID (see
[docs/14-EXECUTION-TRACKER.md](docs/14-EXECUTION-TRACKER.md)) and, where relevant,
the design doc or ADR that motivated it.

## Unreleased

### 2026-08-05 — Editor product pass: full formatting, alignment, sheet management

A depth pass to bring the editor toward real-spreadsheet parity (not MVP stubs).

**Added**

- **Text formatting**, complete: bold, **italic**, **underline** (model gains
  `Style.underline` + `<u/>` import/export), plus a **font-color** swatch picker
  and the existing fill picker. `Ctrl+B/I/U` shortcuts.
- **Horizontal alignment** (left/center/right): new `Style.align` (`HAlign`),
  round-tripped through `cellXfs` `<alignment>`; toolbar buttons reflect and set
  it; the grid renders centered text; `Ctrl+Shift+L/E/R`.
- **Number formats** via a menu (Automatic / Number / 2-decimals / Thousands /
  Percent / Currency / Date) plus one-click **currency** and **percent**. The
  number-format engine now honors **literal runs** — currency symbols
  (`$#,##0.00` → `$1,234.50`), quoted text (`0" kg"`), escapes, and `[$SYM-…]`
  tokens — so currency shows its symbol instead of dropping it.
- **Borders menu**: All / Outer / Clear (position-aware outer edges), replacing
  the single all-or-nothing toggle.
- **Sheet management**: double-click a tab to **rename** inline; right-click for a
  context menu with **Rename / Duplicate / Delete** (`session_rename_sheet` /
  `session_duplicate_sheet` / `session_delete_sheet`; last sheet protected;
  duplicate deep-copies the grid).
- **Header selection**: click a row/column header to select the whole row/column;
  click the top-left corner (or `Ctrl+A`) to select the used range.
- **Auto-fit**: double-click a column boundary to size it to its widest cell.
- The toolbar reflects the **active cell's** formatting state
  (`session_cell_format`), like a real spreadsheet.

### 2026-08-05 — Delimited text: CSV / TSV / PSV (IO-001)

**Added**

- `casual-calc-io` delimited-text adapter: `read_delimited` / `write_delimited`
  over the normalized `Workbook` — RFC 4180 quoting, `\n`/`\r\n` line endings, and
  field typing (finite number → `Number`, `TRUE`/`FALSE` → `Bool`, else interned
  text). A `parse → write → parse` round-trip is a model fixed point (5 tests
  covering typing, quoting, and the comma/tab/pipe delimiters).
- `WorkbookSession::from_workbook`; WASM `session_open_delimited` /
  `session_save_delimited`. The editor's **Open** now accepts `.csv`, `.tsv`,
  `.psv` and routes by extension, and a **Download-as menu** on the toolbar's
  download button exports the active sheet as Excel / CSV / TSV / PSV (numbers use
  General formatting, so exported values read `43.48`, not `43.480000000000004`).
  Verified in-browser: a CSV imports with numbers and booleans typed and
  right-aligned, a quoted `"Wes,t"` field keeps its comma as one cell, and export
  round-trips cleanly.

### 2026-08-05 — Editor: sheet tab bar + SVG icon toolbar

**Added**

- **Sheet tab bar** along the bottom of the editor: one tab per workbook sheet
  (from `session_sheet_names`), click to switch (viewport + selection reset to the
  top-left), the active tab highlighted, and a **`+` to append a blank sheet**
  (`session_add_sheet`). A multi-sheet `.xlsx` is now fully navigable instead of
  showing only the first sheet.

**Changed**

- The editor toolbar uses **inline SVG icons** (new / open / save / undo / redo /
  bold / borders, settings gear, no-fill) instead of text labels — matching how
  real spreadsheet tools present their toolbar.

### 2026-08-05 — Cell borders + editor toolbar (P1A-003c)

**Added**

- **Cell borders**, end to end. `Style` gains `border: Option<Borders>` (per-edge
  `left/right/top/bottom`, each an OOXML line-style token plus optional `RRGGBB`
  color). Import parses the `<borders>` collection and `cellXfs/@borderId`; export
  writes an interned, deduplicated `<borders>` collection with `applyBorder` — so
  borders **round-trip** through the semantic fixed point (the test now carries a
  cell with a blue-thin/medium box border). The editor draws borders (thickness by
  token, dashed/dotted styles) and adds an **All-borders toggle** (toolbar button ▦
  and `Ctrl+Shift+7`), applied as one undoable edit.
- Borders on **empty cells** now render — `session_cells` no longer skips a cell
  that has a border but no text or fill.

**Changed**

- **Editor chrome split into two bars.** A slim app header (brand + settings +
  status) sits above a dedicated **toolbar** grouping document actions
  (New/Open/Save), history (Undo/Redo), and formatting (bold, borders, fills) —
  instead of cramming everything into one header row.
- `session_set_style` (bold/fill) now preserves the cell's other formatting
  (number format, italic, font color, borders) instead of overwriting it.

### 2026-08-05 — Editor: fluid scrolling + drag-to-resize (P1C-004)

**Added**

- **Interactive column/row resize.** Two new undoable operations,
  `Operation::SetColumnWidth` and `SetRowHeight` (each with `None` = revert to
  default), with inverse-preserving `apply`. WASM exposes
  `session_set_col_width`/`session_set_row_height` (+ `_clear_` variants) plus
  `session_col_offset_px`/`session_row_offset_px`/`session_col_at_px`/
  `session_row_at_px`. The editor arms a `col-resize`/`row-resize` cursor when the
  pointer nears a header boundary, previews the new size live under the cursor,
  commits one undoable edit on release, and resets to default on double-click.

**Changed**

- **Fluid pixel scrolling.** The editor now scrolls by an absolute content pixel
  offset (`scrollX/scrollY`) instead of snapping a whole row/column per wheel
  step, so the grid glides smoothly and the first visible line can be partially
  clipped. Grid body and header labels are clipped so partial first cells never
  bleed into the header strips.

### 2026-08-05 — Column widths & row heights (P1C-004)

**Added**

- `AxisSizing` on `Sheet` (`columns`/`rows`): an optional default plus per-line
  size overrides, in twips. The importer reads `<cols>`, `<row @ht>`, and
  `<sheetFormatPr>` defaults; the exporter writes them back (coalescing equal
  consecutive columns into one `<col>` span, emitting `ht` on custom rows). The
  conversions are exact inverses, so sizing survives the **semantic fixed point**
  (the round-trip test now carries a wide column, a narrow column, and a tall
  row).
- `GridGeometry::for_sheet` builds the layout offset index from a sheet's sizing;
  SDK/WASM rendering now uses it, so PNG output honors real widths/heights.
- WASM `session_col_px` / `session_row_px` expose per-line pixel sizes; the
  canvas editor now draws **variable column widths and row heights** —
  cumulative-offset gridlines, headers, hit-testing, selection spans, inline-edit
  placement, and proportional wheel scrolling all follow the real geometry.
  Verified in-browser against a crafted `.xlsx` (40-char column, 4-char column,
  48-point row). Fidelity ledger: sizing Model/Round-trip/Render now `●`.

### 2026-08-05 — Style round-trip: export fonts & fills

**Added**

- `casual-calc-export` now emits `<fonts>`, `<fills>`, and the full `cellXfs`
  (fontId/fillId/numFmtId) — deduplicating fonts, solid fills, and number-format
  codes — so **bold/italic/font-color/fill round-trip** through save→reopen. The
  semantic fixed-point test now includes a bold + red-font + yellow-fill cell.
  The fidelity ledger's Round-trip column for styling is now `●`.

### 2026-08-05 — Editor: toolbar, shortcuts, range selection

**Added**

- Editor **range selection** (click-drag and shift-click / shift-arrows), a
  **formatting toolbar** (Bold toggle + fill-color swatches), **keyboard
  shortcuts** (Ctrl/Cmd+B bold, Z undo, Y/Shift+Z redo, S save, C/V copy-paste,
  Delete clear), and **copy/paste as TSV**.
- `casual-calc-wasm` range operations as atomic batches (one undo step):
  `session_toggle_bold`, `session_set_fill`, `session_clear_range`,
  `session_range_bold`, `session_copy_tsv`, `session_paste_tsv`.

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

- Cell styling (P1A-003b): `casual-calc-model` `Style` gains `bold`, `italic`,
  `font_color`, and `fill_color`; `casual-calc-import` parses `styles.xml` fonts
  and solid fills and links them through `cellXfs`; the editor renders bold/
  italic, font color, and cell fill; a `session_set_style` WASM edit applies
  bold + fill (used to style the demo's header row). Borders and export of
  fonts/fills are follow-ups. The editor's default scroll speed is 0.80.

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
