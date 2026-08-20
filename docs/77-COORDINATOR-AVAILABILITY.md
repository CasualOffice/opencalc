# 77 — Surviving the coordinator: sentinel, cluster mode, and what a node may do without Redis

**Status: Proposed** (triggers **ADR-020**). Written for
[`DEP-13`](14-EXECUTION-TRACKER.md) — *"Redis itself is still a single
unreplicated box"* — which is three questions wearing one row.

> **Proposed decision.** Replication and failover of the coordinator are
> **Sentinel**, not Cluster mode: this system needs one available logical
> primary, never sharding, and Cluster charges a keyspace migration for
> availability it does not actually provide. A node **may not** hold ordering
> for a document through a coordinator outage, however briefly and however
> certain it is that it owns the document — ownership is a fact about the store,
> not about the node. And a Sentinel failover **can still lose an acknowledged
> append**, because Redis replication is asynchronous; that is the residual
> risk, it is named rather than papered over, and `min-replicas-to-write` is how
> a deployment converts it from silent loss into a visible refusal.

## Outcome

Three separable things sit in `DEP-13`, and they are not the same size.

| | Question | Status |
| --- | --- | --- |
| 1 | TLS on the coordinator link | **Done** — shipped under `DEP-13`, no ADR needed |
| 2 | Sentinel or Cluster mode | **Proposed here.** ADR-020, not yet built |
| 3 | May a node hold ordering through a brief outage? | **Answered here: no.** The code already refuses; what shipped is the reasoning, a tightened fence and a link that re-dials |

What shipped under `DEP-13` without needing ADR-020, because none of it changes
what the cluster promises:

- **The coordinator link can be encrypted.** `rediss://`, with
  `OPENCALC_REDIS_CA` for a private CA and
  `OPENCALC_REDIS_CLIENT_CERT`/`_KEY` for mutual TLS. Certificates configured
  against a `redis://` URL are **refused** at startup rather than started in
  clear, and `#insecure` is refused by name. A plaintext link is warned about
  once at startup, the way `Exposure::warnings` treats a plaintext listener.
- **The link re-dials.** The connection was a `MultiplexedConnection`, which does
  not: when the socket died the task driving it ended, and every later command
  on it — and on every clone, which is what the coordinator hands out — failed
  for the life of the process. A Redis that restarted therefore cost a restart
  of *every node in the cluster*, which was most of what "a single Redis failure
  still stops ordering cluster-wide" meant in practice. Subscriptions had the
  same defect and a quieter symptom: the message stream simply ends, which is
  indistinguishable from "nothing more was published", so the per-document
  attendant broke out of its loop and that document silently stopped renewing
  its lease and reading its inbox while the node went on serving it.
- **The fence is an equality.** `epoch ~= held.epoch`, not `epoch < held.epoch`.
  See §3.

## Research

The Redis behaviours this rests on, by name rather than by link, because they
are stable properties of the product and should be re-checked against the
version a deployment actually pins:

- **Replication is asynchronous.** A primary acknowledges a write before any
  replica has it. Redis's replication documentation says so directly, and the
  Sentinel documentation states that Redis and Sentinel are "fundamentally not
  able to guarantee strong consistency".
- **`WAIT numreplicas timeout`** blocks until *n* replicas have acknowledged the
  writes issued so far. Redis's own documentation is explicit that this does
  **not** make Redis a strongly consistent store: a failover can still promote a
  replica that did not have the write.
- **`min-replicas-to-write` / `min-replicas-max-lag`** make a primary stop
  accepting writes when fewer than *n* replicas are within the lag window.
- **Cluster mode** partitions the keyspace into 16384 hash slots. A multi-key
  command — including a Lua script naming several `KEYS` — is refused unless
  every key is in the same slot; a `{tag}` in the key name forces co-location.
- **Cluster pub/sub** broadcast messages across the cluster bus to every node
  until Redis 7.0 added *sharded* pub/sub (`SPUBLISH`/`SSUBSCRIBE`), which is
  scoped to one slot and so needs the same hash tags.
- **`redis` 0.27 (redis-rs)** exposes Sentinel behind a `sentinel` feature
  (`SentinelClient`) and Cluster behind `cluster` / `cluster-async`. The async
  cluster client's pub/sub surface is thinner than the single-node one.

## Design

### What this system asks of Redis, exactly

Three things, and the scaling properties of all three are unremarkable:

1. **Mutual exclusion per document** — the lease, with the epoch as its fence.
   One claim per document every few seconds, per node holding it.
2. **A linearizable conditional append per document** — the log, `after` acting
   as a compare-and-set on the revision. One append per flush window per
   document.
3. **Best-effort fan-out** — pub/sub. Fire-and-forget by design: the log is the
   record and the channel is a prompt (ADR-017).

Every one of those is scoped to a single document, and a document's coordination
traffic is tiny. **Nothing here needs sharding.** That single observation
decides §2.

### 2. Sentinel, not Cluster mode

Cluster mode is a sharding answer to a scaling question this system does not
ask, and it charges for the answer in three places:

- **The append script names two keys** — `…:lease:<document>` and
  `…:log:<document>`. Cross-slot multi-key scripts are refused, so both would
  have to carry a hash tag (`…:lease:{<document>}`), and so would every channel
  name if sharded pub/sub were used. That is a keyspace migration and a
  permanent constraint on key naming, paid for something this system does not
  want.
- **Pub/sub gets worse, not better.** Unsharded, a publication is broadcast over
  the cluster bus to every node, so fan-out cost grows with the cluster —
  scaling *out* makes the relay *more* expensive. Sharded pub/sub avoids that
  and brings the hash-tag constraint back with it.
- **The client is a different client.** `redis-rs`'s async cluster type is not
  the multiplexed connection this code is built on, and its pub/sub surface is
  thinner. That is work, and work spent on a shape that has to be maintained
  alongside the single-node one, because standalone and small deployments will
  never run a cluster.

And the decisive point, which is easy to miss because "cluster" sounds like the
more available option: **Cluster mode does not make a failover safe either.**
Each shard is a primary with asynchronous replicas and its own automatic
failover, with the same loss window described in §2b. The failure `DEP-13`
describes is not "one Redis is too small for the load"; it is "one Redis going
away stops ordering". A shard's primary going away stops ordering for every
document in that shard, in exactly the same way, for exactly the same reason.

Sentinel keeps everything about the client identical — one logical primary,
multi-key scripts unchanged, ordinary pub/sub unchanged, the same
`MultiplexedConnection`/`ConnectionManager` shape — and adds precisely the thing
that is missing: something that promotes a replica when the primary goes, and
tells clients where the primary is now.

**Proposed: Sentinel.** Cluster mode is recorded here as the thing to revisit
*if sharding ever becomes the question*, which it is not; and if it does, the
hash-tag migration above is the price, known in advance.

The shape it would take:

- `OPENCALC_REDIS_URL` gains a sentinel form —
  `redis+sentinel://host1:26379,host2:26379/<service-name>` is the convention
  `redis-rs` and most clients use — resolved through `SentinelClient` to
  whichever node is currently primary, and **re-resolved on a connection
  failure** rather than only at startup. The re-dial delivered under `DEP-13` is
  the hook that goes in: today it re-dials the same address, and against
  Sentinel it would re-ask which address.
- Everything below `Coordinator` is unchanged. That is the point of the trait,
  and it is why this is a contained change rather than a rewrite.

### 2b. What a failover costs, and why it needs an ADR rather than a commit

Redis replication is asynchronous. A primary acknowledges a write before any
replica has it, so a promoted replica can be missing writes the old primary
accepted. Three consequences here, in the order they matter:

- **An acknowledged append can disappear.** ADR-014 §4 makes exactly one
  durability promise — *"an operation is written to the log before the client is
  told it was accepted"* — and that promise is only as strong as the log. If a
  failover loses the last entries, participants hold operations the cluster does
  not, on screens that show them as saved. That is "silent data loss with a
  receipt", which is the phrase ADR-014 uses for the thing it refuses.
- **A lease epoch can rewind.** The store then remembers an older generation
  than a live leader is carrying. This is what §3's equality fence catches, and
  it is caught rather than prevented.
- **A revision can rewind.** The conditional append still refuses two writers at
  one revision, but it cannot un-tell the clients who were already given a
  revision the log no longer has.

So adopting Sentinel does not merely add infrastructure: it changes what *"the
log said yes"* means. That is an ordering-and-durability guarantee, which is
why this is ADR-020 and not a pull request.

Two narrowings, both configuration, both with a cost worth stating:

- **`min-replicas-to-write 1` (with `min-replicas-max-lag`) on the primary.** The
  primary refuses writes when redundancy is below the threshold, so the loss
  window becomes a visible refusal — which the node already reports to the
  client as `Refused { NotSaving }` and to the orchestrator as a 503 on
  `/readyz`. ADR-012 already describes this mechanism ("a minimum in-sync count
  enforced separately"), and ADR-014 §4 already explains why it is not the
  default: refusing writes is a visible outage, and most integrators would
  rather have the log. For a deployment that wants ADR-014's promise to hold
  across a failover, **this is the setting that makes it hold**, and it should
  be the documented recommendation rather than a footnote.
- **`WAIT n timeout` between the append and the acknowledgement.** Converts
  "probably replicated" into "at least *n* replicas said so", at one more round
  trip on the edit path. It is not consensus and Redis says so; it narrows the
  window rather than closing it. Worth pricing in ADR-020; not obviously worth
  paying for on top of `min-replicas-to-write`.

Neither makes the coordinator CP. The honest one-line summary, which ADR-020
should state in as many words:

> **Sentinel makes the coordinator survivable. It does not make the log
> durable.**

A deployment that needs the log to survive a failover with no window needs a
consensus log. ADR-014 rejected in-process Raft for reasons that still hold — it
makes this a consensus system somebody has to operate and debug — but the
alternative it did not price is a **Raft-backed store behind the same
`Coordinator` trait** (etcd, or a Redis-compatible CP store). The trait exists
so that is a swap rather than a rewrite, and ADR-020 should say whether that is
a supported second backend or a road not taken.

### 3. A node may not hold ordering through a coordinator outage

This is the subtle question in `DEP-13` and the answer is a flat no — not "no
for long outages", not "no unless the node is sure". The reason has nothing to
do with duration.

**Ordering here is not a decision a node makes. It is the conditional append.**
The append assigns the revision *and* records it, atomically, fenced by the
epoch. A node cut off from the store can still `commit` into its own copy — that
is the bug shape, not a capability — but it cannot record, and it cannot know
whether its lease is still the current one.

A node that kept acknowledging would be wrong in a way nothing can repair:

1. **Its lease has a deadline in the store's terms, and the store's clock did not
   stop when the node's view of it did.** Once `expires_ms` passes, another node
   may take the document: the claim is atomic and needs nothing from the
   departed node's opinion, which is the entire point of "who decides the leader
   is down: nobody". Two nodes acknowledging different operations at the same
   revisions is divergence, and OT has no join.
2. **Even alone, the acknowledgements are unbacked.** A client told its edit
   landed stops resending it. If the node then dies — or is fenced when the
   store returns — that edit is gone from a screen that shows it as saved.

The tempting middle ground is "hold ordering only for documents this node
*already owns*", and it is precisely the unsafe one, because **ownership is a
fact about the store, not about the node.** Holding it requires a promise that
the document will not be handed elsewhere, and only the store can make that
promise, and only while it is reachable. A node that assumes it on the store's
behalf is what makes split brain; split brain in an OT system is divergent
documents, which is the one failure this project cannot have.

So what is safe during an outage — and is what the code does today:

- **Keep serving what is already open**: reads, presence, cursors. None of it
  needs the store.
- **Keep the lease belief until `expires_ms` and no further.** `leads()` already
  checks the expiry against the local clock, so a node stops considering itself
  the leader on schedule without needing to be told.
- **Refuse the write.** `order()` reports `Refused { seq, reason: NotSaving }` to
  the submitter, and `/readyz` answers 503 so the node is drained rather than
  restarted — `DEP-04`, which made the loss visible. That row's work is what
  makes "stop" an acceptable answer instead of a hang.

`DEP-13`'s contribution to this question is therefore not a new behaviour but
three things that make the existing behaviour hold up:

- **the equality fence** (§3a), so a coordinator that rewinds is refused rather
  than believed;
- **the re-dial**, so "the coordinator came back" stops meaning "restart the
  cluster";
- **a bounded retry budget**, so a coordinator that is gone for good still
  produces an *answer* — an unbounded wait would turn `DEP-04`'s prompt 503 into
  a probe that hangs, which is the same blindness in a new place.

**One knob an operator already has**, recorded so it is not rediscovered as a
fix: raising `OPENCALC_LEASE_MS` widens the window in which a node is certain of
ownership while the store is away, at the cost of that much delay before another
node may take over one that is genuinely dead. It trades failover latency for
outage tolerance. It does not change the rule above.

#### 3a. The fence is an equality, and that is a change worth naming

`Coordinator::append` used to refuse an appender whose epoch was **less than**
the store's. It now refuses any epoch that is not **equal** to it.

An appender *ahead* of the store looks impossible, because epochs only rise
while one store keeps its memory — and it is exactly what a failover produces:
the promoted replica is missing the claim that raised the epoch, so the store
remembers an older generation than a live leader is carrying. Under `<`, that
leader was believed. So was whoever the rewound store thinks holds the lease,
since their lower epoch is not less than itself either. Two live leaders, each
of which commits into its own copy *before* appending, and the one that loses the
conditional append is diverged from the log permanently, with no resync built to
recover it.

The change refuses strictly more than before and accepts nothing new, so it
cannot introduce divergence — only a refusal, which is reported. It does **not**
make a failover safe; it makes the unsafe case visible instead of silent, which
is the same move `DEP-04` made one layer up.

### Model / schema impact

None. The wire protocol, the op model and the snapshot format are untouched.
`PROTOCOL_VERSION` does not move.

### Layers touched & seams

`server/casual-calc-collab-server/src/cluster/` only, and the `Coordinator`
trait's *shape* is unchanged — the equality fence tightens what `append`
accepts, and both implementations hold it identically, which the `contract!`
suite enforces. `crates/` is untouched, so ADR-012's boundary is intact.

The Sentinel work proposed in §2 is contained to `cluster/redis.rs` and one
configuration variable. Nothing above `Coordinator` needs to know.

### Failure modes & limits

- **A resubscribe loses whatever was published during the gap.** Acceptable for
  the reason the channel was fire-and-forget to begin with: the log is the
  record, and `catch_up` reads from where the node actually is on every lease
  tick. It costs latency, not correctness.
- **The command retry budget is bounded** (four attempts, under two seconds).
  Deliberately: an unbounded wait is a hanging `/readyz`. A coordinator that is
  away for longer than the budget is reported as unreachable on each attempt,
  which is an answer, and the next attempt is a few seconds later on the renewal
  tick.
- **The TLS end-to-end test needs a `redis-server` on the path built with TLS**,
  and CI's coordinator is a service container without one. That half of the
  coverage therefore skips in CI and says so. Closing it means adding
  `redis-server` to the test runner — a new row, not a silent gap.
- **Nothing here is exercised against a real failover**, because nothing here
  performs one. The Sentinel work in §2 is what a failover test would be for,
  and its acceptance gate is below.

## Alternatives considered

- **Redis Cluster mode.** §2. Sharding this system does not need, in exchange
  for a keyspace migration, a worse fan-out story and a second client shape —
  and it does not make a failover any safer.
- **Keep one Redis and accept it.** The status quo, and it is not unreasonable
  for a single-tenant deployment: `DEP-04` already makes the loss visible and
  safe. It is rejected as the *only* option because "the coordinator is a single
  point of failure" is not something a production deployment can be told it must
  live with, and because most of the pain — a restart of every node after a
  Redis restart — turned out to be a client defect rather than a topology one,
  and is now fixed.
- **In-process Raft.** Rejected by ADR-014 and still rejected: it makes this a
  consensus system somebody has to operate and debug.
- **A Raft-backed store behind `Coordinator` (etcd, or a CP Redis-compatible
  store).** The only option that actually closes the durability window. Not
  proposed as the default — it is a second dependency for most deployments to
  run — but ADR-020 should say explicitly whether it is a supported backend or a
  road not taken, because the trait makes it cheap and silence here is how it
  gets rediscovered.
- **Dual-writing the log to two independent Redises from the node.** Rejected
  outright: two logs is two orders, and reconciling them is the consensus
  problem with extra steps.

## Acceptance gates (tests / fixtures)

Shipped with `DEP-13`, in `server/casual-calc-collab-server/src/cluster/tests.rs`:

| Test | What it establishes |
| --- | --- |
| `a_secured_coordinator_url_is_one_this_build_can_dial` | `rediss://` is not refused at parse time as an uncompiled feature. Always runs. |
| `the_coordinator_link_can_be_encrypted_against_a_private_ca` | A real `redis-server` over TLS: claim, append and read-back across a handshake against a CA supplied by configuration. |
| `an_untrusted_coordinator_certificate_is_refused` | The certificate is *checked* — without which the test above proves only that a handshake completed. |
| `certificates_configured_against_a_plaintext_coordinator_url_are_refused` | The misconfiguration that works and carries everything in clear. |
| `a_coordinator_url_that_turns_verification_off_is_refused_by_name` | `#insecure` is refused with the setting that replaces it. |
| `a_plaintext_coordinator_link_is_warned_about` | Said once at startup, and without printing the password it is warning about. |
| `the_link_recovers_when_the_coordinator_connection_is_lost` | **The row's gate.** The connection is severed under a live store; ordering resumes. |
| `a_subscription_survives_the_coordinator_going_away` | The same for the second socket, which fails silently. |
| `a_coordinator_that_never_returns_produces_an_error_rather_than_a_hang` | The retry budget is bounded, so `/readyz` still answers. |
| `an_append_from_ahead_of_the_stores_epoch_is_refused` (both backends) | The equality fence. |

Required before ADR-020 can be Accepted, and **not** built here:

- A Sentinel harness — one primary, one replica, three sentinels — that kills the
  primary and asserts that a node whose document was mid-edit resumes ordering
  after promotion, with no revision seen by a client absent from the log
  afterwards.
- The same harness with `min-replicas-to-write` set, asserting that a write
  during a collapsed in-sync set is **refused** rather than accepted and lost.

## ADRs triggered

**ADR-020 — coordinator replication and what a failover may lose.** Triggered on
two counts: a dependency choice (Sentinel, and whether a CP store is a supported
second backend), and the cluster's ordering/durability guarantee, since a
failover changes what a successful append means.

## Tracker IDs

- `DEP-13` — the row this was written for. Items 1 and 3 are delivered; item 2 is
  proposed here and is not built.
- Proposed new rows, so that nothing lives only in this document:
  - **Sentinel support and ADR-020** — the work in §2, gated by the harness
    above.
  - **`redis-server` on the CI runner** — so the TLS half of the coordinator-link
    suite stops skipping.
  - **A leader that is fenced has no resync** — `order()` commits into the local
    session *before* appending, so a `Fenced` or `Stale` refusal leaves that node
    diverged from the log with only a log line to say so. Named in
    `net.rs` as "the recovery is a resync, which is not built"; the equality
    fence makes it reachable more often, which is an argument for building it.
