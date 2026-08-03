# Contributing to OpenCalc

OpenCalc is built design-first and doc-driven. Read [AGENTS.md](AGENTS.md) and
[docs/11-DESIGN-FIRST-PROCESS.md](docs/11-DESIGN-FIRST-PROCESS.md) before opening
a PR. This file is the concrete engineering contract.

> **Note:** the engine is not yet scaffolded. The commands below are the
> *intended* local checks for when the Cargo workspace exists (Phase 0). Until
> then, "contributing" means authoring and refining `docs/`, and the only
> applicable checks are the documentation ones.

## Before you implement

- [ ] The outcome is written down (a numbered `docs/` design note).
- [ ] Competitor behavior is recorded (source + date checked).
- [ ] An ADR exists and is **Accepted** if the change trips an ADR trigger.
- [ ] The work has a row in [docs/14-EXECUTION-TRACKER.md](docs/14-EXECUTION-TRACKER.md)
      with a stable ID and a status.
- [ ] Acceptance gates (tests/fixtures) are defined.

## While you implement

- Small, coherent increments — one capability per PR.
- Mutation only through transactions/commands; every op returns its inverse.
- New behavior ⇒ new tests. Determinism, round-trip, and (once the calc engine
  lands) recalculation results are golden-tested.
- Keep the tracker row moving as status changes. **No untracked work merges.**

## Before you merge

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo test --workspace --all-features --locked` (incl. doc tests)
- [ ] `cargo doc --workspace --all-features --no-deps` with `RUSTDOCFLAGS="-D warnings"`
- [ ] `cargo check --target wasm32-unknown-unknown` (WASM stays green)
- [ ] `cargo deny check bans licenses sources` and `cargo audit --deny warnings`
- [ ] Fixture manifest checksums pass; no merge-conflict markers.
- [ ] Docs and ADRs updated; tracker row updated.

> **Rustdoc gotcha:** intra-doc links are checked under `-D warnings`. A broken
> `[Type]` link fails the `docs` gate even when the crate builds. Run the doc
> gate locally before pushing.

## PR shape

Every PR body includes:

- **Problem** — what and why.
- **Design** — link to the `docs/` note and ADR.
- **Verification** — the checklist above, with results.
- **Impact** — API, compatibility/fidelity, security/limits, performance, UX, docs.
- **Tracker** — the tracker ID(s) this advances.

## Commit style

Conventional prefixes (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `perf:`,
`chore:`), imperative mood, one logical change per commit.

## Quality bar

Production-grade. Correctness and determinism are non-negotiable; a change that
can produce a wrong cell value, corrupt a file, or lose data silently does not
merge regardless of how useful it otherwise is.
