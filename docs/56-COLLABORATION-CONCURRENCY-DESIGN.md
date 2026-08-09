# 56 — Collaboration Concurrency: Operational Transform vs CRDT

**Status: Proposed** (ADR-011). Resolves the open decision recorded in
[24](24-TRANSACTION-AND-EDIT-SEMANTICS.md) §Open decisions. Nothing is built
until this is Accepted.

> **Decision.** Concurrent editing is reconciled by **operational transform
> over the existing closed op set, with a server imposing the total order**.
> Not a CRDT, and not peer-to-peer.

## What is being decided

Phase 5 adds multi-user editing. [24](24-TRANSACTION-AND-EDIT-SEMANTICS.md)
deliberately left *how* concurrent ops reconcile open, having settled that the
op set itself would not change to accommodate the answer. This document closes
that.

It does **not** change the embeddable SDK, which is single-user by decision
([55](55-SDK-EMBEDDING-AND-INTEGRATION-DESIGN.md)): `<opencalc-sheet>` is a
view-and-edit surface for one person, and collaboration is a separate
server-side product. That separation is an input to this decision, not a
consequence of it.

## What we already have, and what it constrains

Four existing commitments do most of the work here. None of them was made with
collaboration in mind, which is what makes them useful evidence rather than
motivated reasoning.

| Commitment | Where | Why it matters here |
| --- | --- | --- |
| A **closed op set** — 16 variants, serializable, invertible, `apply` pure and deterministic | [24](24-TRANSACTION-AND-EDIT-SEMANTICS.md), `casual-calc-transaction` | OT needs a transform function per interacting op *pair*. That is only tractable when the set is small, fixed, and cannot grow behind your back. |
| A **per-cell byte ceiling**, asserted by a memory benchmark | ADR-004, [23](23-CELL-STORE-REPRESENTATION.md) §The `Cell` record | Chosen so 1M populated cells fit a bounded budget ([30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md) T1). Any per-cell concurrency metadata is spent directly against it. |
| **Loss-aware retention**: for anything unmodelled, the *retained bytes* are authoritative | ADR-007, [34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md) | The document is not wholly described by the model, so it cannot be wholly described by a replicated data structure over the model. |
| **Excel is the semantics oracle** | [01](01-ORD.md), [12](12-COMPETITIVE-ANALYSIS.md) | Convergence is necessary and not sufficient. Two replicas agreeing on a result Excel would not have produced is still a bug. |

## The options

### A. Server-mediated operational transform — **chosen**

Each client holds the last server revision it has seen, a buffer of ops sent
but not yet acknowledged, and a buffer of local ops not yet sent. The server
holds the authoritative model and a revision log, and transforms each incoming
op against everything committed since the revision it was based on. This is the
[Google Wave](https://svn.apache.org/repos/asf/incubator/wave/whitepapers/operational-transform/operational-transform.html)
model, and what Google Sheets and [Univer](https://docs.univer.ai/blog/ot) both
run on.

### B. CRDT over the model

Make the workbook a conflict-free replicated data type — cells as a map of
registers, rows and columns as sequences — so that replicas converge without
coordination. Yjs, Automerge and Loro are the mature implementations.

### C. Peer-to-peer OT

The same transform functions, without a server to order them.

### D. Coarse locking

Lease a cell, a range, or a sheet to one editor at a time. Convergence becomes
trivial because concurrency is prevented rather than resolved.

## Why A

**1. The per-cell budget cannot absorb CRDT metadata.** This is the argument
that decides it. A cell is a compact fixed-shape record with a `size_of`
ceiling, chosen specifically so a million of them fit. CRDT elements carry a
unique id, causal ordering information, and tombstones for deletions — for text
CRDTs, measured at
[16–32 bytes per character](https://www.taskade.com/blog/ot-vs-crdt), and
around [23 bytes per character for Fugue](https://arxiv.org/pdf/2305.00583).
Even a coarse map-of-registers over *cells* rather than characters costs a
replica id, a counter, and a tombstone per cell, permanently, in every session
— including the overwhelming majority that have exactly one editor. OT's
concurrency state is **per session, not per cell**: a revision number and a
short queue of in-flight ops, which is zero bytes when nobody else is
connected.

**2. Our documents are files, not histories.** A CRDT merges replicas that
share a causal ancestor. An `.xlsx` has no causal history — it is a snapshot,
and it is the unit our users exchange. Two people opening the same file produce
two independent CRDT states with no common ancestor, so every cell reads as
concurrently created and a merge is meaningless. Making it work would mean
promoting the CRDT state to source of truth and demoting `.xlsx` to
import/export — which directly contradicts ADR-007, where retained bytes are
authoritative for everything the model does not represent. OT's unit is "a base
revision plus a sequence of operations", and *a file plus the edits made to it*
is exactly that.

**3. A server is already assumed, so TP2 is not our problem.** TP2 is the
transformation property that peer-to-peer OT requires; it is famously difficult
to get right and is most of OT's reputation for complexity. Practical OT
systems avoid it by having a server impose a canonical order — Wave requires a
client to await an acknowledgement before sending its next chunk, and it is
[exactly that global ordering that removes the TP2 obligation](https://svn.apache.org/repos/asf/incubator/wave/whitepapers/operational-transform/operational-transform.html).
Collaboration here is a server product regardless. The cost is already paid,
and it buys away the hard half of the algorithm.

**4. Structural ops are the whole problem, and OT addresses them directly.**
Inserting a row shifts every position below it and rewrites the formula
references that cross it. `casual-calc-transaction` already implements that
rewriting for the single-user case, and `transform(op, InsertRows)` is the same
arithmetic applied to a *pending op* instead of to the model — reuse, not new
machinery. The CRDT equivalent requires row identity to become an opaque,
densely-insertable ordered token rather than an integer index, which reaches
into the model, the layout engine, the A1/R1C1 reference algebra and the
writer. That is a rewrite of the parts of the system that are currently most
correct.

**5. Intention, not just convergence.** The cases that matter in a spreadsheet
— I insert a row while you edit a formula that spans it — are questions about
what the two people *meant*, and transform functions are where that judgement
gets written down and tested. A CRDT would answer them by whatever its merge
rule happens to do.

## What choosing A costs

Stated plainly, because these are real and a future ADR may reverse this one.

- **No true offline-first.** A client that diverges for a long time must be
  transformed against the whole history since it left, so the server must
  retain that history. We choose **bounded offline**: history is retained for a
  configured number of revisions, and a client further behind than that
  reloads from a snapshot and loses nothing except its unsent ops, which it is
  told about rather than silently dropping.
- **No peer-to-peer or serverless deployment.** The server is required and
  stateful.
- **Transform correctness is subtle**, and this is the standard way OT ships
  broken. Mitigation is not care; it is a property-based convergence test —
  for generated concurrent pairs `(a, b)` over a shared state `S`, assert
  `apply(apply(S, a), transform(b, a)) == apply(apply(S, b), transform(a, b))`
  (TP1) and that both sides equal a golden expectation. The op set being closed
  and `apply` being pure is what makes exhaustive generation feasible.

If genuine offline-first or serverless operation becomes a requirement, this
decision should be **superseded, not stretched** — a CRDT bolted onto an OT
core is worse than either.

## Why not the others

**B, CRDT** — the four points above, principally the per-cell budget and the
file-not-history problem. Worth being fair to it: CRDTs would give us offline
editing and no required server, and their reputation for merge quality in text
is deserved. Neither advantage pays for tens of megabytes of per-cell metadata
in single-user sessions, which is the common case.

**C, peer-to-peer OT** — TP2, for no benefit we need, given a server exists
anyway.

**D, locking** — rejected as the *product*, kept as a possible *interim*. The
bar is "a viable alternative to Excel", and both Excel Online and Google Sheets
allow genuine simultaneous editing; a product that says "someone else is
editing this row" is visibly lesser. But it converges trivially and is honest
about what it does, so if collaboration must ship before the transform matrix
is proven, range leases are the way to do that without pretending.

## Design commitments this implies

These follow from the decision and are what the implementation is held to.

- **The op set does not change.** If reconciliation needs an op the single-user
  editor does not have, that is evidence the decision is wrong, not a licence
  to widen the set.
- **The server owns the order.** Clients send a chunk, await acknowledgement,
  then send the next — Wave's rule, and what removes TP2.
- **`transform` must satisfy TP1** and is tested as a property, not by example.
- **Most pairs commute trivially.** Different sheets, disjoint ranges. The
  matrix's real content is *structural × everything*, which bounds the work to
  a fraction of the 16×16 surface.
- **Concurrent writes to one cell are last-writer-wins in server order**, not a
  character-level merge of the cell's contents. Excel and Sheets both resolve
  at cell granularity; matching them is the point.
- **Undo stays intention-preserving**, which is the known-hard corner: undoing
  your own op means transforming its inverse against everything committed
  since. The inverses already exist; the transform is the new part.
- **Calculation stays deterministic and host-supplied** — the environment
  (clock, seed) comes from the session ([`SessionConfig`](../crates/casual-calc-sdk)),
  so a server and a client recalculating the same revision agree. That is what
  makes it a free choice where recalculation runs.

## Open questions — to answer before implementation

1. **Is bounded offline acceptable?** If the product needs a week-long
   disconnect to merge cleanly, that changes the answer and this ADR should be
   reconsidered rather than implemented.
2. **Presence — cursors, selections, who-is-here — in the first cut, or after?**
   It is a separate channel and does not go through transform, but it is most
   of what makes collaboration *feel* present.
3. **Where does recalculation run?** Determinism makes server, client, or both
   equally correct; this is a cost and latency question, not a correctness one.
4. **Revision-log compaction.** How often a snapshot is written, and how many
   revisions are retained — the number that defines "bounded" above.
5. **What happens to the retained-bytes side table under concurrent edit?** Two
   clients editing a sheet whose drawing part is preserved verbatim must not
   both rewrite it. Likely answer: retention is server-owned and never
   transformed, but it needs stating.
