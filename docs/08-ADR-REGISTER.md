# 08 — ADR Register

Architecture Decision Records capture decisions that are expensive to reverse —
the ones listed under "ADR triggers" in
[11-DESIGN-FIRST-PROCESS](11-DESIGN-FIRST-PROCESS.md). Each ADR has a stable
number, a status, and a short rationale. ADRs are **append-only**: a superseded
decision is marked `Superseded by ADR-NNN`, not edited away.

## Status values

`Proposed` · `Accepted` · `Superseded` · `Rejected`

## Register

| ADR | Title | Status | Summary |
| --- | --- | --- | --- |
| ADR-001 | Product identity: OpenCalc, `casual-calc-*` | Accepted | The spreadsheet engine is named **OpenCalc**; crates are prefixed `casual-calc-`; it is the sibling of OpenDoc and reuses its format-neutral spine. |
| ADR-002 | Design-first, phased delivery with the calc engine held back | Accepted | Full architecture is designed up front; construction is phased; the formula/calc engine ships in Phase 2 but its seams are decided now. See [06](06-ROADMAP-AND-DELIVERY.md). |
| ADR-003 | Layer division is fixed before code | Accepted | The crate set and dependency DAG in [19](19-WORKSPACE-SCAFFOLD-DESIGN.md) are the contract; changing a crate boundary requires a new ADR. |
| ADR-004 | Sparse cell-grid model | Accepted | The workbook model stores cells sparsely (not a dense 2-D array) so a 1M-populated-cell sheet fits in bounded memory. See [22](22-NORMALIZED-SCHEMA.md). |
| ADR-005 | Reserved calc seams in the model | Accepted | The Phase 1A model carries formula ASTs, a cached value slot, and a dependency-edge side table from day one, so the Phase 2 calc engine adds no model-breaking fields. See [22](22-NORMALIZED-SCHEMA.md) §Reserved calc seams. |
| ADR-006 | Bounded OPC package substrate, shared with OpenDoc | Accepted | XLSX admission reuses the same bounded ZIP/OPC design (`casual-calc-package`) with explicit limits. See [21](21-PARSER-LIMITS.md), [28](28-XLSX-PACKAGE-READER.md). |
| ADR-007 | Loss-aware preservation via dual-axis disposition | Accepted | Import records a model outcome and a retention outcome per construct; a retention mode gives a byte-identical floor; unknown parts are kept in an opaque side table. See [34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md). |
| ADR-008 | Backend-neutral display list + CPU raster backend | Accepted | Layout emits a serializable display list; `casual-calc-render` executes it on `tiny-skia` with `skrifa` glyph outlines. The grid paints cell tiles as display-list items. See [42](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md). |
| ADR-009 | Viewport virtualization is a model/layout concern, not a UI hack | Accepted | Layout and paint operate on the visible window only; the model supports O(visible) queries over a 1M-cell sheet. See [30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md), [42](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md). |
| ADR-010 | Deterministic, byte-stable snapshots | Accepted | The normalized model serializes to deterministic JSON with `deny_unknown_fields` + `skip_serializing_if`, so additive schema changes keep old golden snapshots byte-identical. |
| ADR-011 | Server-mediated operational transform, not a CRDT | Accepted | Concurrent editing reconciles by OT over the existing closed op set, with a server imposing the total order; document state is a **snapshot plus the ops since it**. Decided by the per-cell byte ceiling (ADR-004) — which CRDT metadata would consume in every single-user session — and by documents originating as `.xlsx` snapshots with no causal history to merge. One editor needs no server at all; sessions run on serverless compute. Presence is a separate ephemeral channel, never transformed. Costs bounded-offline and no peer-to-peer, both accepted. See [56](56-COLLABORATION-CONCURRENCY-DESIGN.md). |

| ADR-012 | Collaboration server: a session, not a filing system | Accepted | The server coordinates co-editing and carries auth, admin, telemetry and configuration; the integrator keeps the document of record and receives finished bytes through a **webhook callback**. OnlyOffice's shape, not Excalidraw's — a relay that sees only ciphertext cannot transform, so that path would reopen ADR-011 rather than deploy it. A session starts from the **original file**, never a model snapshot, or ADR-007's retained parts are silently dropped. **A cluster of interchangeable nodes**: a client connects to any of them, one node **leads a given document** and the rest relay to it so the hot path stays in memory, and the leader's appends are **conditional on the revision** so a stale leader is rejected rather than believed — leadership is an optimisation for latency, the conditional append is what makes divergence impossible. Leadership is internal and moves on its own; it is not a sticky session, which is affinity a *client* is subject to. Kafka's topology per document: the leader replicates to **in-sync replicas** so failover is a promotion rather than a rehydrate, acknowledges only *after* replication, refuses writes when redundancy drops below the minimum, and **never** promotes an out-of-sync replica. A leader **epoch** fences zombies, which is what makes a cheap TTL lease safe. **Standalone needs no Redis at all** — one process, leader by definition; a cluster adds a lease and pub/sub, where Redis provides liveness and the epoch provides correctness. The **token is the whole integration contract** — it carries where to fetch and where to send back, so the server holds a signing secret per tenant and no per-document state at all. Clients receive the model snapshot and do not save in a session; the server does, holding both the ordered document and its retained parts. The server is a *host*, not an engine layer: it lives under `server/` and nothing in `crates/` may depend on it. See [57](57-COLLABORATION-SERVER-BOUNDARY.md). |

## Pending / to be written

- ADR for the **dual-host capability trait** (no `#[cfg]` forks in the engine;
  threads/clock/parallelism host-supplied) — promised by
  [19](19-WORKSPACE-SCAFFOLD-DESIGN.md) boundary invariant 7,
  [02](02-ARCHITECTURE.md) §Host targets, and [44](44-TAURI-DESKTOP-SHELL-DESIGN.md);
  to be Accepted at Phase 0.
- ADR for the **edit / operation schema** (the closed op set, inverses, and the
  independently-versioned op wire format) — triggered by
  [24](24-TRANSACTION-AND-EDIT-SEMANTICS.md); to be Accepted before Phase 1A
  editing lands.
- ADR for the **dependency-graph representation** (cell-level vs block-level
  nodes, range handling) — proposed in [40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md),
  to be Accepted before Phase 2.
- ADR for the **recalculation ordering strategy** (topological + dirty
  propagation vs Excel's calc-chain replay).
- ADR for the **cell-store constants** (block height, second-axis blocking,
  `Cell` byte ceiling) — the *shape* is Accepted (ADR-004); the *constants* are
  benchmarked and pinned at Phase 0, see [23](23-CELL-STORE-REPRESENTATION.md).
- ADR for **MSRV / toolchain pin** once the workspace is scaffolded.
