# 66 — Design note: a precedent graph that survives the edit

**For** [`PERF-04`](14a-ARCHIVE-CLOSED-WORK.md) and [`PERF-06`](14-EXECUTION-TRACKER.md).
**Built** — `casual_calc_eval::graph::Precedents` is the kept graph, with the
row-band buckets of `PERF-06` indexing its range edges; the edit path is flat
from ten thousand cells to a hundred thousand. §"Step five" below is the
measurement of what it cost.

This was written as the design step, before the code, because the change
touches the one part of the engine where being wrong is silent. It is kept for
that reasoning, not as a plan.

> The row it was written for, `P2-002` ("incremental dependency graph + dirty
> propagation"), was **closed and then deleted** rather than archived in the
> 2026-08-18 tracker consolidation, so its id resolves to nothing. Cite
> `PERF-04` and `PERF-06`, which survive.

## The measurement

Editing one cell in a sheet of independent formulas scales **9.2x for 10x the
sheet**: 898 µs at a thousand rows, 8.3 ms at ten thousand. The dirty set is one
cell in both cases, so none of that growth is recalculation — it is the cost of
working out *what* to recalculate.

Extrapolated, a million cells is roughly 830 ms against the 50 ms target of
[30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md). About **16x over**, which makes
this required rather than an optimisation.

## Why it is slow

`recalculate_incremental` builds the precedent graph **per pass**. Every edit
walks every formula in the workbook to discover the edges, then BFS's the
changed cell's dependents through them. Discovering the edges is O(formulas);
the answer is usually O(1).

## The shape of the fix

Keep the graph. Update it when an edit changes what it says, rather than
rebuilding it because an edit happened.

That is one sentence and four hard questions.

### 1. Where does it live?

**In the session, not the workbook.** A `Workbook` is the document — it is
serialized, snapshotted, sent across the wire, and compared for equality in
tests. A cache that lives in it is a cache that has to be in the snapshot format,
excluded from equality, and kept correct across `from_snapshot`. None of that
earns its place: the graph is derivable from the document, so it belongs beside
it rather than inside it.

Consequence to accept: a host that edits a `Workbook` directly, without a
session, gets no graph and falls back to the full recalculation. That is the
current behaviour and stays correct.

### 2. What invalidates it?

Three classes, and conflating them is where this goes wrong:

- **A value edit** — no graph change at all. This is the common case and the one
  the whole exercise is for.
- **A formula edit** — one node's outgoing edges are replaced. Cheap and local.
- **A structural edit** — insert or delete rows or columns. Every reference past
  the insertion point shifts, so most edges move. **Drop the graph and rebuild
  it on the next recalculation**, which is what happens today for every edit and
  is acceptable for an edit that already shifts references workbook-wide.

Undo and redo replay operations, so they take whichever class they replay.

### 3. What about ranges?

`=SUM(A1:A100000)` must not create a hundred thousand edges, and `A1` changing
must still find it.

**Bucket by band.** A range edge is recorded against fixed-size row bands per
column (say 1024 rows), so a range spans `ceil(rows/1024)` buckets rather than
its length, and a changed cell looks up the one bucket it falls in. A bucket
returns a superset — some cells in it are not really precedents — which costs a
few unnecessary recalculations and never a missed one. **The error is on the
safe side by construction**, which is the property to preserve when tuning the
band size.

### 4. What about defined names?

Today they are conservatively always-dirty: any formula using one is
recalculated on every pass. Keep that. A name's target can be an expression, and
resolving it precisely is a second dependency problem; the conservative answer
is correct and the population is small.

## How correctness is held

The differential tests already exist and are the reason this is approachable at
all: `incremental == full` over chains, ranges, and forty pseudo-random edits.
They must keep passing unchanged, and the pseudo-random case should be widened
to include the structural edits that drop the graph, since that path is new.

Two properties worth asserting directly, because they are what a stale graph
would violate:

- After any sequence of edits, the graph equals one rebuilt from scratch.
  Cheap to check in a test and impossible to get wrong quietly.
- A cell that should have been dirtied and was not is the failure mode that
  matters. The differential test catches it; the graph-equality check catches it
  *earlier*, which is worth more.

## The steps, in order

1. The graph type and a from-scratch constructor. No behaviour change — the
   existing per-pass path keeps running, and a test asserts the new structure
   agrees with it.
2. Hold it in the session and use it, still rebuilding on every edit. Same
   behaviour, one more place it could be wrong, all existing tests green.
3. Stop rebuilding: value edits leave it alone, formula edits patch one node,
   structural edits drop it.
4. Range buckets.
5. Re-measure against the 9.2x baseline, and record the new number next to the
   old one rather than replacing it.

Steps 1 and 2 are safe by construction — they add a second implementation and
compare it. Step 3 is where the risk is, and where the graph-equality property
earns its keep.

## Step three, worked out before writing it

Steps 1 and 2 held because they compared a new implementation against the old
one on every call. Step 3 removes the comparison: the graph outlives the edit,
and from here a wrong graph is not a wrong answer immediately — it is a cell that
stops being recalculated, and a stale number sitting somewhere nobody is looking.
So the two questions worth settling on paper are *what can move a node* and *what
can move the graph without telling it*.

### Removing one node has to be cheap, or nothing is gained

Patching a node means removing the edges it registered and re-deriving them. The
graph as built is precedent-to-dependents — the wrong direction for that: finding
a dependent's own edges means scanning every edge, which is the O(formulas) walk
this change exists to remove. Patching in the fast direction needs the slow one
recorded.

So each formula cell keeps what it registered: which keys in `direct` it appears
under, which slots in `ranges` are its, and whether it used a name. Removal then
touches only that cell's edges.

`ranges` is a `Vec` and removal is a swap, which moves an unrelated edge and
invalidates the slot somebody else recorded. The mitigation is to fix up the
element that moved, and to remove a node's slots **highest first** — then the
element swapped into place always comes from above every slot still to be
visited, so it can never belong to the node being removed, which is the one node
whose bookkeeping has already been discarded.

### The graph is built the same way it is patched

`build` is a loop calling the same attach used to patch one node, rather than a
second copy of the same walk. A from-scratch graph and a patched one cannot
disagree about what attaching means, because there is only one meaning. That
removes the entire class of bug where the two drift and the property test below
is the only thing standing between that and a wrong number.

### What can move the document without the graph hearing

This is the part that decides whether step 3 is safe, and it is a question about
the SDK rather than about the graph:

- **`RecalcPlan::Cells`** names exactly the cell each of `SetCell`, `SetValue`
  and `ClearCell` writes — the only operations that produce it. So the reported
  set is exactly the set whose outgoing edges can have changed.
- **`RecalcPlan::Skip`** is styles, widths and tab colour. None changes a
  formula, a value, or the numbering the graph is keyed by.

  Two entries were on this list and should not have been, and the reason both
  slipped is that the list was checked against the wrong question. "Does it
  change a formula?" is not sufficient — the graph is also keyed by sheet
  index, and the evaluator reads more of the document than the cells:

  - **Tab order** (`MoveSheet`) renumbers every sheet by removing and
    re-inserting, so a graph keyed by index describes the old order. It also
    changes values outright, because `SHEET()` returns a sheet's position. It
    is `Full`.
  - **Sheet metadata** (`SetSheetMetadata`) is presentation in twenty-one of
    its twenty-three fields. The other two are `hidden_rows` and
    `filter_hidden`: `SUBTOTAL`'s 101–111 codes and `AGGREGATE` skip hidden
    rows, and `Sheet::is_row_hidden` is the union of the two sets. So applying
    a filter changes what a subtotal *is* while writing no cell, which no
    dependency graph can see. It is `Full` when either bit is set, and `Skip`
    otherwise.

  The general rule this list now encodes: an operation is `Skip` only if it
  changes no value **and** leaves every key the graph uses meaning what it
  meant. Anything the evaluator reads that is not a cell — sheet position, row
  visibility — belongs in the second half of that test.
- **`RecalcPlan::Full`** is the reference-shifting and name-resolution edits, the
  two above, and drops the graph. It drops it **before** the manual-calculation
  return, not as part of recalculating: whether a recalculation is wanted now
  and whether the graph still describes this document are different questions,
  and answering only the first left manual mode computing against a stale graph
  later.
- **Undo and redo** replay any of the three and do not say which, so they drop it.
- **`apply_raw` and `workbook_mut`** are the escape hatches, and `workbook_mut`
  already documents the exact reasoning this needs: *"this cannot see what
  happens next"*. It ends the untouched guarantee for that reason, and it ends
  the graph's validity for the same one. Both drop it.
- **Spilling** falls back to a full recalculation, which writes values into cells
  the dirty set never knew about — values, not formulas. The graph survives it.

A `Recalculator` also has no way to know it has been handed a *different*
workbook, so a host reusing one across documents would get a graph describing the
previous file. Sessions own one each, which makes it structurally impossible
rather than a rule to remember.

### What is asserted

The property the design named: **after any sequence of edits, the graph equals
one rebuilt from scratch** — compared as sets, because a patched graph and a
built one differ in the order edges happen to sit in and that difference means
nothing. Order-sensitive comparison here would fail for a reason unrelated to
correctness, which is worse than not comparing at all: it trains you to ignore
it.

The reverse index is compared too, not just the edges. A leaked slot or a
forgotten `direct` key is invisible in the answer today and is exactly what makes
the *next* patch wrong.

## Step five: what it actually cost, measured

Recorded next to the old number rather than replacing it, because they are
answers to different questions and both are still true.

| per edit, 10x the sheet | 1,000 | 10,000 | ratio |
| --- | --- | --- | --- |
| rebuilt every pass (unchanged) | ~2.4 ms | ~24 ms | ~9x |
| kept graph, cell references | 125 ns | ~210 ns | ~1.5x |
| kept graph, range formulas | 2.4 µs | 12.4 µs | ~4.8x |

And a decade further up, because this note warned against extrapolating from ten
thousand and doing it anyway would have been the same mistake in a nicer font:

| per edit, 10,000 → 100,000 | 10,000 | 100,000 | ratio |
| --- | --- | --- | --- |
| kept graph, cell references | 375 ns | 375 ns | **1.00x** |
| kept graph, range formulas | 11.1 µs | 99.6 µs | **8.98x** |

**A cell-reference edit is flat**, and the 830 ms extrapolation that made this
required is now a few hundred nanoseconds that does not move when the sheet grows
by a factor of ten. That was the goal and it is met.

### The measurement had to be fixed before it could say that

The first kept-graph number came out at 19x and looked like a failure. It was
not: the timed closure ended in `workbook.sheets[0].cells.iter().count()`, which
walks every cell. A probe timing *only* that count reproduced the whole
measurement to within noise — the benchmark was timing its own return value.

That mattered here and not before because it is proportional: the same walk was
about 3% of a 2.4 ms rebuild and around 100% of a 2 µs kept-graph edit. An
optimisation that succeeds enough makes the harness around it the thing being
measured, and the first result after a big win is worth distrusting for exactly
that reason.

### So is step four required?

**No — and it is not dismissed either.** Range edges are scanned linearly and the
8.98x above shows it plainly, but 100 µs at a hundred thousand puts a million at
roughly a millisecond against a 50 ms budget. On this evidence buckets buy
headroom rather than viability.

The evidence has a limit worth stating, because it is the reason the row stays
open. The scan runs **once per cell popped off the propagation queue**, so the
real cost is `O(|dirty| x |range edges|)`, and this fixture holds `|dirty|` at
about one. A workbook of range formulas with a deep dependency chain multiplies
both, and nothing measured here would have seen it. Step four is queued on the
shape of that product, not on the number in the table.

## What this does not address

The 50 ms target is for a million cells, and the extrapolation from ten thousand
assumes the constant factor holds. It may not: at a million cells the memory
behaviour of the graph itself matters, and a `BTreeMap` of edges may cost more in
cache misses than it saves in work. The measurement at the end of step 5 is the
one that decides, and it should be taken at a size where that is visible rather
than extrapolated again.
