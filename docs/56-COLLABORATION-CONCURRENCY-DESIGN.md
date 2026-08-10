# 56 — Collaboration Concurrency: Operational Transform vs CRDT

**Status: Accepted** (ADR-011). Resolves the open decision recorded in
[24](24-TRANSACTION-AND-EDIT-SEMANTICS.md) §Open decisions.

> **Decision.** Concurrent editing is reconciled by **operational transform
> over the existing closed op set, with a server imposing the total order**.
> Not a CRDT, and not peer-to-peer.
>
> A document's state is **a snapshot plus the ops since it**, which is what
> makes the server cheap enough to run on serverless compute and what bounds
> the history a late or long-absent client has to be reconciled against.
>
> **Single-user needs no server and never has.** OT is dormant at one editor:
> nothing is contacted, no revision log exists, no transform runs, and no
> per-cell metadata is carried. Collaboration is something a document session
> *acquires*, not a mode the engine is built in.

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
| A **closed op set** — 17 variants plus `Batch`, serializable, invertible, `apply` pure and deterministic | [24](24-TRANSACTION-AND-EDIT-SEMANTICS.md), `casual-calc-transaction` | OT needs a transform function per interacting op *pair*. That is only tractable when the set is small, fixed, and cannot grow behind your back. |
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

## The snapshot model

A document at revision *N* is **a snapshot at revision *M* ≤ *N*, plus the ops
`M+1..N`**. This is not an optimisation bolted on afterwards; it is what makes
three otherwise-awkward things fall out cheaply, and it costs us almost nothing
because [ADR-010](08-ADR-REGISTER.md) already gives us a deterministic,
byte-stable snapshot of the normalized model. A snapshot is that, plus a
revision number.

- **The log does not grow without bound.** Compact on a revision interval or on
  quiesce, and retain ops back to the last snapshot plus a margin.
- **A late joiner gets a snapshot and a tail**, not the whole history. Joining
  a document that has been edited for a year costs the same as joining a fresh
  one.
- **Cold start is cheap**, which is what makes serverless compute viable
  (below). Rehydration is a snapshot read, not a replay.
- **Bounded offline has a defined edge.** A client whose base revision predates
  the oldest retained op cannot be transformed forward — there is nothing to
  transform against. It reloads from the current snapshot, and its unsent ops
  are **surfaced to the user, not silently dropped**. The retention window is
  the number that defines "bounded", and it is a deployment setting.

One property worth having on purpose: because snapshots are deterministic and
byte-stable, **a snapshot can be verified rather than trusted**. Replaying the
op log from an earlier snapshot must produce bytes identical to the stored
later one. That is a strong integrity check on the server's own state, and it
reuses the golden-file machinery the engine already has.

## Deployment: serverless, at both ends of the range

**One editor: no server.** Unchanged from today, and worth stating as a
commitment rather than an accident. The embeddable SDK runs the whole engine
client-side. Nothing about this decision introduces a backend into that path.

**Many editors: serverless compute, not a long-running VM.** OT requires
exactly one serialization point per document for the lifetime of a session —
that is the whole basis of the TP2 argument above. A single-writer actor
addressed by document id is precisely that shape, so a Cloudflare Durable
Object (or equivalent) is a natural fit rather than a compromise: the object
*is* the ordering authority. Sockets can hibernate; on wake the object
rehydrates from the snapshot.

The persistent state is object storage holding snapshot + op log; the compute
between sessions is ephemeral. A queue-plus-function deployment also works but
adds a hop and needs its own single-writer discipline, so it is the fallback,
not the target.

The important consequence: **the snapshot cadence is a latency knob, not just a
storage one.** Too sparse and every cold start replays a long tail.

## Where recalculation runs

**Rule: recalculation runs wherever the authoritative model is.** It is not a
separate deployment decision, and there is no mode where a host holds a model
it cannot evaluate.

| Situation | Model lives | Recalculates |
| --- | --- | --- |
| One editor, no server | the browser | **the browser**, in WebAssembly — the client is the authority |
| A co-editing session | the server orders, every participant holds a copy | **both**, and they agree |
| Desktop | the local process | **native Rust**, at full speed — no WebAssembly, no web app ([44](44-TAURI-DESKTOP-SHELL-DESIGN.md)) |

The middle row is the interesting one, and it is only tenable because
calculation is deterministic. Given the same model, the same engine version and
the same environment, every host computes the same values. So the server does
not recalculate *in order to tell clients the answers* — it recalculates
because it must write snapshots, serve headless export, and answer a client
that has no engine. Clients recalculate because they have to paint.

Two consequences follow, and they are the reason this is worth writing down
rather than leaving as an implementation detail.

**The wire carries ops, not values.** Every participant applies the same
ordered ops to the same base snapshot and arrives at the same cells, so
shipping computed values would be redundant bytes and a second source of truth
to disagree with. It also means the collaboration protocol is the op set and
nothing more — the same thing the single-user editor already speaks.

**The environment becomes server-issued and stamped per revision.** This is the
one thing that can break the guarantee above. `TODAY()`, `NOW()`, `RAND()` and
`RANDBETWEEN()` read `volatile_now` and `volatile_seed`, which the host supplies
and the engine never samples from a clock — a property the engine already has
([`Environment`](../crates/casual-calc-sdk), and gated by test). In a session
those values stop being the client's to choose: **the server stamps each
revision with the environment used to evaluate it**, so two clients recomputing
revision *N* get identical volatiles, and a new revision re-evaluates them as
Excel does.

The honest edge: a client evaluating its own not-yet-acknowledged ops has no
stamp yet and uses a provisional one, so a volatile cell can show a value that
changes when the revision lands. That is confined to volatiles, it is transient,
and the alternative — round-tripping to the server before showing the user
anything — is worse.

## What is hybridised, and what is not

Mixing reconciliation strategies is fine **partitioned by data domain** and
wrong **layered over the same data**. The line:

| Domain | Strategy | Why |
| --- | --- | --- |
| The document — cells, structure, styles, formulas | **OT**, server-ordered | One reconciliation system, one convergence argument, one place intention is decided. |
| Presence — cursors, selections, who is here | **Ephemeral last-writer-wins per client, with a TTL.** Never transformed, never persisted, never in a snapshot | It is not part of the document, nothing depends on its history, and losing it costs nothing. Sending it through transform would be pure overhead. This is what Yjs's awareness protocol is, and it is the right shape. |

What is deliberately **not** done is a CRDT layer over the cell grid alongside
OT. That gives two convergence arguments that must compose — which nobody can
reason about — and it reinstates the cost that decided this ADR in the first
place: the moment cells carry replica identity, the per-cell budget is spent in
every single-user session, which is most of them.

Comments and annotations are a plausible future third row in that table:
append-mostly, no structural interaction. Not decided here.

## What choosing A costs

Stated plainly, because these are real.

- **No true offline-first, by choice.** Bounded offline as described above.
  Confirmed acceptable: co-editing always goes through a server.
- **No peer-to-peer.** Two clients never reconcile directly with each other.
- **Transform correctness is subtle**, and this is the standard way OT ships
  broken. Mitigation is not care; it is a property-based convergence test —
  for generated concurrent pairs `(a, b)` over a shared state `S`, assert
  `apply(apply(S, a), transform(b, a)) == apply(apply(S, b), transform(a, b))`
  (TP1) and that both sides equal a golden expectation. The op set being closed
  and `apply` being pure is what makes exhaustive generation feasible.

**What would supersede this.** Exactly one thing: a requirement for two clients
to co-edit with no server between them — peer-to-peer, or a merge after
divergence long enough that the server no longer holds the history. That needs
TP2 or a CRDT, and it should be a new ADR rather than an extension of this one.
The hinge that keeps that door open is **the closed op set and the
deterministic snapshot format**, not any CRDT machinery built in advance. A
half-built CRDT would not ease that migration; it would start it from a worse
position.

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
  matrix's real content is *structural × everything* — four ops
  (`InsertRows`, `DeleteRows`, `InsertColumns`, `DeleteColumns`) against the
  rest — which bounds the work to a fraction of the 17×17 surface. `Batch`
  transforms elementwise.
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

## Settled, and by what

| Question | Answer |
| --- | --- |
| Is bounded offline acceptable? | **Yes.** Co-editing always goes through a server; there is no offline-merge requirement. This was the one answer that could have reversed the decision. |
| Peer-to-peer co-editing? | **No.** See *What would supersede this*. |
| Serverless? | **Both senses.** No server at one editor; serverless *compute* for sessions. |
| Presence through transform? | **No** — separate ephemeral channel. |
| Where does recalculation run? | **Wherever the authoritative model is** — browser when there is no server, both ends in a session, native on the desktop. Determinism is what makes that a free choice; the price is a server-issued, revision-stamped environment. |
| Presence in the first cut? | **Yes** — cheap, and its absence reads as broken rather than minimal. |
| Snapshot cadence? | **200 ops or on quiesce**, retaining two intervals. Tuned for cold-start latency, because a Durable Object hibernates after 10s. |
| Retained bytes under concurrent edit? | **Server-owned, never transformed**, invalidation idempotent, one writer at save. |
| Comments — presence or document? | **Document.** They persist and round-trip; ephemerality is what earns the presence channel. |

**One prerequisite came out of this research and is not optional:**
`SetSheetMetadata` is a 23-field whole-sheet bundle, so under OT almost every
concurrent non-cell edit silently discards the other. It needs a change mask
and stable ids for collection elements **before** collaboration is built. See
below.

## Resolving the scoping items

### Presence — in the first cut

Not because it is important, but because it is *cheap* and its absence is
loud. A session with no cursors does not read as a minimal collaboration
feature; it reads as a broken one, because the only evidence anyone else exists
is cells changing by themselves.

[Yjs's awareness protocol](https://docs.yjs.dev/getting-started/adding-awareness)
is the shape to copy, and it is deliberately not a document concern:

- A `Map<clientId, state>` where each client owns **exactly one entry** and
  overwrites it wholesale. No merge, therefore no transform.
- **Dropped after ~30 seconds without a refresh** — Yjs's number, which is
  roughly three missed heartbeats at a 10-second interval and is proven in
  practice. Adopt it rather than inventing one.
- Not persisted, not in a snapshot, not replayed. Losing it costs nothing,
  which is exactly why it is allowed to take the cheap path.

Yjs skips state vectors here on the reasoning that awareness states are small
enough that exchanging them whole has little cost. The same holds for us: a
cursor, a selection rectangle, a name and a colour.

Payload: `{ name, colour, sheet, selection, activeCell, ts }`. Refreshed on
change and on a heartbeat.

### Snapshot cadence — on a count *or* on quiesce, whichever comes first

[ShareDB's milestone snapshots default to every 1,000 versions](https://share.github.io/sharedb/adapters/milestone),
tunable per collection. That is the right shape and the wrong number for us,
for two reasons.

**Our ops are coarser.** A `Batch` paste is one op and can carry a hundred
thousand cells, so "1,000 ops" is a far larger span of change than it is for a
character-wise text OT. **And our snapshots are cheap** — deterministic
normalized JSON we already produce, not a bespoke serialization.

**Serverless changes the trade entirely.** A
[Durable Object hibernates after 10 seconds of inactivity, discarding in-memory
state, and re-runs its constructor on the next event](https://developers.cloudflare.com/durable-objects/concepts/durable-object-lifecycle/).
Cold start is therefore not a rare event to be amortised — for a document
edited intermittently it is *the normal case*. Every wake replays the tail. So
the cadence is tuned for **wake latency**, not for storage:

| Setting | Starting value | Why |
| --- | --- | --- |
| Snapshot after | **200 ops** | An order of magnitude under ShareDB's default, because our ops are coarse and our snapshots are cheap. Bounds the replay a cold start pays. |
| Snapshot on | **quiesce, before hibernation** | The highest-value moment there is: the very next event is guaranteed to be a cold start. The common pattern is to flush to durable storage and clear the accumulated partial updates. |
| Retain ops for | **2 snapshot intervals** | So a client that was away across one snapshot boundary is still transformable rather than forced to reload. This number *is* the definition of "bounded offline". |

These are starting points to be measured and revised, not derived. The metric
that matters is time-to-first-paint on a cold start, and it should be
benchmarked before the numbers are treated as settled.

### Retained bytes — server-owned, never transformed

The rule that makes this safe is that **retention is not in the op set**. Ops
carry cell and structure changes; they never carry drawing XML. The retention
side table is derived from the file and mutated only as a *side effect* of ops
that invalidate a part — deleting a chart, or editing an imported one, which
flips it from the retained-bytes regime to the model regime
([54](54-PIVOT-TABLES.md), CHT-01).

So:

- **The server owns the table.** Clients hold a read-only projection for
  rendering and never author it.
- **Invalidation is ordered like any other op**, because it is caused by one.
  The server applies it in server order.
- **Invalidation is idempotent.** Two clients deleting the same chart
  concurrently: the second is a no-op, not an error.
- **The dangling-anchor stripper runs server-side at save** (CHT-04), so there
  is exactly one writer of the drawing bytes. This is the whole point — two
  writers of a preserved part is precisely the "file needs repair" outcome the
  fidelity work exists to prevent.

### Comments — the document row, not presence

They are persisted, they survive the round-trip to `.xlsx`, and someone
expects them to be there tomorrow. Ephemerality is the only thing that earns a
place in the presence channel, and comments are not ephemeral.

## The finding that blocks implementation: `SetSheetMetadata`

Researching the above turned up something more important than any of it.

`SetSheetMetadata` installs a **23-field whole-sheet bundle**: merges, column
and row sizing, hidden lines, view, autofilter, outline levels, validations,
conditional formats, comments, visibility, protection, hyperlinks, tables,
**charts**, **pivots**, print setup and sort state. It exists for a good
single-user reason — a structural delete drops merges and freeze bands that
re-inserting an empty band cannot recover, so the inverse carries a
pre-mutation snapshot.

Under OT it is a **collision magnet**. One client resizes a column while
another adds a comment: two `SetSheetMetadata` ops, no way to merge them, and
the later one silently discards the earlier one's work. Almost every concurrent
*non-cell* edit in the product collides this way — comments, conditional
formats, filters, hyperlinks, table config, chart edits, pivot edits, print
setup. Cell editing would work beautifully and everything else would quietly
lose data, which is the worst possible failure shape for a product whose first
principle is no silent data loss.

Two stages fix it, and **both belong before collaboration, not during**:

1. **A change mask.** `SetSheetMetadata` records which fields it *intends* to
   change. Transform then merges disjoint field sets and conflicts only on the
   same field. The single-user path is unaffected. This alone removes the large
   majority of collisions, because concurrent editors are usually touching
   different things.
2. **Identity for collection elements.** `Table` and `PivotTable` already carry
   a stable `id`; charts, comments, validations, conditional formats and
   hyperlinks are **positional** — a `Vec` index. Index-keyed collections are
   exactly what breaks under concurrency, since two concurrent inserts both
   claim index 2. Giving these elements stable ids turns the field into a keyed
   set where inserts merge and only same-element edits conflict. This is a
   model change, which is why it wants doing first.

**A correction to this document.** Above, under *Design commitments*, it says
that needing an op the single-user editor lacks is evidence against the
decision. That is right about new *semantics* and wrong as stated: refining an
existing op's **granularity** — a field mask, or element identity within a
collection — adds no operation and no semantics. It makes an op say what it
already meant. The commitment is against widening what can be done, not against
describing it precisely.
