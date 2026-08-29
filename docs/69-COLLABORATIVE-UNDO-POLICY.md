# 69 — Design note: what undo *means* when somebody else has been typing

**For** COL-28, second half. The first half — making undo travel at all — is
implemented: `History::undo`/`redo` return the operation they applied and the
session records it on the same outgoing log an edit uses, so peers converge
instead of silently diverging.

This note settles the question that convergence does not answer: **when a peer
has changed the thing you are undoing, what should undo do?** Convergence only
promises everyone ends up the same. It does not say *which* same.

Written before the code, because the wrong answer here is not a crash — it is a
product that quietly discards somebody's work, and the person who lost it was
not the one who pressed the button.

## The situation, concretely

    revision 4   Ada    A1 = 100        (was 3)
    revision 5   Grace  A1 = 250
    revision 6   Ada    presses Ctrl+Z

Ada's stored inverse says `A1 = 3`. Applying it reverts Grace's edit too — an
edit Ada may never have seen, made after hers, that had nothing to do with her.

The same shape with structure is worse:

    revision 4   Ada    inserts a row at 10
    revision 5   Grace  types into that new row
    revision 6   Ada    presses Ctrl+Z

The inverse deletes row 10. Grace's data goes with it.

## What the options actually are

**Clobber.** Apply the inverse as written. Ada gets what she expects; Grace
loses an edit with no notification. This is what Excel and Google Sheets both
do for the cell case, and it is what the code does today now that undo travels.

**Refuse.** Detect that the target moved under the inverse and decline, telling
Ada why. Nobody loses data. Ada's undo silently does nothing, which is the
failure mode [`History::apply`](../crates/casual-calc-transaction/src/lib.rs)
already goes out of its way to avoid — a user who presses undo and sees no
change concludes undo is broken.

**Narrow.** Apply the parts of the inverse that nobody else has touched, skip
the rest. Preserves the most work and produces a state neither participant
asked for — Ada's undo half-happened, and no undo stack anywhere describes the
result.

## The decision

**Cell-level edits clobber. Structural edits refuse.**

The split is not a compromise; the two cases differ in what is at stake.

*Cell edits* already resolve last-writer-wins at cell granularity — docs/56
settles that for concurrent writes, and an undo is a write. Undo is also the
one command whose entire promise is "put it back the way it was"; a version of
it that sometimes declines is worse than one that sometimes overwrites a single
cell, and Excel and Sheets have both made that trade in front of far more users
than this project has. The value is recoverable: it is one cell, and the peer
who lost it has their own undo stack.

*Structural edits* are different in kind. Deleting a row that somebody has since
filled destroys work that was never in that row when the undo was recorded, and
no undo stack contains it — Grace's history has "typed into row 10", not "here
is row 10's content", so pressing undo does not bring it back. The loss is
unbounded and unrecoverable, and it is caused by an operation whose author could
not have known.

So: an undo of a structural edit is refused when the affected band is no longer
empty of other people's work. It is refused **loudly** — a message naming what
stopped it — because the alternative failure (a button that appears to do
nothing) is the one this codebase has already been bitten by.

## What "no longer empty of other people's work" means

Checked against the band the inverse would remove, at the moment of undo:

- Any cell inside it that this session did not write, and
- that is non-empty.

Deliberately coarse. A precise answer needs per-cell authorship, which the model
does not carry and which would cost more per cell than the check saves. A
conservative test refuses a little more often than strictly necessary, and every
extra refusal is a case where the user is told to look rather than a case where
data disappears.

## Redo is not the inverse of this

A redo is a **new intention**, not the cancellation of one, and gets no special
treatment: it travels as an ordinary operation and lands wherever the transform
puts it. The asymmetry is deliberate — undoing is a claim about the past ("this
did not happen"), while redoing is a claim about the present ("do it again").
Only the first can be invalidated by what somebody else did in between.

## Why this needs no wire change

An undo is submitted as an ordinary operation against the sender's current
revision, so the server transforms it against everything committed since exactly
as it does any edit. Nothing new appears on the wire, no message changes shape,
and `PROTOCOL_VERSION` does not move. The refusal is a **local** decision taken
before submitting — which is the right place for it, because the client is the
only party that knows the operation is an undo rather than an edit. The server
deliberately does not: it orders operations and does not interpret intent.

That is worth stating plainly because it is a constraint on the design, not an
observation about it. A policy that required the server to know "this is an
undo" would need a wire field, a protocol bump, and a server that treats two
identical operations differently depending on a claim the client makes about
its own state — which is exactly the kind of trust boundary
[ADR-012](57-COLLABORATION-SERVER-BOUNDARY.md) keeps the server out of.

## How it gets verified

The interleaving matrix `docs/67` asks for, with the outcome column filled in by
this policy rather than by whatever the code happens to do:

| Ada | Grace, concurrently | Ada undoes | Expected |
| --- | --- | --- | --- |
| `A1 = 100` | nothing | `A1 = 100` | back to the old value, both sides |
| `A1 = 100` | `A1 = 250` | `A1 = 100` | back to Ada's old value; Grace's 250 is gone, both sides agree |
| `A1 = 100` | `B9 = x` | `A1 = 100` | A1 reverts, `B9` untouched |
| insert row 10 | nothing in it | the insert | row removed, both sides |
| insert row 10 | types in row 10 | the insert | **refused**, with a message; both sides unchanged |
| delete row 10 | edits row 12 | the delete | row restored, Grace's edit still at its shifted address |
| `A1 = 100` | disconnects, reconnects, `A1 = 250` | `A1 = 100` | same as the concurrent case — resume changes nothing about this |
| two edits | nothing | twice | both reverted, in order, on both sides |

Every row asserted on **both clients, the server's model and the saved
`.xlsx`** — four places that can disagree, and the file is the one that
outlives the session.

## What this does not settle

Undoing somebody *else's* operation is not in scope and is not offered: the
stack is per-session and holds only what this session did. Selective undo
("undo that thing from ten minutes ago, not my last one") is a different feature
with a different transform problem, and nothing here forecloses it.
