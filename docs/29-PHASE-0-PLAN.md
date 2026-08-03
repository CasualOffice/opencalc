# 29 — Phase 0 Plan & Scaffold Specs

The concrete, ordered plan for **Phase 0 — Foundation**, plus the exact build
scaffolding (workspace manifest, toolchain, deny policy, CI) written out so
instantiation is mechanical. Phase 0 is the first phase that creates build files;
it writes **no engine logic** beyond the bounded reader and a minimal model
shell. Every step is a tracked `F-###` row ([14](14-EXECUTION-TRACKER.md)) and is
gated by [15](15-CI-AND-RELEASE-GATES.md).

Exit gate (from [06](06-ROADMAP-AND-DELIVERY.md)): CI green on all platforms; a
hostile fixture is rejected within limits; the model round-trips an empty
workbook snapshot byte-stably.

## Ordered work items

| ID | Item | Depends on | Exit signal |
| --- | --- | --- | --- |
| F-001 | Workspace skeleton: root `Cargo.toml` + empty crate dirs per [19](19-WORKSPACE-SCAFFOLD-DESIGN.md) | — | `cargo check --workspace` builds |
| F-002 | `rust-toolchain.toml`, workspace lints, release profile | F-001 | pinned toolchain resolves |
| F-003 | `deny.toml` supply-chain policy | F-001 | `cargo deny check` passes |
| F-004 | CI workflow (all gate jobs from doc 15) | F-001..003 | CI green on macOS/Linux/Windows/WASM |
| F-005 | `.github` PR/issue templates already present; wire CI badges | F-004 | badges live |
| F-006 | Fixture corpus scaffold + `manifest.json` (SHA-256) + a synthetic generator | F-001 | `repository-policy` job validates checksums |
| F-007 | Benchmark harness (`tools/casual-calc-benchmark`) + one committed baseline | F-001 | `benchmark-smoke` job validates JSON via `jq` |
| F-008 | Fuzz workspace (`fuzz/`, pinned nightly) with a bounded-package target | F-001 | `fuzz-build` job compiles targets |
| F-009 | `casual-calc-package`: bounded OPC/ZIP admission + limits ([21](21-PARSER-LIMITS.md)) | F-001 | a zip-bomb / traversal fixture is rejected cleanly |
| F-010 | `casual-calc-model` shell: IDs, envelope, empty `Workbook`, deterministic snapshot I/O + reserved seams (fields only) | F-001 | empty workbook snapshot round-trips byte-stably |
| F-011 | Minimal `casual-calc-ooxml`: open a package, resolve content-types + rels, discover the workbook part | F-009 | opens a trivial `.xlsx`, lists sheet parts |

F-009..011 are the only items with real logic, and each is small and bounded. The
remaining crates from [19](19-WORKSPACE-SCAFFOLD-DESIGN.md) are created as empty
skeletons in F-001 and filled in later phases.

## Scaffold specs (ready to instantiate)

> These are the intended contents. Values marked **(ADR-pending)** are finalized
> by the MSRV/toolchain ADR ([08](08-ADR-REGISTER.md)); the numbers below are the
> proposed defaults, matching OpenDoc's proven policy.

### Root `Cargo.toml`

```toml
[workspace]
resolver = "3"
members = [
    "crates/casual-calc-model",
    "crates/casual-calc-formula",
    "crates/casual-calc-package",
    "crates/casual-calc-ooxml",
    "crates/casual-calc-import",
    "crates/casual-calc-export",
    "crates/casual-calc-ods",
    "crates/casual-calc-io",
    "crates/casual-calc-transaction",
    "crates/casual-calc-selection",
    "crates/casual-calc-eval",
    "crates/casual-calc-layout",
    "crates/casual-calc-render",
    "crates/casual-calc-sdk",
    "crates/casual-calc-wasm",
    "tools/casual-calc-benchmark",
    "tools/casual-calc-fidelity",
]

[workspace.package]
edition = "2024"
license = "Apache-2.0"
repository = "https://github.com/CasualOffice/opencalc"
rust-version = "1.88.0"          # MSRV (ADR-pending)

[workspace.lints.rust]
unsafe_code = "forbid"
missing_debug_implementations = "warn"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }   # priority -1 so the deny lints below win
dbg_macro = "deny"
todo = "deny"
unimplemented = "deny"

[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"
```

### `rust-toolchain.toml`

```toml
[toolchain]
channel = "1.96.0"              # dev toolchain (ADR-pending)
components = ["clippy", "rustfmt"]
targets = ["wasm32-unknown-unknown"]
profile = "minimal"
```

### `deny.toml`

```toml
[advisories]
yanked = "deny"

[bans]
multiple-versions = "warn"
wildcards = "deny"
allow-wildcard-paths = true   # intra-workspace path deps are not real wildcards

[licenses]
allow = [
    "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "MIT",
    "NCSA", "Unicode-3.0", "Unlicense", "Zlib",
]

[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

### `.github/workflows/ci.yml` (job skeleton)

Job **names** are the contract in [15](15-CI-AND-RELEASE-GATES.md); this is the
shape (steps abbreviated):

```yaml
name: ci
on:
  push: { branches: [main] }
  pull_request:
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true
permissions:
  contents: read
jobs:
  format:            # cargo fmt --all -- --check
  lint:              # cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
  test:              # cargo test --workspace --all-features --locked
  docs:              # RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
  wasm:              # cargo check --target wasm32-unknown-unknown
  benchmark-smoke:   # casual-calc-benchmark --smoke | jq validate
  fuzz-build:        # build cargo-fuzz targets (pinned nightly), assert fuzz/Cargo.lock unchanged
  dependency-policy: # cargo deny check bans licenses sources + cargo audit --deny warnings
  repository-policy: # fixture manifest sha256; reject merge-conflict markers
  platform:          # matrix: macOS-arm64, Windows-x64, + MSRV check
```

(`browser-smoke` is added in Phase 1E when the WASM editor exists.)

### `fixtures/manifest.json` (shape)

```json
{
  "schemaVersion": 1,
  "fixtures": [
    { "path": "generated/empty.xlsx", "sha256": "…", "kind": "generated" },
    { "path": "generated/zip-bomb.xlsx", "sha256": "…", "kind": "hostile" }
  ]
}
```

### Benchmark report (shape)

```json
{
  "schemaVersion": 1,
  "environment": "mac-m-series-baseline",
  "cases": [
    { "id": "open-1m-cells", "medianNs": 0, "p95Ns": 0, "outputChecksum": "…",
      "maxRegressionBasisPoints": 500 }
  ]
}
```

## Guardrails

- Instantiating these files is the **first Phase 0 commit** and requires the
  "no engine code yet" hold to be lifted. Until then this doc is the blueprint.
- No item is `Done` until its exit signal (a green CI job or a passing fixture)
  is real — not merely because the file exists ([16](16-DOCUMENTATION-MAINTENANCE.md)).
- The MSRV/toolchain values become fixed only when their ADR is Accepted; treat
  the numbers here as proposed defaults until then.
