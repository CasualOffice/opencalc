# Changelog

All notable changes to OpenCalc are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/); semantic versioning applies once
a published crate line begins. Until then, everything lives under **Unreleased**,
grouped by date.

Each entry should cite the driving tracker ID (see
[docs/14-EXECUTION-TRACKER.md](docs/14-EXECUTION-TRACKER.md)) and, where relevant,
the design doc or ADR that motivated it.

## Unreleased

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

**Notes**

- OpenCalc is in the **documentation phase**. No engine code exists yet. The
  design record commits to the full architecture — including the layer division
  and the virtualization strategy — up front, so later phases (notably the
  Phase 2 calc engine) slot in without a do-over.
