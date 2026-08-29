# 62 — ADR-016: Pipelined submissions and cumulative acknowledgement

**Status: Accepted**
**Extends** [ADR-011](56-COLLABORATION-CONCURRENCY-DESIGN.md) and
[ADR-015](61-COLLABORATION-RESUME.md).

## The problem

A client sent one chunk of edits and would not send another until the server
acknowledged the first. Stop-and-wait.

The cost is a round trip per chunk, and it is paid by everyone. On a 200 ms link
a client can submit about five times a second no matter how fast anybody types —
and the delay is not only in the typist's own saving indicator. The *other*
participants do not see the second batch until the first has been acknowledged,
because it was never sent. The slower the link, the more collaboration behaves
like taking turns.

Nothing about the ordering required this. The reason given was narrower: "a
client with two outstanding chunks cannot say which the server's acknowledgement
was for" — which sequence numbers, which submissions already carry, answer
directly.

## The real obstacle, and where it came from

The second chunk is written on top of the first, locally, before the first has
been ordered.

A submission names the revision it was written against, and the server rebases
it against everything committed since. If the second chunk names the same base
as the first, the server rebases it against the first as well — but the second
chunk already includes the first. It is transformed twice and lands wrong, which
is silent divergence rather than an error.

The client cannot name the right base, because the right base is wherever the
first chunk landed, and it will not know that until the acknowledgement arrives.
That is the loop stop-and-wait exists to avoid.

## The decision

**A sender does not need to know the receiver's position.** A submission's base
is now one of two things:

- `Revision(n)` — written against revision `n`. Used for the first chunk after a
  join or a resume, which is the only time the client knows an absolute answer.
- `Chained` — written on top of this client's previous chunk, wherever the server
  put it.

The server resolves `Chained` itself. It already records, per client, the
sequence number and revision of the last chunk it accepted — the table that
suppresses duplicates — and that revision is exactly the base a chained chunk
needs. It can do this because one client's chunks arrive in order on one
connection, so by the time chunk *n* is read, chunk *n−1* has been ordered.

A `Chained` chunk from a client with no accepted chunk is refused rather than
guessed at. It cannot happen from a correct client, and inventing a base is how
divergence starts.

**Acknowledgement becomes cumulative.** `Ack` now carries `through` — every
sequence up to and including it has been ordered — rather than naming one chunk.
The client drops that prefix of its outstanding queue.

This falls out almost for free: the server orders one client's chunks in
sequence, so acknowledging chunk *n* already implies every chunk before it. What
changes is that the *client* is allowed to rely on that, which makes a lost or
skipped acknowledgement self-healing — the next one covers it — and makes the
resume conversation expressible as a single number rather than a list.

**Outstanding chunks are bounded.** Past a limit, `flush` stops producing new
chunks and edits accumulate as they did before. Unbounded pipelining turns a
client on a bad link into unbounded memory here and unbounded work for the
server, and degrading to stop-and-wait under pressure is exactly the right
behaviour to degrade to, since it is what this replaced.

## Where the TCP analogy stops

It is worth being precise, because the analogy is good enough to mislead.

TCP acknowledges **bytes having arrived**. This acknowledges **operations having
been ordered against everyone else's**. Pipelining therefore hides latency; it
does not reduce work. The server still transforms each chunk in sequence, and
the client still holds every unacknowledged chunk, because incoming remote
operations must be rebased past all of them.

And there is no selective acknowledgement, deliberately. TCP can accept a later
segment and ask for a missing earlier one, because bytes at different offsets are
independent. Operation 2 has no meaning without operation 1 — it was written in
coordinates that assume it. So this is an in-order stream with a cumulative
acknowledgement and nothing else from the TCP repertoire.

## What was rejected

**Selective acknowledgement.** See above: the operations are not independent.

**Letting the client guess the base by counting its own chunks.** It would have
to predict how the server rebased them, which is the thing the server exists to
decide.

**Keeping stop-and-wait and shortening the flush interval.** Sends more, smaller
chunks and does not touch the round trip, which is the actual cost.

**Switching to a CRDT, which is asynchronous by construction.** This is the
honest alternative and ADR-011 rejected it on grounds that still hold: the
per-cell byte ceiling of ADR-004, which CRDT metadata would spend in every
single-user session, and documents that arrive as `.xlsx` snapshots with no
causal history to merge. Experience since adds a third: row and column
insertion has to commute while preserving formula references, which is where
spreadsheet CRDTs become genuinely hard. Pipelining gets most of the
responsiveness without paying any of that.

## How it is verified

Convergence over **interleavings**, in `session_tests.rs`, which is where this
class of bug lives — a client that edits three times while two acknowledgements
and someone else's insert are in flight is exactly the case stop-and-wait made
unreachable and this makes ordinary. Plus: a chained chunk resolving to the right
base, a cumulative acknowledgement dropping a prefix, the bound degrading to
stop-and-wait rather than growing, and a reconnect that resends several
outstanding chunks in order with only the first naming an absolute revision.
