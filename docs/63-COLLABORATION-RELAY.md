# ADR-017: How a node that does not lead a document serves its clients

**Status:** accepted
**Extends** [ADR-012](57-COLLABORATION-SERVER-BOUNDARY.md) and
[ADR-014](59-COLLABORATION-SERVICE-STACK.md).

## The problem

A client connects to whichever node a load balancer gave it. One node leads a
given document. Those are independent facts, so a node routinely has clients for
a document it does not lead — and until now it had nothing correct to do with
them.

ADR-012 already rules out the easy answer. Redirecting the client to the leader
makes it subject to affinity, which is the sticky session that decision
explicitly rejects; and leadership moves on its own, so the redirect would be
stale the moment a lease lapsed.

## What was rejected

**Direct node-to-node connections to the leader.** Lower latency by one hop, and
the internal endpoint already exists for it. Rejected because it requires every
node to accept inbound connections from every other — which is a real
constraint in the deployments this targets, where nodes are pods behind a
service and are not individually addressable without extra machinery. It also
puts a second transport, a second authentication story and a second set of
reconnection semantics into a system that already has one of each.

**Making the leader the only node that holds the document.** Relays would then be
dumb pipes. Rejected because it makes every read on a relay a round trip, and
because a leadership change would leave the new leader with nothing in memory —
turning a lease lapse into a rehydrate for everybody, which is exactly what
ADR-012's in-sync replicas exist to avoid.

**A dedicated ack channel back to the originating node.** The obvious shape:
leader commits, replies to whoever forwarded. Rejected as unnecessary — see
below, it falls out of the broadcast for free — and every message type removed
here is one that cannot get its routing wrong.

## The decision

**Every node holding a document is a replica. The leader is only the writer.**

A submission arriving at a node that does not lead is published to the
document's inbox. The leader — the only node acting on that inbox — orders it
the way it orders anything: transform against what has been committed since its
base, append to the log conditional on the revision and fenced by the epoch,
then publish the result to the document's channel.

**The published batch says who wrote it** — the originating client and its
sequence number — and that is what removes the acknowledgement channel. A node
seeing a batch sends `Ack { through: seq, revision }` to the writing client if
that client is one of its own, and `Apply` to every other client it holds on
that document. Nothing is routed back to the node that forwarded, so nothing
about that routing can be wrong.

**One fan-out, two feeds.** An earlier draft of this decision said every node
applies from the channel *including the leader*, so that there would be exactly
one code path. That is not implementable and the reason is worth recording:
ordering an operation and applying it are the same step. `commit` transforms a
submission against what has been committed since its base — which it can only do
by applying it — so a leader that waited for its own publication before applying
would have to transform the *next* submission against a document that did not
yet contain the previous one.

So the leader applies at commit and a relay applies when the batch arrives, and
these are two feeds. What they feed is the same thing: the per-document
broadcast every connection on this node already subscribes to. "Tell my clients"
stays one path, which is the part that matters; "apply to my copy" is two, and
they are two because the leader's copy is the thing the ordering is defined
against.

A leader that receives its own publication therefore finds it already applied,
and ignores it by the same rule that ignores any duplicate delivery — no special
case for being the writer.

**A gap is detected and closed, not tolerated.** Pub/sub is fire-and-forget: a
node can miss a message and Redis will not know. So each node tracks the
revision it has applied, and a batch that does not follow it directly is not
applied — the node reads the log from where it is with `since` and catches up.
Fire-and-forget is acceptable *because* of this, and would be silent divergence
without it, which is why the log is the authority and the channel is only a
prompt.

**Leadership is taken by claiming, on a timer, exactly as ADR-014 describes.**
No node watches another. A node with clients on a document calls `claim`
periodically; while somebody else holds the lease it is told so and relays,
and the moment the lease lapses the same call takes it over. The changeover is
a consequence of an atomic operation, not a decision — and the epoch fences
whoever held it before, including a leader that is alive and merely slow.

**Standalone changes in no way.** One process, leader of every document by
definition, no Redis, no inbox, no channel. ADR-012 is explicit that this is a
first-class mode, and the relay must not become a tax on the deployments that
do not need it.

## What this costs

Two Redis hops on a relayed edit rather than one on a led edit. Acceptable, and
partly illusory: the leader must append to the log before acknowledging
(ADR-014) whatever happens, so Redis is already in the hot path and the relay
adds a publish and a receive to it.

Every node holding a document holds the whole document. That is memory spent to
avoid a rehydrate on every leadership change, and the document cap already
bounds it.

## What this does not solve

An operation published but never appended cannot happen — the append comes
first — but an operation appended and never published can, if the node dies in
between. The gap detection covers it: the next publication does not follow, and
every node catches up from the log. What is not covered is a document where
*nothing further happens*, whose subscribers then sit one revision behind until
something does. A periodic reconciliation against the log would close it, and is
not built.

## How it is verified

Two server processes against one Redis, with a client on each — the only
arrangement where a relay exists at all. Specifically: an edit made on a relay
reaching a client on the leader and coming back acknowledged; an edit made on
the leader reaching a client on the relay; both nodes agreeing afterwards; and a
node that has missed a publication catching up from the log rather than applying
out of order.
