# 85 — Calculated fields, Show Values As and date grouping

**Status: proposed.** §5 through §9 are not built. §1, §3 and §4 are measured
against a clean tree at `b147efa`, and every number and every quoted string in
them is reproducible by the method in §11. §10 is the first slice, and it is the
only part of this note that is implemented alongside it.

**Why this exists.** [12](12-COMPETITIVE-ANALYSIS.md) §3.9 and its
switching-blocker #8: *"Pivot tables have no calculated fields, no Show Values
As, no date grouping. The pivot exists; the analysis on top of it does not."*
`PIV-05` closed the half of that blocker which was a **correctness** defect — a
pivot whose fields named columns its source did not have, and a save that
produced a file Excel offered to repair. What is left is the half that is
genuinely absent, and it is three features rather than one because they arrive
through three different stages of the same pipeline.

They are designed together, and filed as one row (`PIV-07`), because they share
a wire object and therefore share a protocol decision. They are **built**
separately, and §9 says in what order and why.

**Relates to** `PIV-01` (the engine), `PIV-02` (export, still `Partial`),
`PIV-04` (`GETPIVOTDATA`), `PIV-05`/`PIV-06`/`PIV-08` (the field-index space and
the three producers that corrupted it), `CHT-05`/`CHT-07` (the same wire
argument, one release earlier), `COL-54` (a new wire enum variant is a hard
break), `CF-01` and `PERF-11` (the shape this is **not**), ADR-010 (additive
schema change), ADR-011 (what may cross the operation wire), and
[54](54-PIVOT-TABLES.md), which this note extends rather than replaces.

---

## 1. Four things the measurement changed

The row this note answers records two premises and a version. One premise is
right, one is **backwards**, and the version is stale. Each is worth stating
before the design, because two of them would have produced a worse plan.

### 1.1 `PROTOCOL_VERSION` is already 8, not 7. This is 8 → 9

The row says the version *"must move 7 → 8"*. It is 8
(`crates/casual-calc-transaction/src/protocol.rs:58`); `CHT-07` moved it earlier
on the same day, for a chart, for the same reason. So the move this note asks
for is **8 → 9**.

That is not merely a corrected number. Two independent features, arriving a day
apart, each reached the same conclusion from the same rule about the same wire
object — which is the strongest available evidence that the rule in
[62](62-COLLABORATION-PIPELINING.md) is a rule and not a habit somebody has got
into. §5.4 shows that the two arrived at it by **opposite mechanisms**, which is
better evidence still.

### 1.2 The break here is the **loud** kind, and the row says it is the quiet kind

The row says *"`CHT-07` applies exactly … an additive field is the **quiet**
break — an old client drops it on its next submit"*. Measured, it is the
opposite, and the difference is one serde attribute:

| Type | attribute | an old peer given a message with a new field |
| --- | --- | --- |
| `ChartView` (`crates/casual-calc-model/src/chart.rs:219`) | `rename_all` only | parses it, silently drops the field |
| `PivotTable` (`crates/casual-calc-model/src/pivot.rs:188`) | `rename_all`, **`deny_unknown_fields`** | refuses the whole message |

Verbatim, from §11.2:

```
PROBE pivot_with_new_field=Some("unknown field `calculated`, expected one of
  `id`, `name`, `sourceSheet`, `source`, `anchor`, `rows`, `cols`, `filters`,
  `values`, `rowGrandTotals`, `colGrandTotals`, `style`, `output`, `part`
  at line 1 column 20")
PROBE chart_with_new_field=None
```

All four pivot types deny unknown fields — `PivotAxisField` (`:136`),
`PivotFilterField` (`:155`), `PivotValueField` (`:172`) and `PivotTable`
(`:188`) — so *every* field this note adds lands in the loud regime, wherever it
is added.

**The conclusion does not change; the reason does, and the reason is better.**
`COL-54` established that a new wire *variant* fails loudly and that loud is
better than quiet, because the peer that loses says so. Here an additive
*field* fails loudly too. An old tab does not silently write a stacked-shaped
pivot back flattened; it stops, and its user is told. The version bump exists to
make it stop at the handshake rather than mid-session, which is earlier and
cheaper, but the failure mode if somebody forgot the bump would be an error
rather than a corruption.

That distinction matters for §9's ordering: it means the three features may land
in three releases without any window in which a mixed fleet quietly loses data.
Each landing pays its own bump. Paying one bump up front for behaviour that is
not there yet is what `CHT-07` explicitly refused — *"a bump with nothing behind
it, followed by a wire change with no bump, is two half-measures"* — and this
note keeps that refusal.

### 1.3 The report cannot render a date at all, and every one of the three needs it to

Before any of these features, a pivot with a date on an axis produces this
(§11.1, verbatim):

```
PROBE labels=[["Date", "Sum of Amount"], ["45306", "10"], ["45307", "10"],
              ["45323", "10"], ["45672", "10"], ["Grand Total", "40"]]
PROBE label_cells=["string/None", "string/None", "string/None", "string/None",
                   "string/None", "string/None"]
```

The source cells are `2024-01-15`, `2024-01-16`, `2024-02-01`, `2025-01-15`,
each styled `yyyy-mm-dd`. The report writes the **serial numbers, as text, with
no number format**. `docs/12` §3.9 undersells this: it says a pivot over a date
column *"produces one row per distinct day"*, which suggests the labels are days.
They are five-digit integers.

The mechanism is one function. `key_at`
(`crates/casual-calc-eval/src/pivot.rs:312`) reduces a cell to a `PKey`, keeping
the `f64` and discarding the style; `key_text` (`:331`) renders a
`PKey::Number` with `format_general`. Nothing on the path can see a number
format, and the report cell is written as `Value::Text`, so it is a string even
where a number would round-trip.

This is a prerequisite rather than a nice-to-have, and §10 is built on it: a
grouped item is precisely **an item whose sort key and its display text are
different things** — `1` that reads as `Jan`, and must sort before `2` rather
than after `10`. Date grouping cannot be layered onto a path that has only one
notion of an item's text, and today's path has only one *and it is the wrong
one*.

### 1.4 The exporter would turn each of these into a `PIV-05`-shaped lie

`PIV-02` writes an authored pivot as a real `pivotCacheDefinition` +
`pivotTableDefinition`, with `saveData="0" refreshOnLoad="1"`
(`crates/casual-calc-export/src/pivot.rs:160-162`) — the cache carries no
records, so **Excel recomputes the whole report when the file is opened**.

The writer emits `<dataField name=… fld=… subtotal=… baseField="0"
baseItem="0"/>` (`:267`) with no `showDataAs`, and `<cacheField name=…
numFmtId="0"><sharedItems/></cacheField>` (`:173`) with no `<fieldGroup>` and no
`@formula`. So a pivot authored here with any of the three features, saved and
opened in Excel, is refreshed into a report that has none of them, under our
captions.

That is exactly the fault `PIV-05` was raised as a **P0** for: *"a field
captioned `Pct of total` showed raw sums"*. Reintroducing it from the writing
end instead of the reading end is not better.

**So the export line is part of each feature, not a follow-up**, and it is
stated here as a hard prerequisite so that nobody can land the model half alone
and call the feature done. `crates/casual-calc-export/src/pivot.rs` is outside
the boundary this note's author was given; §9 row **B** is the row it needs.

---

## 2. What exists

### 2.1 The model

`PivotTable` (`crates/casual-calc-model/src/pivot.rs:189`) is a query: a
`source_sheet` + `source` rectangle including its header row, an `anchor`,
`rows`/`cols` of `PivotAxisField`, `filters` of `PivotFilterField`, `values` of
`PivotValueField`, two grand-total switches, a style, the `output` extent the
last refresh wrote, and the retained `part` path.

Every field addresses its data by one number: `source_column`, *"a zero-based
offset into `PivotTable::source`"*. `PIV-05`, `PIV-06` and `PIV-08` are three
separate rows about that one number being written from the wrong index space,
and the last of them says why it is unforgiving: *"`source_column` is a `u32`
addressing a cache field"* with no in-band way to say `#REF!`, where a chart
series is a *string* and can hold one.

**This note does not widen `source_column`.** §5.3 says what it does instead.

### 2.2 The engine

`casual-calc-eval::pivot` is three stages ([54](54-PIVOT-TABLES.md) §Aggregation):

1. `read_records` (`:388`) reads the source into `Record { rows, cols, values }`,
   keeping only the columns the pivot uses, and builds an ordered `Axis` per
   *slot* — per entry in `rows` then `cols`, not per source column.
2. `accumulate` (`:627`) walks every record into **every (row-prefix,
   column-prefix) pair**, producing `BTreeMap<(Prefix, Prefix, u32), Acc>`. The
   grand total is the empty prefix on both axes; a subtotal is a short prefix.
3. `compute` (`:700`) lays the report out and reads the map inline, one
   `acc.get(&key)` per cell, as it goes.

`plan` (`:1047`) turns the report into cells, interning a `Style` per cell that
already carries `number_format` from the measure, and sizing columns with
`format_number` when a format is present.

**Two properties of this design are load-bearing for everything below** and
neither was built for it:

- The accumulator map is keyed by *prefix pair*, so every total, subtotal and
  grand total a Show-Values-As mode divides by is **already in the map**, at a
  key that is a truncation of the cell's own key. §5.1.
- An `Axis` is built per *slot*, so two entries in `rows` naming the **same**
  `source_column` already work and already get independent item lists. That is
  what multi-level date grouping is. §5.2.

### 2.3 The wire

`SheetMetadata` carries field 20 `PIVOTS` (`crates/casual-calc-transaction/src/lib.rs:301`),
beside field 19 `CHARTS` (`:300`). Capture clones the sheet's whole vector
(`:330`) and apply replaces it whole (`:370`). A pivot is edited through
`session_set_pivot` (`crates/casual-calc-wasm/src/objects.rs:975`), which
deserializes a host-facing `PivotWire` (`:655`) — a separate type on purpose, so
a panel never sees a `SheetId`.

### 2.4 The importer, and what it already knows

`PIV-05` left the importer knowing exactly the three things this note adds, and
recording each as a **loss** rather than modelling it
(`crates/casual-calc-import/src/pivot.rs:108-136`, `:186-200`):

| `Slot` / counter | what it means | today |
| --- | --- | --- |
| `Slot::Calculated` | `<cacheField formula=… databaseField="0">` | field dropped, `PivotLosses::calculated_fields` |
| `Slot::Derived` | a cache field with no source column — a group field | field dropped, `PivotLosses::group_fields` |
| `PivotSpec::shown_as` | `<dataField @showDataAs>` other than `normal` | measure kept as a plain aggregate, `PivotLosses::shown_as` |

So the compatibility report is already honest about all three, and the import
side of this work is *removing* report entries, not adding them. The importer is
out of scope for the first slice and its row is §9 **F**.

---

## 3. What Excel actually does

Checked against `schemas/ooxml/sml.xsd` in this repository rather than from
memory, because two of the three features have a menu that is larger than the
file format.

### 3.1 Show Values As: fifteen menu entries, nine tokens, six extensions

`ST_ShowDataAs` (`schemas/ooxml/sml.xsd:1515-1527`) has exactly **nine**
enumerations:

```
normal  difference  percent  percentDiff  runTotal
percentOfRow  percentOfCol  percentOfTotal  index
```

Excel's *menu* has fifteen entries. The six that are not in the list above —
**% of Parent Row Total**, **% of Parent Column Total**, **% of Parent Total**,
**% Running Total In**, **Rank Smallest to Largest**, **Rank Largest to
Smallest** — arrived in Excel 2010 and are not expressible in
`<dataField>` at all. They travel in the `x14` extension namespace, as
`<extLst><ext><x14:dataField pivotShowAs=…>`, which this repository has no
schema for and the writer has no seam for. `pivotShowAs` appears nowhere in
`schemas/`.

`<dataField>` also carries `@baseField` (default `-1`) and `@baseItem` (default
`1048832`) (`sml.xsd:1270-1281`). Four of the nine modes are meaningless without
them: `difference`, `percent` and `percentDiff` need a base field **and** a base
item, and `runTotal` needs a base field to run along. `@baseItem` additionally
has reserved sentinel values above any real item index for *(previous)* and
*(next)*, which are a second addressing space rather than an index.

**The five that need nothing but the accumulator map** are therefore
`normal`, `percentOfTotal`, `percentOfRow`, `percentOfCol` and `index`. That
line is not drawn for convenience; it is drawn by the schema.

### 3.2 Date grouping: eight units, and a cache field per level

`ST_GroupBy` (`sml.xsd:805-816`): `range`, `seconds`, `minutes`, `hours`,
`days`, `months`, `quarters`, `years`. It is the `@groupBy` of `CT_RangePr`
(`:795-804`), which also carries `@autoStart`, `@autoEnd`, `@startNum`,
`@endNum`, `@startDate`, `@endDate` and `@groupInterval` (default 1);
`CT_RangePr` sits inside `CT_FieldGroup` (`:786-794`) beside `<discretePr>`
(grouping chosen items by hand) and `<groupItems>` (the resulting item list),
with `@par` and `@base` naming the parent and base cache fields.

Three facts about Excel's behaviour follow from that shape, and each one is a
design decision below:

- **Grouping into more than one level produces more than one field.** Ticking
  Years, Quarters and Months in Excel's Group dialog puts **three** fields on
  the row axis, each a cache field with `@base` pointing at the original. The
  Months field's items are the *twelve* months pooled across years, so `Jan`
  under `2024` and `Jan` under `2025` are the same item at that level.
- **Grouping is a property of the cache, not of the pivot.** Every pivot table
  sharing a cache is grouped together, and ungrouping one ungroups all. This is
  a long-standing Excel wart. We have no shared cache — each `PivotTable` names
  its own `source` — so we cannot reproduce it and would not want to. §7.
- **`range` is the same machinery on numbers.** Excel's Group dialog on a
  numeric field offers *starting at / ending at / by*, which is `groupBy="range"`
  with `@startNum`/`@endNum`/`@groupInterval`.

Google Sheets differs in two visible ways worth recording: it offers compound
buckets (`Year-Month`, `Year-Quarter`) as a *single* field where Excel needs two,
and it offers `Day of week`, which Excel has no token for. §7 refuses both, and
§8 says why the refusal costs nothing that cannot be added later.

### 3.3 A calculated field is a formula over **sums**, not over records

This is the fact that decides §5.3, and it is the single most misunderstood
thing about Excel pivot tables.

A calculated field is stored as `<cacheField name="Bonus" formula="Amount*0.1"
databaseField="0"/>`. Excel evaluates it by binding each field name in the
formula to the **sum of that field over the group**, and then applying the
formula once. It does not evaluate per source record and aggregate afterwards.

The consequence users trip over: a calculated field `Units*Price` reports
`SUM(Units) × SUM(Price)`, not `SUM(Units × Price)`. And a calculated field's
grand total is the formula applied to the grand total's operands, so **the
column does not add up** — the grand total of `Revenue/Units` is total revenue
over total units, not the sum of the ratios above it.

Both of those are correct for the questions calculated fields are actually used
for (a rate, a commission, a weighted ratio), and both are what our accumulator
map produces naturally, since the map holds an `Acc` per prefix pair and the
grand total is just the empty prefix.

The formula language is not the workbook formula language. Excel refuses cell
references, ranges, defined names and any function taking a range; names with
spaces are written bare or quoted. Field names bind; nothing else does.

---

## 4. The evaluation domain, stated once

`PIV-07` says a calculated field is *"not `CF-01`'s shape"*, and the arithmetic
is worth writing down because it is what makes all of this affordable.

`CF-01`/`PERF-11` evaluate a rule **per grid cell in view**: the domain is the
visible window of a sheet that may hold a million cells, and the work scales
with what is on screen and how many rules are attached.

Here the domain is the **accumulator map**, and its size is the number of
distinct (row-prefix, column-prefix) pairs, which is bounded above by the number
of cells in the *report* — not by the number of records, and not by the sheet.
A pivot of three row-field levels over four column-field levels with three
measures is in the low hundreds of report cells; a wide one is a few thousand.
The engine already visits every one of them once, in `compute`'s layout loop.
Adding a derivation per report cell is therefore adding a constant to a loop
that already runs, not adding a loop.

What *does* scale with the records is one thing, and §5.3 names it: a calculated
field needs its operand columns accumulated, which grows `accumulate`'s inner
work from `(R+1)·(C+1)·V` to `(R+1)·(C+1)·(V+O)` per record for `O` distinct
operand columns. `O` is the number of distinct field names across all calculated
fields, typically one or two. It is measured before this ships (§9 **E**), and
it is the only part of this note that touches a per-record path at all.

---

## 5. The model, and what it must gain

Each decision names the OOXML it maps to, and each is made rather than offered.

### 5.1 Show Values As is a field on the measure, and its bases are already in the map

**Decision.** Add to `PivotValueField`:

```rust
/// `<dataField @showDataAs>`: the measure is reported as a derivation of
/// itself rather than as the aggregate. `None` is the schema's `normal`.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub show_as: Option<PivotShowAs>,
/// `<dataField @baseField>`: the grouping field a base-relative mode is
/// measured against, as a `source_column`. Unset for the modes that need
/// none.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub base_field: Option<u32>,
/// `<dataField @baseItem>`: which item of `base_field` is the base.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub base_item: Option<PivotBaseItem>,
```

`PivotShowAs` carries **all nine** `ST_ShowDataAs` tokens. `PivotBaseItem` is
`Previous`, `Next`, or `Item(u32)` — the three things `@baseItem` can mean.

**Why the whole enum, when only five modes are honoured.** `COL-54` is the
reason: a new externally-tagged variant is a break an old peer cannot read at
all. Adding `RunTotal` later would therefore be a protocol change on top of the
protocol change this note already asks for. The variant set is completed once,
in the bump that has working behaviour behind it, so that every later mode is a
*behaviour* change. The same argument carries `base_field` and `base_item`: they
are the whole of what the four unhonoured modes need, they are `Option` and
skipped when absent, and adding them costs an unupgraded tab nothing while
omitting them costs it a session later.

**Honoured in the first release** — five of nine, and the five are exactly the
ones whose base is a truncation of the cell's own key:

| Mode | value at `(r, c)` | base key |
| --- | --- | --- |
| `normal` | `acc[r, c]` | — |
| `percentOfTotal` | `v / base` | `([], [])` |
| `percentOfRow` | `v / base` | `(r, [])` |
| `percentOfCol` | `v / base` | `([], c)` |
| `index` | `v · acc[[], []] / (acc[r, []] · acc[[], c])` | all three, above |

Every one of those keys is written by `accumulate` today, for every record,
whether or not the report shows the corresponding total — `row_grand_totals` and
`col_grand_totals` control the *layout*, not the map. So `% of Row Total` is
correct on a report with the grand-total column switched off, which is the case
that would have caught a design that read the base out of the rendered report.

**Refused in the first release** — `difference`, `percent`, `percentDiff`,
`runTotal`. Each needs `base_field`, and three of them need `base_item` with its
*(previous)*/*(next)* sentinels. They are refused **loudly**, at
`session_set_pivot`, with the mode named in the error. Accepting one and
computing `normal` would be the `PIV-05` fault written by hand.

**Consistency is by construction, not by care.** On a `percentOfRow` measure the
row's grand-total cell has key `(r, [])`, which is its own base, so it reports
`1`. A subtotal row has key `(r', [])` for its own shorter prefix, so it reports
`1` too, and the leaf cells under it sum to `1` because they and it come from the
same accumulators. There is no second code path for a total to disagree from —
the same property [54](54-PIVOT-TABLES.md) claims for sums.

**Degenerate bases** follow the module's existing rules rather than new ones. A
base over no numbers is `Value::Empty`, because *"an aggregate over no numbers
is empty, not zero"*. A base that is numerically zero is `#DIV/0!`, which is
what `Average` already answers with.

**The default number format.** A percentage mode with no `number_format` writes
`0.00%`; `index` writes `0.00`. Without this the first thing a user sees is
`0.4848484848`, and `number_format` already flows to the cell style in `plan`
(`:1115-1146`), so this is a default, not a mechanism.

### 5.2 Date grouping is a field on the axis field, and a level is a field

**Decision.** Add to both `PivotAxisField` and `PivotFilterField`:

```rust
/// `<cacheField><fieldGroup><rangePr>`: bucket this field's values before
/// grouping by them. `None` groups by the value itself, as today.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub group: Option<PivotGroup>,
```

```rust
pub struct PivotGroup {
    /// `@groupBy`. All eight `ST_GroupBy` tokens.
    pub by: PivotGroupBy,
    /// `@groupInterval`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<f64>,
    /// `@startNum` / `@startDate`, as a serial. Unset is `@autoStart`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<f64>,
    /// `@endNum` / `@endDate`. Unset is `@autoEnd`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<f64>,
}
```

`PivotGroupBy` carries all eight tokens, for §5.1's reason. **Honoured in the
first release: `years`, `quarters`, `months`** — the three
[12](12-COMPETITIVE-ANALYSIS.md) §3.9 names. `days`, `hours`, `minutes`,
`seconds` and `range` are refused at `session_set_pivot`, by name. `days` with
the default interval is what an ungrouped date field already does, so the
refusal costs nothing today.

**Multi-level grouping is repeated axis fields, and it needs no new mechanism.**
Years + Quarters + Months is three `PivotAxisField`s naming the same
`source_column` with three different `group.by`. This works because `Axis` is
built per *slot* rather than per column (`read_records`, `:475-479`) — checked
in the code rather than assumed, and named here because it is the reason a
group-level does not need its own type.

That also reproduces Excel's pooling exactly: the Months field's items are the
twelve month ordinals, so `Jan` appears once per year group because the *pair*
`(2024, Jan)` is what the row tuple holds, not because months were re-derived
per year.

**On the filter axis too**, because a page filter over an ungrouped date column
beside a grouped row field would offer a dropdown of five thousand days to
narrow a report showing twelve months. It is the same key derivation, so it is
the same code.

**Grouping changes the key, not the label, and that is the whole point.** A
grouped item's `PKey` is the **ordinal** — `2024`, `1..4`, `1..12` — so
`PivotSort::Ascending` orders months January-first because it sorts numbers, and
`Jan` does not land between `Feb` and `Mar` the way a text key would. The label
`Jan` is display, produced by the seam §10 installs. This is the direct reason
§10 goes first: without a display/identity split, a grouped field must choose
between sorting correctly and reading correctly.

**A non-numeric value in a grouped field groups as `(blank)`.** Excel refuses
the grouping outright when the field holds text. Refusing at refresh time would
mean one stray cell in a fifty-thousand-row export blanks the whole report, and
`(blank)` is already a real item here rather than a dropped record
([54](54-PIVOT-TABLES.md) §Item order). A stray value is visible in a named
bucket instead of invisible in a refusal.

**The epoch is the workbook's.** `Workbook::date1904` exists
(`crates/casual-calc-model/src/workbook.rs:406`) and `casual-calc-layout` has
`format_number_1904` beside `format_number`. Deriving a year from a serial
without consulting it is wrong by 1462 days for every Mac-authored workbook, and
it is the kind of wrong that produces a plausible report. (`plan`'s column-width
estimate already uses the 1900 form unconditionally at `:1172`; that is a
pre-existing cosmetic inaccuracy in a width, not in a value, and it is named
here rather than fixed silently.)

### 5.3 A calculated field is a measure with no source column

**Decision.** Add to `PivotTable`:

```rust
/// Fields computed from the aggregated values rather than read from the
/// source. `<cacheField @formula databaseField="0">`.
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub calculated: Vec<PivotCalculatedField>,
```

```rust
pub struct PivotCalculatedField {
    /// The field's name, which is what the field list and the report caption
    /// show and what a sibling formula may not reference (§7).
    pub name: String,
    /// The formula text, in Excel's pivot dialect: field names and operators,
    /// no cell references, no ranges, no defined names.
    pub formula: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_format: Option<String>,
}
```

and to `PivotValueField`:

```rust
/// The calculated field this measure reports, as an index into
/// `PivotTable::calculated`. When set, `source_column` and `aggregate` are
/// not read.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub calculated: Option<u32>,
```

**Why an index beside `source_column` and not an enum replacing it.** Three
options were on the table and two are refused on the record:

- *Widen `source_column` to `PivotFieldRef { Column(u32), Calculated(u32) }`.*
  This changes an existing field's **type** on the wire and in the snapshot, so
  it is not additive under ADR-010 — `SCHEMA_VERSION` moves and every golden
  snapshot is rewritten. It is also the larger blast radius: `source_column` is
  read in the engine, the wasm wire type, the exporter, the importer and
  `structural.rs`.
- *Address a calculated field by an index **past the end** of the source width,
  the way OOXML's cache-field index does.* **Refused, and this is the important
  one.** That is precisely the collision `PIV-05` was a P0 about — *"those two
  index spaces coincide only while every cache field is a source column, and a
  calculated field or a group field ends that immediately"*. Introducing the
  ambiguity deliberately, one row after paying to remove it, would be the worst
  decision available. `source_column` keeps meaning exactly what `PIV-05` made
  it mean.
- *A separate index, chosen.* Additive, so ADR-010 holds and old snapshots are
  byte-identical. `Option::is_none` means an ordinary measure serializes exactly
  as before.

**The formula binds field names to sums, per §3.3.** The row's own phrasing —
*"field names bound to sibling measures"* — is half right: the binding is not to
cells, which is the important half. But it is not to sibling *measures* either.
`Amount` in a calculated field's formula means `SUM(Amount)` over the group,
whatever aggregate a sibling `PivotValueField` on `Amount` happens to use, and
whether or not any sibling names it at all. That is Excel, and — decisively —
it is what the `<cacheField @formula>` we write **means to Excel** when the file
is reopened. Any other binding gives the same file two different answers
depending on which program refreshed it. §12 records this as the one decision to
overturn if the product disagrees, because it is cheap before the wire moves and
expensive after.

**Parsing** reuses `casual-calc-formula`'s parser rather than adding a second
one, with the identifier nodes rebound to field names at evaluation; anything
carrying a cell reference, a range, or a defined name is **refused at
`session_set_pivot`**, with the offending token named. Excel refuses the same
set. A formula that cannot be parsed is refused rather than stored, because a
stored-but-unevaluable formula is a report cell that is `#NAME?` forever with no
way to see why.

### 5.4 The wire: `PROTOCOL_VERSION` 8 → 9

`SheetMetadata` field 20 `PIVOTS` is a **whole-vector replace**, exactly as
field 19 `CHARTS` is. §1.2 measured that the consequence is not the same:

> **`PROTOCOL_VERSION` moves to 9.** A chart's additive field was the *quiet*
> break, because `ChartView` tolerates unknown fields and an old client writes
> the vector back without them. A pivot's additive field is the **loud** one,
> because all four pivot types deny unknown fields and an old client refuses the
> message. The version moves for both, and the fact that one rule catches two
> opposite mechanisms is the argument that it is a rule.

The bump is paid by whichever of the three lands first, and again by each later
one, per §1.2. The gate that will say so is §9 **C** — a test in
`casual-calc-transaction` shaped like
`a_new_conditional_format_variant_is_a_protocol_change`
(`crates/casual-calc-transaction/src/tests.rs:2382`), asserting the wire field
set of the four pivot types with a failure message that gives the instruction.
That test is what turns this paragraph into something a future change cannot
walk past.

---

## 6. Evaluation order

The three features are three stages, and running them in the wrong order gives
answers that are individually defensible and collectively wrong. The order is:

1. **Key derivation** (§5.2), in `read_records`, *before* anything is counted. A
   grouped field's records carry the bucket ordinal, so the axes, the prefixes
   and the map are all in terms of buckets. Nothing downstream knows grouping
   happened.
2. **Accumulation**, unchanged, over `V` measures **plus `O` operand columns**
   that calculated fields name (§4). Operand accumulators are not measures and
   never reach the layout.
3. **Resolution**, new, once per (row-prefix, column-prefix) pair — the stage
   `compute` does inline today and must stop doing inline:
   - **3a. Plain measures.** `acc[key].finish(aggregate)`, as now.
   - **3b. Calculated measures.** Evaluate each formula with every field name
     bound to the operand accumulator's `Sum` **at this same prefix pair**. A
     calculated field may not name another calculated field (§7), so 3b needs no
     ordering within itself and no cycle check.
   - **3c. Show Values As.** Divide by the base key, which is a truncation of
     this cell's own key (§5.1).
4. **Layout**, reading a resolved grid instead of the map.

**3b before 3c** is a decision, not an accident: it means a calculated field can
itself be shown as a percentage of the grand total, and the percentage is of the
calculated field's own grand total — the formula applied to the total's
operands. The reverse order would divide operands by totals and then combine
them, which is a different and meaningless number.

**Resolution is separated from layout** because both 3b and 3c need values at
keys other than the cell's own, and the layout loop visits cells in report order,
which is not an order in which a base is guaranteed to have been computed. The
existing inline `acc.get(&key)` cannot express "the value of this measure at a
shorter prefix" without recomputing, and recomputing in the layout loop is how a
subtotal ends up disagreeing with the rows above it — the exact failure
[54](54-PIVOT-TABLES.md) says the one-pass accumulation exists to make
impossible.

---

## 7. What is deliberately not built

A design note that only says yes is not a design. A refusal here is cheap for
one specific reason, and it is worth stating before the table: **an imported
pivot's part is retained byte for byte until the user edits it**, so a refused
feature is a *report entry*, not a lost file. `PIV-05` already counts all three
kinds. The cost of refusing is a capability, not data.

| Not built | Why |
| --- | --- |
| The six Excel-2010 Show Values As modes (**% of Parent Row/Column/Total**, **% Running Total In**, **Rank** ×2) | Not in `ST_ShowDataAs`. They are `x14:dataField/@pivotShowAs` in an extension namespace this repository has no schema for and the writer has no seam for. The maths for the two parent modes is a truncation of the key like any other — the refusal is about the **file**, not the arithmetic. |
| `difference`, `percent`, `percentDiff`, `runTotal` in the first release | Each needs `@baseField`, three need `@baseItem` with *(previous)*/*(next)* sentinels — a second addressing space with its own edge cases (what "previous" means at the first item, and at a subtotal). Modelled in §5.1 so adding them is a behaviour change, not a protocol change. `runTotal` is the most asked-for of the four and should be the next one built. |
| `range`, `days`, `hours`, `minutes`, `seconds` grouping in the first release | Same treatment: in the enum, refused by name. `days` at the default interval is today's behaviour, so the refusal is not a gap. |
| **Calculated items** (`<pivotField><items><item f="1">`) | A formula over the *items* of one field — `West − East` as a row. A different feature that shares a name-shaped confusion with calculated fields, a different part of the schema, its own item-index arithmetic, and its own interaction with subtotals. Its own row. |
| Manual grouping (`<discretePr>`, "group these three regions") | An item→group mapping table on the wire, and an item identity that survives the source changing under it. Excel's own answer here is fragile. Its own row if it is wanted. |
| Compound buckets (`Year-Month`, `Year-Quarter`) and `Day of week` | Google Sheets has them; the OOXML token set does not. `Year-Month` is two fields here, which is one more column and the same information. A compound token would be unwritable, and an unwritable model field is the `PIV-05` shape again. |
| Excel's shared-cache grouping semantics | In Excel, grouping a date field groups it in **every** pivot on that cache, and ungrouping one ungroups all. We have no shared cache — each `PivotTable` names its own `source` — so grouping is per pivot. This is a divergence that helps the user, which is the test [12](12-COMPETITIVE-ANALYSIS.md) sets, and it is recorded so it is a decision rather than an accident. |
| A calculated field referencing another calculated field | Excel allows it. It turns §6 stage 3b into a dependency graph inside a report cell, with cycles, an evaluation order, and an error to invent for a cycle. Refused at `session_set_pivot` by name, with the reason. |
| Calculated fields on the row, column or filter axes | An expression over aggregates has no per-record value, so there is nothing to group by. Excel refuses it too. `PivotAxisField` and `PivotFilterField` keep `source_column` only, which is why §5.3 adds `calculated` to `PivotValueField` alone. |
| Slicers and timelines | [12](12-COMPETITIVE-ANALYSIS.md) §3.9 lists them beside these three, and they are not these three: a slicer is `<x14:slicer>` plus a drawing plus a persisted selection shared between pivots. A feature, not a variant. Its own row. |
| Compact layout | [54](54-PIVOT-TABLES.md) refuses it already and this note does not reopen it. Named because Excel's date grouping is usually *seen* in compact form, and the tabular equivalent — one column per level — is the shape §5.2 produces. |

**Built:** three total-relative Show Values As modes plus `index`, date grouping
into years, quarters and months at any number of levels, and calculated fields
over sums. Those are the three things `docs/12`'s blocker #8 names.

---

## 8. Failure modes

Named before they are met, because each has a shape this repository has already
paid for once.

- **An old client refuses the session instead of losing data.** §1.2. The bump
  makes it refuse at the handshake. Without the bump it refuses at the first
  submit carrying a pivot, which is later, noisier and still not silent.
- **A saved file refreshed by Excel loses the feature.** §1.4. Mitigated only by
  §9 **B** landing with the feature, not after it.
- **A grouped field whose source column stops being a date.** The group key
  derivation sees non-numbers and buckets them as `(blank)` (§5.2), so the
  report degrades to a blank row rather than to a refusal or to a wrong month.
- **A base field that is no longer on an axis.** `base_field` is a
  `source_column` and `structural.rs` renumbers those (`PIV-06`, `PIV-08`), so
  it moves with its column. A base field naming a column that is not on any axis
  is refused at `session_set_pivot`, the same rule `lookup` already applies —
  *"a field that is not on any axis is refused; answering would report a figure
  the report does not show"*.
- **A calculated field whose operand column is deleted.** `PIV-08` established
  that a pivot cannot hold `#REF!` the way a chart series can. The formula is
  text, so it survives; the operand no longer resolves, and the measure reports
  `#REF!` in every cell rather than a plausible number. That is the loud
  outcome and it is the right one.
- **`GETPIVOTDATA` against a derived measure.** `lookup` (`:534`) re-aggregates
  the source rather than reading the report, deliberately. A calculated or
  derived measure must go through the same resolution stage or the two will
  disagree — which means §6 stage 3 has to be callable for a single key, not
  only for a whole report. Designing stage 3 as a function of `(map, key)` from
  the start is what makes that free; designing it as a pass over a grid is what
  would make it a rewrite.

---

## 9. The order to build in

Each row is a separate landing, and the letters are what §12 asks to be turned
into tracker ids.

| | What | Where | Bump |
| --- | --- | --- | --- |
| **A** | **The report cell learns its type and its format.** A numeric item's label is written as the number with the source column's format, and the item's *display text* is separated from its *identity*. Fixes the measured `45306` defect (§1.3). | `casual-calc-eval` only | no |
| **B** | **The exporter writes what the model holds**: `@showDataAs` + `@baseField`/`@baseItem` on `<dataField>`, `<fieldGroup><rangePr>` on the grouped `<cacheField>`, and `@formula databaseField="0"` for a calculated one. **Must land in the same release as C/D/E** (§1.4). | `casual-calc-export` | no |
| **C** | **Show Values As**, five modes honoured, four refused by name. The protocol test that pins the pivot wire shape (§5.4) lands here. | model, eval, transaction, wasm | **8 → 9** |
| **D** | **Date grouping**, years/quarters/months, any number of levels, on axis and filter fields. | model, eval, wasm | bump if a separate release |
| **E** | **Calculated fields**, with the operand-accumulation cost measured against [30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md) before it ships. Blocked on §12 Q1. | model, formula, eval, wasm | bump if a separate release |
| **F** | **The importer stops reporting these as losses** and models them instead, removing three `PivotLosses` counters. Strictly a widening; nothing that resolves today stops resolving. | `casual-calc-import` | no |
| **G** | **The panel.** Every control above is unreachable from the editor until this lands, so C/D/E are engine features reachable only through the SDK. | `webapp` | no |

**A is first, and it is the one the rest depends on.** Not because it is
smallest, but because all three features change *what a report cell is*, and A
is the only part of that change that needs neither the wire nor the exporter:

- Date grouping needs an item whose sort key (`1`) and display text (`Jan`)
  differ. Today there is one function, `key_text`, and it is both.
- Show Values As needs a measure whose format is **decided by the report** —
  `0.00%` — rather than copied from the definition.
- A calculated field needs a measure with no source column and therefore no
  inherited format at all.

All three are the same seam, and today that seam does not exist: a label is a
pre-formatted string and the only format in the pipeline arrives from
`PivotValueField::number_format`. A also fixes a defect that is live now,
independent of any of this, which is the test of whether a foundation was worth
laying separately.

---

## 10. Slice A, as built

Implemented alongside this note; C through G are not.

**What changed.** `PKey` gains a display form distinct from its identity form.
`key_text` remains the **identity** — what a `PivotFilterField::selected`
compares against and what `GETPIVOTDATA` matches — so no persisted selection and
no existing formula changes meaning. A new `key_display` renders an item using
the source column's number format, and it is used for every place an item is
*shown*: report row labels, column headers, subtotal captions, and the field-item
list a filter dropdown is built from. Matching accepts **either** rendering, so
a selection stored before this change and a selection made after it both work.

**A numeric leaf label is written as a number**, not as text, carrying the
column's format — which is what Excel writes, and what makes the column-width
estimate (`plan`, `:1168-1176`) size the label rather than the serial. A subtotal
caption stays text, because `2024-01-15 Total` is not a number.

**Where the format comes from**, and the three things that rule out the obvious
alternatives. It is the format on the **first data cell of the column that holds
a value**:

- **Not** the commonest format, because that is a second pass and a tie-break
  rule for something OOXML itself carries as a single `<cacheField @numFmtId>`.
  A format is a property of the field, not of the record.
- **Not** read from the records the refresh keeps, even though that would be
  free: the kept records are post-filter, so narrowing a page filter would
  respell the items that survive it. Determinism outranks the saved pass.
- **Not** "the first cell that has a format", which is the version this slice
  was first written with. That scans the whole column whenever there is no
  format at all — the ordinary case for a text field — and so adds a full
  column scan per field to every refresh at the capacity target
  [30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md) sets. Stopping at the first
  *valued* cell is bounded.

A blank leading record is skipped rather than deciding the field has no format,
which is pinned by a test: a column whose first record is missing is ordinary,
and a field silently losing its format because of one is not.

**What it deliberately does not do.** It does not group, it does not touch the
wire, and it does not touch the exporter. It changes what a report cell holds,
which is a change to *cells*, and cells are not part of the pivot definition —
so an authored pivot's `.xlsx` is unaffected, and Excel's `refreshOnLoad` rebuild
produces the same labels it always would have.

**The ambiguity it does widen, stated rather than glossed.** Two items can now
answer to one name where before only one did: a text item spelled `2024-01-15`
and a date item that *displays* as `2024-01-15` are both matched by a filter
selecting `2024-01-15`. The class is not new — a text item `45306` and the
number `45306` have always collided, because `key_text` renders both the same —
but this widens it from "the identity form collides" to "either form collides".

It is accepted rather than fixed, for one reason: the alternative is to make a
selection carry the item's *type* as well as its text, and
`PivotFilterField::selected` is a `Vec<String>` in a persisted, wire-visible,
`deny_unknown_fields` struct. Changing its shape is the protocol change §5.4
describes, paid for a collision that needs a workbook mixing a date column with
text that looks like a date. It is named here so a later reader meets it as a
decision rather than as a surprise.

---

## 11. Reproducing this

### 11.1 The date labels (§1.3)

Add to `crates/casual-calc-eval/src/pivot_tests.rs` a test that builds a
two-column source — a `Date` column holding the serials `45306`, `45307`,
`45323`, `45672` each styled `yyyy-mm-dd`, and an `Amount` column of `10` — puts
the date column on the row axis and `Sum of Amount` on values, refreshes, and
panics with the rendered grid together with, for each label cell, whether it is
a number or a string and what number format its style carries. Output as quoted
in §1.3.

### 11.2 The wire (§1.2)

Add to `crates/casual-calc-transaction/src/tests.rs` a test that serializes a
`PivotTable`, splices `"calculated":[],` into the JSON, and deserializes it
back; then does the same to a `ChartView` with an unknown key. Panic with both
errors. Output as quoted in §1.2. The pivot JSON in full, for reference:

```json
{"id":1,"name":"P1","sourceSheet":"00000000000000020000000000000001",
 "source":{"start":{"row":0,"col":0},"end":{"row":4,"col":1}},
 "anchor":{"row":0,"col":0}}
```

### 11.3 The schema facts (§3)

```
grep -n -A 20 'simpleType name="ST_ShowDataAs"' schemas/ooxml/sml.xsd
grep -n -A 20 'simpleType name="ST_GroupBy"'    schemas/ooxml/sml.xsd
grep -n -A 14 'complexType name="CT_FieldGroup"' schemas/ooxml/sml.xsd
grep -n -A 20 'complexType name="CT_DataField"'  schemas/ooxml/sml.xsd
grep -rn "pivotShowAs" schemas          # no matches: the 2010 modes are x14
```

---

## 12. Decided by the product owner, 2026-08-30

Both questions below were put to the product owner and answered. They are kept
in full rather than deleted, because a decision is only worth as much as the
alternative it was chosen over.

**Q1 is settled: Excel's binding.** Asked which to take, the answer was "do what
our competitors use" — and for this question the two competitors do not
disagree in a way that leaves room. Excel's binding is the one that survives the
round trip: `<cacheField @formula>` is what we write, Excel reads it as the sum
of the source field, and choosing anything else hands one file two answers
depending on who opens it. Sheets' explicit-aggregate shape is what a user
usually *means*, and it is not available to a document that has to be an `.xlsx`
— it would need a formula language of our own and a file Excel refreshes
differently from us. If that shape is ever wanted it is an **additional**
feature that does not round-trip, not a different reading of this one.

**Q2 is settled: three releases.** §1.2 measured the failure as a refusal rather
than a corruption, which makes the cost a fleet cost — every unupgraded tab
loses its session at each bump — and not a data risk. Three keeps each feature's
wire change independently revertible, which is worth more than the two bumps it
saves.

## 12a. What this note still does not decide

**Q1 — what a calculated field's operands bind to.** §5.3 decides *Excel*: every
field name in the formula means the **sum** of that source field over the group,
whatever aggregate a sibling measure uses. The reason is not preference — it is
that `<cacheField @formula>` is what we write, and Excel reads it that way, so
any other binding gives one file two answers.

The alternative is Google Sheets' shape, where a custom formula names aggregate
functions explicitly (`=SUM(Revenue)/COUNT(Revenue)`) and the ambiguity does not
arise. It is what a user usually *means* — Excel's binding is the single most
complained-about pivot behaviour, because `Units*Price` reports
`SUM(Units)×SUM(Price)` — and choosing it would mean a formula language of our
own, a file that Excel refreshes differently from us, and a different wire
shape.

**This is the product owner's call and it must be made before E lands**, because
it decides the wire, and the wire is what a protocol bump makes expensive to
revisit. It does not block A, C or D.

**Q2 — whether C, D and E ship in one release or three.** One release is one
bump and one panel change; three releases are three bumps, each of which costs
every unupgraded tab its session (§1.2 measures that this is a refusal, not a
corruption, so it is a cost rather than a risk). The engineering answer is
three; the fleet answer may be one.
