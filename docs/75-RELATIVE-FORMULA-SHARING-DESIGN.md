# 75 — Relative Formula Sharing (PERF-11)

_Design note. Not implemented. `PERF-11` is the row; this is what agreeing to
build looks like before anybody writes code, per [AGENTS.md](../AGENTS.md)._

## The gap

[docs/40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md) promises that "a
filled-down column of the *same* relative formula shares one AST plus a per-cell
origin". `PERF-09` delivered interning of **identical** ASTs, which is a
different thing: fill-down produces `A1*2`, `A2*2`, `A3*2` — three distinct
trees, because a `CellReference` holds an absolute `row`/`col`.

So a filled-down column of 100 000 cells holds 100 000 ASTs today. The promise
in docs/40 is not kept, and the row exists rather than the promise being quietly
lowered.

## What is actually stored now

```rust
// casual-calc-formula
pub struct CellReference {
    pub sheet: Option<String>,
    pub col: u32,             // absolute
    pub row: u32,             // absolute
    pub col_absolute: bool,   // `$` anchoring — about copying, not storage
    pub row_absolute: bool,
    // …
}

// casual-calc-model
pub struct Cell { pub formula: Option<FormulaHandle>, /* … */ }
pub struct Workbook {
    pub formulas: Vec<Expr>,
    formula_index: BTreeMap<u64, Vec<u32>>,   // fingerprint → candidates
}
```

`store_formula` fingerprints the tree and reuses an equal one. Everything needed
for sharing is already there **except** that the trees are not equal.

## The proposal

**Store a formula normalised to its own cell.** A reference that is not
`$`-anchored is kept as a *delta* from the cell holding the formula; an anchored
one keeps its absolute address, because `$` is precisely the statement "do not
move with me".

`A1*2` in `B1` and `A2*2` in `B2` both normalise to *"the cell one column left,
times two"* — one tree, and `PERF-09`'s existing interning collapses them with no
further work.

### The origin is free

docs/40 says "one AST plus a per-cell origin". **There is no per-cell origin to
store**: the origin *is* the cell's own address, which the sheet already knows
because the cell is stored at it. A cell at `(r, c)` resolves a relative
reference `(dr, dc)` as `(r + dr, c + dc)`.

That makes this a pure win on memory — one `Expr` per *shape* of formula rather
than per cell, with nothing added per cell.

### The danger, and the only acceptable mitigation

Every existing reader of `CellReference.row` / `.col` currently gets an address.
After this change, a reader that has a *stored* formula in hand gets a delta.
There are such readers in the parser, the evaluator, the shifter, import's
shared-formula path, export, the structural rewrite (`FID-24`), and the cut
repointing (`UX-CUT-03`/`UX-CUT-04`).

A single missed call site reads a delta as an address. It does not crash — it
computes a plausible wrong answer, in a spreadsheet, silently. That is the worst
failure this project can produce, and it is exactly the class `AGENTS.md` ranks
correctness above everything else to prevent.

**So the relativity must be in the type, not in a convention.** A stored
formula's references get a distinct type from a resolved one:

```rust
/// A reference as stored: relative offsets unless `$`-anchored.
pub struct StoredRef  { /* dr: i32, dc: i32, anchored flags, sheet */ }
/// A reference resolved against a cell: an address.
pub struct ResolvedRef { /* row: u32, col: u32, sheet */ }

impl StoredRef  { fn resolve(&self, at: CellRef) -> Option<ResolvedRef>; }
impl ResolvedRef { fn store(&self, at: CellRef) -> StoredRef; }
```

The compiler then finds every one of those call sites. A convention in a doc
comment would not, and this change is too wide to review by eye.

## Interaction with work already landed

- **`UX-CUT-03` / `UX-CUT-04` (a cut repoints what pointed at it).** A cut moves
  cells and their formulas must travel *verbatim*. Under normalised storage a
  moved cell's relative references would silently re-anchor to the new position
  — the opposite of what a cut means. **A cut must resolve to absolute at the
  old origin and re-store at the new one.** This is the single most likely place
  to get it wrong, and it already has tests that would go red.
- **`FID-24` (insert/delete shifts defined names).** Defined names are stored on
  the workbook, not in a cell, so they have no origin. They stay absolute.
- **Import's shared-formula path** already reconstructs followers from a master
  plus an offset — which is this design, arriving from the file format. That
  path gets *simpler*: the master is the stored form.
- **Export** must write absolute A1 text, so it resolves before emitting. `xlsx`
  can also emit `<f t="shared">`, which this makes natural rather than special.

## What to measure, and the honest expectation

`PERF-09`'s benchmark already builds an adversarial workbook. The gate is:

| workbook | today | target |
| --- | --- | --- |
| 100 000 filled-down cells, one shape | 100 000 ASTs | **1** |
| 100 000 cells, all distinct shapes | 100 000 | 100 000 (unchanged) |

The second row matters as much as the first: a design that collapses distinct
formulas has broken the document, and the benchmark must show it does not.

Recalc time is **not** expected to improve, and claiming it would be dishonest —
the evaluator still visits every cell. What improves is resident memory and the
cost of interning, which is what `PERF-10` will be able to measure once it can
measure memory at all.

## The reader this design missed: the snapshot

Found while starting stage 3, and it changes the plan.

`Workbook::formulas: Vec<Expr>` is **serialised**. Snapshots are durable and
they are on the wire — `casual-calc-collab-server` holds one per document as
`snapshot: Vec<u8>` and hands it to joining clients. The list of readers above
names the parser, the evaluator, the shifter, import, export, the structural
rewrite and the cut path. It does not name the snapshot, and the snapshot is the
one that breaks worst.

A relative `A1` serialises today as:

```json
{"reference":{"col":0,"row":0}}
```

Bare `col` and `row` — the anchor flags are `skip_serializing_if` and absent
when false. The instant `Expr` holds a `StoredRef` of the same shape, that same
JSON reads back as **offset zero**: the cell itself. An existing `B1 = A1*2`
comes back as `B1 = B1*2`, a circular reference. Every relative reference in
every saved document, silently, and `ADR-010` closes the usual escape by saying
`SCHEMA_VERSION` must not move.

Proved rather than reasoned about: adding the derive and feeding a real
serialised formula back through it is a two-line test, and it was.

### The decision: the format does not change

**Relativity is an in-memory representation only.** On write, each shared tree
is resolved back to an absolute one per cell; on read, each cell's tree is
stored relative at its own origin and re-interned. Nothing about the bytes
changes, so there is no migration, no version bump, and a mixed-version cluster
keeps working — which matters because a snapshot travels between nodes that may
not be running the same build.

What it costs is honest and worth stating: a snapshot of a 100 000-cell filled
column expands to 100 000 trees again, so **snapshot size and collaboration
payloads keep their present cost while resident memory gets the win**. The win
this row is for is memory, and that is the one that is kept.

What it implies for the code: `Expr` can no longer be serialised on its own,
because a shared tree has no single origin to resolve against. The arena's
serialisation moves up to `Workbook`, which walks cells — the only place that
knows which origin belongs to which tree.

## Staging

Deliberately not one change:

1. Introduce `StoredRef`/`ResolvedRef` with `store`/`resolve`, unused. Pure
   addition, fully testable on its own. **Landed.**
2. Move the evaluator to resolve at read. No storage change yet — the trees are
   still absolute, so this is behaviour-preserving and provable.

   **Cheaper than this note assumed.** `Evaluator` already carries
   `current: Option<(usize, CellRef)>` — the cell whose formula is being
   evaluated, saved and restored around each one, because `ROW()` and `COLUMN()`
   with no argument need exactly that. The origin is already threaded and
   already correct; stage 2 is to *use* it for references rather than to
   introduce it.

2a. **Serialise the arena from `Workbook`, not from `Expr`.** New, and it comes
   before stage 3 rather than with it: see "the reader this design missed"
   above. Absolute on the way out, relative on the way in, with the cell
   supplying the origin. Behaviour-preserving and testable on its own — a
   snapshot written before it must read back byte-identical after it, which is
   the assertion that makes it safe to build stage 3 on.
3. Switch `store_formula` to normalise. Interning collapses the column; the cut
   path is fixed in the same commit as this one, because it is the step that
   breaks it. **Also the `Expr` shape**: one tree type holding `StoredRef`, with
   entry points taking an `Origin`, rather than a generic `Expr<R>`. The
   compiler still finds every call site — `.row` stops being a `u32` address —
   so the mitigation this design turns on is kept without a type parameter
   through every signature that touches an expression.
4. Move import, export and the structural rewrites onto the typed API, deleting
   the absolute paths as each is proved.

Each stage is independently revertible. Stage 3 is the one that needs the
benchmark and the full cut/paste suite green before it lands.

## What would make this not worth doing

Stated so the answer is not assumed:

- If real workbooks turn out not to be dominated by filled-down columns, the
  win is theoretical. `PERF-10` measuring resident memory first would tell us,
  and is the cheaper thing to do.

  **Measured, and it is not theoretical.** The benchmark now reports an
  `arena` block, counted rather than sampled — nodes times `size_of::<Expr>()`,
  which is exact and works on a machine with no `/proc`:

  | | |
  |---|---|
  | cells in a filled-down `=A1*2` column | 5 000 |
  | distinct trees the arena holds | **5 000** |
  | `size_of::<Expr>()` | 80 bytes |
  | arena bytes per formula cell | **240** |
  | a cell's own cost (`PERF-10`, on the runner) | ~83 |

  So a filled column costs about **323 bytes a cell, of which 240 — three
  quarters — is the tree**, and sharing it would take that to 83. A nine-node
  formula like `IF(A1>0,SUM($B$1:B1)*C1,0)` is 720 bytes of tree against the
  same 83, where sharing removes nine tenths.

  It also confirms the premise directly: `distinctTrees` equals `cells`, so
  `PERF-09`'s interning collapses **none** of a filled column, exactly as this
  note argued it could not.

  The figure is reproducible — `cargo run -p casual-calc-benchmark` — and
  pinned by a test that fails when a filled column starts sharing trees, which
  is to say when stage 3 lands.
- If the typed migration cannot be completed in one series, a half-migrated
  codebase — some sites typed, some not — is **worse than not starting**,
  because the convention it leaves behind is exactly the silent-wrong-answer
  risk above.
