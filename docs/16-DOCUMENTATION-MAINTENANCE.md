# 16 — Documentation Maintenance

Docs are part of the implementation contract. A change isn't done until the
written design matches the code. This doc says when and how to keep `docs/` true.

## When docs must be updated

- **Before** implementing: the design note exists and is finalized.
- **With** any behavior change: the relevant design note, the
  [18-SUPPORT-MATRIX](18-SUPPORT-MATRIX.md), and the
  [14-EXECUTION-TRACKER](14-EXECUTION-TRACKER.md) row move in the same PR.
- **On** any ADR-trigger decision: an ADR is added to
  [08-ADR-REGISTER](08-ADR-REGISTER.md).
- **On** a limit/error/schema change: the corresponding contract doc
  ([20](20-ERROR-CODE-REGISTRY.md), [21](21-PARSER-LIMITS.md),
  [22](22-NORMALIZED-SCHEMA.md), [28](28-XLSX-PACKAGE-READER.md)) is updated
  and versioned.

## Numbering rules

- Numbers are stable and never reused. Retire with a tombstone
  (`> Retired: merged into 40`), don't renumber.
- New docs take the next free number in the appropriate range (see
  [00-README](00-README.md) §Numbering discipline).

## ADR rules

- Append-only. Supersede, don't rewrite.
- Every ADR names the trigger it fired and links the design note.

## Tracker rules

- One stable ID per unit of work; cited from PRs, changelog, ADRs, and design
  notes. Status uses the controlled vocabulary only. Never let a row go stale.

## Research freshness

Competitive and format research goes stale. When you cite Excel/LibreOffice/
OnlyOffice/Univer/IronCalc/Formualizer/Google Sheets behavior, record:

- the **source** (doc, product version, or observed behavior),
- the **date checked**, and
- the **impact** (what decision it supports).

Re-verify before relying on aged research for a new decision.

## Pre-close documentation review

Before marking a phase `Done`, confirm:

- [ ] Every delivered capability has a design note.
- [ ] The support matrix reflects reality (target vs implemented).
- [ ] The changelog cites the tracker IDs.
- [ ] No ADR-trigger decision is undocumented.
- [ ] No design note describes behavior the code doesn't have (or vice versa).
