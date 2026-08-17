# 59 — The collaboration service: transport, identity, coordination, durability

**Status: Accepted** (ADR-014). Triggered by [COL-16](14-EXECUTION-TRACKER.md),
which is the network service [ADR-012](57-COLLABORATION-SERVER-BOUNDARY.md)
designed the boundary for.

> **Decision.** Clients speak **WebSocket** to an `axum` server. Identity is a
> **host-signed JWT** verified against a JWKS endpoint. A cluster coordinates
> through **Redis** — leases, pub/sub and Streams — and a standalone process
> uses none of it. An operation is **appended to the log before it is
> acknowledged**.

[ADR-011](56-COLLABORATION-CONCURRENCY-DESIGN.md) chose server-mediated OT and
[ADR-012](57-COLLABORATION-SERVER-BOUNDARY.md) drew the boundary: what the
server is for, what it refuses to be, and how a cluster keeps a document
consistent without sticky sessions. Both are about *spreadsheets*. This one is
about the four choices that are about **infrastructure**, and it exists as a
separate record because they are the ones an operator has to live with and the
ones that are expensive to reverse after a deployment exists.

## 1. Transport: WebSocket over `axum`

The protocol is already message-shaped — `ClientMessage` and `ServerMessage` in
`casual-calc-transaction::protocol` — so a bidirectional frame transport is a
direct carrier for it rather than a translation.

It does **not** reintroduce affinity. A WebSocket pins a *connection* to a node,
which is unavoidable for any long-lived connection; a sticky session pins a
*client's identity* to a node, so that reconnecting has to land in the same
place. Under ADR-012 a client may connect to any node and that node relays to
whichever one currently leads the document. Losing a node drops its
connections; the clients reconnect anywhere and resume from their last
acknowledged revision.

**Rejected: SSE down, POST up.** It survives proxies that break the WebSocket
upgrade, which is a real problem in some corporate networks. It costs two
channels to correlate, a second failure mode when only one of them dies, and
latency on exactly the path — the edit — that must feel immediate. Worth
revisiting as a *fallback* if a deployment actually hits the proxy problem;
not worth paying for before then.

**Rejected: gRPC bidirectional streaming.** Excellent between servers, and the
replication path may yet use it. From a browser it needs grpc-web and a
translating proxy, which adds a hop and an operational component to the case
that matters most.

## 2. Identity: a host-signed JWT, verified against JWKS

ADR-012 already established that **the token is the whole integration
contract** — it names the document, the participant, their access level, and
where the finished bytes are to be sent. This settles how the signature is
checked.

The integrator signs with an asymmetric key (RS256 or ES256) and publishes the
public half at a JWKS URL the server is configured with and caches. Three
consequences, in the order they matter:

- **The server never holds a signing key.** It can verify a token and cannot
  mint one. A compromised collaboration node cannot issue itself access to a
  document; with a shared secret it could.
- **Rotation is the integrator's business alone.** They publish a new key,
  the server picks it up at the next fetch, and no coordinated restart is
  needed.

  "The next fetch" is two fetches, and it needs to be, because rotation has two
  halves that fail differently. A key being **added** announces itself: a token
  arrives naming a `kid` this server does not hold, so the join path re-reads
  the key set once and retries — throttled by `OPENCALC_JWKS_MIN_REFRESH_MS`,
  since the trigger is anything that can invent a `kid` and would otherwise make
  this server hammer somebody else's endpoint on demand. A key being
  **withdrawn** announces nothing at all — nobody presents a token for a key
  that is being revoked — so a timer re-reads every
  `OPENCALC_JWKS_REFRESH_MS`, and that interval is the bound on how long a
  revoked key keeps working.

  This paragraph exists because the property above was stated here and was not
  true of the code: `read_verifier` fetched once at startup and moved the key
  set into the configuration for the life of the process. A scheduled rotation
  therefore locked **every** user out of **every** document until an operator
  restarted every node — reported to the client as the same `NotAuthorised` a
  bad token gets — and revoking a compromised key had no effect on a running
  node at all.
- **`kid` selects the key**, so old and new coexist during a rotation instead of
  there being a moment when both halves must change at once.

HS256 with a shared secret is supported for **standalone and development**,
where there is one process and no key server, and where requiring one would
make the simple case need infrastructure the whole standalone mode exists to
avoid. It is documented as what it is: the weaker option, chosen for
convenience, not offered as an equal.

**Rejected: opaque token plus an introspection callback.** The most flexible,
and it puts the integrator's endpoint on the join path — so their outage
becomes an outage for documents already open. A JWT is verifiable offline,
which is the property that matters when the network is the thing failing.

### The claims

ADR-012 said the token is the whole integration contract; this is that contract
written out. A host signs one of these per join:

```json
{
  "iss": "https://host.example",
  "aud": "opencalc-collab",
  "exp": 1786500000,

  "user": {
    "id": "u-17", "name": "Ada Lovelace",
    "email": "ada@host.example", "avatarUrl": "https://…/ada.png",
    "group": "Finance", "color": "2F6DF6"
  },
  "document": {
    "key": "file-1:rev-9", "id": "file-1", "title": "Budget.xlsx",
    "version": "9", "ownerId": "u-1",
    "url": "https://host.example/files/1"
  },
  "permissions": {
    "access": "edit", "download": true, "print": true, "copy": true
  },
  "callback": { "kind": "url", "url": "https://host.example/callback" }
}
```

`callback` may instead be `{"kind":"wopi","src":…,"token":…}` for a WOPI host,
which is why it is tagged rather than being a bare URL: the two need different
requests, and guessing from the shape of a string is how that goes wrong.

Four things about it are decisions rather than fields:

- **`document.key` is not `document.id`.** The key identifies an *editing
  session*; the id identifies the file. To start a fresh session over the same
  file — after restoring an old version, or after a save the host wants to be
  the new baseline — the host issues a **new key**. Reusing the old one joins
  the session that is still running, which is still holding the content the
  host just replaced.
- **`access` is a mode, not a label.** `comment` refuses a cell edit at the
  operation level, including one hidden inside a batch or a metadata bundle
  that names comments among other fields. A permission that is transported and
  then ignored reads like a guarantee in the integrator's code and is a
  suggestion in ours.
- **The absent default is the least.** A token that omits `permissions` grants
  `view`, not `edit`.
- **`copy` is honestly client-side.** The bytes are on the participant's
  machine by the time they can see them, and any system claiming otherwise is
  describing a screenshot it cannot prevent. It is honoured because
  integrators' policies ask for it, and documented as a deterrent.

### The URLs are the dangerous part

A token names addresses this server will connect to — one to fetch the document
from, one to post it back to. That makes a leaked or mis-issued token a
**request-forgery primitive** aimed at whatever the server can reach, including
addresses inside the deployment that nothing outside it can.

The host signed the URL, so a valid token means the host chose it. That is not
enough on its own, so the server also holds an **allow-list of hosts** and
insists on `https` unless told otherwise. The check parses the authority
properly rather than looking for the allowed host as a substring, because
`https://host.example@attacker.example/` and
`https://host.example.attacker.example/` both contain it.

## 3. Coordination: Redis, and only in a cluster

One dependency doing three jobs, each with a Redis primitive that already fits:

| Job | Primitive | Why this one |
| --- | --- | --- |
| Per-document leader lease | `SET key value NX PX ttl` | Atomic acquire with expiry. The **epoch** in the value is what makes a cheap TTL safe (ADR-012): a lease can expire wrongly under load, and the epoch means a zombie leader's appends are rejected rather than believed. |
| Relay fan-out | pub/sub | Non-leader nodes forward a submission to the leader and receive the broadcast. Fire-and-forget is correct here: the op log, not the channel, is the record. |
| The op log | Streams | Ordered, append-only, with consumer positions — which is what a replica replaying from a leader needs, and what makes §4 possible at all. |

**A standalone process uses none of it.** One node leads every document by
definition, the log is in memory, and there is nothing to fan out to. This is
not a degraded mode; it is the mode most deployments will run, and requiring
Redis for it would be requiring infrastructure to solve a problem the operator
does not have.

**Rejected: etcd/Consul for leases plus NATS for fan-out.** Better primitives
for each job taken separately — real leases with watches, proper subject
routing. Two more things to run, monitor, secure and upgrade, for a system
whose stated requirement is to be lightweight.

**Rejected: in-process Raft.** No external dependency at all, which is
genuinely attractive. It also makes this a consensus system that somebody has
to operate and debug, and consensus bugs are the kind that appear under
partition at 3am. Redis plus the epoch fence gets the same *correctness*
guarantee — divergence is impossible because appends are conditional on the
revision — while keeping the hard part in a component that is already
understood.

## 3b. Exposure: TLS, plain, and the reverse proxy

Every deployment answers this differently and none of the answers is a default
worth imposing, so all of it is configuration: an evaluation wants plain HTTP on
one port; a Kubernetes deployment terminates TLS at an ingress and runs plain
behind it; a regulated one wants TLS to the process and between its own nodes.
A node therefore has a **public endpoint** and, in a cluster, a separate
**internal endpoint**, each independently plain or TLS — they are different
security problems (browsers over the internet, versus a handful of known peers
on a network the operator controls) and deserve different answers.

**The dangerous part is the forwarded headers, not the certificates.**
`X-Forwarded-For` is a header, so anyone who can reach the port can write one. A
server that believes it unconditionally has no idea who its clients are, and
every rate limit, audit line and allow-list downstream is keyed on a value the
client chose. Two rules follow:

- Forwarded headers are believed **only when the immediate peer is a configured
  proxy**. The default is to trust nothing.
- The chain is walked **right to left**. Each hop *appends*, so the rightmost
  entries came from the proxies nearest this server — the ones whose honesty is
  a configuration decision — and the leftmost was written by whoever spoke
  first, which includes the client. Taking the leftmost is the common
  implementation and is exactly backwards: it lets a client choose its own
  address by sending the header itself.

An unreadable entry **ends** the walk rather than being skipped past: a chain
this server cannot parse is one it should stop believing at, not one to step
over on the way to something less accountable.

`Exposure::warnings` says the probably-wrong combinations out loud at startup —
plain with no proxy in front of it, TLS that also trusts any peer, a plain
internal endpoint, one address shared between the client and cluster ports.
None of these will ever *fail*, which is exactly why nothing else would mention
them.

## 3c. Discovery: Redis, and the address you advertise

**There are two address spaces, and only one of them is the proxy's.** A client
reaches a node *through* whatever the operator put in front of it — an ingress,
a load balancer, a reverse proxy — and often the node does not know that address
at all, because the proxy owns it. Relay and replication do not go that way: a
node dials another **directly**, on the cluster network, at the address it found
in Redis. Sending peer traffic back out through the public proxy would be slower
and would make cluster traffic arrive looking like a client.

Three things follow, and are checked rather than described:

- The advertised address is checked against the **internal** endpoint's port. A
  node advertising its public port sends every peer through the proxy.
- The internal endpoint **never** honours forwarded headers. A peer is not a
  proxy, there is no hop between two nodes for a header to describe, and
  honouring one there would let anything that reaches the cluster port claim to
  be anything.
- The internal endpoint is where **mutual TLS** belongs. Its peers are a known,
  small, operator-controlled set — exactly what client certificates are good at,
  and exactly what a browser is not. TLS without a client CA there proves the
  traffic is private, not that the peer is one of yours, and that combination is
  called out at startup.

Discovery uses the same Redis the rest of the cluster does — a node registers
itself under a TTL'd key carrying its **advertised address**, refreshes it, and
reads the others. No second dependency, and a node that dies stops refreshing
and disappears on its own.

**Advertising is not binding, and this is the trap.** A node binds
`0.0.0.0:8443` so it accepts on every interface, and that is precisely the
address no peer can dial. The same goes for a container's `127.0.0.1` and for a
pod IP that is right until the pod moves. So the address peers are told is
separate configuration — `NodeIdentity { id, advertise }` — and is never
derived from the listener.

Deriving it is the mistake that makes a cluster look configured and never form,
and every symptom points elsewhere: peers that never appear, a leader that is
never elected, several nodes quietly running the same document standalone. So
`NodeIdentity::problems` refuses the shapes that cannot work — an unspecified
address, loopback, port zero, an empty id — at the point the configuration is
read, rather than letting them present later as an unexplained absence of
peers. The **id** is the discovery key and part of the leader lease, so two
nodes sharing one are two nodes claiming to be the same leader: it wants
something the orchestrator already guarantees is unique, like a pod name, not a
hostname that may repeat.

### Who decides the leader is down: nobody

There is no failure detector, and that is the design rather than an omission.
No replica watches the leader, no node forms an opinion about another's
liveness, and nothing votes.

A leader proves it is alive by **renewing its own lease**. If it stops — dead,
partitioned, paused by a long garbage collection, or merely slow — the lease
lapses on Redis's clock without anybody having judged it. Any node that wants
the document calls `claim` periodically: while the lease is held it is told who
holds it and relays there, and the moment the lease has lapsed the same call
takes it over. The changeover is a consequence of an atomic operation, not of a
decision.

**Heartbeat detection is the obvious design and the wrong one.** A replica
noticing silence and declaring the leader dead needs the replicas to *agree*
that it is down, which is the consensus problem wearing a disguise: under a
partition each side sees the other's silence, each concludes the other is gone,
and both promote. Liveness cannot be observed remotely, only inferred, and two
nodes can infer differently from the same silence.

The lease sidesteps it by never asking the question. The only signal is the
absence of a renewal; it is evaluated in one place, atomically, by the store.
Two nodes claiming at the same instant do not race — one succeeds and the other
is **told who won**, which is also what it needs in order to relay.

That leaves exactly one hole, and it is the one the epoch fills: a leader that
was alive all along and lost its lease to a slow moment. It still believes it
leads, it cannot be told in time, and its next append is refused by an epoch
that has moved past it. It learns by being refused, which is the only way it
can.

## 4. Durability: append before acknowledging

An operation is written to the log **before** the client is told it was
accepted.

The alternative — acknowledge immediately, replicate afterwards — is faster and
fails in the way this project refuses. A client that has been told its edit
landed will show it as landed, and will not send it again. If the leader then
dies before replicating, that edit is gone while every participant's screen
still shows it. That is silent data loss with a receipt.

The cost is one round-trip on the edit path. It is worth it here because the
receipt is the whole point: `Commit::Applied` is a promise, and a promise that
is sometimes false is worse than a slower one that is always true.

**On `acks=all`**, which prompted this question: acknowledging after *k*
replicas confirm is stronger still, and carries the trap that if the in-sync
set collapses to the leader alone, `k=1` is satisfied and the guarantee
evaporates without any error being raised. If a deployment wants that
guarantee it needs a **minimum in-sync count enforced separately** — refusing
writes when redundancy drops below it, which ADR-012 already describes. That
remains configurable and is not the default, because refusing writes is a
visible outage and most integrators would rather have the log.

## 5. Probes: liveness and readiness are different questions

`/healthz` is unconditional: it answers whenever the process is serving. A `no`
means restart this pod.

`/readyz` consults the coordinator. On a clustered node that cannot reach Redis
it answers **503**, because such a node can accept an edit and then order
nothing — every submission it takes is one it will refuse. A `no` means take it
out of the pool and leave it running; restarting it would drop the sessions it
is holding and fix nothing, since the fault is not in this process. Standalone
has no coordinator to lose, so it is ready as soon as it listens.

Clients are told too. When the log refuses an append, the submitter gets
`Refused { seq, reason: NotSaving }` rather than silence — without it a client
cannot distinguish "slow" from "will never land", and retries forever.

The container image carries no `curl`, so the binary answers for itself:
`--healthcheck` fetches `/healthz`, `--readycheck` fetches `/readyz`. Compose
gates dependants on the readiness one; a Kubernetes deployment should map them
to `livenessProbe` and `readinessProbe` respectively.

Redis itself is still a single node (DEP-13). This section makes its loss
*visible and safe*, not survivable.

## Consequences

- The server gains an async runtime, an HTTP stack, a JWT verifier and an
  optional Redis client. All of it lives under `server/`; the CI boundary check
  from ADR-012 keeps it out of `crates/`.
- **Standalone has no new required dependency.** Redis is behind a feature and
  a configuration; absent both, the server runs as one node.
- A JWKS URL becomes required configuration for the recommended auth path. The
  server must tolerate the endpoint being briefly unreachable — a cached key set
  keeps working, since an integrator's key server going down should not evict
  everybody from a document they already joined.
- The edit path has one more hop than it strictly needs. If that ever shows up
  in a latency budget, the honest fix is a faster log, not a weaker promise.
