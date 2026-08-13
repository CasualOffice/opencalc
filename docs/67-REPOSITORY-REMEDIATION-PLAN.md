# 67 — Repository Remediation Plan

**Status:** Active planning baseline  
**Audit date:** 2026-08-14  
**Scope:** the complete repository audit plus the failures reproduced through the
Docker product path  
**Tracker IDs:** SEC-001, SEC-002, SEC-003, COL-28, COL-29, COL-30, PROD-12,
PROD-13, COL-27, UX-CLIP-01, SDK-008, SDK-009, PERF-07, PERF-08, CI-005,
DOC-025, MNT-001, MNT-002

## Outcome

Move OpenCalc from a strong alpha with broad spreadsheet functionality to a
release candidate whose collaboration, untrusted-input, Docker, SDK and
performance claims are enforced end to end.

This plan is intentionally broader than the three visible Docker failures. It
records every actionable issue confirmed by the repository-wide audit, orders
them by OpenCalc's own engineering priorities, and gives each one a gate that
must fail if the defect returns. A green existing suite is not sufficient: the
highest-risk findings are precisely paths that the current suite does not
exercise.

This document sequences design and implementation. It does **not** waive the
design-first rule. Work that changes transaction semantics, the public SDK,
parser policy or a performance contract must update the named design note and,
where called out below, land an accepted ADR before implementation.

## Priority policy

The order is fixed by failure class, then dependency:

1. silent divergence, corruption, script execution or unbounded admission;
2. a primary Docker workflow that cannot complete;
3. silently degraded interoperability or an unstable public API;
4. published capacity claims that are not actually gated;
5. release-process and maintainability debt.

`P0` blocks any production or stable-SDK claim. `P1` blocks a stable public
preview. `P2` is required before 1.0. `P3` is continuing maintenance and must not
displace an unresolved correctness item.

## Ordered execution plan

| Order | Wave | ID | Priority | Status | Result required |
| ---: | --- | --- | --- | --- | --- |
| 1 | A | SEC-001 | P0 | Open | Workbook-controlled text cannot create or execute DOM. |
| 2 | A | COL-29 | P0 | Open | Only successfully applied operations enter the collaboration queue. |
| 3 | A | COL-28 | P0 | Open | Undo and redo are transported, transformed and convergent. |
| 4 | A | COL-30 | P0 | Open | Every user mutation, including retained-part changes, is an invertible wire operation. |
| 5 | A | SEC-002 | P0 | Open | One non-bypassable admission budget reaches every parser and model allocation. |
| 6 | A | SEC-003 | P0 | Open | Docker build/runtime defaults do not ingest or silently rely on deployable secrets. |
| 7 | B | PROD-12 | P0 | Open | A shared Docker link reaches collaboration from another machine and under HTTPS. |
| 8 | B | PROD-13 | P0 | In progress | Uploading a real workbook lands in a durable editable shared session. |
| 9 | B | COL-27 | P1 | In progress | Remote selections and real participant names are visible and gated. |
| 10 | C | UX-CLIP-01 | P1 | Open | Supported Excel/LibreOffice/Sheets clipboard formatting pastes safely. |
| 11 | C | SDK-008 | P1 | Open | Live session configuration and mutation cannot bypass required invariants. |
| 12 | C | SDK-009 | P1 | Open | All published packages expose typed, versioned, testable contracts. |
| 13 | D | PERF-07 | P1 | Open | First-edit and adversarial incremental recalc satisfy the published 50 ms target. |
| 14 | D | PERF-08 | P1 | Open | Resident memory and the 8 ms engine frame budget are measured and enforced. |
| 15 | D | CI-005 | P1 | Open | Supply-chain and workflow provenance claims are enforced by CI. |
| 16 | D | DOC-025 | P1 | Open | Contracts, roadmap, support matrix and audits describe the current code. |
| 17 | E | MNT-001 | P2 | Open | Browser failures are observable; blanket exception swallowing is eliminated. |
| 18 | E | MNT-002 | P2 | Integration monoliths are split behind tests without changing behavior. |
| 19 | E | PERF-06 | P2 | Range-edge propagation is bounded for a large dirty set, if measurement still requires it. |

Items inside a wave may proceed in parallel only when they do not touch the same
invariant. In particular, COL-28 and COL-30 must not be implemented as competing
changes to the operation/history representation. COL-29 is deliberately first
because every later collaboration fix relies on the outgoing log being truthful.

## Wave A — contain correctness and security failures

### SEC-001 — workbook-controlled DOM injection

**Finding.** Name Manager inserts imported defined-name text with `innerHTML`.
Other status/reporting paths also construct markup from values that can originate
in a workbook. An untrusted `.xlsx` can therefore create active HTML in the
editor origin when the relevant UI is opened.

**Plan.**

1. Inventory every non-static `innerHTML`, `insertAdjacentHTML` and
   `document.write` sink in the editor and host pages.
2. Replace workbook/user-controlled sinks with element construction and
   `textContent`; keep static template construction separately auditable.
3. Add a small safe-markup helper only if a real static-markup use cannot be
   expressed clearly with DOM methods. It must never accept workbook strings.
4. Add a browser security fixture containing hostile defined-name formulas,
   validation messages, comments, sheet names and compatibility-report text.
5. Consider a Trusted Types policy only after the sinks are removed; a policy
   that simply blesses existing strings is not a fix.

**Gate.** Opening every affected UI creates text nodes only; no `img`, `script`,
event-handler attribute, navigation or network request is produced. Add a CI
source check that requires an explicit audited marker beside any remaining
dynamic HTML sink.

**ADR:** none if this remains a sink-removal change.

### COL-29 — record only successful local operations

**Finding.** `WorkbookSession::edit` narrows and appends an operation to the
outgoing log before `History::apply` succeeds. A locally rejected operation can
therefore be sent to the server and applied by peers.

**Plan.** Compute the narrowed wire candidate against the pre-edit workbook,
apply through history, and append only after success. A refused edit must leave
workbook, source preservation, history, recalc state and outgoing log unchanged.

**Gate.** For every fallible operation class, force application failure while
recording and assert `take_applied()` is empty, history is unchanged, save bytes
are unchanged and `collab_flush()` emits nothing. Mutation-test by restoring the
pre-apply append.

**ADR:** none; this enforces the existing transaction and collaboration design.

### COL-28 — convergent collaborative undo and redo

**Finding.** Local undo/redo mutate history but do not enter the applied-operation
log. One participant reverts locally while the server and peers retain the edit.
This contradicts docs/56's intention-preserving undo contract.

**Plan.**

1. Make history return the exact inverse/applied operation it executes.
2. Record that operation through the same successful-application path as a new
   edit; do not introduce a second transport.
3. Transform an undo inverse against every committed operation since the edit it
   intends to reverse. Redo is a new intention and receives the same treatment.
4. Define behavior when structural operations delete or replace the target.
   Prefer an explicit no-op/refusal over undoing somebody else's work.
5. Keep local single-user labels and bounded history behavior unchanged.

**Gate.** A deterministic interleaving matrix covers different cells, the same
cell, row/column insertion and deletion, sheet operations, disconnect/resume and
two sequential undos. After every ordering, both clients, the authoritative
server model and the saved `.xlsx` agree. The browser suite must press the real
Undo/Redo controls in both participants.

**ADR/design:** update docs/56 and docs/24 before implementation. Write an ADR if
the operation or wire schema changes; bump the protocol version if old and new
clients would interpret a message differently.

### COL-30 — complete the transaction/wire mutation boundary

**Finding.** Chart deletion/editing drops retained parts after the metadata
operation, pivot application detaches parts after its transaction, and named
style application mutates the workbook style table before applying cell styles.
These changes are not fully undoable, serializable or collaborative.

**Plan.**

1. Audit every `workbook_mut()` caller and classify it as construction-only,
   interning-only or user-visible mutation.
2. Add explicit invertible operations for retained-part/relationship deltas and
   workbook-level named-style definitions, or include those deltas atomically in
   the owning operation.
3. Localize every workbook-table reference by value on the wire.
4. Until each operation exists, disable the affected command in collaborative
   sessions with an explicit explanation rather than allowing divergence.
5. Make the feature-correctness scanner fail on new user mutation paths that do
   not reach `session.edit`.

**Gate.** For charts, pivots and named styles: apply, undo, redo, save/reopen and
two-peer collaboration must agree both semantically and in retained-part/report
state. Deleting an imported chart and undoing it must restore the original opaque
part and relationship bytes.

**ADR/design:** transaction and wire representation are ADR triggers. Update
docs/24, docs/34 and docs/56 and accept the representation before implementation.

### SEC-002 — one end-to-end resource budget

**Finding.** Caller-supplied `OoxmlLimits` governs OPC discovery but semantic
readers use unrelated hard-coded limits; chart/pivot paths do not consistently
count depth/elements. Documented caps for populated cells, strings, names,
formula size, merged ranges, snapshots and recalc work are incomplete. Public
fields can also be raised beyond the documented hard ceiling.

**Plan.**

1. Define `HardAdmissionCeilings` and a host `AdmissionPolicy` that can tighten
   but never raise them.
2. Pass one resolved immutable budget through package admission and every XML
   reader, including styles, themes, charts, pivots, comments and relationships.
3. Count semantic allocations separately from XML tokens: cells, shared strings
   and bytes, defined names, formulas/AST nodes, merges, validations, comments,
   charts, pivots, retained bytes and snapshot output.
4. Add bounded/cancellable full recalculation and import work at the SDK/server
   boundary; a wall-clock cutoff alone must not make model results nondeterministic.
5. Apply explicit request limits to host upload and callback bodies, and align
   them with collaboration fetch limits.
6. Return stable error codes naming the exhausted axis; never partially admit.

**Gate.** Each axis has limit−1, limit and limit+1 tests, plus fuzz seeds that
reach the semantic reader rather than only OPC discovery. A service-supplied
tight limit must reject the same package that desktop defaults accept. Raising a
public policy beyond the compiled hard maximum must fail construction.

**ADR/design:** parser/security policy is an ADR trigger. Update docs/20 and
docs/21 and accept the hard-ceiling/configuration model first.

### SEC-003 — Docker secret and deployment hygiene

**Finding.** `.env` is not excluded from the Docker build context, Compose has a
known development signing secret fallback, and the browser endpoint default is
easy to expose unchanged. The host is explicitly a demo, but its one-command
shape makes those defaults likely to escape development.

**Plan.** Exclude `.env`, keys, certificates, document stores and local reports
from build contexts; keep secrets out of image layers and generated diagnostics;
require an explicit non-development secret when binding non-loopback or document
the deployment as insecure and refuse public startup; add a configuration
preflight shared by the host and collaboration server.

**Gate.** A build-context test rejects secret-like fixture paths, image history
contains no configured secret, logs/admin state redact secrets, and public-mode
startup fails with the known development secret.

**ADR:** update the existing server exposure/security design; a new ADR is only
needed if the token trust model changes.

## Wave B — make the advertised Docker product work

### PROD-12 — browser-reachable collaboration

Use the detailed tracker row as the execution contract. Keep internal fetch URLs
separate from browser WebSocket URLs, support same-origin reverse proxying and
`wss://`, and replace infinite unexplained reconnect with configuration guidance.

**Gate.** Two browsers on a non-loopback hostname and the TLS proxy shape reach
`live`, exchange edits and presence, reconnect, and persist the result.

### PROD-13 — upload to durable editable session

Finish the in-progress multipart-limit increment. Pass the configured limit
through Compose, validate before persistence, check both atomic writes, handle
session-fetch/import errors in the document page, and resolve File ▸ Open during
collaboration through a host-owned upload/new-session command rather than local
workbook replacement.

**Gate.** The Docker browser suite covers a valid workbook over 2 MB, oversized,
malformed, disabled-upload and unwritable-store cases, followed by two-party edit
and saved download verification.

### COL-27 — visible remote presence and real identity

Finish the in-progress canvas rendering, remove diagnostic-only globals, render
the server-issued color/name across scroll, zoom, merges and frozen panes, send
the current roster to late joiners, and let the host supply or prompt for a real
display name instead of silently inventing `Guest NN`.

**Gate.** Pixel/overlay evidence in a real two-browser test proves position,
color, label, movement, late join, sheet filtering and departure. Merely receiving
a presence message is not sufficient.

## Wave C — interoperability and stable API

### UX-CLIP-01 — rich paste from other spreadsheet editors

Consume the actual paste event; prefer a sanitized detached `text/html` table
and fall back to text. Map the subset browsers expose reliably: values/formulas
where present, font properties, colors, fills, borders, alignment, number
formats, merges and spans. Never inject clipboard HTML and never fetch its URLs.

**Gate.** Checked-in, non-executable clipboard fixtures from Excel, LibreOffice
Calc and Google Sheets reproduce supported values/styles; hostile markup proves
no execution/network effect; text-only and internal rich paste do not regress.

### SDK-008 — controlled live-session mutation and configuration

**Finding.** `config_mut()` exposes fields whose correct update requires side
effects; callers can change environment, calculation mode or undo depth without
recalculation/history updates. `workbook_mut()` bypasses transactions, and
`apply_raw()` invalidates untouched-source preservation before a read-only
refusal.

**Plan.** Replace broad mutable access with narrow setters and a construction
builder; make configuration fields private; define a scoped programmatic-load
transaction for setup; deprecate or feature-gate raw workbook mutation; perform
all refusal checks before invalidation. Preserve an escape hatch only if its
name/type makes the invalidation and no-undo contract impossible to miss.

**Gate.** Compile-fail/API tests prevent direct field mutation, and behavioral
tests prove each setter updates workbook state, recalc/stale state, history
capacity and source preservation atomically.

**ADR/design:** public SDK and mutation semantics are ADR triggers. Update
docs/55 and docs/24 and publish a migration path before removal.

### SDK-009 — typed, versioned package contracts

**Finding.** `@opencalc/engine` ships declarations; `@opencalc/sheet` and
`@opencalc/react` do not. The preview API is broad and stringly typed while the
roadmap calls stability the remaining Phase 4 work.

**Plan.** Define generated or source-owned declarations for element methods,
commands, events, themes, access, collaboration and React props; export maps must
advertise them. Add API-surface snapshots and a compatibility policy for `0.x`
through 1.0. Narrow APIs before stabilizing them, especially SDK-008.

**Gate.** TypeScript fixture projects for DOM, React and Next compile under the
supported versions; an accidental export/signature removal fails an API diff;
package tarballs contain every declared file and no repository-only artifact.

## Wave D — prove the published performance claims

### PERF-07 — worst-case and first-edit recalculation

**Finding.** The kept graph makes warm independent-cell edits flat, but the first
edit still builds it over all formulas. Current cases do not cover the documented
long dependency chain, wide fan-out or a large dirty set multiplied by range
edges, and do not enforce an absolute 50 ms target.

**Plan.** Build or restore the graph at open/snapshot adoption outside the first
interactive edit, then benchmark cold first edit, warm edit, deep chain, wide
fan-out, overlapping ranges, structural invalidation and mixed formula edits.
Use a named baseline environment for the absolute release gate and scaling gates
for shared PR runners.

**Gate.** Every documented worst-case fixture is correct against full recalc and
under 50 ms on the baseline machine. CI detects complexity regression; a
scheduled/baseline job enforces duration. PERF-06 proceeds only if the expanded
range fixture demonstrates that row-band indexing is required.

### PERF-08 — resident memory and real frame budget

**Finding.** The million-cell test measures payload `size_of`, not resident
memory. The rendering gate allows four 16.67 ms frames while docs publish an
8 ms engine-side budget. The committed benchmark baseline is incomplete.

**Plan.** Add an isolated-process RSS/peak-memory harness without introducing
unsafe into engine crates; account separately for cells, indexes, strings,
styles, formulas and dependency graph. Gate the 8 ms engine frame budget on the
baseline environment, retain a looser smoke alarm for shared runners, and commit
all current benchmark cases with environment metadata.

**Gate.** A real one-million-populated-cell workbook opens, calculates, lays out
and saves inside a documented resident-memory ceiling; dense viewport layout +
render meets 8 ms on baseline; deliberate allocation/frame regressions fail.

### CI-005 — supply-chain and workflow provenance

**Finding.** docs/15 promises full-SHA action pinning, `cargo audit` and a weekly
security/fuzz workflow; current workflows use mutable major tags, omit
`cargo audit`, and have no schedule matching the contract.

**Plan.** Pin third-party actions to full SHAs with version comments; run
`cargo deny` including advisories and `cargo audit --deny warnings`; add a weekly
bounded fuzz/audit job; cover npm lockfiles and container base images; minimize
job permissions and produce release provenance/SBOM where the release workflow
claims it.

**Gate.** A repository-policy script fails on tag-pinned actions, a deliberately
advisory-affected lockfile, missing schedule, excessive permissions or an
unreviewed dependency source.

### DOC-025 — restore one source of truth

**Finding.** AGENTS, README, docs/00, docs/18, CONTRIBUTING, SECURITY, docs/15,
docs/33, docs/60 and docs/66 disagree with shipped code. Examples include calling
the collaboration server unbuilt, calling the workspace unscaffolded, saying npm
packages are unpublished, and describing the persistent graph as pending.

**Plan.** Reconcile claims against code/tests; separate historical audits from
live state; update the workspace dependency diagram from Cargo metadata; close or
write the ADRs still marked pending for decisions already implemented; add a
small generated facts block for crate/package/job/test counts so prose cannot
silently drift again.

**Gate.** A documentation consistency check rejects the known contradictory
phrases and validates generated facts. Every support-matrix solid marker names a
test. The phase table, README, AGENTS current-state section and execution tracker
must agree in the same change.

## Wave E — make failures diagnosable and the code reviewable

### MNT-001 — browser diagnostics and static checks

**Finding.** The editor contains many empty `catch` blocks, including user
commands. Runtime failures can be swallowed while the UI redraws as though the
operation succeeded. There is no dedicated lint/typecheck gate for the large JS
surface.

**Plan.** Introduce one structured command-error path and status/event reporting;
allow empty catches only for explicitly optional capability probes; add ESLint
rules for empty catches, unsafe HTML sinks and floating promises; incrementally
typecheck JS with `checkJs` or migrate module-by-module after SDK declarations
exist.

**Gate.** Injected failures in undo, paste, open, collaboration and save surface
an actionable message and event. CI rejects a new unexplained empty catch,
unsafe dynamic HTML sink or syntax/type error.

### MNT-002 — reduce integration monoliths without a rewrite

**Finding.** `webapp/editor.js`, the WASM bridge and the formula function module
are very large, making ownership, review and targeted tests difficult. Global
module state is also why each embedded editor needs a cache-busted module copy.

**Plan.** Refactor only behind characterization tests. Extract collaboration
presentation, clipboard, workbook-open lifecycle, command dispatch and panels
from the editor; group WASM exports by domain behind internal modules while
preserving generated external names; split formula functions by category with a
single catalog/dispatch contract. Decide whether the four-line selection crate
is deleted and keep ODS explicitly deferred rather than implying implementation.

**Gate.** Behavior/browser/API snapshots are unchanged, multiple embedded
editors no longer depend on accidental shared state, and file/module ownership
is documented. No big-bang rewrite and no feature work hidden inside moves.

## Cross-cutting verification matrix

Every wave closes with the ordinary workspace gates plus the tests below.

| Area | Required evidence |
| --- | --- |
| Collaboration | Pure transform/property tests, client/server model equality, two-browser interaction, saved-file equality |
| Security | Hostile fixtures, boundary tests, fuzz seeds, no execution/network side effect |
| Fidelity | Import → action → undo/redo → save/reopen; compatibility report for unsupported dimensions |
| Docker | Fresh image, stock Compose/proxy configuration, non-loopback browsers, persistent volume |
| SDK | Rust behavior tests, TypeScript compile fixtures, package-content/API snapshot |
| Performance | Correctness oracle first, scaling on PRs, absolute duration/RSS on named baseline |
| Documentation | Generated facts, contradiction scan, support claim linked to a biting gate |

Required commands before a row becomes `Done` remain those in CONTRIBUTING.md.
For Docker/product rows, Rust unit tests alone are never sufficient. For browser
security and clipboard rows, a mocked WASM call alone is never sufficient.

## Release checkpoints

### Checkpoint A — correctness containment

SEC-001, COL-29, COL-28, COL-30, SEC-002 and SEC-003 are `Done`. Until then,
collaboration remains preview-only and untrusted-workbook hosting must not be
described as hardened.

### Checkpoint B — Docker preview

PROD-12, PROD-13 and COL-27 are `Done`. The one-command Docker demo can then be
advertised as a real two-machine co-editing evaluation path.

### Checkpoint C — stable SDK candidate

UX-CLIP-01, SDK-008, SDK-009, PERF-07, PERF-08 and CI-005 are `Done`, with the
documentation reconciled by DOC-025. Only then should the packages move beyond
the current `0.0.x` preview posture.

### Checkpoint D — 1.0 maintainability

MNT-001 and the bounded increments of MNT-002 are complete; PERF-06 is either
implemented because the expanded benchmark requires it or closed with measured
evidence that it does not. Remaining deliberate deferrals are named rather than
represented by empty crates or ambiguous support claims.
