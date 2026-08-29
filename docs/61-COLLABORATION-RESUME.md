# 61 — ADR-015: Resuming a collaborative session after a disconnect

**Status: Accepted**
**Supersedes nothing.** Extends [ADR-011](56-COLLABORATION-CONCURRENCY-DESIGN.md),
[ADR-012](57-COLLABORATION-SERVER-BOUNDARY.md) and
[ADR-014](59-COLLABORATION-SERVICE-STACK.md).

## The problem

A browser loses its socket. It reconnects — the transport already does, with
backoff and jitter. What happens to the edits it had made and not yet had
acknowledged?

Before this decision, two things, both wrong:

**The work was silently lost.** On rejoin the server sent a `Welcome` carrying a
snapshot, and the client replaced its workbook with it. Anything the client had
typed that the server never received was in the old workbook and in nothing
else. No error, no warning; the cells simply reverted while the user watched.
That violates the project's first rule.

**Or it was silently duplicated.** The obvious repair — resend the outstanding
chunk after reconnecting — is worse. `ClientId` came from a per-connection
counter, so a reconnecting participant was a *new* client to the server. The
server suppresses duplicates by `(client, seq)`; with a new id that suppression
does not apply, and a chunk the server had already committed just before the
disconnect would be committed a second time.

Both failures are silent, which is what makes them serious. A user who loses a
sentence knows something went wrong. A user whose spreadsheet quietly has a row
inserted twice does not, and may not for months.

## What was rejected

**Resend and accept the risk.** No. Double-applying an insert-rows is data
corruption, not a glitch.

**Reload on every reconnect and accept the loss.** This is what many editors do,
and it is defensible when a disconnect is rare and short. It is not defensible
here: a laptop closing its lid, a train tunnel, a Wi-Fi handover and a rolling
deployment of the server itself are all ordinary, and the rule is no silent data
loss rather than not much data loss.

**Identify the participant by the token's user id.** Nearly right, and wrong in
one common case: the same person with the same token open in two tabs is two
participants who must not share a client id, or each tab's submissions suppress
the other's.

**Have the server keep the connection's identity alive by IP or socket.** Neither
survives what actually happens — a new IP is the *normal* outcome of a network
change, and the socket is precisely what was lost.

## The decision

**The client generates a resume key.** Opaque, random, created once per
`collaborate()` call and held for the life of that tab. It is presented on
`Join`, alongside the revision the client believes it is at.

**The server maps the key to the client id it issued**, scoped to the
authenticated user. A `Join` that presents a known key *whose recorded user
matches the token's user* is given back the same `ClientId` it had before.
Anything else — unknown key, user mismatch, no key — gets a fresh id and a fresh
`Welcome`.

The scoping matters. Without it, someone holding a valid token for a document
could guess another participant's key and adopt their identity, which would let
them have that participant's submissions suppressed as duplicates. Requiring the
user to match means a key is only ever useful to the person it was issued to,
which reduces it from a credential to a disambiguator. It is not a secret and is
not treated as one; the token remains the only thing that authorises anything.

**A resumed client is sent what it missed, not a snapshot.** If its revision is
still within what the server retains, it gets `Resumed` carrying the operations
committed since — which its session folds in, rebasing its own outstanding edits
past them as it goes, exactly as it does for any other arrival. Its document is
continuous, its outstanding chunk is still expressed against a workbook it
understands, and it then resends that chunk **with its original sequence
number**. Same client id, same seq: if the server had already committed it, it
answers `Duplicate` and nothing happens twice.

**When the client is too far behind to catch up, it is told.** The server sends
`TooFarBehind` — naming the oldest revision it can still rebase and where the
document now is — *before* the fresh `Welcome` that replaces the workbook. The
work is still lost in that case; what changes is that it is lost loudly, and a
host can offer to put the unsaved cells somewhere before the snapshot lands.

`PROTOCOL_VERSION` goes to **2** for this change. It has since moved on — 3 for
`Ping`/`Pong`, 4 for `Opening` — and the number is checked for equality before
anything else, so a mismatched pair stops rather than misreading each other.

## What this does not solve

A client that is disconnected for longer than the server retains history still
loses unacknowledged work. That is a bounded-offline limit and ADR-011 already
owns it; this decision only ensures it is announced rather than silent.

A client that never reconnects — the tab closed, the laptop wiped — loses it too.
Persisting unsent edits to local storage is a separate decision with its own
privacy consequences, and is deliberately not taken here.

## How it is verified

At three levels, because the last one is where the earlier bugs in this area
lived:

- **Unit**, in `casual-calc-transaction`: a client session that resumes keeps its
  outstanding chunk, its sequence numbering, and rebases correctly past what it
  missed.
- **Protocol**, in `casual-calc-collab-server`: a resumed join is given the same
  client id; a key presented by a different user is not; a resend of an already
  committed chunk answers `Duplicate` and broadcasts nothing.
- **End to end**, in `tests/browser/collab.spec.mjs`: a real browser is taken
  offline mid-edit with Playwright's own network control, reconnects on its own,
  and the edit it made while disconnected arrives in the other browser.
