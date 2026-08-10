# 57 — The Collaboration Server: what it owns, and what it must not

**Status: Proposed** (ADR-012). Follows [ADR-011](56-COLLABORATION-CONCURRENCY-DESIGN.md),
which settled *how* concurrent edits reconcile. This settles *where the thing
that does it runs*, and what else it is allowed to know about.

> **Decision.** A collaboration server that is **an editing session, not a
> filing system**. It coordinates co-editing, and carries the concerns that
> only exist once a deployment exists — auth, admin, telemetry, configuration.
> The document of record stays with the integrator. The server hands finished
> bytes back through a **webhook callback**; it never becomes the place the
> file lives.
>
> **A cluster of interchangeable nodes.** A client connects to any of them. One
> node **leads a given document** and the rest relay to it, so the hot path
> stays in memory; the leader's appends are nevertheless **conditional on the
> revision**, so a stale leader is rejected rather than believed. Leadership is
> internal and moves on its own — it is not a sticky session, which is affinity
> a client is subject to.

## Two prior arts, and only one of them fits

The shape asked for was "the OnlyOffice path, or you could say the Excalidraw
path". Those are different systems, and the difference decides this document.

| | [Excalidraw](https://plus.excalidraw.com/blog/building-excalidraw-p2p-collaboration-feature) | [OnlyOffice](https://api.onlyoffice.com/docs/docs-api/get-started/how-it-works/saving-file/) |
| --- | --- | --- |
| Server role | relays **encrypted** messages between peers, "does no centralized coordination" | holds the document for the session, coordinates, compiles the result |
| Server sees the content | **no** — ciphertext only | yes |
| Storage | nothing server-side at all | nothing durable: the host downloads the finished file through `callbackUrl` |
| Reconciliation | client-side, last-writer-wins | server-ordered |

**The Excalidraw path cannot carry ADR-011.** Server-mediated operational
transform requires the server to *be the order* — to transform each incoming
operation against everything committed since its base. A relay that sees only
ciphertext cannot transform anything; that is precisely why Excalidraw
reconciles on the client instead. Choosing it would mean reopening the CRDT
decision, not deploying the one we made.

**The OnlyOffice path is what we have already built.** `ServerSession` plus
`Snapshot` *is* a document server: an ordering authority that holds the model
for a session's life and can assemble the file at the end. Nothing new is
needed to adopt it.

So: OnlyOffice's **shape**, with its storage model — the host owns the file,
the server hands it back — which is the part the request was really about.

## What the server owns

- **The order.** The single serialization point per document, per ADR-011.
- **The session's state**: the model, the revision log, the snapshots. Durable
  enough to survive a hibernation, not durable enough to be a system of record.
- **Authentication and authorization** — enforced, not decided. See below.
- **Admin, telemetry, configuration.** These exist only in a deployment, which
  is exactly why they live here and not in the SDK
  ([55](55-SDK-EMBEDDING-AND-INTEGRATION-DESIGN.md): the embeddable element
  ships with none, and that stays true).

## What the server must not own

- **The document of record.** It is not a file store, a version history, or a
  backup. It holds a working copy for as long as people are editing it.
- **Who may open what.** The host mints a signed token naming the document, the
  user, and their permission; the server verifies and enforces it. A server
  that decided access would need the host's whole permission model, and would
  be wrong about it.
- **Rendering, conversion, or a headless export service.** The engine can do
  those; bundling them here turns a session coordinator into a product with
  four jobs.

## The lifecycle

1. **Join.** The client presents a host-signed token. The server, on the first
   join, fetches the document from the URL the token names.
2. **Edit.** Ordinary ADR-011 traffic: operations in, transformed operations
   out, revisions counted, snapshots on the cadence from
   [`SnapshotPolicy`](../crates/casual-calc-transaction).
3. **Save points.** The server assembles `.xlsx` with the same engine — native
   Rust, the same writer the desktop uses — and **POSTs it to the host's
   callback**. On quiesce, on a revision interval, and when the last
   participant leaves.
4. **Idle.** State persists; compute may hibernate. Waking is a snapshot read.
5. **Close.** Final callback, then the session's state may be discarded.

### The window where work can be lost

Between save points. That is inherent to the host owning storage, and it is
worth stating rather than discovering: a server lost between callbacks costs
whatever was edited since the last one. The cadence is therefore a durability
setting, not only a performance one, and the default should be aggressive —
quiesce is a save point precisely because "everyone stopped typing" is the
cheapest moment to make the last minutes safe.

## The fidelity trap this design walks into

**A session must start from the original file, not from a model snapshot.**

[ADR-007](08-ADR-REGISTER.md) makes retained bytes authoritative for everything
the model does not represent — an unrecognised chart, a VBA part, `customXml`,
a drawing with shapes we do not model. That side table is built by *importing
the file*. A session that began from a model-only snapshot would be internally
consistent, converge perfectly, and write back a document with every preserved
part silently removed.

That is the exact failure the entire fidelity effort exists to prevent, and
collaboration is the one path that could reintroduce it — because it is the
only path where the document is reconstructed from something other than the
bytes it arrived as. So:

- the session's durable state is **the original bytes, the model snapshot, and
  the operation log** — not the snapshot alone;
- the callback assembles from the retention path exactly as a single-user save
  does;
- a session that cannot obtain the original bytes **refuses to start** rather
  than proceeding with a lossy copy.

## Webhooks now, WOPI later — and not foreclosed

Webhooks are perhaps a tenth of WOPI's surface and most of its value: a signed
POST carrying the finished document and the session's outcome. WOPI is a
protocol with locking, `CheckFileInfo`, `PutFile`, proof-key rotation and a
discovery document, and its real payoff is admission to the SharePoint and
Office ecosystems. That is a customer-driven reason, not an architectural one.

The design should therefore keep the seam where WOPI would attach — the
document is fetched through an interface, not a hardcoded HTTP call — so
adding it later is an implementation of that interface rather than a rewrite.

## Where the code lives

[ADR-003](08-ADR-REGISTER.md) fixes the crate DAG, so this needs saying
explicitly.

- **The protocol stays in the engine**, where it already is:
  `casual-calc-transaction::{transform, session}` is pure logic with no I/O,
  no clock and no transport, which is what let it be tested against thousands
  of interleavings without a socket.
- **The server is a host, not a layer.** AGENTS.md's rule — *the engine
  computes, the host decides I/O, network and persistence* — makes a network
  service the wrong thing to put inside the engine's dependency graph. It
  would drag an async runtime and an HTTP stack into a library that currently
  has neither.

Proposed: a workspace member under `server/`, alongside the existing `tools/`
convention, with one boundary rule — **nothing under `crates/` may depend on
it.** Versioned with the engine, excluded from the engine DAG, released on its
own `server-v*` tag ([15](15-CI-AND-RELEASE-GATES.md) §Release tags).

## Two deployments, one binary

**Standalone needs nothing.** One process, no Redis, no database, no broker. It
is the leader for every document by definition, so there is no election, no
epoch to check, no fan-out — every participant's socket is on the same machine.
Snapshots go to disk. This is the default, and it should stay genuinely
dependency-free: "lightweight" means someone can run it with one binary and a
directory, or the word means nothing.

**Cluster adds exactly two things**, and it is worth being precise about which
does what:

| Concern | Standalone | Cluster |
| --- | --- | --- |
| Who leads a document | this process | a lease in Redis, fenced by epoch |
| Reaching other participants | the same process | per-document pub/sub |
| Durable snapshots + log | local disk | shared object store or database |
| Replicas | none | in-sync followers holding the document |

**Redis is for liveness, not for correctness.** A lease in Redis can be
violated under failover or a partition — this is the well-worn Redlock
argument, and it is right. That is tolerable here *only* because the epoch
fence is what actually decides: a stale lease produces a zombie leader, and the
zombie is refused by replicas that have seen a newer epoch. Redis says who
*probably* leads; the epoch says who *does*. Putting correctness in Redis would
be a mistake; putting liveness there is exactly what it is good at.

The same binary runs both. Clustering is configuration, not a second product —
which is a discipline as much as a design: the coordination surface is a small
interface with an in-process implementation and a Redis one, so the standalone
path is the same code with the trivial implementation rather than a special
case that quietly rots while the cluster path gets the attention.

## Horizontal scale without sticky sessions

A hard requirement, and the one that shapes the rest: nodes must be
interchangeable behind a dumb load balancer. No affinity, no "route this user
back to the box that has their document".

This looks like it contradicts ADR-011, because operational transform needs
**one serialization point per document** — two nodes ordering the same document
concurrently diverge, and no amount of transform fixes that. The resolution is
to notice what the serialization point has to *be*:

> **The serialization point must be one, not one *node*.**

Make it the datastore, and the nodes stop mattering.

### A leader per document, and every other node relays to it

A client connects to **any** node — that is the requirement, and it is
satisfied at the load balancer, which stays dumb. Inside the cluster, one node
is **leader for a given document** and the others relay to it:

```text
client ──▶ any node ──(not leader? forward)──▶ leader for that document
                                                  │ commits, assigns the revision
                ◀── pushes to its own sockets ◀────┘ broadcasts to nodes holding participants
```

The leader keeps the document in memory, so a commit is an in-process call —
the `ServerSession::commit` that already exists — rather than a round trip to
storage per operation. That matters because operations are small and frequent:
paying a database write on every one puts the datastore on the typing path.

This is the shape OnlyOffice, Univer and Durable Objects all converge on, and
it is worth being clear that it does **not** reintroduce sticky sessions.
Stickiness is affinity a *client* is subject to — the thing that makes deploys,
restarts and rebalances painful. Leadership is internal, assigned by the
cluster, and moves on its own.

### Leadership makes it fast; the conditional append makes it safe

Leader election has one failure mode that matters: two nodes believing they
lead the same document, during a partition or a slow lease renewal. Two leaders
ordering the same document is divergence, and no transform recovers from it.

So the leader is **not** trusted to be unique. It still appends **conditionally
on the revision** — the compare-and-swap below — and a stale leader's append is
rejected by the store the moment a newer one has committed. It learns it is no
longer leader by being told *no*, and steps down.

That is the synthesis worth keeping:

> **Leadership is an optimisation for latency. The conditional append is what
> makes divergence impossible.** Neither alone is enough: election without the
> guard is correct only while the election is, and the guard without a leader
> puts a storage round trip on every keystroke.

Which also means election can be cheap and approximate — a lease in shared
state with a TTL — rather than a consensus protocol. Getting it briefly wrong
costs a rejected append and a re-elected leader, not a corrupted document.

### Replication: the leader keeps followers warm

A document is a partition and its operation log is the log, which is not an
analogy so much as a restatement — [COL-07](14-EXECUTION-TRACKER.md) already
built a per-document log with snapshots. So the Kafka topology maps directly:
the leader replicates each committed operation to a small set of **in-sync
replicas**, and a replica already holding the document can be promoted in
place, without the snapshot read and tail replay a cold node would need.

Three rules make that safe, and all three are places Kafka has been bitten.

**1. Acknowledge after replication, never before.** If the leader acknowledges
a client and then dies before the operation reaches a replica, the client
believes its work is saved and it is not. That is silent data loss, which this
project does not do. A commit is complete when it has reached the required
number of replicas — memory over a LAN, so cheaper than a database write, but
not free, and that cost is the price of the guarantee.

**2. Refuse writes when redundancy is gone.** The subtle one. In Kafka,
`acks=all` with the in-sync set collapsed to just the leader
[still acknowledges](https://www.conduktor.io/kafka/kafka-topic-configuration-unclean-leader-election) —
"all" means "all of the ones still here", which is one. The document is then a
single crash from losing acknowledged edits while reporting perfect health.
`min.insync.replicas ≥ 2` is what actually refuses the write. We take the same
setting and treat dropping below it as a **read-only document with an explicit
reason**, not as business as usual.

**3. Never promote an out-of-sync replica.** Kafka makes this configurable and
defaults it off, because turning it on means
[messages committed to the old leader but not replicated are gone forever](https://vedanthv.github.io/data-engg-docs/streaming/kafka/52-Kafka_Broker_Configuration_Unclean_Leader_Election/).
For a spreadsheet that is edits vanishing from under people who watched them
appear. We do not make it configurable: if no in-sync replica exists, the
document is recovered from its snapshot and log instead — slower, and it loses
nothing that was ever acknowledged.

### Fencing: the epoch, not the lease

Leadership changes hands, and the old leader does not always know. It may be
alive, unpartitioned from its clients, and still processing — a zombie.

Every leadership term carries an **epoch**, incremented on each election, and
every replicated append carries it. A replica that has seen epoch *n* refuses
anything from *n − 1*. So a zombie cannot reach the replicas it needs to
commit, which means it cannot acknowledge, which means no client is ever told
that work was saved when it was not. It discovers it has been deposed by being
refused.

This is what makes cheap election acceptable. The lease can be sloppy — a TTL
in a shared store, occasionally wrong under a partition — because the epoch is
what actually decides, and it is checked at the moment of every write.

### Placement is not election

Choosing a leader by spare capacity, latency to the participants, or locality
is a **placement** policy, and a good one. It is not an election.

- **Placement** proposes: this node *should* lead, on the evidence available.
  Advisory, approximate, and free to be wrong.
- **Election and fencing** decide: this node *does* lead, at epoch *n*, and
  everyone else will refuse the previous one. Authoritative.

Keeping them separate matters because merging them lets a load metric decide
correctness — a node that looks least busy wins a race it should have lost, and
two nodes proceed. Placement picks the candidate; the epoch settles the
outcome.

### What replication costs

Memory, per copy: the workbook, and the original bytes the retention path needs
(see the fidelity trap above — those are not optional). For a large workbook
that is not small, and three copies of a hot document is three times it.

The replication factor is therefore a dial between failover speed and memory,
not a value to fix once. Two in-sync copies gives instant promotion; one plus
snapshot recovery is cheaper and slower to fail over. Large documents may
rationally choose the latter.

### Failover, and the one thing the protocol still needs

When a leader dies its lease expires, another node takes the document, and
rehydrates from snapshot-plus-tail — which is exactly what
[COL-07](14-EXECUTION-TRACKER.md) built.

The client needs no special handling, because the protocol already holds
unacknowledged work: `ClientSession` keeps its sent chunk until it is
acknowledged, so a chunk lost with the old leader is simply still there to
send. That falls out of ADR-011's one-chunk-in-flight rule rather than being
designed for failover.

**But it exposes a real gap.** A leader can commit a chunk and die *before
acknowledging it*. The client, having had no acknowledgement, resends — and the
new leader has no way to tell a lost chunk from a duplicate one, so it applies
it twice. Typing "5" into a cell twice is invisible; inserting a row twice is
not.

`Submission` therefore needs an **idempotency key** — a client id and a
per-client sequence number — with the server recording the last sequence it
accepted from each client and treating anything at or below it as already
committed. This is a small change to a type that is already written, and it is
required before any of this runs, not optional hardening. Tracked as COL-09.

### The commit is a compare-and-swap

Every node runs the same `ServerSession::commit` we already have. What changes
is where the revision counter lives:

1. Read the document's current revision and the operations since the client's
   base — from cache, if the node has it.
2. Rebase the chunk (existing code, pure, no I/O).
3. Append at `revision + n` **conditionally**, on the document still being at
   `revision`.
4. If the condition fails, someone else committed first. Pull what they
   committed, rebase onto it, and retry.

The database decides the order. That is a real serialization point — it is
simply not a machine anybody has to route to. Any node may accept any
connection for any document, which is precisely the property being asked for.

Step 4 is not a special case: it is `transform` against a handful of newly
committed operations, which is what this layer does anyway. Contention is
per-document, and a document has tens of concurrent editors rather than
thousands, so a retry is cheap and rare.

### What the nodes hold

Caches, and nothing that cannot be rebuilt:

- the workbook, its revision, and the **original bytes** the retention path
  needs (see the fidelity trap above — this is not optional state);
- rehydrated from snapshot-plus-tail on a miss, which is what
  [COL-07](14-EXECUTION-TRACKER.md) exists to make cheap.

A failed compare-and-swap is also a cache-invalidation signal: it says the
node's copy is behind, and names how far.

### The part that actually costs something: fan-out

Without affinity, one document's participants are spread across nodes, so a
node that commits must tell nodes it does not know about. That needs a
per-document pub/sub channel — Redis, `LISTEN/NOTIFY`, NATS — and it is the
genuine price of dropping stickiness. It is worth paying: fan-out is a
well-understood, horizontally scalable problem, whereas session affinity is an
operational tax on every deploy, restart and rebalance for as long as the
product exists.

### Why not the alternatives

- **Sticky sessions** — rejected outright.
- **Leader election alone**, trusting the lease — correct exactly as long as
  the election is, and a partition is precisely when it is not. The conditional
  append costs one comparison and removes the failure mode entirely.
- **The conditional append alone**, with no leader — genuinely stateless and
  the simplest thing that works, but it puts a storage round trip on every
  commit, and commits happen at typing speed. Kept as the safety net rather
  than as the mechanism.
- **A single-writer actor per document** (Cloudflare Durable Objects) — the
  same topology with the platform supplying the leadership, which is a good fit
  and binds the deployment to one vendor. Nothing here forecloses it: a Durable
  Object is simply a leader whose election someone else operates.
- **A partitioned log** (Kafka) — correct and scalable, and a broker to operate
  for a workload that is one conditional write per commit.

### What has to be measured before this is believed

Compare-and-swap trades a round trip for statelessness, so the numbers that
matter are commit latency under contention, and the retry rate as concurrent
editors on a single document rise. Tens should be uneventful; the hundreds that
[Univer advertises](https://docs.univer.ai/blog/ot) would need proving rather
than assuming.

## Open questions

1. **Does the server ever serve the file to the client, or only the model?**
   Everyone in a session must start from the same revision, which argues the
   server hands out snapshot-plus-revision on join rather than letting each
   client fetch the file and start somewhere different.
2. **Save-point cadence defaults**, given the loss window above.
3. **Token shape.** JWT is the obvious choice and what OnlyOffice uses; what
   needs deciding is the claim set — document id, user id, permission, expiry,
   and whether the fetch URL is inside the token or resolved by the host.
4. **Presence identity.** The name and colour shown to other participants come
   from the token, not from the client, or a participant can claim to be
   someone else.
5. **What happens when the host's callback fails.** Retry with backoff is
   obvious; what is not is how long the server keeps trying before it has to
   tell the participants their work is not being saved.
