# 54 — Pivot tables

A pivot table is a **query**, and its answer is **ordinary cells**. Those two
sentences decide almost everything below.

## What a pivot is here

The query lives in `casual_calc_model::PivotTable`: a source rectangle, fields
on the row axis, fields on the column axis, page filters, and measures. The
answer is written into the sheet as plain cells.

That is not a shortcut — it is what Excel does. Open any `.xlsx` with a pivot in
it and the figures are sitting in `sheetData`; the `pivotTable` part describes
how they were produced, not what they are. It is why an imported pivot renders
correctly in a viewer that has never heard of pivot tables, and why a workbook
we refresh opens everywhere.

## Layout: tabular, not compact

Excel's default is **compact** form: every row field stacked into one column,
distinguished by indent. We write **tabular**: one column per row field.

Compact is a presentation trick. The indent carries the field structure, which
means `A5` might hold a region or a product depending on how far it is pushed
in, and nothing in the cell says which. A formula, a chart, or a second pivot
reading that column cannot tell them apart. Tabular puts each field in its own
column and every cell says what it is.

The header block, for `C` column fields and `V` measures:

| Rows | Contents |
| --- | --- |
| `top` | the measure caption (or `Values` when `V > 1`) in the corner; the outer column field's **name** over the first data column |
| `top+1 … top+C` | one row per column field, showing its **items** across the data area |
| `top+C+1` | the measure captions, repeated per column group — only when `V > 1` |
| last header row | the row-field names, in the left `R` columns |

With no column field the whole header is one row: row-field names, then measure
captions.

A label is written **only where it changes**, so an item spanning four columns
appears once. No cells are merged: a merge is something every formula
downstream then has to see through.

Subtotal and grand-total labels sit on the header row for the level at which
their line *stops* — `Grand Total` spans no field so it goes on the outer row,
`Gadget Total` stops after one so it goes on the row below. The row axis follows
the same rule, indenting a subtotal into column `c0 + depth`.

## Aggregation

Every record is accumulated into **every (row-prefix, column-prefix) pair** in
one pass. The grand total is the empty prefix on both axes; a row subtotal is a
short row prefix against the empty column prefix; a leaf figure is both prefixes
at full length.

This costs `(R+1)·(C+1)` accumulator updates per record and buys two things: one
traversal instead of one per subtotal level, and the impossibility of a subtotal
disagreeing with the rows above it.

Each accumulator holds six numbers — count, numeric count, sum, sum of squares,
product, min/max — from which every offered aggregate is derivable. Adding
`Average` beside `Sum` therefore costs nothing.

An aggregate over no numbers is **empty, not zero**. Writing 0 where a group has
no numeric data claims a measurement nobody took.

## Item order

Numbers, then text, then booleans, then errors, then blanks — Excel's ordering,
which is why a column of mixed types comes out in blocks rather than
interleaved, and a mistyped number is visible instead of hidden among the real
ones. Numbers compare with `total_cmp`, so a NaN cannot make two refreshes of
one workbook disagree.

`(blank)` is a real item, not a dropped record.

## Refresh refuses rather than overwrites

If the new report would land on a cell that was not part of the previous one and
is not empty, **nothing is written** and the caller is told where. A refresh
that filled the cells that happened to fit would leave a report half one answer
and half another.

The previous extent is stored on the pivot (`output`) rather than recomputed,
because recomputing it would only work while the source had not changed — the
one case a refresh is not for.

Refresh is a **command**, never a side effect of editing. That is Excel's
behaviour too, and the reason is the same: a report that moves under the cursor
while its source is being typed makes both unreadable.

Cells inside a report are not typable. Excel refuses the edit rather than
letting it stand until the next refresh wipes it, and a value that survives only
until an unrelated action erases it is worse than one that was never accepted.

## Round trip

| | Reading | Writing |
| --- | --- | --- |
| Imported, untouched | parsed into the model **and** retained byte for byte | the original parts, unchanged |
| Imported, refreshed or edited | — | the cells; the parts are dropped |
| Created here | — | the cells |

Parsing an imported pivot is what makes it **live** — listed in the panel,
reconfigurable, refreshable — without changing a byte of what is written back
until the user edits it.

The moment one is refreshed the retained parts are dropped, because our tabular
report and Excel's compact `pivotTable` part would then describe different
things, and a reader believes the part. Dropping it takes the cache, the cache's
records, every relationship reaching them, and the `<pivotCache>` element in
`workbook.xml` — a cache left behind with nothing pointing at it is what Excel
reports as a file needing repair.

### Known gap: a created pivot exports as values

We do not yet **write** `pivotTableDefinition`. A pivot created here saves as its
cells: correct figures, correct formatting, openable anywhere, but not a live
pivot in Excel. The definition survives in our own snapshot, so reopening in
OpenCalc keeps it live.

This is staged deliberately rather than attempted. A malformed pivot part does
not degrade — Excel prompts to repair the file, which is a worse outcome than
the gap, and two mature libraries (openpyxl, xlsxwriter) decline to write these
parts for the same reason. The route when it lands is a cache with
`saveData="0" refreshOnLoad="1"`, which needs no records part and lets Excel
rebuild the items itself. It is tracked as **PIV-02**.

### Bug found on the way in

`<pivotCaches>` was being **dropped** on import. The cache parts and the
relationship reaching them were all retained, with nothing left in
`workbook.xml` declaring them. Excel reports that as a file needing repair
rather than as a missing pivot, so every pivot workbook opened and saved before
this came back broken. `<pivotCaches>` follows `<calcPr>` in `CT_Workbook`'s
sequence, not `<sheets>`, so it needed its own synthesized wrapper in its own
place — the same shape of mistake as the `<ignoredError>` and `<protectedRange>`
wrappers in FID-25.

## Layering

| Crate | Holds |
| --- | --- |
| `model` | `PivotTable` and its parts — the query, and the extent last written |
| `eval` | reading, aggregating, laying out; `plan` (no writes) and `refresh` (writes) |
| `import` | `pivotTableDefinition` + `pivotCacheDefinition` → the model |
| `transaction` | `pivots` in `SheetMetadata`, so a definition change is undoable |
| `wasm` | a host-facing wire type; fields as source offsets, sheets as indices |

`plan` exists so the host can put a whole refresh — definition *and* figures —
through its transaction layer as one undoable step. Undo reversing the layout
while the numbers stayed would read as corruption rather than as an undo.

The planner is handed the real workbook, not a copy: at the capacity target a
copy means duplicating a million cells on every keystroke in the panel. The
definition is swapped in, planned against, and swapped straight back out;
`plan` writes no cell, so nothing between those two lines can leave it
installed.
