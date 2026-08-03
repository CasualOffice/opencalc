<!--
OpenCalc is design-first and doc-driven. See AGENTS.md and
docs/11-DESIGN-FIRST-PROCESS.md. Fill every section; delete none.
-->

## Problem

<!-- What and why. What can a host/user do afterward that they can't now? -->

## Design

<!-- Link the docs/ design note and any ADR. Summarize the approach. -->

- Design note: docs/NN-...
- ADR(s): ADR-...

## Verification

<!-- Results of the local gates (see CONTRIBUTING.md). -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo test --workspace --all-features --locked` (incl. doc tests)
- [ ] `cargo doc` with `RUSTDOCFLAGS="-D warnings"`
- [ ] `cargo check --target wasm32-unknown-unknown`
- [ ] `cargo deny check` / `cargo audit`
- [ ] New/changed behavior has tests (determinism / round-trip / recalc goldens)

## Impact

- **API:**
- **Compatibility / fidelity:**
- **Security / limits:**
- **Performance** (1M cells / 60 fps / <50 ms recalc):
- **UX:**
- **Docs updated:**

## Tracker

<!-- The docs/14-EXECUTION-TRACKER.md ID(s) this advances. -->

- Tracker ID(s): DOC-... / F-... / P1A-... etc.
