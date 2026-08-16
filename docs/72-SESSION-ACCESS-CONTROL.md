# 72 — Changing what somebody may do, while they are doing it

**For** COL-40. **Status:** design, awaiting discussion.

The user's ask: *"admin or owner of file can lock files for other users, and
manage realtime access."*

Half of this already exists and is worth naming precisely, because the half that
is missing is small and the half that is present is the part people usually get
wrong.

## What is already true

**Access is enforced at the operation level, not in the toolbar.**
`Access::{View, Comment, Edit}` rides in the token, and
`server/casual-calc-collab-server/src/net.rs` checks *every operation of every
submission* against it — `claims.permits(&wire.op)`. A viewer whose buttons were
merely hidden is one bug away from editing a document they may not; here the
engine refuses. `Access::permits` is deliberately **deny-by-default**: an
operation added later is refused for anyone below `Edit` until somebody decides
which side of the line it belongs on, because the failure of forgetting is
silent and the failure of refusing is a bug report.

**Identity is the host's, never the client's.** The name and colour a
participant is shown under come from the token. The editor has no way to set its
own, and must not acquire one.

**Changing access already works — slowly.** The integrator mints a new token and
the participant rejoins. For "Bob is a viewer from now on", that is correct and
needs nothing new.

## What is missing

*Now.* There is no way to change what a live participant may do without them
reconnecting, and no way to say "nobody edits this for the next ten minutes".

## The decision

**A session-scoped override, held by the server while the document is live, on
top of the token — never instead of it.**

The override may only ever *reduce* what the token grants. That is the whole
safety property: a compromised or buggy client cannot promote itself, because
the token remains the ceiling and the server takes the minimum of the two. An
owner who wants to *grant* more access mints a token, which is the existing
path and involves the system of record.

**Durable policy stays with the host.** ADR-012 is explicit that the server
holds no per-document state and that *the token is the whole integration
contract*. So the override is deliberately **ephemeral**: it lives as long as
the document is resident, and a document evicted for idleness comes back with
the token's own permissions. Anything longer-lived is the host's to persist and
re-express in the tokens it mints. This is a real limitation and it is the right
one — the alternative is a collaboration server that has become a permissions
database with no backup, no audit and no owner.

**Who may set it** comes from the token too: a new `owner: bool` claim, absent
meaning false. Not inferred from `Access::Edit` — every editor being able to
lock every other editor out is a different feature and a worse one.

## Two things it must do that are easy to miss

**Tell the person.** A participant who is demoted mid-edit must be told in
words, not discover it when a keystroke silently stops working. They go
read-only immediately, and their in-flight local edits are *not* silently
dropped: the same "lost unsent work" notice built for the unresumed-reconnect
case applies, because the situation is identical from the user's side.

**Refuse loudly at the seam.** A submission that arrives after the override is
refused by the same per-operation check as anything else — the override changes
the effective `Access` and nothing about the enforcement path. There is no
second code path to keep in step.

## The wire

Two additions, and a version bump to **6**:

- `ClientMessage::SetAccess { client, access }` — from an owner, refused
  otherwise.
- `ServerMessage::AccessChanged { access, by }` — to the affected participant,
  and the roster's entry updates for everybody so the change is visible rather
  than mysterious.

`PROTOCOL_VERSION` moves because an old client would not understand
`AccessChanged` and would go on believing it may edit — old and new peers
interpreting the same session differently, which is exactly the rule in
[62](62-COLLABORATION-PIPELINING.md).

**Fold COL-38 into the same bump.** A parse failure is currently reported as
`Refusal::CannotMerge`, which names the transform — the one part that was
working — and cost a live debugging session. It needs a distinct variant, which
is also a bump. Two bumps to ship two small things would cost every unupgraded
tab its session twice; one bump costs it once.

## Document lock

"Lock the file" is this mechanism applied to everyone at once: every participant
below the owner is held at `View` for the life of the session. It is not a
separate concept and must not become separate code — a lock that works
differently from an access change is a lock with its own bugs.

## How it gets verified

- An owner reduces a participant's access; that participant's next edit is
  refused **by the engine**, their editor goes read-only, and they are told why.
- A non-owner attempting `SetAccess` is refused.
- The override cannot raise access above the token's: an owner "promoting" a
  viewer whose token says `View` changes nothing.
- The affected participant's unsent work is announced, not silently discarded.
- A document evicted and reopened comes back with the token's permissions,
  asserted rather than assumed — that is the ephemerality, and it is the part
  somebody will later mistake for a bug.
- Two browsers: the roster shows the change on both sides.
