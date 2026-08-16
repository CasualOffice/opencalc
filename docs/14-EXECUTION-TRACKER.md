# 14 — Execution tracker

**The only tracker.** Every unit of work has one row here, whatever kind of work
it is. Closed rows move to [14a](14a-ARCHIVE-CLOSED-WORK.md); design reasoning
lives in the commit that made the change.

There were thirteen overlapping tracking documents. They disagreed with each
other about what was done, what was severe, and in two cases about what an id
even meant — `FID-13` named both a sheet-quoting defect and a `sheetView`
construct, so `git log --grep FID-13` returned two unrelated changes. A tracker
nobody can trust is worse than no tracker, because work gets planned from it.

## How a row works

```
| ID | Title | St | Sev | Mechanism | Gate |
```

- **Mechanism** — the cause, in one line. Not the story of finding it.
- **Gate** — the named test that fails if it regresses. If you cannot name one,
  the row is not `Done`.
- **St** — `Open`, `WIP`, `Partial`, `Blocked`, `Designed`, `Done`, `Dropped`.
  Nothing else.
- **Sev** — by what reaches the *file*, not by what shows on screen. Silent
  corruption is P0 however small it looks; a missing menu item is P3 however
  loud.

Rows stay short on purpose. The long account of a defect — what made it
invisible, which two wrong turns preceded the fix — belongs in the commit
message, where `git blame` finds it beside the diff and where it costs nothing
to keep.

**Before adding a row, run `tools/check-tracker-ids.py`.** Ids are never reused;
it is a pre-commit hook and a CI gate because five rows were added without
reading the id space above them.

## P0 — silent corruption, or nothing ships until it is fixed

| ID | Title | St | Sev | Mechanism | Gate |
| --- | --- | --- | --- | --- | --- |
| COL-28 | Collaborative undo does not converge | Open | P0 | Undo is applied locally without transforming against concurrent remote operations. Design in [69](69-COLLABORATIVE-UNDO-POLICY.md). | Two browsers, interleaved undo, both documents equal |
| COL-30 | Retained parts and tables bypass the operation wire | Open | P0 | Mutations to `retained_parts` and `sheet.tables` are applied directly, so a replica never sees them. | A change to either reaches the second browser |
| SEC-002 | No end-to-end admission budget | Open | P0 | Limits exist per subsystem; nothing caps the total work one request can cause. | A crafted upload is refused, not merely slowed |
| SEC-003 | Docker secret hygiene | Done | P0 | Three holes: `.env` was in neither `.gitignore` nor `.dockerignore` while the README tells everyone to create one holding the signing secret; and compose defaulted to `dev-secret-change-me`, so a deployment that forgot got a **published** key — and a shared secret lets its holder *mint* tokens, not merely check them. Compose now has no default (missing fails loudly) and the server **refuses** every placeholder this repo publishes, plus anything under 16 bytes. | `secret_tests` reads the values out of `.env.example` and both compose files, so a new placeholder cannot slip in |
| PROD-12 | Docker collaboration endpoint is unreachable | Done | P0 | The default was `ws://127.0.0.1:8443/collab` — the *browser's* loopback, on a different port from the page — so a share link worked only on the Docker host. Now derived from the request the browser just made (same origin, `X-Forwarded-Proto` picks `wss`), and standalone gains an nginx so one origin is true. Explicit config still wins. | `endpoint_tests` (4); CI brings the stack up and upgrades `/collab` on the page's origin |
| PROD-13 | Upload → store → open → co-edit | WIP | P0 | Limits landed; Docker reachability, error UX and the end-to-end gate remain. | Upload a real `.xlsx`, share it, edit from two browsers |

## P1 — wrong answers, lost work, or a promise the code does not keep

| ID | Title | St | Sev | Mechanism | Gate |
| --- | --- | --- | --- | --- | --- |
| SEC-011 | No populated-cell admission cap | Open | P1 | [21](21-PARSER-LIMITS.md) presents six spreadsheet-scale limits; only the grid bound exists. `OC-IMP-0003` and [23](23-CELL-STORE-REPRESENTATION.md) R7 both rely on one. | A cell count over the cap is refused with that code |
| SEC-012 | Nothing is cancellable | Open | P1 | [07](07-QUALITY-SECURITY-AND-COMPATIBILITY.md) and [21](21-PARSER-LIMITS.md) promise cancellable long operations; no token or deadline exists in any crate. | A long import stops when asked |
| RND-05 | Headless render omits what the canvas draws | Open | P1 | Conditional formatting, charts and images are drawn only by `webapp/editor.js`; `casual-calc-render` references none of them. | A PNG of a chart sheet matches the canvas |
| PERF-09 | No AST interning | Open | P1 | `store_formula` appends with no dedup, so a filled-down column of N cells costs N ASTs — against [40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md) and the 1M-cell budget. | Memory for N identical formulas is O(1) ASTs |
| PERF-07 | Cold and adversarial recalc unproven | Open | P1 | The <50 ms budget is asserted for warm incremental recalc only. | A first edit and a worst-case chain both inside budget |
| PERF-08 | Memory gate measures payload, not RSS | Open | P1 | The 1M-cell gate is arithmetic over payload size; the frame gate allows four frames, not one. | RSS measured; frame ceiling at 16.67 ms |
| UX-CUT-03 | A cut does not repoint formulas that referenced it | Open | P1 | Excel rewrites every other formula pointing at cut cells so it follows them. Only the moved cells' own formulas are handled. | A formula referencing a cut cell follows it |
| COL-40 | Owner cannot change access or lock a live document | Designed | P1 | Needs `PROTOCOL_VERSION` 5→6; design in [72](72-SESSION-ACCESS-CONTROL.md), shares the bump with COL-38. | A demoted participant is refused by the engine |
| COL-32 | Filter sharing and personal views | Designed | P1 | A filter is per-document, so one participant's filter hides rows for everyone. Design in [71](71-FILTER-SHARING-AND-VIEWS.md). | One participant filters; the other's rows are unchanged |
| UX-CLIP-01 | Cross-application rich HTML paste | WIP | P1 | Borders deliberately deferred; the rest of the inbound path lands. | Paste from Excel keeps fonts, merges and number formats |
| DEP-03 | Redis op log never trimmed | Open | P1 | `RPUSH` with no `LTRIM`, `EXPIRE` or `DEL`, and `LRANGE 0 -1` every lease tick per document per node. Redis has no `maxmemory`. | Log length bounded across a long session |
| DEP-04 | Redis is an unreplicated SPOF | Open | P1 | No sentinel, no cluster, no TLS. On loss, `order()` fails its append and returns without acking, so clients resend forever while `/healthz` says `ok`. | A readiness probe fails; clients are told |
| DEP-05 | Drain cannot finish inside a stop grace period | Open | P1 | Sequential per-document saves, 10s each, no global deadline; compose sets no `stop_grace_period`. | A node with many documents drains inside its grace |
| DEP-06 | No metrics | Done | P1 | `/stats` returned two integers, so "are saves failing?" could only be answered by tailing logs — which no alert can do. `/metrics` now serves ten counters and two gauges in Prometheus text, hand-written rather than adding a dependency to a server that reads untrusted files. | `metrics_count_a_save_that_really_happened` asserts the **delta**; mutation gives `0 -> 0` |
| DEP-07 | CI never built an image | Partial | P1 | Both images now build in CI and the standalone stack is brought up and probed, so a Dockerfile that stops building fails here rather than at an integrator. **Publishing still open** — it needs a registry and a tagging decision. | `docker-build` job |
| DEP-08 | No Kubernetes manifests or Helm chart | Open | P1 | The design targets k8s ([59](59-COLLABORATION-SERVICE-STACK.md)) and ships nothing for it. Needs DEP-04, DEP-05 and DEP-06 first. | A chart deploys and a pod restart loses nothing |
| CI-005 | Supply-chain and provenance gates | Open | P1 | Largely closed by SEC-007..009 (audit, SHA pinning, weekly run). Remaining: workflow permissions, npm and container inputs. | Policy fails on an unreviewed source |
| DOC-025 | Live docs and ADRs reconciled | WIP | P1 | This consolidation is its deliverable. Remaining: the 142 stale claims the doc audit found, and the pending-ADR list. | No doc claims a gate CI does not run |
| SDK-008 | `WorkbookSession` escapes | Open | P1 | `config_mut`, `workbook_mut` and `apply_raw` let a host bypass the invariants the session exists to hold. | A host cannot reach an invalid workbook |
| SDK-009 | npm packages ship no types | Open | P1 | `@opencalc/sheet` and `/react` have no declarations; all three are at `0.0.0`. | `tsc` resolves the public surface |

## P2 / P3

| ID | Title | St | Sev | Mechanism | Gate |
| --- | --- | --- | --- | --- | --- |
| COL-38 | A parse failure is reported as `CannotMerge` | Open | P2 | The refusal names the transform, which is the part that worked. Needs the 5→6 bump; ride with COL-40. | A malformed submission reports a parse failure |
| DEP-09 | Load-aware placement never consumed | Open | P2 | `announce` publishes load and `elect()` picks the least-loaded peer, but `elect` is called only from tests. | A full node redirects instead of refusing |
| DEP-10 | Multi-tenancy documented, not implemented | Open | P2 | One `Verifier`, one key set, and `iss` is never checked — so one tenant's minter can mint for another's document. **P0 for anyone multi-tenanting.** | A token from the wrong issuer is refused |
| DEP-11 | Secrets are env-vars only | Open | P2 | No `*_FILE` fallback, so the signing key is visible in `docker inspect` and `/proc/1/environ`. | A mounted secret file is read |
| DEP-12 | No backup/restore, and partly non-atomic writes | Open | P2 | No stated RPO; `create`, `upload` and `settings.json` use plain `fs::write`, so a crash mid-upload leaves a truncated file. | A crash mid-write leaves the previous file intact |
| PERF-06 | Range precedents scanned linearly | Open | P2 | One edge per range, scanned per popped cell: `O(dirty × ranges)`. 8.98x for 10x the sheet — headroom, not viability. | Range-edit scaling flat under a kept graph |
| PIV-02 | Pivots export as cells, not as a live pivot | Blocked | P2 | Route is a cache with `saveData="0" refreshOnLoad="1"`. Blocked on validating output against Excel itself. | Excel opens a created pivot as a pivot |
| PROD-08 | Fuzzing reaches some parsers | Partial | P2 | Package, number format, wire, XML readers, transform and token verifier are fuzzed. The corpus is small and comes from one upstream project. | Every untrusted parser fuzzed, corpus from several producers |
| MNT-001 | Empty catches swallow command failures | Open | P2 | 82 of 161 `catch` blocks in the editor are empty, so a failed command is silent. | A failing command reports |
| MNT-002 | Integration modules are review monoliths | Open | P2 | `editor.js`, the WASM bridge and formula dispatch are too large to review as a diff. | Hot files split behind their seams |
| P1C-003 | Text shaping is wired but not drawn | Partial | P2 | `rustybuzz` is behind a feature and correct; `draw_glyphs` still walks per `char`. Bundled fonts cover Latin and Hebrew only — other scripts need a font decision, not a shaper. | A Hebrew run draws right-to-left |
| UX-GRID-02 | `ensureVisible` ignores zoom | Open | P2 | The viewport rect is compared against grid units without dividing by `state.zoom`, so scroll-into-view overshoots at 200%. | Scroll-into-view lands correctly at 200% |
| UX-P08a | Border palette polish | Partial | P3 | Diagonals, composite placements and true double rendering shipped. Line-colour swatch feedback remains. | — |
| UX-P09a | Elapsed-time formats | Partial | P3 | `[h]`, `[m]` and `[s]` shipped as fields. Locale-prefix polish remains. | — |
| UX-P13a | Sheet visibility | Partial | P3 | Model, round-trip and hide/unhide UI shipped. Tab overflow, move-to and protection remain. | — |
| UX-CHROME-01 | Hiding the menu bar hides the roster | Open | P3 | The roster is a child of the menu bar, so `.oc-hide-menubar` takes it too. | A host hiding the menu bar keeps the roster |
| TAURI-001 | Tauri desktop shell | Open | P3 | Designed in [44](44-TAURI-DESKTOP-SHELL-DESIGN.md); deliberately not started. | — |

## The Excel parity backlog

[73](73-EXCEL-UX-PARITY-AUDIT.md) holds roughly thirty-five findings that have no
row here yet — the largest untracked surface in the repository. Its
`[verified]`/`[unverified]` split is the point of it: three verified P1s became
`FID-23`, `UX-NAV-01` and `UX-NAV-02` and are closed, and the rest must be
reproduced before they are scheduled. Rows arrive here as they are verified;
the audit is worked down, not merged in wholesale.

## Where a finding goes

One question, in order:

1. **Does the code do something wrong?** A row here.
2. **Does a document describe code that does not exist?** Fix the document. No
   row — a doc edit is not tracked work.
3. **Does a document state a contract the code does not keep?** A row here, and
   **the document keeps its text.** Never resolve this by deleting the promise:
   that is how a security bound disappears without anyone deciding to drop it.
   [23](23-CELL-STORE-REPRESENTATION.md) says it plainly — *a documented limit
   nothing in the code enforces is not a limit*.

A new tracking document is never the answer. That is how there came to be
thirteen.
