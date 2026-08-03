# 21 — Parser Limits

Every admission path in OpenCalc is **bounded**. Untrusted input cannot exhaust
memory or CPU; when a limit is hit, admission fails cleanly with a diagnostic
code ([20](20-ERROR-CODE-REGISTRY.md)) — never a crash, hang, or OOM. This is a
**contract** doc; limits are configurable within hard ceilings that cannot be
raised past a bypass point.

Design shared with OpenDoc's `casual-doc-package`; specialized for the shapes and
scale of SpreadsheetML.

## Package (OPC / ZIP) limits

| Limit | Purpose | Indicative ceiling |
| --- | --- | --- |
| Max input size | Reject oversized uploads | 1 GiB |
| Max entry count | Cap number of parts | 50,000 |
| Max total expanded size | Zip-bomb defense | 4 GiB |
| Max expansion ratio | Zip-bomb defense | 1000:1 |
| Max path bytes | Path-abuse defense | 4 KiB |
| Path traversal | Reject `..`/absolute/escape | rejected |

(Exact defaults finalized in Phase 0 and asserted by a fuzz/fixture gate.)

## XML limits (per part)

| Limit | Purpose |
| --- | --- |
| Max element count | Bound `quick-xml` streaming work per part |
| Max nesting depth | Stack-abuse defense |
| Max attribute count / size | Reject pathological elements |
| Entity expansion | Disabled / bounded (billion-laughs defense) |
| External entity resolution | **Disabled** (no XXE) |

## Spreadsheet-scale limits

Because a single sheet can legitimately be huge, these are separate from and
larger than the document-oriented caps, but still bounded:

| Limit | Purpose | Note |
| --- | --- | --- |
| Max rows / columns | SpreadsheetML maximum | 2^20 rows × 2^14 cols |
| Max populated cells (admission) | Bound model memory | Tuned to the T1 budget ([30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)); over-cap ⇒ clean rejection |
| Max shared strings | Bound the string table | — |
| Max defined names | Bound name resolution | — |
| Max formula length / AST depth | Bound the parser | Rejects pathological formulas |
| Max merged ranges | Bound layout | — |
| Max snapshot size | Bound the model serializer | — |

## Calc limits (Phase 2)

| Limit | Purpose |
| --- | --- |
| Max dependency-chain depth | Bound recalc; detect runaway graphs |
| Iterative-calc iteration cap | Bound intentional cycles |
| Iterative-calc convergence threshold | Stop when stable |
| Max spill region size | Bound dynamic-array expansion |
| Recalc time budget (cancellable) | Keep hostile workbooks from wedging the host |

## Policy

- **No macro / VBA execution**, ever. VBA parts are opaque.
- **No automatic external fetch** — links, external references, remote images are
  never resolved automatically.
- **Cancellable jobs** — admission and full recalc are bounded *and* cancellable.
- **Fail closed** — on any limit breach, reject with a code; do not partially
  admit into an inconsistent state.

## Verification

Limits are exercised by the fuzz targets (`fuzz/`, pinned nightly) and by hostile
fixtures in the corpus (zip bomb, deep nesting, oversized parts, pathological
formulas, over-cap cell counts). A hostile fixture that is *not* rejected within
limits is a release blocker ([15](15-CI-AND-RELEASE-GATES.md)).
