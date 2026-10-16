# 101 — Formula grammar extensions: whole rows, array constants, `#` / `@`, 3-D references

Status: **implemented** on `feat/formula-grammar-014-017` (rows `XL-FEAT-016`,
`XL-FEAT-014`, `XL-FEAT-015`, `XL-FEAT-017`). Reference: Excel 2026 (Microsoft
365, Windows desktop). This note is the design the four rows were built to; it
extends [40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md) §"`casual-calc-formula`".

One parser, one invariant (`parse(print(e)) == e`), so the four were built by
one worker in sequence rather than in parallel.

## 1. Whole-row references (`XL-FEAT-016`)

Excel: "5:5 — All cells in row 5; 5:10 — All cells in rows 5 through 10"
([Overview of formulas in Excel](https://support.microsoft.com/excel/get-started/overview-of-formulas-in-excel)).

**No AST change.** `StoredRef` already carried `col_implicit` and the
evaluator, the shifter and the printer already handled it — `$1:$1` worked. The
defect was lexical: a bare digit run lexes as a number, so `1:1` was
`Number Colon Number`. The lexer now emits a **word** for a pure digit run that
touches a colon (`1:1`, `Sheet1!3:3`, `$1:5`), which is exactly what `$1` already
was. `1.5:2` is still a number and then a syntax error.

The parser now also refuses a range whose two ends are different shapes
(`A1:3`, `A:B2`, `A1:B`). The last of those used to parse as a cell joined to a
whole column — a range with no meaning that evaluated to something anyway.

R1C1: the codebase has no R1C1 formula grammar (the A1/R1C1 flag of
`INDIRECT`/`ADDRESS` is A1-only), so there is nothing to make symmetric there.

## 2. Array constants (`XL-FEAT-014`)

Excel ([Use array constants in array formulas](https://support.microsoft.com/en-us/excel/use-array-constants-in-array-formulas)):
`,` separates columns, `;` separates rows (en-US, which is also the file
grammar); text in double quotes; a constant cannot nest another constant, a
reference, a formula or a function.

**AST**: `Expr::Array(Vec<Vec<Expr>>)`, rows of equal length, each element one
of `Number` (a leading `-` is folded in), `Text`, `Bool`, `Error`. The parser
enforces that, so nothing downstream has to re-check it: a reference, a
function, `%`, an empty element or a ragged row is a parse error, as it is in
Excel.

**Bounds**: the parser caps a constant at `MAX_ARRAY_CONSTANT` elements
(`casual-calc-formula::parse`). Excel bounds a formula at 8,192 characters
([specifications and limits](https://support.microsoft.com/excel/excel-specifications-and-limits)),
which cannot hold more than ~4,096 one-digit elements; the cap is above that
and far below the evaluator's `checked_array_size` ceiling. Parsing is one pass,
linear in the element count.

**Evaluation**: `Value::Array`. Scalar context takes the top-left element (the
rule the evaluator already applied to every array); array contexts — an
aggregate, a lifted scalar parameter (`COUNTIF(r,{"a","b"})`), `CHOOSE({1,2},…)`
— see the whole block.

**xlsx**: written as typed; Excel's file grammar is the en-US one.
**ODS**: OpenFormula writes inline arrays as `{1;2|3;4}` — `;` between columns,
`|` between rows (ODF 1.2 part 2, §5.13 *Inline Arrays*). Mapped both ways.

## 3. Spill reference `A1#` and implicit intersection `@` (`XL-FEAT-015`)

Excel: "placing the spilled range operator (#) after the range"
([Spilled range operator](https://support.microsoft.com/en-US/excel/spilled-range-operator));
`@` — "If the value is a range, return the value from the cell on the same row
or column as the formula … If the value is an array, pick the top-left value"
([Implicit intersection operator: @](https://support.microsoft.com/en-US/excel/implicit-intersection-operator)).

**No new AST variant.** The file format already names both as functions —
`A1#` is stored as `_xlfn.ANCHORARRAY(A1)` and `@x` as `_xlfn.SINGLE(x)` — so
the AST does too: `Function { name: "ANCHORARRAY", args: [ref] }` and
`Function { name: "SINGLE", args: [x] }`. The printer shows the operator
spelling; both names are future functions, so export prefixes `_xlfn.` and
import strips it, through the machinery every other post-2010 function uses.
Every pass that walks function arguments — shifting, renaming, the dependency
graph — handles them already, which is the reason for the choice.

`#` is lexed as the spill operator only when it immediately follows a word
character (`A1#`, `Sheet2!B3#`, `Name#`); anywhere else it starts an error
literal, so `#N/A`, `#REF!`, `#SPILL!` are untouched. `@` outside brackets is a
prefix operator; `[@Col]` is inside a structured-reference specifier and never
reaches it.

`ANCHORARRAY(r)`: the anchor's whole spilled array; `#REF!` when the cell is
not a spill anchor (Excel's answer for `A1#` on a cell that does not spill).
`SINGLE(x)`: a range is intersected with the formula's row (a one-column range)
or column (a one-row range), `#VALUE!` when there is no intersection; an array
gives its top-left; a scalar is itself.

## 4. 3-D references (`XL-FEAT-017`)

Excel ([Create a 3-D reference to the same cell range on multiple worksheets](https://support.microsoft.com/en-us/office/create-a-3-d-reference-to-the-same-cell-range-on-multiple-worksheets-40ca91ff-9dcb-4ad1-99d2-787d0bc888b6)):
accepted by SUM, AVERAGE, AVERAGEA, COUNT, COUNTA, HSTACK, MAX, MAXA, MIN, MINA,
PRODUCT, STDEV, STDEVA, STDEVP, STDEVPA, VAR, VARA, VARP, VARPA, VSTACK (and the
`.S`/`.P` forms of STDEV and VAR, which older revisions of the page list).

**AST**: `Expr::Reference3d { first, last, start, end }` — two sheet *names*
and a stored reference or range with no sheet of its own. Names, not indices:
the span is "whatever sheets sit between the two endpoints", resolved by
position **at evaluation**, so inserting, moving or deleting a sheet between
the endpoints needs no rewrite at all — which is Excel's rule for each of those
cases on the page above. Renaming an endpoint rewrites the name
(`rename_sheet_references`). Deleting an endpoint moves it one sheet inward;
deleting the only sheet of the span makes the reference `#REF!`.

Row and column edits on one sheet do **not** shift a 3-D reference: it names
the same cells on several sheets, and shifting it for one would move it for all.
(Not verified against Excel; tracked as a follow-up row.)

Outside the functions above a 3-D reference evaluates to `#VALUE!`.

**xlsx**: `Sheet1:Sheet3!A1`, quoted as one pair (`'Jan 2026:Dec 2026'!B5`).
**ODS**: OpenFormula writes `[$Sheet1.A1:$Sheet3.A1]`, which the reader used to
refuse; it is now mapped to and from `Reference3d`.

## 5. The wire

`Expr` crosses the collaboration wire by value (`WireOperation::formulas`), so a
new variant is a **loud** break: a peer that has never heard of `array` or
`reference3d` refuses the whole batch. `PROTOCOL_VERSION` moves **11 → 12** for
`Expr::Array` and `Expr::Reference3d` together (one branch, one bump), pinned
by `a_new_formula_variant_is_a_protocol_change`. `#`/`@` and whole rows cost
nothing on the wire.
