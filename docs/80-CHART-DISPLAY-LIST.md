# 80 — General geometry in the display list

**For** `ADR-021`. **Status: Accepted** — on the evidence of being built:
`RND-11` shipped to this note and closed, all seven chart kinds draw
model-to-PNG, and `PaintItem::Polyline`/`Polygon`/`Wedge` are in the display
list with an arm each in the renderer. It stayed "proposed" through all of
that, which is what `tools/check-adr-status.py` now prevents.

**Decides** the option
[76](76-CHART-RENDERING-BACKEND.md) recommended, and the display-list change
that option requires. **Relates to** ADR-008 (the display-list contract),
ADR-018 (what may go into the WebAssembly bundle), `RND-11`, `RND-10`,
`RND-05`, `RND-06`, `RND-07`.

## The decision, first

1. **Charts are plotted by porting the editor's plotter down into
   `casual-calc-layout`.** No charting crate is added.
2. **`PaintItem` gains three geometry variants** — `Polyline`, `Polygon`,
   `Wedge` — plus a `Point` type and an `Align::Center`. These are a change to
   *what the display list is*, which is why this is an ADR and not a note.
3. **Series resolution moves down** out of `casual-calc-wasm` into
   `casual-calc-layout::chart_data`, so both renderers ask one function.

[76](76-CHART-RENDERING-BACKEND.md) argues (1) at length and is not repeated
here. The short form: the row's acceptance criterion is "a PNG of a chart sheet
matches the canvas", and a third-party plotter draws a third-party picture, so
that criterion fails **by construction** unless the canvas is rewritten to match
the crate. Porting gives one plotter and two backends that agree by
construction rather than by effort — and adds nothing to a 12.9 MB WebAssembly
bundle, which is the same argument ADR-018 already accepted for shaping.

## Why this is not the same kind of change as `DataBar` or `Image`

`PaintItem::DataBar` (`RND-07`) and `PaintItem::Image` (`RND-06`) also enlarged
the enum, and neither needed an ADR. The difference is worth stating precisely,
because "we added a variant last time without one" is the argument that erodes
a contract.

**A leaf variant names a thing layout has already resolved.** `DataBar` carries
a cell rectangle, a fraction and a colour: layout decided *which cell*, *which
rule* and *what fraction*, and the backend decides only how to turn that into
pixels — the inset, the alpha, the device rounding. `Image` carries a frame and
a part path: layout decided *where the picture goes*, and the backend resolves
the media and scales it. Each is a **noun from the spreadsheet domain** with the
open questions left to the device. Neither adds any expressive power: you cannot
describe a shape with them that you could not describe before, because they do
not describe shapes at all.

**A geometry variant names no domain thing.** A polyline is not a spreadsheet
concept. `Polygon { points, fill }` can express a bar, an area fill, a legend
swatch, a cell background, a triangle, and any picture at all given enough
points. That is a change of kind:

- **The display list stops being a list of resolved cell decorations** and
  becomes something that can describe arbitrary marks. Every future consumer —
  the canvas under `RND-10`, an SVG or PDF backend, a golden-image differ — now
  has to implement a path rasteriser, not a rectangle filler.
- **It creates a second way to say the same thing.** A rectangle is now
  expressible as `CellBackground` *and* as a four-point `Polygon`. That is
  accepted deliberately (see below), but a contract with two spellings for one
  picture is a contract that can drift, and deciding that in a pull request is
  how it drifts.
- **It moves a layer boundary.** A `chart` module in layout makes **plot
  construction a layout responsibility** — value extents, tick placement, plot
  rectangles, palettes. [19](19-WORKSPACE-SCAFFOLD-DESIGN.md) says layout "owns
  all geometry", so this is within the letter of it; it is a large addition to
  the spirit of it, and it is the kind of thing that should be visible in the
  register rather than only in a crate.

[11-DESIGN-FIRST-PROCESS](11-DESIGN-FIRST-PROCESS.md) lists "the display-list
contract" as an ADR trigger. This is that.

## What the variants are

All coordinates are **twips in sheet space**, like every other rectangle in the
list. All colours are `RRGGBB`. All values are resolved: nothing here needs the
model, the workbook or a font to be interpreted.

```rust
Polyline { points: Vec<Point>, width: i64, color: String }
Polygon  { points: Vec<Point>, fill: String }
Wedge    { center: Point, radius: i64, inner_radius: i64,
           from: f64, sweep: f64, fill: String }
```

**`Polyline` is open and is not closed for you.** A closed outline repeats its
first point as its last. Closing implicitly would make a rectangle border and a
three-sided bracket the same display list, and the difference would be
unrecoverable.

**`Polygon` is implicitly closed**, because an open filled path is not a thing
anyone wants to describe. It fills by the **non-zero winding rule**. Nothing
layout emits self-intersects, so the rule is not observable today; it is
specified so that it cannot quietly become observable when something does.

**`Wedge` measures angles in degrees clockwise from twelve o'clock.** Not
radians, and not counter-clockwise from three o'clock as most raster APIs do.
Two reasons. A display list is read by people, and the convention that matches
the picture is the one that survives being ported between backends. And Excel
starts a pie at twelve and goes clockwise, so a wedge whose numbers match the
picture needs no mental conversion at the point where mistakes are made. Each
backend converts once, in one function.

`sweep` is an **extent, not an end angle**, so a slice knows how big it is
without knowing where the next one starts — and so a full circle is
`sweep = 360.0` rather than `from == to`, which would otherwise be
indistinguishable from an empty slice.

`inner_radius` makes a wedge an **annular sector**, which is how a doughnut is
drawn. The hole is a hole in the geometry, not a background-coloured disc
painted over a pie. The canvas does the latter, which is the same picture only
while the background is opaque and its colour known; a headless backend knows
neither.

`Align::Center` is added for the same reason: a chart title is centred over a
point, and the two existing alignments both anchor to an edge.

### The overlap with `CellBackground` is deliberate

A four-point `Polygon` can draw a cell background. They are kept separate
because they are different statements: `CellBackground` says *this cell has this
fill*, addressed by the grid, and a backend may optimise it, snap it to device
pixels, or skip anti-aliasing on it precisely because it knows it is a cell.
`Polygon` says *fill these four points*. Collapsing the two would lose the
information that makes cell painting fast and crisp.

### What a backend must be able to do

This is the real cost of the decision, so it is stated as a requirement rather
than left implicit. A conforming backend must be able to:

1. **Stroke an open polyline** at a given width, in sheet units it converts
   itself. Joins and caps are the device's business; nothing here depends on
   them.
2. **Fill a closed polygon** by non-zero winding.
3. **Fill a circular sector**, including an annular one — which, for a backend
   with no arc primitive, means approximating an arc with cubic Béziers split
   at no more than a quarter turn. `casual-calc-render` does exactly that,
   because `tiny-skia` has no arc; a canvas backend calls `arc` and converts the
   angle convention.
4. **Centre a text run** in its box.

Any backend that cannot do all four renders a chart wrongly rather than
partially, so this is a floor and not a menu.

## What is actually built, and what is not

Built, and drawing end to end from model to PNG:

| Piece | State |
| --- | --- |
| `chart_data::ref_cells` / `ref_text` / `ref_numbers` | moved down, `session_charts` delegates |
| Frame ground, border, title, "no data" note | drawn |
| Value axis, zero line, axis extreme labels | drawn |
| Bar, Column | drawn |
| Line, Area, Scatter | drawn |
| Pie, Doughnut | drawn |
| Category labels under the plot | drawn |
| `ChartKind::Unsupported` | named in the picture, as the canvas names it |

**Not drawn, and each is a divergence from the canvas that a reader should
know about rather than discover:**

- **The legend.** The canvas sizes its legend box from `ctx.measureText` of the
  widest series name. **Layout has no text advances at all** — it emits a
  string and a rectangle and leaves shaping to the backend, which is ADR-008
  working as designed. A guessed width would move the *plot rectangle*, which
  is the one thing this port exists to prevent, so the legend is omitted and
  its side of the frame is **not reserved**. A chart with a legend therefore
  renders with a wider plot in the PNG than on screen. This is the largest
  remaining divergence, and closing it needs a text-advance seam in layout —
  which column autofit needs anyway, and which is a row of its own rather than
  something to improvise here.
- **The y-axis title**, which the canvas draws rotated a quarter turn.
  `PaintItem::Text` has no rotation. The space it takes **is** still reserved,
  so the plot rectangle agrees with the canvas whether or not the label is
  drawn: a missing label is a smaller error than a plot in the wrong place.
- **Clipping.** The canvas clips each chart to its frame; the display list has
  no clip primitive, so an over-long title can overhang where the canvas would
  cut it. Every *geometric* item is inside the frame by construction.
- **A right-aligned label's last two twips.** A backend insets a right-aligned
  run from its box by its own text padding, so the axis extremes sit a hair
  left of where the canvas puts them. Layout could compensate by writing the
  backend's inset into the plot arithmetic; it does not, because `DataBar`
  already settled that the inset is the backend's business and a device
  quantity copied into layout is a copy that drifts. Named rather than fixed.
- **The chrome colours** — frame ground, border, axis, muted label — come from
  the editor's CSS theme on the canvas and from fixed light-theme constants
  here. A workbook carries no UI theme, so there is nothing to read them from.
  **Series** colours are unaffected: they come from the workbook's own theme
  accents and agree exactly.

None of these is silent. The first two are stated here and in the module
documentation; `ChartKind::Unsupported` and an unresolvable series both write
their condition into the picture, which is what the canvas does and is now
possible headlessly for the first time — before this, a chart in a PNG was
absent *and unreportable*, because it had no display-list representation to
report from.

## What it costs, and what it does not

**No new dependency.** Nothing is added to `Cargo.toml` in the render path.
`casual-calc-layout` gains a path dependency on `casual-calc-formula`, which is
bedrock beside the model in [19](19-WORKSPACE-SCAFFOLD-DESIGN.md)'s DAG, has no
dependencies of its own, and is needed because a chart series names its values
with a reference string that has to be parsed. That is a downward edge, not a
new layer, and it adds nothing to the WebAssembly bundle that was not already in
it.

**No `unsafe`**, no `cfg(target_*)`, and both crates still compile for
`wasm32-unknown-unknown`.

**The display list stays serializable and golden-testable**, which was ADR-008's
central promise: the geometry variants round-trip through JSON, and a chart is
therefore diffable as data and not only as pixels.

## Alternatives, briefly

Rejected in [76](76-CHART-RENDERING-BACKEND.md) and not reopened: adding a
charting crate (a second picture, a second font stack, and a bundle tax on every
editor user for a defect none of them can see); rasterising in the host and
passing pixels down (the same document would render differently on a server than
in a tab).

One alternative *is* new here, and was rejected while building: **a single
`Path` variant** carrying a general command list (move/line/cubic/close). It is
more expressive and it is worse. It makes every backend implement a
mini-language, it makes a wedge unrecognisable as a wedge in a golden file, and
it removes the one property that keeps this reviewable — that each variant means
one picture. Three specific shapes cover the seven chart kinds; a general path
would cover everything and be checkable for nothing.

## Consequence for `RND-10`

[76](76-CHART-RENDERING-BACKEND.md) noted this and it is now concrete: the
canvas consuming display lists stops being abstract once a second primitive
family is worth sharing. A chart is now a display list, so the canvas's three
hundred lines of plotting have a replacement to converge on — and until they do,
the two are proven equal only by the constants being written side by side, which
is weaker than executing the same list and should be said so.
