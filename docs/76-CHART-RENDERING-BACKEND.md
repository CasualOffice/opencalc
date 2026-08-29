# 76 — Drawing charts in the headless renderer

**Status:** decided — **Option C**, recorded as `ADR-021` in
[80](80-CHART-DISPLAY-LIST.md) (Accepted) and built by `RND-11`. This note is
kept as the analysis that led there; the decision, the display-list contract
and the list of what is drawn and what is not all live in
[80](80-CHART-DISPLAY-LIST.md).
**Relates to** `RND-06` (headless render omits charts and images), `RND-05`
(the same shape, for conditional formatting), `RND-10` (no display list across
the WebAssembly boundary), ADR-008 (display list), ADR-018 (what may go into
the WebAssembly bundle).

## The problem, stated correctly

`RND-06` reads "charts need a plot backend", which frames the work as a
dependency choice: pick a Rust charting crate, feed it the series, blit the
result. That framing is wrong, and it is wrong in a way that would have been
expensive to discover after the crate was in the lock file.

**The plot backend already exists.** It is in `webapp/editor.js`, it is about
three hundred lines (`drawCharts` through `drawPie`, roughly lines 250-575),
and it draws bar, column, line, area, scatter, pie and
doughnut charts with titles, axis titles, a legend on any of five sides, and
series colours taken from the workbook's own theme accents
(`drawChartFrame`, `legendBox`, `drawLegend`, `drawBarChart`, `drawLineChart`,
`drawPie`, `seriesColors`, `valueExtent`). A user looking at a chart in the
editor is looking at a complete picture.

**What the headless renderer is missing is not a plotter.** It is missing the
two things underneath one:

1. **Resolved series values.** `ChartSeries::values` is a *formula string* —
   `Sheet1!$A$2:$A$9` — not numbers. They are resolved by `ref_numbers` and
   `ref_text` in `crates/casual-calc-wasm/src/lib.rs`, and handed to the canvas
   by `session_charts`. `casual-calc-wasm` is a **host** crate; the render path
   cannot depend on it, and must not.
2. **Paint primitives for a plot.** `PaintItem` has rectangles, thin lines,
   text and (since this row's first half) pictures. It has no polyline, no
   filled polygon and no wedge, so a display list cannot describe a line chart
   or a pie at all.

That is exactly `RND-05`. There, the conditional-formatting rules were resolved
inside `casual-calc-wasm`, the canvas showed colour scales and every headless
PNG showed plain cells, and the fix was not to acquire a rule engine — the logic
was not missing, it was in the wrong crate. It moved to
`casual-calc-layout::conditional`, and now **both renderers ask one function**.

## What the row's acceptance criterion already tells us

> A PNG of a chart sheet matches the canvas

Read literally, that criterion **rules out a third-party plot backend**, because
a third-party plot backend draws a third-party picture. `plotters` renders its
own axes, its own tick placement, its own default palette and its own fonts;
none of them will agree with `drawBarChart`, and no amount of configuration will
make them agree pixel for pixel. Satisfying the criterion with a charting crate
means rewriting the canvas to match the crate — which is a much larger change
than the row describes, and throws away a working, theme-aware renderer to buy
a dependency.

## The options

### A. Add a Rust charting crate (`plotters` or similar)

The reading `RND-06` invites.

**What it costs.**

- **A new dependency in the render crate**, which is compiled into the
  WebAssembly bundle. ADR-018 measured that bundle at 12.9 MB and refused to add
  a *shaping* stack to it for a defect no editor user can see; this is the same
  argument again, and stronger, because the editor already draws charts
  correctly. Whatever a plot crate weighs, every editor user downloads it to fix
  a picture only the PNG path gets wrong.
- **`unsafe_code = "forbid"`** is a `[workspace.lints]` entry and therefore
  binds *our* crates, not our dependencies — so this is not a hard blocker, and
  it would be dishonest to present it as one. What it does mean is that any
  `unsafe` arrives unaudited and outside the property the workspace otherwise
  asserts about itself, and `deny.toml` grows another entry to justify.
- **The wasm target.** A plotting crate's default features typically drag in a
  font stack (`font-kit`, system font enumeration) and an image codec; both are
  hostile to `wasm32-unknown-unknown`, and font enumeration duplicates
  `casual-calc-render::fonts`, which already resolves faces and reports the
  scripts it cannot cover. Two font stacks in one crate is how a PNG comes to
  render text differently from the cell beside it. `std::time::Instant::now`
  panics on wasm32, and every transitive dependency has to be checked for it.
- **A second picture.** The stated gate fails by construction, per above.

**What it buys.** Chart *kinds* we do not draw today, eventually — but the model
only has seven kinds and the canvas draws all seven.

### B. Rasterise a chart in the host and hand the pixels down

The picture the canvas already draws, read back with `canvas.toDataURL`, passed
in as media, blitted like `PaintItem::Image`.

**What it costs.** It inverts the dependency: the engine would need a browser to
render a chart, so *the same document renders differently on a server than in a
tab* — the exact failure ADR-018 rejected for shaping. Server-side rendering,
thumbnails and the fidelity harness have no canvas at all, and those are the
three places the headless renderer exists for.

### C. Move plot construction into `casual-calc-layout`, and give the display list geometry (**recommended**)

Three pieces, in order, each shippable on its own:

1. **`casual-calc-layout::chart_data`** — resolve a `ChartSeries`'s formula to
   values against the workbook's *cached* cell values, the way `layout` already
   reads them (it never invokes the calc engine). This is `ref_cells` /
   `ref_numbers` / `ref_text` moved down out of `casual-calc-wasm`, exactly as
   `conditional.rs` was moved for `RND-05`. `session_charts` then calls it
   instead of owning it, so the canvas and the PNG cannot resolve a range
   differently.
2. **Geometry in `PaintItem`** — the smallest set that covers the seven kinds:
   `Polyline { points, width, color }`, `Polygon { points, fill }` (bars, area
   fills, legend swatches) and `Wedge { centre, radius, inner_radius, from, to,
   fill }` (pie and doughnut). Text and rectangles already exist. All in twips,
   all resolved, all serialisable — the display list stays golden-testable and
   the backend stays dumb, which is ADR-008's whole contract.
3. **`casual-calc-layout::chart`** — the plot construction itself: value extent
   including zero, plot rectangle after title, axis titles and legend, tick
   positions, series palette from the theme accents. A port of the JavaScript,
   not a reinvention, so the picture is the one that already ships.

**What it costs.** The most code of the three options, and the most careful
work: the port has to be faithful or the gate fails. It also enlarges
`PaintItem`, which is an ADR trigger.

**What it buys.**

- **No new dependency.** Nothing added to the WebAssembly bundle but the layout
  code itself; `tiny-skia` already fills paths and `skrifa` already draws text.
- **No `unsafe`, no wasm risk**, because nothing new is compiled.
- **One picture, by construction** — the thing the gate asks for. The canvas
  keeps painting; it stops deciding, which is the settlement `RND-08` reached
  for data bars and gave a name to.
- **It is the step `RND-10` needs anyway.** Once the plot is a display list, the
  canvas consuming display lists stops being an abstract idea and has a second
  primitive family worth sharing — `RND-10` explicitly says it is worth doing
  when that happens.

## Recommendation

**Option C**, staged. Ship (1) on its own — it is a crate move with no visible
change, and it removes the "two renderers can resolve a range differently"
hazard immediately. Then (2) and (3) together, because a primitive with no
producer and no consumer is untestable.

Do **not** add a charting crate. The dependency is not what is missing.

## Why this needs an ADR

`docs/11-DESIGN-FIRST-PROCESS.md` lists as triggers "**layout units, the
display-list contract, or a render backend**" and "**a dependency choice (a new
crate, a font/shaping/raster library)**". Option C trips the first and Option A
trips both. The display-list change here is not a leaf variant of the kind
`PaintItem::DataBar` (`RND-07`) and `PaintItem::Image` (this row's first half)
were — those describe a thing layout already resolved, under ADR-008 unchanged.
A polyline, a polygon and a wedge introduce **general geometry** into a contract
that has so far only carried resolved cell decorations, and a `chart` module in
layout introduces plot construction as a layer responsibility. That is a change
to what the display list *is*, so it is decided in an ADR before it is built.

## What is true today, so nobody has to rediscover it

- ~~A chart in a headless PNG is **absent, and not reported**.~~ **Fixed by
  `RND-11`.** It was absent and unreportable because charts had no display-list
  representation at all, so there was nothing to report from. They now have one:
  a chart is geometry, and the two conditions a chart can be in that the
  picture cannot show — an unresolvable series, and a `ChartKind` this does not
  draw — write themselves into the picture as the canvas writes them ("no data",
  "unsupported chart not drawn"). What is still *not* drawn is the legend and
  the rotated y-axis title, named in [80](80-CHART-DISPLAY-LIST.md) §"What is
  actually built".
- The chart *part* is retained byte for byte and written back
  (`ChartView::part`), so nothing is lost from the **file**. What is lost is the
  picture, and only in the headless path.
- `ChartKind::Unsupported` already exists for chart types the model does not
  draw, and is documented as "visibly incomplete rather than silently wrong".
  Whatever Option C draws should keep using it rather than inventing a
  substitute.
