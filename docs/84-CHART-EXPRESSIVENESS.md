# 84 — Stacking, combination, a secondary axis and data labels

**Status: proposed.** Nothing in §5 through §8 is built; §2, §3 and §4 are
measured against a clean tree at `6825b7e`, and every number in them is
reproducible by the method in §11.

**Why this exists.** [12](12-COMPETITIVE-ANALYSIS.md) §8 ranks charts **sixth**
on the switching-blocker list: *"seven types with no subtypes. No stacked, no
combo, no secondary axis, no data labels. A stacked bar has no route at all."*
Unlike most of what has been fixed recently this is not wiring — it is a change
to what a chart **is** in this engine, and it lands in a rendering pipeline that
already exists and already works. That is exactly the case
[11](11-DESIGN-FIRST-PROCESS.md) says gets a design note first.

**Relates to** `RND-10`/`RND-11` and [80](80-CHART-DISPLAY-LIST.md) (ADR-021,
the one plotter), [76](76-CHART-RENDERING-BACKEND.md) (why there is no charting
crate), `FID-26` (surgery on a retained chart part), `FID-28` (a chart
reference across the OT wire), `COL-50` (insert-meets-delete on a range),
`COL-54` (a new wire enum variant is a protocol break), ADR-010 (additive
schema change), `PERF-D-01` (the frame budget).

---

## 1. The four things the measurement changed

The brief for this note assumed a shape, and three of its four assumptions were
wrong. They are worth stating before the design, because each of them would
have produced a different and worse plan.

1. **A chart this engine cannot express is not lost on save.** The assumption
   was that a stacked bar in somebody's `.xlsx` is dropped — the `CF-01` shape,
   where preservation outranks evaluation. It is not. All six probe fixtures
   round-trip **byte-identical** through an unedited open-and-save (§3.2). So
   preservation is not the first row, and saying it was would have bought
   nothing that already works.

2. **What is actually broken is worse in a different way: the picture is
   silently wrong, and nothing reports it.** A stacked column, a 100%-stacked
   bar, a combo chart and a secondary-axis chart all produce a display list
   *identical* to the clustered control — same item count, same fills (§3.3) —
   and the compatibility report carries **zero** entries mentioning a chart in
   every case. A user sees a chart that looks finished and says something the
   file does not. That breaks the "no silent data loss" rule in AGENTS.md more
   sharply than dropping the chart would, because a dropped chart is visible.

3. **There is only one renderer to change, not two.** `RND-10` deleted the
   canvas's own plotter; `drawCharts` now paints the engine's display list
   (`webapp/editor.core.js:1257-1305`). So the `RND-05` rule that the canvas and
   the headless renderer must not diverge is satisfied **by construction** for
   charts, and a chart feature is one edit to
   `crates/casual-calc-layout/src/chart.rs`, not two. This is the single largest
   reason the work below is smaller than it looks.

4. **A chart already costs real frame budget, and nobody is measuring it.**
   `push_chart` emits one polygon per data point with no cap and no
   downsampling. Six series over 1,000 rows is 6,007 display-list items and
   **722 KB of JSON across the WebAssembly boundary, every frame** (§3.5).
   `tests/browser/editor.frame-budget.spec.mjs` is structural, not wall-clock,
   and mentions charts nowhere. Data labels add an item per point, so the cap
   is a prerequisite for the feature rather than a follow-up.

---

## 2. What exists

### 2.1 The model

`ChartView` (`crates/casual-calc-model/src/chart.rs:112-171`) holds an id, a
cell anchor with EMU offsets, a `ChartKind`, a title, two axis titles, a legend
side, a `Vec<ChartSeries>`, and a retained part path. `ChartSeries`
(`:59-71`) holds three things: a display name, an optional categories formula
string, and a values formula string.

`ChartKind` (`:35-54`) is eight unit variants: `Bar`, `Column`, `Line`, `Area`,
`Pie`, `Doughnut`, `Scatter`, `Unsupported`. The comment at `:31-32` is the
model's own statement of the principle this note extends — *"`Bar` and `Column`
are one element in OOXML (`<c:barChart>`) distinguished by `<c:barDir>`; they
are separate here because they are separate pictures."*

**A chart is in one of two regimes** and `ChartView::part` says which
(`:1-23`). Read from a file, the chart part is authoritative and written back
from its own bytes. Made here, this type *is* the chart and the writer builds
the part. `ChartView::detach` (`:196-198`) moves a chart from the first regime
to the second, and it is called from exactly one place —
`session_set_chart` (`crates/casual-calc-wasm/src/objects.rs:526`).

### 2.2 What the importer reads, and what it does not

`parse_chart` (`crates/casual-calc-import/src/chart.rs:222-364`) reads chart
group elements, `<c:ser>`, `<c:tx>`, `<c:cat>`/`<c:xVal>`, `<c:val>`/`<c:yVal>`,
`<c:legendPos>`, `<c:barDir>` and titles.

It never reads `<c:grouping>`, `<c:overlap>`, `<c:dLbls>`, or the `<c:axId>`
pairing that distinguishes a primary from a secondary axis. Verified by search
across `crates/` and `webapp/`: the only `grouping` matches in the chart path
are the four *writer* literals at `crates/casual-calc-export/src/chart.rs:700`,
`:705`, `:710` and `:714`, and there is no `dLbls` anywhere in the tree outside
the probe fixtures.

Two consequences are structural rather than incidental:

- **Only the first chart group decides the kind** (`:289-298`), by explicit
  comment — *"A combination chart has several; drawing the first is wrong in a
  way that is visible, which beats drawing nothing at all."*
- **Every `<c:ser>` from every group is pushed into one flat list** (`:349-353`),
  because the `</c:ser>` handler does not know which group it is in. So a
  combo chart's series all survive the import; what is lost is only *which
  group each belonged to*. That is a much smaller gap than it appears, and §5.2
  is built on it.

### 2.3 One plotter, two backends

`casual_calc_layout::chart::push_chart`
(`crates/casual-calc-layout/src/chart.rs:404`) builds the whole picture as
display-list geometry. The headless renderer draws that list; so does the
canvas, through `session_chart_items`
(`crates/casual-calc-wasm/src/objects.rs:22-55`) and `paintList`. Series
colours come from the workbook's own theme accents through `series_colors`
(`:101-109`), cycling **six** slots (`ACCENT_SLOTS`, `:92`).

### 2.4 The editor

Seven kinds in the picker (`webapp/editor.core.js:3629-3632`), five legend
positions (`:3634-3636`), title, axis titles, and named series with ranges.
No subtype control of any sort exists.

---

## 3. The measurements

Verbatim output is in §11. What it establishes:

### 3.1 A chart the model cannot express imports as a plausible lie

| Fixture | OOXML it carries | Imported as | Series kept |
| --- | --- | --- | --- |
| `stacked-column` | `<c:grouping val="stacked"/>` | `Column` | 2 |
| `pct-stacked-bar` | `<c:grouping val="percentStacked"/>` | `Bar` | 2 |
| `combo` | `barChart` + `lineChart` | `Column` | 3 |
| `secondary-axis` | second `<c:valAx>`, `axId` 4 | `Column` | 2 |
| `data-labels` | `<c:dLbls><c:showVal val="1"/>` | `Column` | 1 |
| `clustered-control` | `<c:grouping val="clustered"/>` | `Column` | 2 |

Every one is `Column` or `Bar` with its series intact. Nothing distinguishes
the stacked chart from the clustered one anywhere in the model.

### 3.2 The file survives an unedited save; it does not survive an edit

Unedited, all six write back **byte-identical** chart parts. That is stronger
than [76](76-CHART-RENDERING-BACKEND.md) claims: the part is not merely
retained, its series references are **surgically rewritten** to follow
structural edits by `retune_series`
(`crates/casual-calc-export/src/chart.rs:362-420`, the mechanism `FID-26`
added), which replaces only the text inside a series' `<c:f>` and leaves every
other byte alone.

Edited, it does not survive. After `detach`, the writer rebuilds the part from
the model, and the model has no grouping, no second axis and no second group:

| Fixture | part before | part after edit | what the file now says |
| --- | --- | --- | --- |
| `stacked-column` | 1358 B | 1300 B | `<c:grouping val="clustered"/>` |
| `pct-stacked-bar` | 1365 B | 1300 B | `<c:grouping val="clustered"/>` |
| `combo` | 1726 B | 1495 B | one chart group; the line is gone |
| `secondary-axis` | 1741 B | 1302 B | one value axis |
| `data-labels` | 1154 B | 1107 B | no `<c:dLbls>` |

Retitling a chart, moving it, or resizing it all route through
`session_set_chart`, so **dragging a stacked chart two cells to the left
converts it to a clustered chart in the file.** The model's own documentation
is honest about this class of loss (`chart.rs:21-23`); what makes it worth a row
is that the user's action gives no hint that it is a conversion.

### 3.3 The two renderers agree, and both are wrong together

`push_chart` over a 400×300 frame:

```
stacked-column    : 15 items {"Polygon": 7, "Polyline": 3, "Text": 5}
clustered-control : 15 items {"Polygon": 7, "Polyline": 3, "Text": 5}
```

Identical, including the polygon fills. `RND-05` holds; it just holds on the
wrong picture. And in all six cases:

```
compatibility entries mentioning "chart": 0
```

### 3.4 Failure modes, measured

- **A range that shrinks: correct.** Deleting a row inside the plotted range
  rebases the model (`S!$B$2:$B$4` → `S!$B$2:$B$3`,
  `crates/casual-calc-transaction/src/structural.rs:1456-1467`), moves the
  anchor, and — because of `retune_series` — writes the corrected reference into
  the retained part. The part stays attached. There is no divergence.
- **A series naming a deleted sheet: broken.** After `RemoveSheet`, the series
  is still in the model spelled `Other!$A$1:$A$3` — **not** rewritten to
  `#REF!` — and `resolve` silently returns two series where there were three.
  The chart loses a series with no mark in the picture and no report. Worse,
  the string still names a sheet by name, so creating a new sheet called
  `Other` silently repopulates the chart from unrelated data.
- **More series than colours: broken at seven.**
  `series_colors(&wb, 7)` returns
  `["4472C4","ED7D31","A5A5A5","FFC000","5B9BD5","70AD47","4472C4"]` — series 1
  and 7 share a fill, and the legend cannot tell them apart. This is asserted
  as intended behaviour today (`crates/casual-calc-layout/src/tests.rs:1335`,
  *"the palette cycles"*), which is the right call for eight series and the
  wrong one for seven.
- **Collaboration: mostly handled, with `COL-50` inherited.** `FID-28` already
  shifts a chart's series references when a bundle crosses a concurrent
  structural op (`crates/casual-calc-transaction/src/transform.rs:1016-1039`).
  But `shift_reference_text` (`structural.rs:1618-1622`) parses the reference
  and hands it to the same `rewrite_expr`/`rewrite_range` a formula uses — so
  **`COL-50` applies to a chart's series range verbatim**: an insert meeting a
  delete at the range's last row does not commute, and a chart is a range
  reference. Not separately reproduced for a chart; the shared function is the
  claim.

### 3.5 What a chart costs per frame

`drawCharts` calls `session_chart_items` for every on-screen chart on **every**
frame, and that call re-runs `resolve()` — so a visible chart re-reads every
cell it plots, every frame. There is no memoization anywhere on that path.
`push_bars` (`crates/casual-calc-layout/src/chart.rs:607-633`) loops
`for i in 0..points` with no cap.

Native (wasm is slower; these are lower bounds), 60 fps = 16.7 ms:

```
    rows   series      items  per call (us)   at 60fps
      10        2         30            8.2       0.0%
     100        2        207           38.5       0.2%
     500        3       1507          120.9       0.7%
    1000        6       6007          457.0       2.7%
    5000        6      30007         2796.6      16.7%
   10000        6      60007         5687.1      34.1%
```

And the list is serialized to JSON for the canvas on each of those calls:

```
rows=10     series=2  items=30     JSON across the wasm boundary = 3558 bytes
rows=100    series=2  items=207    JSON across the wasm boundary = 24826 bytes
rows=1000   series=6  items=6007   JSON across the wasm boundary = 721946 bytes
```

Two points make this a design constraint rather than a footnote. A 400 px plot
cannot show 1,000 distinct bars — `bar_w` is clamped to one pixel by
`.max(PX)` at `chart.rs:606`, so the extra 5,000 polygons buy a smear. And a
6-series 5,000-row chart consumes the entire frame budget in the engine alone,
before serialization, before the boundary crossing, before painting.

---

## 4. Two documents that no longer describe the code

Filed as rows, not fixed here — [14](14-EXECUTION-TRACKER.md) §"Where a finding
goes" governs and another agent owns that file.

1. **[80](80-CHART-DISPLAY-LIST.md) §"What is actually built" says the legend is
   not drawn.** It says *"Layout has no text advances at all"*, that the legend
   *"is omitted and its side of the frame is not reserved"*, and that *"a chart
   with a legend therefore renders with a wider plot in the PNG than on
   screen"*. All three are false. `legend_box`
   (`crates/casual-calc-layout/src/chart.rs:286-300`) reserves the box and
   shrinks the plot, `draw_legend` (`:344`) draws it, and `label_width`
   (`:268-277`) measures it with `casual_calc_text::advance_width` — a
   dependency the crate carries at `crates/casual-calc-layout/Cargo.toml:20`.
   The module documentation at `chart.rs:25-39` describes the built behaviour
   correctly, so the ADR and the code it governs disagree with each other.
2. **[76](76-CHART-RENDERING-BACKEND.md) repeats it** in its last section
   (*"What is still not drawn is the legend and the rotated y-axis title"*), and
   its §"The problem, stated correctly" still describes the plot backend as
   living in `webapp/editor.js` as `drawChartFrame` through `drawPie`. `RND-10`
   deleted those 224 lines; none of the named functions exists.
   `crates/casual-calc-layout/src/chart.rs:4-5` and `:280` cite them too.

Neither is load-bearing on its own. Both are the shape of drift that already
cost this repository a red gate, and #1 in particular would send anybody
planning chart work at a problem that was solved.

---

## 5. The model, and what it must gain

The constraint is the file format, not our preference. Each decision below
names the OOXML it maps to, and each is made rather than offered.

### 5.1 Stacking is a field, not a set of kinds

**Decision.** Add to `ChartView`:

```rust
/// `<c:grouping val>`: how a bar, column, line or area group combines its
/// series. `None` is the schema default for the group's own element.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub grouping: Option<ChartGrouping>,
```

with `ChartGrouping` being `Clustered`, `Stacked`, `PercentStacked` and
`Standard`. That is the **union** of the two OOXML types, which are not the
same: `ST_BarGrouping` takes all four, while `ST_Grouping` — used by
`<c:lineChart>` and `<c:areaChart>` — takes `standard`, `stacked` and
`percentStacked` and has no `clustered`. One Rust enum covering both is the
right trade: the alternative is two enums that differ by one variant, and the
importer reads whichever the group's own element permits. A `Clustered` on a
line chart is refused on the way in rather than modelled.

**Why not `ChartKind::StackedColumn`.** Two independent reasons, either
sufficient.

- *OOXML says it is orthogonal.* `<c:grouping>` is a sibling of `<c:barDir>`
  inside `<c:barChart>`, not a different element. Encoding it in the kind makes
  a cross product — `{Bar, Column, Line, Area} × {clustered, stacked,
  percentStacked}` is twelve variants for what the file spells as one
  attribute — and the model would then have to un-multiply it on the way out.
  The existing `Bar`/`Column` split is not a counter-example: those genuinely
  *are* two pictures with different axis orientation, and the model's own
  comment says so.
- *`COL-54` makes it a hard protocol break.* `ChartKind` crosses the
  collaboration wire inside `SheetMetadata` as an externally-tagged serde enum
  — measured on the wire as `"kind":"column"` — and a client that has never
  heard of `"stackedColumn"` cannot deserialize the message **at all**. That is
  precisely the failure `COL-54` closed as a P0.

**What the field costs, stated plainly.** It is additive under ADR-010, so
`SCHEMA_VERSION` does not move and an old snapshot round-trips unchanged. But
it is **not** free on the wire, and the tempting conclusion that the protocol
version can stay where it is, is wrong. `SheetMetadata` carries charts as
field 19, `CHARTS` (`crates/casual-calc-transaction/src/lib.rs:300`), and that
field is a **whole-vector replace**. An old client parses the message fine,
drops `grouping` because serde ignores unknown fields, and then — the next time
it captures the sheet and submits anything at all — writes the whole chart
vector back **without it**. A stacked chart silently becomes clustered because
somebody else's tab is out of date. So:

> **`PROTOCOL_VERSION` moves to 8.** An additive field on a whole-vector wire
> object is a *quiet* break, not an absent one. `COL-54` established that a new
> variant fails loudly; the lesson to carry forward is that failing quietly is
> worse, not better.

### 5.2 Combination is per-series

**Decision.** Add to `ChartSeries`:

```rust
/// The chart group this series belongs to, when it differs from the chart's
/// own kind. `None` means the chart's kind — which is every series of a
/// single-group chart, so nothing is written for one.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub kind: Option<ChartKind>,
```

**Why this is small.** The importer already flattens every `<c:ser>` from every
group into one list (§2.2), so a combo chart's *data* is already fully
imported — measured, three series from `combo.xlsx`. The only missing fact is
which group each came from, and that is one field set at the point the parser
already knows the answer, because `group` is in scope when `</c:ser>` fires.

**Why per-series and not a `Vec<ChartGroup>`.** A group-shaped model is closer
to OOXML and would be the right answer if the model owned axes. It does not,
and adding a group layer would restructure every consumer — `resolve`,
`series_colors`, `retune_series`'s nth-`<c:ser>` correspondence, and the wire
shape — to express something the flat list already carries. The cost of the
flat choice is real and worth naming: **a per-series kind cannot express two
groups of the same type with different groupings** (a stacked bar group beside
a clustered bar group in one chart). Excel permits it; it is rare; it stays in
the retained-part regime and is refused in §6.

### 5.3 A secondary axis is a per-series flag, and nothing more

**Decision.** Add to `ChartSeries`:

```rust
/// Whether this series is plotted against the secondary value axis.
#[serde(default, skip_serializing_if = "std::ops::Not::not")]
pub secondary_axis: bool,
```

`push_chart` then computes `value_extent` **twice** — once over the primary
series and once over the secondary — and draws a second value axis on the right.

**Why a flag rather than an axis object.** This is the decision that could go
either way, so here is what it costs. OOXML expresses a secondary axis as a
second `<c:axId>` pair and a second `<c:valAx>` carrying its own scaling,
crossing, tick and format properties. A `ChartAxis` type modelling that would
also unlock axis min/max, logarithmic scale, tick intervals and axis number
formats — all of which users want. It is also several times the work, touches
the plot arithmetic everywhere, and adds a second nested object to the wire
shape.

The flag is chosen because it is the whole of the *switching blocker*. What
makes a missing secondary axis fatal is not the tick control, it is that one
series disappears. Measured, with revenue in millions beside a margin
percentage on one shared axis:

```
value_extent (one shared axis) = (0, 1600000)
the margin series' tallest bar is 0.000058 px of a 200 px plot
```

The series is drawn and is invisible. A boolean fixes that completely. Axis
scale control is refused in §6 and remains available as a later `ChartAxis`
without invalidating the flag — a flag is derivable from an axis object, so this
is not a decision that has to be undone.

### 5.4 Data labels are per-series, and capped

**Decision.** Add to `ChartSeries`:

```rust
/// `<c:dLbls><c:showVal val="1"/>`: draw each point's value beside it.
#[serde(default, skip_serializing_if = "std::ops::Not::not")]
pub data_labels: bool,
```

Values only. `<c:dLbls>` can show category name, series name, legend key,
percentage, leader lines and a custom number format; `showVal` is the one that
is reached for, and each of the others is a separate flag that can be added
additively later if it is ever asked for.

**And a cap, which is not optional.** A data label is a `PaintItem::Text` per
point, so labels roughly double a bar chart's item count and add text shaping to
a path that already spends 457 µs on 6,007 items (§3.5). The cap is stated as
part of this decision rather than deferred:

> **Above 200 plotted points per series, labels are not drawn**, and the chart
> says so in the picture the way `ChartKind::Unsupported` does. Two hundred
> labels do not fit in a 400 px plot in any case, so the cap costs nothing a
> reader could have used.

The uncapped *geometry* is a separate pre-existing defect and gets its own row
(§10, row **A**); the feature should not ship on top of it.

---

## 6. What is deliberately not built

Excel ships roughly fifty chart types. A design note that only says yes is not
a design, so here is the refusal and its basis.

**The basis.** A refused chart type is **not a lost file**. `ChartKind::Unsupported`
is retained and written back byte for byte, and §3.2 measured that this works.
So the cost of refusing is a picture, not data — which is what makes it
affordable to refuse a great deal. A type earns a place only if it fails all
three of these tests: it is reached for often, the model can express it without
a new nested object, and the plotter can draw it without new display-list
geometry.

**Refused, with the reason:**

| Not built | Why |
| --- | --- |
| 3-D anything (`bar3DChart`, `pie3DChart`, `surfaceChart`) | Needs a projection and a depth axis. The importer already flattens `bar3DChart` to `Column`, which is a better picture than a bad perspective. |
| Radar, surface, stock | Each needs its own plot construction and its own axis model. Rare outside one domain each. Retained today; stay retained. |
| Treemap, sunburst, waterfall, funnel, histogram, box-and-whisker | The Excel 2016 family. All are `<c14:>`/`<cx:>` extension parts, not `<c:chart>` at all — a **different schema** with a different reader. That is a project, not a variant. |
| Bubble as its own kind | `bubbleChart` currently imports as `Scatter`, losing the third dimension. Correct as a picture minus one channel; a bubble size needs a third reference per series, and `ChartSeries` would gain a field used by one kind. |
| Pie-of-pie, bar-of-pie | `ofPieChart` imports as `Pie`. The split rule is its own sub-model. |
| Trendlines, error bars | `<c:trendline>` needs regression in the layout crate, and `<c:errBars>` needs a second data reference plus five error modes. Both are analysis, not shape, and neither is on `docs/12`'s list. |
| Axis scale control (min, max, log, tick interval, axis number format) | Needs the `ChartAxis` object §5.3 declined. Named here so it is a known omission rather than an oversight. |
| Per-point and per-series colour overrides (`<c:spPr>`) | The palette is the workbook's theme accents and matching the file it came from is the current, correct behaviour. A colour override is a large surface (fill, line, gradient, pattern) for a small gain. |
| Sparklines | A different feature entirely — `<x14:sparklineGroups>` on the worksheet, not a chart part. Its own row if it is ever wanted. |
| Two groups of the same type with different groupings | The stated cost of §5.2's flat series list. Rare; stays retained. |

**Built:** stacking and 100%-stacking, combination, a secondary axis, value data
labels. Those are the four `docs/12` names, and they are the four that pass all
three tests.

---

## 7. The order to build in

Preservation is **not** first, because §3.2 measured that preservation already
works. The order that follows from the measurements is:

The rows are lettered here rather than numbered with tracker ids on purpose:
this note does not own [14](14-EXECUTION-TRACKER.md), and a document citing an
id no tracker defines is what `tools/check-doc-references.py` exists to catch.
§10 lists them for whoever files them.

**A. Report what cannot be drawn.** Read `<c:grouping>`,
`<c:dLbls>` and the secondary `<c:axId>` pairing in `parse_chart`, and when the
model cannot express what was read, write a `CompatibilityEntry`. **Nothing is
drawn differently.** This is first because it is the smallest change that stops
the engine claiming a chart is fine when it is not, it needs no model change at
all beyond what the parser already has in scope, and AGENTS.md's no-silent-loss
rule is being broken today with a measured count of zero entries.

**B. Cap the plot.** The uncapped point loop is a pre-existing
frame-budget defect and a prerequisite for labels. It is independent of
everything else here and can land in parallel.

**C. The model and the wire.** The four fields of §5, `ChartGrouping`,
`PROTOCOL_VERSION` 7 → 8, and the round-trip through `SheetMetadata`. No
rendering, no UI. Landing the wire change alone means the version bump is
reviewable on its own, which given `COL-54` is worth the extra round.

**D. Import and export.** `parse_chart` fills the new fields;
`retune_series` gains a second surgical edit so a *grouping* change can be
written into a retained part without detaching it — which is the mechanism that
stops "I retitled it" from meaning "I converted it". The writer emits the right
`<c:grouping>`, `<c:overlap>`, `<c:dLbls>` and second `<c:valAx>` for an
authored chart.

**E. The plotter.** Stacked and percent-stacked bar geometry, a
per-series kind in `push_chart`'s dispatch, a second value extent and a
right-hand axis, and label text items under B's cap. One edit, both backends.

**F. The editor.** Subtype control, a per-series kind and axis
control, a labels checkbox.

A through C are worth doing even if D through F never happen: they stop the
engine lying, stop it burning frame budget, and settle the wire.

---

## 8. Failure modes

Each is measured in §3.4 unless marked.

1. **A chart over a range that shrinks.** Works today and must keep working.
   The rule the new fields inherit: a *reference* change is surgery on the
   retained part (`retune_series`), never a detach. Step D extends the same
   mechanism to grouping rather than inventing a second one.
2. **A series naming a deleted sheet.** Broken today: the reference survives
   spelled by name, the series vanishes from the picture with no mark, and
   recreating a sheet of that name repopulates the chart from unrelated data.
   The fix is the one Excel uses — rewrite to `#REF!` on
   `RemoveSheet` — and the picture should name the dead series rather than
   drop it.
3. **More series than colours.** Broken at seven. Cycling is right
   eventually; what is wrong is cycling at 7 when the theme has six accents and
   a legend is on screen. Stacking makes this sharper, because a stacked chart
   is *read* by colour in a way a clustered one is not — two identically
   coloured bands in one column are unreadable, where two identically coloured
   bars side by side are merely ambiguous.
4. **A chart in a collaborative session while somebody edits its data.**
   `FID-28` already carries a chart's references across a concurrent structural
   op. `COL-50` is inherited unchanged: `shift_reference_text` calls the same
   `rewrite_range` a formula uses, so an insert meeting a delete at the last row
   of a plotted range settles differently on the two replicas — and unlike a
   formula, nothing recalculates and nothing shows an error, so the two users
   see charts of different heights and neither sees a problem. **This design
   does not fix `COL-50` and must not claim to.** What it must not do is make it
   worse: none of the four fields is positional, so none of them adds a new way
   to diverge.
5. **A chart whose kind and grouping disagree** (not measured; new with this
   design). `grouping: Stacked` on a `Pie` is meaningless. The plotter ignores
   grouping for kinds that have no groups, and the importer does not set it for
   them. Stated so it is a decision and not a discovery.

---

## 9. What this costs

**Rendering.** Stacking changes the arithmetic in `push_bars`, not the item
count — a stacked bar is the same polygon at a different `y0`. Combination and a
secondary axis add a second `value_extent` pass, which is linear in points and
negligible beside the polygon loop. **Data labels are the only real cost**: one
`PaintItem::Text` per plotted point, roughly doubling item count and adding text
shaping. §5.4's cap is what keeps that inside the budget, and §7's step B is
what makes the cap meaningful.

**Model size.** Three of the four fields are `Option`/`bool` on `ChartSeries`,
one on `ChartView`, all skipped when absent. A chart carries single-digit
series in practice. This is not measurable against a workbook.

**Wire size.** Measured: one `ChartView` as JSON is **259 bytes**; a 12-series
chart over 500 rows is **1,100 bytes**. The four fields add roughly 20 bytes to
a chart that uses them and zero to one that does not. Wire *size* is a
non-issue; wire *version* is not — see §5.1. **`PROTOCOL_VERSION` moves to 8.**

**What does not change.** No new dependency, no `PaintItem` variant, no ADR
trigger under [11](11-DESIGN-FIRST-PROCESS.md) — the display-list contract is
untouched, which is the dividend ADR-021 paid for by putting general geometry in
once. `SCHEMA_VERSION` stays at 1 under ADR-010.

---

## 10. Rows this note asks for

**No ids are assigned here.** The id space was read — the chart prefix runs
`CHT-01` through `CHT-04`, all closed and archived in
[14a](14a-ARCHIVE-CLOSED-WORK.md), so these are the next four onwards — but
`tools/check-doc-references.py` fails a document that cites an id no tracker
defines, and this note does not own [14](14-EXECUTION-TRACKER.md). Whoever
files them assigns the ids; if the citation is wanted here, the rows and the
edit to this section land in the same commit.

| # | Row | Why it is a row and not a doc edit |
| --- | --- | --- |
| A | `push_chart` emits one polygon per point with no cap or downsampling: 6 series × 1,000 rows is 6,007 items and 722 KB of JSON across the wasm boundary **per frame**, and 5,000 rows is the whole 16.7 ms budget in the engine alone | Defect, pre-existing, independent of the feature |
| B | A chart whose OOXML the model cannot express imports with **zero** compatibility entries and draws a plausible lie | Breaks AGENTS.md's no-silent-loss rule |
| C | The model and wire change: `grouping`, per-series `kind`, `secondary_axis`, `data_labels`; `PROTOCOL_VERSION` 7 → 8 | §5 |
| D | Import and export the four; extend `retune_series` so a grouping change does not require a detach | §7 D |
| E | Plot stacked, percent-stacked, combination, a secondary axis and capped labels | §7 E |
| F | Editor controls for the four | §7 F |
| G | A chart series naming a deleted sheet keeps the sheet **name**, vanishes from the picture unreported, and re-resolves against unrelated data if a sheet of that name is created again | Defect, measured |
| H | A seven-series chart repeats accent 1, and its legend cannot distinguish series 1 from series 7 | Defect, measured |
| I | [80](80-CHART-DISPLAY-LIST.md) and [76](76-CHART-RENDERING-BACKEND.md) both state the chart legend is not drawn and that layout has no text advances; both are false (§4) | A document stating a contract the code does not keep |
| J | [76](76-CHART-RENDERING-BACKEND.md) and `crates/casual-calc-layout/src/chart.rs:4-5,280` cite `webapp/editor.js` plotter functions `RND-10` deleted | Dangling citation |

---

## 11. Reproducing this

The fixtures are six minimal `.xlsx` packages differing only in their chart
part — stacked, percent-stacked, combo, secondary-axis, data-labels, and a
clustered control. Each was run through `casual_calc_import::import_package`,
`casual_calc_export::write_workbook`, and
`casual_calc_layout::chart::push_chart`, out of tree so nothing was added to the
repository. Timings are native `--release` on darwin/arm64; the wasm path is
slower, so §3.5 is a lower bound.

Verbatim output of the six-fixture pass, abbreviated to one representative
case and the control:

```
================ stacked-column.xlsx ================
charts modelled: 1
  kind      : Column
  title     : ""
  legend    : None
  part      : Some("xl/charts/chart1.xml")
  series    : 2
     name="Rev" cats=Some("S!$A$2:$A$4") values="S!$B$2:$B$4"
     name="Cost" cats=Some("S!$A$2:$A$4") values="S!$C$2:$C$4"
compatibility entries mentioning "chart": 0
round-trip (unedited): chart part byte-identical (1358 bytes)
after edit (detach): rebuilt part 1300 bytes, grouping=Some("<c:grouping val=\"clustered\"/>..."), dLbls=0, groups=2
display list: 15 items {"Polygon": 7, "Polyline": 3, "Text": 5}
polygon fills: ["FFFFFF", "4472C4", "ED7D31", "4472C4", "ED7D31", "4472C4", "ED7D31"]

================ clustered-control.xlsx ================
  kind      : Column
  series    : 2
compatibility entries mentioning "chart": 0
round-trip (unedited): chart part byte-identical (1338 bytes)
display list: 15 items {"Polygon": 7, "Polyline": 3, "Text": 5}
polygon fills: ["FFFFFF", "4472C4", "ED7D31", "4472C4", "ED7D31", "4472C4", "ED7D31"]
```

Failure modes:

```
=== 1. A chart over a range that shrinks ===
before: model values  = ["S!$B$2:$B$4", "S!$C$2:$C$4"]
before: retained part refs = ["S!$B$1", "S!$A$2:$A$4", "S!$B$2:$B$4", ...]
after : model values  = ["S!$B$2:$B$3", "S!$C$2:$C$3"]
after : part attached = Some("xl/charts/chart1.xml")
after : WRITTEN part refs  = ["S!$B$1", "S!$A$2:$A$3", "S!$B$2:$B$3", ...]

=== 2. A chart whose series name a deleted sheet ===
remove sheet 1: Ok
chart series after: [("Rev", "S!$B$2:$B$4"), ("Cost", "S!$C$2:$C$4"),
                     ("FromOther", "Other!$A$1:$A$3")]
resolve() -> cats=["Q1", "Q2", "Q3"]
   series "Rev"  -> [Some(100.0), Some(120.0), Some(140.0)]
   series "Cost" -> [Some(60.0), Some(70.0), Some(80.0)]
   (the third series is gone, with no entry and no mark)

=== 3. More series than colours ===
series_colors(7) = ["4472C4","ED7D31","A5A5A5","FFC000","5B9BD5","70AD47","4472C4"]
series 1 and series 7 share a colour: true

=== 4. Scale disparity ===
value_extent (one shared axis) = (0, 1600000)
the margin series' tallest bar is 0.000058 px of a 200 px plot

=== 5. Wire cost ===
one ChartView as JSON: 259 bytes
{"id":1,"anchor":{...},"kind":"column","series":[...],"part":"xl/charts/chart1.xml"}
a 12-series ChartView as JSON: 1100 bytes
```
