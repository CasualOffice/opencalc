# Design note: a precedent graph that survives the edit

**For** P2-002 / PERF-04. **Not yet implemented** — this is the design step, and
it is written down first because the change touches the one part of the engine
where being wrong is silent.

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

## What this does not address

The 50 ms target is for a million cells, and the extrapolation from ten thousand
assumes the constant factor holds. It may not: at a million cells the memory
behaviour of the graph itself matters, and a `BTreeMap` of edges may cost more in
cache misses than it saves in work. The measurement at the end of step 5 is the
one that decides, and it should be taken at a size where that is visible rather
than extrapolated again.
