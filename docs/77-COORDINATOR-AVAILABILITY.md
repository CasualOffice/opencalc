# 77 — Surviving the coordinator: sentinel, cluster mode, and what a node may do without Redis

**Status: Accepted as ADR-020, and built.** Written for
[`DEP-13`](14-EXECUTION-TRACKER.md) — *"Redis itself is still a single
unreplicated box"* — which is three questions wearing one row.

> **ADR-020 — Accepted.** Replication and failover of the coordinator are
> **Sentinel**, not Cluster mode: this system needs one available logical
> primary, never sharding, and Cluster charges a keyspace migration for
> availability it does not actually provide. A node **may not** hold ordering
> for a document through a coordinator outage, however briefly and however
> certain it is that it owns the document — ownership is a fact about the store,
> not about the node. And a Sentinel failover **can still lose an acknowledged
> append**, because Redis replication is asynchronous; that is the residual
> risk, it is named rather than papered over, and `min-replicas-to-write` is how
> a deployment converts it from silent loss into a visible refusal. That last
> clause is not advice: `OPENCALC_REDIS_MIN_REPLICAS` makes the node **check**
> the setting is really there, at startup and after every failover, and refuse a
> primary that is below it.

## Outcome

Three separable things sit in `DEP-13`, and they are not the same size.

| | Question | Status |
| --- | --- | --- |
| 1 | TLS on the coordinator link | **Done** — shipped under `DEP-13`, no ADR needed |
| 2 | Sentinel or Cluster mode | **Decided and built.** ADR-020: Sentinel |
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

## The decision, in one place

**Accepted, and what was built for it:**

1. **Sentinel, not Cluster mode.** `OPENCALC_REDIS_URL` gains
   `redis+sentinel://h1:26379,h2:26379/service` (and `rediss+sentinel://`),
   resolved through `redis`'s `Sentinel` to whichever node leads now. Cluster
   mode is **rejected**, and the reasoning is §2: it is a sharding answer to a
   question this system does not ask, it costs a keyspace migration and a worse
   fan-out story, and — the point that decides it — *it does not make a failover
   any safer*, because each shard is a primary with asynchronous replicas and
   the same loss window.
2. **The address is re-asked, not merely re-dialled.** This is the half that is
   code rather than configuration. `DEP-13`'s `ConnectionManager` re-dials one
   address, which is right for a coordinator that restarted and useless for one
   that moved. Worse, the *demotion* case looks healthy: a sentinel can promote
   the replica without anything dying, and the old primary then answers every
   write `READONLY` on a socket that never closes — which `redis` classifies
   `NoRetry`, so the connection reconnects to nothing and refuses every claim
   and every append for the life of the process. So the link re-resolves on a
   connection failure **and on `READONLY`**, with a generation counter so a node
   holding a hundred documents asks the sentinels once between them rather than
   once each.
3. **A durability floor that is checked, not recommended.**
   `OPENCALC_REDIS_MIN_REPLICAS=n` refuses a coordinator whose own
   `min-replicas-to-write` is below `n` — at startup, and again on every
   resolution. The second half is the one worth having: the setting is per
   server, and the standard mistake is setting it on the primary and nowhere
   else, which passes the startup check and fails at exactly the moment it was
   set for. A promoted primary below the floor is **not adopted**, so the node
   keeps reporting `NotSaving` and answering 503 rather than resuming against a
   coordinator that will accept writes it can lose. Unset is unchecked, which is
   the pre-ADR-020 behaviour and is correct for a single-box coordinator.
4. **`NOREPLICAS` maps onto the existing vocabulary, and no new one is
   invented.** A collapsed in-sync set arrives as `AppendError::Unavailable`,
   which `order()` already turns into `Refused { seq, reason: NotSaving }` and
   which `/readyz` already answers 503 for. That variant's contract — *the caller
   must not treat this as "it did not land" and must not retry blindly* — is
   exactly right here. What was added is the **explanation**: the raw driver
   error reads as a network fault and sends an operator to look at the link,
   where what happened is the primary holding ADR-014's promise. The text now
   says the write did not happen and names the replicas as the thing to look at.
5. **`WAIT n timeout` is rejected**, as §2b prices it: it narrows the window
   rather than closing it, Redis says so itself, and it costs a round trip on
   every edit on top of a setting that already turns the window into a refusal.
6. **A Raft-backed store behind `Coordinator` is a road not taken, not a
   supported second backend.** The trait makes it cheap and it remains the only
   option that closes the window, so it is recorded here as the answer for a
   deployment that needs one — but nothing in this repository implements, tests
   or ships it, and claiming it as "supported" would be a promise with no code
   under it.

**What this does not decide, said plainly:** Sentinel makes the coordinator
survivable; it does not make the log durable. The windows that stay open are
named under "Failure modes & limits".

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

**Accepted: Sentinel.** Cluster mode is recorded here as the thing to revisit
*if sharding ever becomes the question*, which it is not; and if it does, the
hash-tag migration above is the price, known in advance.

The shape it took — as built, and it is the shape this section predicted, with
one addition §2 did not foresee:

- `OPENCALC_REDIS_URL` gains a sentinel form —
  `redis+sentinel://host1:26379,host2:26379/<service-name>`, and
  `rediss+sentinel://` for TLS — resolved through `redis`'s `Sentinel` to
  whichever node is currently primary, and **re-resolved on failure** rather
  than only at startup. The re-dial delivered under `DEP-13` is the hook it went
  into: it re-dials the same address, and against Sentinel the link re-asks
  *which* address.
- **Re-resolution triggers on `READONLY` as well as on a dead socket**, and that
  is the addition. A sentinel failover need not kill anything: `SENTINEL
  FAILOVER` promotes the replica and demotes the old primary in place. The
  connection to it stays up, reads keep working, and every write comes back
  `READONLY` — which `redis` classifies as `NoRetry`, so `ConnectionManager`
  re-dials nothing and the node refuses every edit forever while looking
  perfectly healthy. That is `DEP-13`'s original defect wearing a live
  connection, and a re-dial cannot reach it. `NOREPLICAS` deliberately does
  **not** trigger it: a primary refusing writes because its in-sync set has
  collapsed is the right primary doing the right thing, and re-asking would find
  the same node.
- The subscription connection re-resolves too. A pub/sub socket is a second
  connection with a quieter failure, and one reopened against the node that used
  to be the primary is subscribed to a channel nobody publishes on any more.
- **A generation counter serialises it.** Every command carries the generation
  of the connection it used and hands it back on failure, so a node holding a
  hundred documents that all fail at the same instant asks the sentinels once
  between them rather than a hundred times.
- **A private CA cannot be carried on a sentinel URL, and that is refused rather
  than ignored.** `redis` 0.27 builds the connection to the resolved primary
  through `Client::open`, which takes no certificates — `build_with_tls` takes a
  *URL*, and the URL here names sentinels rather than the primary. A `root_ca`
  or client certificate configured alongside a sentinel URL would be read,
  validated and never used, leaving a link that reads as pinned and is not. That
  is the same shape as certificates against a `redis://` URL, and it is refused
  the same way.
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

Two narrowings were priced. **The first is accepted and the second rejected:**

- **`min-replicas-to-write 1` (with `min-replicas-max-lag`) on the primary —
  accepted, and checked.** The primary refuses writes when redundancy is below
  the threshold, so the loss window becomes a visible refusal — which the node
  already reports to the client as `Refused { NotSaving }` and to the
  orchestrator as a 503 on `/readyz`. ADR-012 already describes this mechanism
  ("a minimum in-sync count enforced separately"), and ADR-014 §4 already
  explains why it is not the default: refusing writes is a visible outage, and
  most integrators would rather have the log.

  What ADR-020 adds beyond recommending it is that a deployment can make the
  node **enforce that the recommendation was followed**.
  `OPENCALC_REDIS_MIN_REPLICAS=n` refuses a coordinator configured below `n`, at
  startup and on every resolution. A recommendation in a document is followed
  once, on the primary, by whoever read the document; the failover then promotes
  a node nobody configured, and the promise quietly stops holding at the exact
  moment it was supposed to hold. Verified beats documented here, and verifying
  costs one `CONFIG GET`.

  Its price is real and is the reason it is not the default: with the floor set,
  a failover that leaves no replica in sync is a **visible write outage** until
  one returns. That is the trade — a refusal you can see instead of a loss you
  cannot.

  Empirically, on Redis 8.6: a `SET`, a `ZADD` and a write from inside a Lua
  script all answer `NOREPLICAS Not enough good replicas to write.` and the
  write does not happen; reads and read-only scripts keep working, so
  `since()` and the health probes are unaffected. `redis` has no `ErrorKind` for
  it, so it arrives as an extension error whose `code()` is `NOREPLICAS`.
- **`WAIT n timeout` between the append and the acknowledgement — rejected.**
  It converts "probably replicated" into "at least *n* replicas said so", at one
  more round trip on the edit path. It is not consensus and Redis says so; it
  narrows the window rather than closing it. On top of `min-replicas-to-write`
  it buys a narrower version of a window that is already a refusal, and it buys
  that on the hot path of every keystroke batch. Rejected on that ratio, and
  recorded so it is not rediscovered as a fix.

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

- **The demotion window is not closed.** A sentinel promotes the replica and
  only *then* reconfigures the old primary, and the gap is seconds. In it the old
  primary is still `role:master` and still accepts writes — writes it is about to
  have rewound, and which nothing on the client side can distinguish from any
  other successful append. `min-replicas-to-write` narrows it, since the old
  primary loses its replicas as they follow the new one; nothing here closes it.
  This window is also what made the first version of the failover test unable to
  fail, which is recorded under "Acceptance gates" because the same trap is
  waiting for whoever writes the next one.
- **Sentinel credentials cannot differ from the data nodes'.** The URL's
  userinfo is applied to both. A deployment that puts a different `requirepass`
  on its sentinels is not expressible, and would fail as an authentication error
  against whichever half was not configured. Named rather than silently
  half-applied; the fix is a second variable if anybody needs one.
- **A private CA is unavailable on the sentinel path**, and refused rather than
  ignored (§2). A deployment needing both a private CA and failover has to wait
  for a `redis` release that can build the resolved primary's client with
  certificates, or terminate TLS in front of Redis.
- **The floor is only as good as `CONFIG GET`.** A managed Redis that renames or
  disables it makes the check impossible; that is reported as a refusal to start
  rather than treated as a pass, because "I could not check" quietly becoming
  "it is fine" is how the check would stop being one.
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
- **The failover harness is local.** It starts two `redis-server`s and three
  `redis-sentinel`s from the test itself, and skips where neither binary is on
  the path — which is CI today, whose coordinator is a service container. Closing
  that is the same row as the TLS gap above: `redis-server` **and**
  `redis-sentinel` on the test runner. Until then the failover half runs on a
  developer's machine and says so when it does not run.

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
  store).** The only option that actually closes the durability window.
  **ADR-020 records it as a road not taken, not a supported backend.** The trait
  makes it cheap to add, and that is exactly why the distinction has to be
  written down: nothing in this repository implements it, nothing tests it, and
  calling it "supported" would be a promise with no code under it. It stays the
  recorded answer for a deployment that cannot live with the window, and
  building it is a row rather than a footnote.
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

Shipped with ADR-020, in the same file. The harness starts two `redis-server`s
and three `redis-sentinel`s per test and kills them with it; the tests skip, and
say so, where those binaries are not on the path.

| Test | What it establishes |
| --- | --- |
| `a_sentinel_url_names_its_sentinels_and_its_service` | The URL form, including the default port, the percent-decoded password, and the shapes refused rather than guessed at. `redis` has no sentinel URL form of its own, so every field this drops fails later as "the sentinels are down". |
| `a_private_ca_cannot_be_hidden_behind_a_sentinel_url` | Certificates that this build cannot use on the sentinel path are refused rather than read and ignored. |
| `a_sentinel_link_is_warned_about_by_the_same_rule` | `rediss+sentinel://` is encrypted and is not warned about; the plaintext form is. |
| `ordering_survives_a_coordinator_failover` | **The gate.** Three sentinels, a real `SENTINEL FAILOVER`, and the primary **demoted rather than killed** — the case a re-dial cannot survive. Asserts both halves: ordering resumes, and every revision acknowledged before the failover is still in the log. |
| `ordering_resumes_after_the_coordinator_primary_is_killed` | The same gate taken literally: the primary process is killed, the sentinels promote, and the node finds the new address without a restart. |
| `a_write_with_the_in_sync_set_collapsed_is_refused_rather_than_lost` | **The durability gate.** A primary with `min-replicas-to-write 1` and its replica killed: the append is refused, the log is unchanged, the refusal names the setting, and ordering resumes when redundancy does — so the refusal is a state and not a wedge. |
| `a_coordinator_below_the_required_replica_floor_is_refused` | `OPENCALC_REDIS_MIN_REPLICAS` against a coordinator that does not hold the floor: refused at startup, naming the setting. Unset stays unchecked. |
| `a_promoted_primary_below_the_replica_floor_is_not_adopted` | The floor is re-checked after a failover, so "configured on the primary and nowhere else" is caught at the moment it matters rather than believed. |

**A trap worth recording, because it cost two green runs that proved nothing.**
The first version of `ordering_survives_a_coordinator_failover` passed with
re-resolution disabled. Two reasons, and both are the same mistake in different
clothes — asserting on a state the system had not reached yet:

- **`SENTINEL get-master-addr-by-name` changes seconds before the old primary is
  told.** In that gap the old primary is still `role:master` and still accepts
  writes, so the test was "recovering" by writing to the node it was supposed to
  have stopped using. The harness now waits for the old node's own `ROLE` to say
  `slave`.
- **Redis waits five seconds before a diskless full sync**
  (`repl-diskless-sync-delay`), and the whole failover happened inside that
  window. The replica was promoted with an **empty dataset**, and the assertion
  that the lease survived passed *vacuously*, because a claim against an empty
  store simply takes a fresh lease that looks exactly like the old one. The
  harness now sets the delay to zero and waits for `master_link_status:up`
  before it will hand a cluster to a test.

Still open, and a row rather than a silent gap:

- **CI has no `redis-sentinel`**, so the failover half skips there. Same runner
  change as the TLS half already needs.

## ADRs triggered

**ADR-020 — coordinator replication and what a failover may lose. Accepted.**
Triggered on two counts: a dependency choice (Sentinel, and whether a CP store is
a supported second backend — it is not), and the cluster's ordering/durability
guarantee, since a failover changes what a successful append means. Registered in
[08](08-ADR-REGISTER.md).

## Tracker IDs

- `DEP-13` — the row this was written for. All three items are now delivered:
  TLS and the re-dial under `DEP-13` itself, and Sentinel plus the durability
  floor under ADR-020.
- Proposed new rows, so that nothing lives only in this document:
  - **`redis-server` and `redis-sentinel` on the CI runner** — so the TLS half
    *and* the failover half of the coordinator suite stop skipping. This is now
    the larger of the two gaps: the failover behaviour is the one ADR-020 exists
    for and it is unexercised in CI.
  - **A leader that is fenced has no resync** — `order()` commits into the local
    session *before* appending, so a `Fenced` or `Stale` refusal leaves that node
    diverged from the log with only a log line to say so. Named in
    `net.rs` as "the recovery is a resync, which is not built". The equality
    fence makes it reachable more often, which is an argument for building it.
  - **`the_log_is_bounded_rather_than_growing_forever::in_redis` is
    wall-clock-flaky** — it takes a lease with a 1 s TTL (so a 4 s key expiry)
    and then issues 10 250 appends, which under full-suite load exceeds it and
    fails as `Unled`. Reproduced on unmodified `main`, so it predates ADR-020 and
    is not caused by it; it fails perhaps one full run in two on a busy machine.
  - **Sentinel credentials that differ from the data nodes'** — the URL's
    userinfo is applied to both, which is the common deployment and not the only
    one. A second variable if anybody needs it; named in "Failure modes" rather
    than left to be discovered.
  - **A private CA on the sentinel path** — refused today because `redis` 0.27
    cannot build the resolved primary's client with certificates. Worth
    revisiting when it can, since "encrypted coordinator link" and "replicated
    coordinator" should not be mutually exclusive.
