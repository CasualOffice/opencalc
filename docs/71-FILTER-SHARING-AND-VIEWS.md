# 71 — Filtering with other people in the document

**For** COL-32. **Status:** implemented. The first increment is built as described; named, shareable view *definitions* remain out of scope.

A filter hides rows. In a document with one person in it that is a display
choice; in a document with three people in it, it is a question nobody has
answered yet: **whose rows?**

Today OpenCalc has one answer and it is the shared one. `SetSheetMetadata`
carries `auto_filter` and `filter_hidden`, it goes through `session.edit` like
any other operation, and it transforms and relays. So filtering already changes
what everyone sees. That is Excel's model and it is a legitimate answer — but it
is the only answer available, and the common complaint about it is exactly the
one that made Google build filter views: *sorting and filtering a shared sheet
yanks the floor out from under everyone else who is reading it.*

This note settles what the alternative is and, more importantly, what it must
not be allowed to do.

## The three things that get conflated

Pulling them apart is most of the design:

1. **The shared filter.** Part of the document. Everyone sees the same hidden
   rows. Saved to `.xlsx` as `<autoFilter>`, which is where Excel expects it.
2. **A view's definition** — "Q3, region = EMEA, sorted by amount". A named
   object that is part of the document, so a colleague can be told "open the Q3
   view" and find it there.
3. **Which view a given person currently has open.** Not part of the document
   at all. It is a property of a session, like the scroll position or which
   cell is selected.

The mistake to avoid is treating (2) and (3) as one thing. Google Sheets gets
this right: a filter view is saved and shared, while *having it open* affects
only you. Two people can have two different views of the same sheet open at
once, and a third can be looking at the unfiltered sheet.

## The decision

**Applying a filter asks which kind it is**, defaulting to shared, because
shared is what a spreadsheet has always done and what the file format can
express.

- **Filter for everyone** — the existing path. `SetSheetMetadata` with
  `auto_filter` and `filter_hidden`, on the wire, undoable, saved to the file.
- **Filter just for me** — a personal view. Never on the wire. Never in
  `SheetMetadata`. Never in the undo stack.

Named views (2) are deliberately **out of scope for the first increment**. The
user-visible need is "let me filter without disturbing anyone", and that is (3)
with an anonymous definition. Naming and sharing view definitions is a second
step that this design does not foreclose: it adds a document-level list, which
is a new operation, which is an ADR trigger.

## The constraint that decides the implementation

**A personal view must not change a single cell value.**

This is not a preference. Cell values are document state: they are what
`recalculate` writes into the model, what `save()` serializes, and what every
participant's copy is required to agree about. If a personal view could change
a value, two people would hold different numbers for the same cell and the
convergence property the whole collaboration design rests on would be false.

That has one sharp consequence, and it is the reason this note exists:

> `SUBTOTAL`'s 101–111 codes and `AGGREGATE` skip hidden rows. They must read
> **only the shared hidden rows** — `Sheet::hidden_rows` and
> `Sheet::filter_hidden` — and must never see a personal view's hidden set.

So a personal view hides rows *visually* and the subtotal underneath it does not
move. That is what Google Sheets does, and it surprises people the first time;
the alternative is a spreadsheet where the same cell reads 4 on one screen and 6
on another, which is worse than surprising.

The implication for the model is that a personal view's hidden set cannot live
in `Sheet`. `Sheet::is_row_hidden` is what the evaluator asks, and anything
reachable from there is by definition shared. The personal set belongs on the
session, beside the other things a session owns and a document does not.

## Where each piece lives

| Thing | Lives in | On the wire | Undoable | Saved |
| --- | --- | --- | --- | --- |
| `auto_filter`, `filter_hidden` | `Sheet` | yes | yes | yes |
| `hidden_rows` (hand-hidden) | `Sheet` | yes | yes | yes |
| Personal view rules | `WorkbookSession` | **no** | no | no |
| Personal view hidden rows | `WorkbookSession` | **no** | no | no |
| Which view is open | `WorkbookSession` | no | no | no |

The layout asks the session for row visibility and gets the union of the shared
sets and the personal one. The evaluator asks the *sheet* and gets only the
shared ones. Two questions that look identical and are not, which is exactly the
kind of thing that needs writing down before it is coded rather than after.

## Undo

A personal view change is not a document edit, so it does not enter the history.
Pressing undo after applying a personal filter undoes whatever you last did *to
the document*, which is the correct behaviour and will nonetheless surprise
somebody — so clearing a personal view needs to be one obvious click rather than
something you reach for undo to accomplish.

The shared filter keeps the undo behaviour it has, including the refusal rule in
[69](69-COLLABORATIVE-UNDO-POLICY.md): undoing a structural edit is refused when
the band is no longer empty of other people's work.

## What this fixes that is already broken

Independently of views, the shared filter has an engine bug that this work
depends on and which is **already fixed** (CALC-01's second half):
`SetSheetMetadata` was classified `RecalcPlan::Skip`, so applying a filter never
recomputed the `SUBTOTAL` beneath it. A filter that relays perfectly and leaves
every total stale is not a working feature, and it would have looked like a
collaboration bug rather than a calculation one.

## How it gets verified

- Two browsers. A applies a shared filter; B's rows hide and B's `SUBTOTAL`
  moves. A applies a personal filter; **B sees nothing at all** — not the rows,
  not the subtotal, not a flicker.
- A personal view and a shared filter at once: the union hides, and the saved
  `.xlsx` contains only the shared one.
- `SUBTOTAL(109, …)` under a personal view equals the value on every other
  participant's screen, asserted on both clients *and* in the saved file.
- A personal view survives nothing: reload and it is gone, which is what "not
  part of the document" means.
- `collab_flush()` emits nothing at all for a personal view. This is the test
  that would fail if somebody later routes it through `session.edit` for
  convenience.

## What the building added to this note

Two things the design did not anticipate, both recorded because the next person
will meet them:

**The file format has no filter-hidden row.** ECMA-376 stores a filtered row as
`<row hidden="1">`, exactly like a hand-hidden one, so the `filter_hidden` /
`hidden_rows` distinction this engine keeps is *its own* and does not survive a
save. A shared filter comes back from a round trip in `hidden_rows`. The first
version of the save test asserted on `filter_hidden` and failed — correctly.

**A view keyed by sheet index has to be resequenced.** Insert, remove or move a
sheet and an unmaintained key goes on hiding rows on whichever sheet inherits
the number. That surfaces as rows vanishing on a sheet the participant never
filtered, with nothing on the wire to explain it and nothing in the history to
undo it — the worst shape a defect can take, because every instinct points at
the collaboration layer and the cause is local bookkeeping.

**The filter control is shared; only the rule is personal.** Turning the
autofilter on is a document edit — Excel stores `<autoFilter>` and every
participant sees the buttons. Only the values ticked inside it can be personal.
The other way round would put one participant's dropdown on another's screen
with no operation to account for it.
