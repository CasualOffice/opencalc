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

## Staging

Deliberately not one change:

1. Introduce `StoredRef`/`ResolvedRef` with `store`/`resolve`, unused. Pure
   addition, fully testable on its own.
2. Move the evaluator to resolve at read. No storage change yet — the trees are
   still absolute, so this is behaviour-preserving and provable.
3. Switch `store_formula` to normalise. Interning collapses the column; the cut
   path is fixed in the same commit as this one, because it is the step that
   breaks it.
4. Move import, export and the structural rewrites onto the typed API, deleting
   the absolute paths as each is proved.

Each stage is independently revertible. Stage 3 is the one that needs the
benchmark and the full cut/paste suite green before it lands.

## What would make this not worth doing

Stated so the answer is not assumed:

- If real workbooks turn out not to be dominated by filled-down columns, the
  win is theoretical. `PERF-10` measuring resident memory first would tell us,
  and is the cheaper thing to do.
- If the typed migration cannot be completed in one series, a half-migrated
  codebase — some sites typed, some not — is **worse than not starting**,
  because the convention it leaves behind is exactly the silent-wrong-answer
  risk above.
