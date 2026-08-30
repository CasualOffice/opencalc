# 15 — CI & Release Gates

CI is part of the architecture, not an afterthought. The job names below are a
**contract**: they are stable, and a PR is not mergeable until they pass. This
doc defines the gates for when the workspace exists (Phase 0 onward) and records
which gates are not yet built.

> **Current state:** fifteen jobs run on every push — `format`, `lint`, `test`,
> `docs`, `wasm`, `benchmark-smoke`, `sdk-types`, `browser-smoke`,
> `oracle-diff`, `repository-policy`, `fuzz-build`, `docker-build`,
> `dependency-policy`, `desktop`, and a three-platform `platform` matrix

**Four of these run on main and not on every pull request** (`CI-030`):
`benchmark-smoke`, `fuzz-build` and `docker-build` are skipped on a pull
request, and `desktop` drops to Linux alone. The reason is arithmetic rather
than taste — the organisation's entire allowance is **20 concurrent jobs shared
across 17 repositories**, and one run of this workflow was taking 19 of them, so
a pull request here stalled every other repository for ten minutes.

**What that trades is real and is stated here rather than left to be
discovered.** A pull request can now be green where main is not: a macOS-only
bundling break, an image that stops building, or a fuzz target that stops
compiling will be caught on main, an hour later, instead of in review. That is
the same window `CI-031` describes for merge results, widened deliberately. The
jobs were chosen because a failure in any of them is a *build* failure somebody
fixes forward, not a correctness failure that should never have merged —
`oracle-diff`, `browser-smoke`, `test` and the three-platform `platform` matrix
all still run on every pull request, and they are the ones that answer whether
the change is right.

> including MSRV. Two more run on a schedule in `security.yml`: `advisories`
> and `fuzz-campaign`.
>
> The list is a contract in **both** directions — every job below exists, and
> every job CI runs is below. `tools/check-doc-claims.py` checks both, because
> the first three jobs to be added here (`sdk-types`, `docker-build`,
> `desktop`) were added to the workflow and not to this table, and a contract
> that silently gains clauses is not one.

## PR gates

| Job | Command (intent) | Enforces |
| --- | --- | --- |
| `format` | `cargo fmt --all -- --check` | Formatting |
| `lint` | `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Lints as errors |
| `test` | `cargo test --workspace --all-features --locked` (+ doc tests) | Correctness, determinism, round-trip, recalc goldens |
| `docs` | `cargo doc --workspace --all-features --no-deps` with `RUSTDOCFLAGS=-D warnings` | Doc builds; intra-doc links valid |
| `wasm` | `cargo check --target wasm32-unknown-unknown --workspace --exclude casual-calc-collab-server` | The **engine** stays WASM-clean. The collaboration server is a host (ADR-012) with an async runtime and an HTTP stack, and was never meant to run in a browser |
| `benchmark-smoke` | run `casual-calc-benchmark --smoke`, validate JSON shape with `jq`. **Main only** (`CI-030`) | Perf harness shape + determinism (**implemented**, F-007) |
| `fuzz-build` | build cargo-fuzz targets on pinned nightly; assert `fuzz/Cargo.lock` unchanged. **Main only** (`CI-030`) | Fuzz targets compile (**implemented**, F-008) |
| `platform` | matrix: macOS-arm64 + Windows-x64 full tests; **MSRV** check | Cross-platform + minimum Rust |
| `sdk-types` | `npm ci && npm test` in `sdk/types` | The published `.d.ts` surface compiles under `strict` **and** names only methods that exist — a declaration nothing compiles against drifts (`SDK-009`) |
| `docker-build` | build every image in the repository. **Main only** (`CI-030`) | A Dockerfile that stops building fails here rather than at an integrator (`DEP-07`) |
| `desktop` | `tauri build`, one bundle format each: **all three platforms on main, Linux alone on a pull request** (`CI-030`) | The desktop shell's own Cargo workspace still builds — the gate `CI-014` had to add for `fuzz/` after a signature change broke it silently, present from the day the workspace existed (`ADR-023`) |
| `dependency-policy` | `cargo deny check bans licenses sources` + `cargo audit --deny warnings` | Supply-chain policy |
| `repository-policy` | fixture manifest SHA-256 check; reject merge-conflict markers | Repo integrity (**implemented**, F-006) |
| `oracle-diff` | recalculate a corpus in LibreOffice Calc and diff the values; re-save a feature workbook through it and diff the structure | The evaluator and the **writer**, against an implementation that shares neither our code nor our reading of the spec (**implemented**, P2-003 + P1B-003) |
| `browser-smoke` | `wasm-pack` build + Playwright against `webapp/editor.html` | The editor loads, paints, calculates, edits, undoes — and copies, fills, inserts, formats, references another sheet and saves (**implemented**, CI-002 + CI-004) |

Actions are pinned to full commit SHAs; workflows run read-only where possible;
concurrency-cancel is on.

### What `browser-smoke` is for

It is the only gate that runs the editor the way a person does, and it exists
because everything else here proves the *engine* is right. None of the Rust jobs
would notice the WebAssembly glue failing to instantiate, a canvas that never
paints, or bindings and engine that disagree about a signature — a stale
`webapp/pkg/` once shipped 25 of 222 exports with every gate green. AGENTS.md
requires verifying in a browser before calling anything done; this is that,
automatically, on every push.

It **builds the engine itself** rather than taking `webapp/pkg/` from the tree
(which is not committed anyway): a gate that would accept a stale build cannot
catch a stale build. It asserts through real user surfaces only — the name box,
the formula bar, and the accessibility mirror the editor maintains of the
visible cells — so there is no test-only hook to keep in step, and a failure in
the mirror is a failure a screen-reader user would have hit.

It runs with **no retries**, deliberately: the gate is here to catch a real
breakage, and a retry turns an intermittent one — the kind most worth knowing
about — into a pass.

Two suites run under it: `editor.smoke.spec.mjs` asks whether the editor works
at all, and `editor.editing.spec.mjs` (CI-004) asks whether the things people do
with a spreadsheet all day work — copy and paste with reference adjustment,
fill, insert and delete lines, formatting, a second sheet, keyboard navigation,
and saving a genuine `.xlsx`.

Between them their first runs found **five defects, all shipped, none visible to
any Rust test**: UX-B04, UX-B05, UX-B06, FID-05 and UX-NV5 in
[14](14-EXECUTION-TRACKER.md). That ratio is the argument for the gate.

## Determinism & fidelity gates (spreadsheet-specific)

These are the gates that make OpenCalc trustworthy and are additions over
OpenDoc's set:

- **Round-trip fixed point** — `import(retention) → write → reopen` yields an
  identical model; an unedited `.xlsx` reconstructs byte-for-byte
  ([34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)).
- **Golden snapshots** — the normalized model serializes byte-stably; goldens are
  committed and diffed.
- **Golden display lists** — layout output for fixture sheets is golden-tested,
  including the virtualized-viewport path (which must equal the full-layout path).
- **Package acceptance** — a workbook exercising the structure a `.xlsx` has to
  carry (values of every kind, formulas, a future function, merges, a freeze, a
  hidden row, a resized column, two sheets, a cross-sheet formula, a defined
  name) is **re-saved through LibreOffice** and re-imported. A full re-save,
  not merely an open: it drives another implementation's reader over every part
  and then writes back what it understood, which is what catches a part it
  skipped or a merge it never saw (P1B-003). The differences LibreOffice
  legitimately introduces — rewriting boolean and error literals — are listed
  with reasons rather than normalised away, so they stay visible. **This is
  LibreOffice's acceptance, not Excel's**; Excel's repair prompt cannot be
  tested without Excel.
- **Recalc oracle** — computed cell values are diffed against LibreOffice Calc
  for the formula corpus (`oracle-diff`, P2-003). It exists because every other
  test of the evaluator was written from the specification by whoever wrote the
  code it tests: that catches mistakes but not **misreadings**, where the test
  agrees with the bug. An independent implementation does not share the
  misreading.

  Two honest limits. The oracle is LibreOffice and the target is Excel, so
  where those two differ, matching LibreOffice proves nothing — such a case is
  recorded in the corpus as `@differs: <reason>`, and the run then fails if the
  difference ever *disappears*, so an excuse cannot quietly rot. And LibreOffice
  exports its own error codes (`Err:502`), which SpreadsheetML has no token for,
  so where both sides error but spell it differently the run reports
  **error-class agreement** rather than claiming to have adjudicated `#NUM!`
  against `#VALUE!`.
- **Recalc latency budget** *(Phase 2)* — the benchmark harness asserts the
  worst-case incremental recalc stays under the [30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)
  budget on the named baseline environment.
- **Scroll/paint budget** — the grid-render benchmark asserts a visible-window
  repaint fits the 60 fps frame budget on the baseline environment.

## Scheduled / security

A weekly workflow re-runs `cargo audit` / `cargo deny` and a bounded seeded
fuzz campaign against the XLSX package reader.

## Release tags

Releases are triggered by a **component-scoped tag**, never a bare `v0.0.0`.
This repository ships more than one artefact, and the moment two release
workflows watch the same tag namespace, tagging either one fires both — a
failure that is invisible until the day it publishes something nobody meant to.

| Tag | Releases | Workflow |
| --- | --- | --- |
| `sdk-v*` | `@opencalc/sheet`, `@opencalc/engine`, `@opencalc/react` on npm | [`release-sdk.yml`](../.github/workflows/release-sdk.yml) |
| `engine-v*` | the `casual-calc-*` crates on crates.io | reserved |
| `desktop-v*` | the Tauri desktop app | reserved |
| `server-v*` | the collaboration server | reserved |

```sh
git tag sdk-v0.0.0
git push origin sdk-v0.0.0
```

The version in the tag must equal the version in the packages; the workflow
checks and refuses rather than publishing something the repository cannot be
searched for. `sdk_v*` is accepted alongside `sdk-v*` so a mistyped separator
is a release rather than a silent no-op, but the hyphen is canonical.

Publishing credentials live on the **`release`** GitHub environment, not in
repository secrets, so a release requires whatever protection rules that
environment carries. `PKG_PASS` holds an npm access token with **read and write
on the `@opencalc` scope** — scoped to the org rather than to named packages,
because a token cannot be granted rights over a package that has never been
published, which is every package on a first release.

The workflow tries that secret as a token and confirms with `npm whoami` rather
than guessing at its shape, and only falls back to treating it as an account
password if that fails. The fallback exists for completeness and stops working
the moment the account requires two-factor authentication for publishing; the
token path is the supported one.

## Future gates (not built yet)

- A CSV/ODS interop gate once those adapters land.
- A memory-ceiling gate measuring **resident** memory, not payload. The
  per-cell record and the 1M-cell payload are asserted
  (`casual-calc-model/tests/memory_ceiling.rs`); what is still unmeasured is the
  allocator's real footprint, which needs a counting global allocator and
  therefore `unsafe`, which this workspace forbids.

When a needed gate does not exist, name it here and add a tracker row rather than
silently proceeding without it.
