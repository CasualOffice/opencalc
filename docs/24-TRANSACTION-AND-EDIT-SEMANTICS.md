# 24 — Transaction & Edit Semantics

Every mutation of the workbook model goes through **one** layer:
`casual-calc-transaction`. This is a **contract** doc — the op set, atomicity,
inverse, and reference-rewriting rules are fixed now because three later things
ride directly on them: undo/redo, incremental recalc (Phase 2), and
collaboration (Phase 5). Getting this boundary right is how those phases avoid a
do-over. The layer boundary is fixed by ADR-003; the op schema versions
independently ([02](02-ARCHITECTURE.md)).

> **The op set / op-schema ADR was to be Accepted before Phase 1A editing, and
> was never written** ([08](08-ADR-REGISTER.md) §Pending). Phase 1A shipped,
> and then Phase 5 shipped on top of the same op set — `PROTOCOL_VERSION` is at
> 5, and ADR-011's operational transform depends on the set being *closed*,
> which is the property that ADR was to establish. So the set is load-bearing
> for four accepted decisions and is itself undecided on the record. `DOC-039`
> is the row; this paragraph is not the fix.

Owning crates: `casual-calc-transaction` (the ops + inverses) and
`casual-calc-selection` (caret/range state), over `casual-calc-model`, using
`casual-calc-formula` for reference rewriting.

> **Status: cell-level ops implemented (E-001).** `casual-calc-transaction`
> provides `Operation` (`SetCell`, `SetValue`, `SetStyle`, `ClearCell`, `Batch`);
> `apply(workbook, op)` returns the inverse; `Batch` is atomic with rollback; a
> `History` gives undo/redo. All model mutation flows through this layer. The
> structural ops (insert/delete rows & columns) and their formula-reference
> rewriting — the subtle part below — are the next increment (E-002).

## Core principles

1. **Mutation is centralized.** No other crate mutates the model. Layout, render,
   calc, import/export are all read-or-produce; only transactions write.
2. **Every operation is atomic.** An op either fully applies or leaves the model
   unchanged — never a half-edit.
3. **Every operation returns its inverse.** `apply(model, op) -> Inverse`. Undo is
   applying the inverse; redo re-applies the op. No separate "undo implementation"
   to drift out of sync.
4. **Every operation emits a dirty set.** The set of changed cells/ranges is the
   input to incremental recalc ([40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md))
   and to tile invalidation in layout
   ([42](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md)).
5. **Operations are the collaboration unit.** The same closed op set is what a
   collaboration layer (Phase 5) transmits and transforms — so nothing above the
   transaction layer needs to change to add collaboration.

## Positions & ranges

- A cell position is `(sheet: SheetId, row: u32, col: u32)` — a grid address, not
  a text offset. (Contrast OpenDoc, whose positions are text offsets; a
  spreadsheet's edit unit is the cell.)
- A range is a rectangle `(sheet, top, left, bottom, right)`, plus the
  whole-row/whole-column and multi-range selection forms.
- In-cell text editing (editing the string/formula *within* one cell) is a
  sub-operation on that cell's content; cross-cell structure is the grid op set
  below.

## The operation set (closed)

Grouped; extended only via ADR + tracker.

> **This is the *designed* set; the implemented one is narrower and is the
> authoritative list.** `casual-calc-transaction` consolidates several of these
> — `SetCell` carries value, formula and style together; `SetSheetMetadata`
> covers the view, hidden and outline operations — arriving at **17 variants
> plus `Batch`**. That count is what ADR-011 sizes the transform matrix
> against, so the two must not be confused. Where a designed op below has no
> implemented counterpart (`MoveRange`, `CopyRange`, `MergeCells`), it is
> currently expressed as a `Batch` of cell writes.

**Cell content**
- `SetCellValue` — literal value (number/string/bool/error).
- `SetCellFormula` — set/replace a formula (stores AST + clears cached value dirty).
- `ClearCell` — content only.
- `SetCellStyle` / `ClearStyle` — style index.
- `SetRangeValues` — bulk set over a rectangle (fill, paste); one atomic op.

**Structure**
- `InsertRows(at, count)` / `DeleteRows(at, count)`
- `InsertColumns(at, count)` / `DeleteColumns(at, count)`
- `MoveRange(from, to)` / `CopyRange(from, to)`
- `MergeCells(range)` / `UnmergeCells(range)`
- `SetRowSize` / `SetColumnSize` / `SetHidden` / `SetOutlineLevel`
- `InsertSheet` / `DeleteSheet` / `RenameSheet` / `ReorderSheet`

**Definitions**
- `SetDefinedName` / `DeleteDefinedName`
- `SetNumberFormat` (on a style)
- `SetSheetView` (frozen panes, splits, active cell)

**Grouped**
- `Transaction(Vec<Op>)` — an ordered list applied atomically with a single
  combined inverse (so a paste-with-formatting is one undo step).

## Reference rewriting (the subtle, must-be-right part)

Structural ops change what every formula's references *mean*. This is where
correctness is easily lost, so the rules are explicit and owned by
`casual-calc-formula`'s reference algebra
([40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md)):

| Op | Effect on references |
| --- | --- |
| `InsertRows(at,n)` | references at/below `at` shift down by `n`; a range straddling `at` grows |
| `DeleteRows(at,n)` | references below shift up; references **into** the deleted band become `#REF!`; a straddling range shrinks |
| `InsertColumns` / `DeleteColumns` | symmetric on the column axis |
| `MoveRange(from,to)` | references **to** moved cells follow them; relative references **inside** moved formulas are preserved (Excel move semantics, not fill semantics) |
| `CopyRange` / fill | relative references shift by the copy delta; absolute (`$`) references don't (fill semantics) |
| `DeleteSheet` / `RenameSheet` | cross-sheet references become `#REF!` / are rewritten to the new name |

- **Every affected formula AST is rewritten in the same atomic op**, and the
  rewrite is captured in the inverse so undo restores the original references
  exactly (not a re-derived approximation).
- Rewriting scans only formulas that *could* reference the affected band — the
  dependency graph (Phase 2) makes this precise; before Phase 2, import-time
  precedent info bounds it. Correctness never depends on the calc engine being
  present: reference rewriting is a `-formula` operation.
- Defined names and structured (table) references are rewritten by the same rules.

## Atomicity & failure

- An op validates against model invariants *before* committing; on violation it
  returns an error code ([20](20-ERROR-CODE-REGISTRY.md)) and leaves the model
  untouched (`OC-TXN-0001/0002`).
- A `Transaction(ops)` that fails on op *k* rolls back ops `0..k` (their inverses
  are applied), so the group is all-or-nothing.

## Undo / redo

- The host (SDK/wasm/Tauri bridge) keeps paired undo/redo stacks of inverses.
- `undo` pops the last inverse, applies it, and pushes the resulting inverse onto
  redo. Because inverses are produced by `apply`, undo/redo are automatically
  correct and revision-checked.
- A grouped `Transaction` is a single undo step.

## Dirty-set emission & downstream

Each op returns, alongside its inverse, a **`DirtySet`** of changed addresses
(and, for structural ops, the shifted regions). Consumers:

- **Calc (Phase 2):** dirties those cells' transitive dependents and recomputes
  only the dirty sub-graph.
- **Layout:** invalidates only the tiles intersecting the dirty set; a single-cell
  edit repaints one tile, not the sheet.
- **Selection:** `casual-calc-selection` remaps the active cell/range across
  structural ops (a selected row that was deleted collapses predictably).

## Determinism & the collaboration seam

- Given a model and an op, `apply` is a pure, deterministic function — same result
  and same inverse every time. Golden-tested.
- Because the op set is closed, serializable, and invertible, a Phase-5
  collaboration layer can transmit ops and resolve concurrency **above** this
  layer without any change to the model, layout, or calc engine. The op-schema
  version ([02](02-ARCHITECTURE.md)) governs wire compatibility.

## Open decisions (to ADR before Phase 1A editing lands)

- ~~Exact operational-transform / CRDT choice for Phase 5~~ — **decided and
  accepted**: server-mediated operational transform over this op set, not a
  CRDT (ADR-011, [56](56-COLLABORATION-CONCURRENCY-DESIGN.md)), with document
  state held as a snapshot plus the ops since it. The op set is unchanged, as
  anticipated here; if reconciliation ever needs an operation the single-user
  editor does not have, that is evidence against the decision rather than
  licence to widen the set.
- Whether `SetRangeValues` carries a dense or sparse payload for very large fills.
- Coalescing policy for rapid same-cell edits into one undo step.
