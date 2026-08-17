# 20 — Error-Code Registry

Stable, namespaced diagnostic codes so hosts can react programmatically and logs
are greppable. Codes are **append-only**: never reuse or repurpose a code; retire
with a note. A code is part of the public contract once shipped.

> Distinguish two things: **engine diagnostics** (admission/model/IO failures —
> this registry) vs **cell error values** (`#REF!`, `#VALUE!`, … — spreadsheet
> data, defined in [17-GLOSSARY](17-GLOSSARY.md) and produced by the calc
> engine). This doc is about the former.

## Format

`OC-<AREA>-<NNNN>` — `OC` = OpenCalc; `AREA` groups by subsystem.

| Area | Subsystem |
| --- | --- |
| `PKG` | Package / OPC admission (`casual-calc-package`) |
| `XML` | XML decode (`casual-calc-ooxml`) |
| `IMP` | Semantic import (`casual-calc-import`) |
| `MDL` | Model invariants (`casual-calc-model`) |
| `TXN` | Transactions / edit (`casual-calc-transaction`) |
| `FML` | Formula parse (`casual-calc-formula`) |
| `CAL` | Calc engine (`casual-calc-eval`, Phase 2) |
| `LAY` | Layout (`casual-calc-layout`) |
| `RND` | Render (`casual-calc-render`) |
| `EXP` | Export (`casual-calc-export`) |
| `IO`  | Format detection / dispatch (`casual-calc-io`) |

## Seed registry (illustrative; extended as code lands)

| Code | Meaning | Category |
| --- | --- | --- |
| `OC-PKG-0001` | Input exceeds max package size | limit |
| `OC-PKG-0002` | Entry count over limit | limit |
| `OC-PKG-0003` | Expansion ratio over limit (possible zip bomb) | limit |
| `OC-PKG-0004` | Path traversal / unsafe path rejected | security |
| `OC-PKG-0005` | Not a valid OPC/ZIP package | malformed |
| `OC-PKG-0006` | Requested part not found in the package | lookup |
| `OC-XML-0001` | Element count over limit | limit |
| `OC-XML-0002` | Nesting depth over limit | limit |
| `OC-XML-0003` | External entity resolution refused (XXE) | security |
| `OC-XML-0004` | Malformed XML in part | malformed |
| `OC-IMP-0001` | Required part missing (`workbook.xml`) | malformed |
| `OC-IMP-0002` | Unresolvable relationship (`r:id`) | malformed |
| `OC-IMP-0003` | Populated-cell count over admission limit | limit |
| `OC-IMP-0004` | Shared-string table over admission limit | limit |
| `OC-IMP-0005` | Defined-name count over admission limit | limit |
| `OC-IMP-0006` | Merged-range count over admission limit | limit |
| `OC-IMP-0007` | Import cancelled by the caller | cancelled |
| `OC-MDL-0001` | Invariant violation: zero/duplicate ID | invariant |
| `OC-MDL-0002` | Cell address out of range | invariant |
| `OC-MDL-0003` | Dangling interned reference | invariant |
| `OC-MDL-0004` | Snapshot (de)serialization failed | snapshot |
| `OC-TXN-0001` | Operation target not found | edit |
| `OC-TXN-0002` | Operation would violate an invariant | edit |
| `OC-FML-0001` | Formula parse error | parse |
| `OC-FML-0002` | Formula length / AST depth over limit | limit |
| `OC-CAL-0001` | Circular reference (iterative calc disabled) | calc |
| `OC-CAL-0002` | Iterative-calc iteration cap reached | calc |
| `OC-CAL-0003` | Spill region over limit | limit |
| `OC-LAY-0001` | Layout invariant violation | layout |
| `OC-RND-0001` | Render refused (viewport over limit, or PNG encoding failed) | render |
| `OC-EXP-0001` | Model not writable in requested mode | export |
| `OC-IO-0001`  | Unrecognized / ambiguous format | detection |

## Rules

- Every returned error carries its code, a human message, and (where useful) the
  offending part/address for diagnosis.
- Codes that map to a [21](21-PARSER-LIMITS.md) limit are categorized `limit`;
  hosts can treat all `limit`/`security` codes as "reject the file."
- Adding a code is a docs change (this registry) plus the tracker row that
  introduced it.
