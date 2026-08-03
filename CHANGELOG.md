# Changelog

All notable changes to OpenCalc are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/); semantic versioning applies once
a published crate line begins. Until then, everything lives under **Unreleased**,
grouped by date.

Each entry should cite the driving tracker ID (see
[docs/14-EXECUTION-TRACKER.md](docs/14-EXECUTION-TRACKER.md)) and, where relevant,
the design doc or ADR that motivated it.

## Unreleased

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
