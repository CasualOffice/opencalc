//! Plot construction: a chart becomes display-list geometry here.
//!
//! **This is a port, not a design.** The picture it builds is the one
//! `webapp/editor.js` has been drawing since charts landed — `drawChartFrame`
//! through `drawPie`, with the same padding, the same value extent, the same
//! zero line, the same twelve-o'clock start and the same theme-accent series
//! palette. It is here rather than there because a chart drawn only by the
//! canvas is a chart the headless renderer cannot draw *or report*: thumbnails,
//! previews and server-side export showed the cells and nothing over them
//! (`RND-11`, `docs/76`).
//!
//! Porting rather than acquiring a charting crate is the decision recorded in
//! ADR-021 (`docs/80-CHART-DISPLAY-LIST.md`). The reason is short: the row's
//! own acceptance criterion is "a PNG of a chart sheet matches the canvas", and
//! a third-party plotter draws a third-party picture. One plotter, two
//! backends, and they agree by construction.
//!
//! # Units
//!
//! The canvas works in CSS pixels; layout works in twips, at no particular
//! resolution. Every constant here is the canvas's own number multiplied by
//! [`PX`], so the two can be compared line by line — which is the only way a
//! port stays faithful once somebody edits one of them.
//!
//! # The legend
//!
//! Drawn, and sized the way the canvas sizes it: the box takes the widest
//! series name plus padding, and the plot rectangle is what is left over.
//! Layout could not do that for a while — it had no text advances at all, so it
//! left the legend out and gave the plot the whole frame, which rendered every
//! chart with a legend with a plot the width of the legend too wide.
//! [`casual_calc_text::advance_width`] measures it now (`RND-11`).
//!
//! The measurement is of the **bundled** face; the canvas measures the
//! browser's `system-ui` with `measureText`, so the two do not agree exactly.
//! That is the same disagreement the two have had about every other string
//! since the PNG path existed, and it is a far smaller error than not
//! reserving the box at all. What matters is that the string is measured in the
//! face it is *drawn* in, so the box fits its contents.
//!
//! # What this does not draw, and why
//!
//! Named here because a chart the display list cannot describe has to be
//! visible somewhere other than in a diff:
//!
//! - **The y-axis title**, which the canvas draws rotated a quarter turn.
//!   [`PaintItem::Text`] has no rotation. The space it occupies **is** still
//!   reserved, so the plot rectangle matches the canvas whether or not the
//!   title is drawn — a missing label is a smaller error than a plot in the
//!   wrong place.
//! - **Clipping.** The canvas clips every chart to its frame; the display list
//!   has no clip primitive, so a long title can overhang where the canvas would
//!   cut it. Every *geometric* item this emits is inside the frame by
//!   construction.
//! - **The chrome colours** — frame, border, axis, muted label — come from the
//!   editor's CSS theme on the canvas and from the constants below here. A
//!   workbook carries no UI theme, so there is nothing to read them from; these
//!   are the light-theme values. The **series** colours are not affected: those
//!   come from the workbook's own theme accents, and agree exactly.

use casual_calc_model::{ChartGrouping, ChartKind, ChartView, Workbook};

use crate::chart_data::{ref_strip, ref_text, strip_numbers};
use crate::display::{Align, DisplayList, PaintItem, Point, Rect};

/// Twips in one CSS pixel at 96 dpi: 1440 / 96.
///
/// Every geometric constant below is the canvas's pixel number times this, so
/// the port can be diffed against `webapp/editor.js` by eye.
pub const PX: f64 = 15.0;

/// A CSS pixel font size in points, which is what [`PaintItem::Text`] carries.
fn pt(px: f64) -> f32 {
    (px * 72.0 / 96.0) as f32
}

/// The frame's ground. The canvas uses the editor's `--bg`; a workbook has no
/// UI theme, so headless takes the light one.
const CHART_BG: &str = "FFFFFF";
/// The frame's outline.
const CHART_BORDER: &str = "888888";
/// Title and legend text.
const CHART_FG: &str = "000000";
/// Axis numbers, category labels, axis titles, and the "no data" note.
const CHART_MUTED: &str = "888888";
/// The axis lines themselves.
const CHART_AXIS: &str = "666666";

/// The palette a chart falls back to when the workbook carries no theme: the
/// stock Office accents, which is what [`Workbook::theme_slot`] returns anyway.
/// Named so the fallback is visible rather than implied.
const ACCENT_SLOTS: [usize; 6] = [4, 5, 6, 7, 8, 9];

/// How the seventh series and beyond vary the accent they share with an
/// earlier one: Excel's own theme-colour variants, in Excel's order.
///
/// A positive number is a **tint** — that fraction of the way to white,
/// Excel's "Lighter 40%". A negative one is a **shade**, its magnitude being
/// the fraction of the way to black, Excel's "Darker 25%". Round zero is the
/// accent itself and is not in the table.
///
/// Five rounds past the accent is thirty-six distinguishable series, which is
/// past the point a legend is readable at all; beyond that the table cycles and
/// two series can share a colour again. That is a bound worth stating rather
/// than a claim of infinite distinctness.
const ACCENT_VARIANTS: [f64; 5] = [-0.25, 0.40, -0.50, 0.60, 0.80];

/// `n` series colours from the workbook's own theme accents.
///
/// The workbook's, not a palette invented here — a chart should match the file
/// it came from. [`Workbook::theme_slot`] already falls back to the stock
/// Office accent for a slot this file does not define, which is what the canvas
/// does by filtering the slice and substituting its own list.
///
/// **Past the sixth series the accent is varied, not repeated** (`CHT-09`).
/// A theme has six accents and this used to cycle them, so a seven-series
/// chart gave series 1 and series 7 the same `4472C4` and a legend that could
/// not tell them apart — correct at eight series and wrong at seven, since
/// eight is where a reader expects to see a colour twice and seven is not.
/// Series 7 is now accent 1 darkened 25%, series 8 accent 2 darkened 25%, and
/// so on through `ACCENT_VARIANTS`: the chart still matches the file's theme,
/// which is what the cycling was protecting, and the colours are distinct,
/// which is what a legend needs.
#[must_use]
pub fn series_colors(workbook: &Workbook, n: usize) -> Vec<String> {
    (0..n).map(|i| series_color(workbook, i)).collect()
}

/// The colour series `i` gets, without building the ones before it.
///
/// [`series_colors`] is the whole list, which is what a legend and a bar plot
/// both want. A pie does not: it has one colour per *value*, and a pie of ten
/// thousand values would allocate ten thousand `String`s per frame to use the
/// handful that survive `push_pie`'s merge.
#[must_use]
pub fn series_color(workbook: &Workbook, i: usize) -> String {
    let slots = ACCENT_SLOTS.len();
    let accent = workbook.theme_slot(ACCENT_SLOTS[i % slots]);
    match (i / slots).checked_sub(1) {
        None => accent.to_owned(),
        Some(round) => vary(accent, ACCENT_VARIANTS[round % ACCENT_VARIANTS.len()]),
    }
}

/// Tint (`amount > 0`, toward white) or shade (`amount < 0`, toward black) an
/// `RRGGBB` hex colour, returning `RRGGBB`.
///
/// A colour that is not six hex digits is returned unchanged rather than
/// guessed at: [`Workbook::theme_slot`] can hand back whatever the file wrote,
/// and a chart that invents a colour for an unreadable one is worse than a
/// chart that repeats the accent.
fn vary(color: &str, amount: f64) -> String {
    let hex = color.strip_prefix('#').unwrap_or(color);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return color.to_owned();
    }
    let mut out = String::with_capacity(6);
    for i in 0..3 {
        let Ok(channel) = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16) else {
            return color.to_owned();
        };
        let v = f64::from(channel);
        let moved = if amount >= 0.0 {
            v + (255.0 - v) * amount
        } else {
            v * (1.0 + amount)
        };
        // `round` and not a truncation: a shade of 0.75 on 0x44 is 51.0 exactly,
        // and truncating a value that arrived at 50.999… from the other side
        // would make the palette depend on floating-point noise.
        out.push_str(&format!("{:02X}", moved.round().clamp(0.0, 255.0) as u8));
    }
    out
}

/// One series, resolved to the numbers it plots.
#[derive(Debug, Clone)]
pub struct ResolvedSeries {
    /// The series name as displayed, empty when it has none.
    pub name: String,
    /// Its points, in order; `None` is a gap rather than a zero.
    pub values: Vec<Option<f64>>,
    /// Its reference names no cells at all — `#REF!`, an unparseable string, or
    /// a sheet this workbook does not have. Distinct from *empty*: a series
    /// over a range of blank cells is not broken, it is unfilled.
    pub broken: bool,
    /// Points the read bound refused, and therefore points this series does not
    /// carry (`CHT-11`).
    ///
    /// Zero for every chart anybody drew on purpose: the bound is
    /// [`MAX_SERIES_POINTS`](crate::chart_data::MAX_SERIES_POINTS), 65,536
    /// points, and it exists so that a reference naming a region of the grid
    /// cannot make the reader allocate without limit. Non-zero is a **loss**,
    /// so the legend says so rather than the picture quietly showing a prefix.
    pub truncated: usize,
    /// What this series is drawn as: its own kind in a combination chart, and
    /// the chart's kind otherwise. Resolved here so the plotter never has to
    /// pair a resolved series back up with the model series it came from —
    /// which it could not do anyway, since a series that resolved to nothing
    /// is dropped and the two lists stop being index-aligned.
    pub kind: ChartKind,
    /// Whether it is measured against the secondary value axis.
    pub secondary_axis: bool,
    /// Whether each of its points is labelled with its value.
    pub data_labels: bool,
    /// Its palette slot, which is its position among the series that survived
    /// resolution — the index [`series_color`] is asked for.
    ///
    /// Carried rather than recomputed because a combination chart draws its
    /// bars and its lines in two passes, and a colour taken from a position in
    /// one of those passes is not the colour the legend drew.
    pub slot: usize,
}

/// A chart's series and category labels, resolved against the cached values.
///
/// A series with no value that resolved is dropped rather than plotted as
/// zeroes — a chart of flat zeroes looks like data, which is worse than a chart
/// with one series missing. That rule is the canvas's, kept.
///
/// **A series whose reference resolves to no cells at all is kept, and marked**
/// ([`ResolvedSeries::broken`]). Dropping it is the same silence a cell would
/// have if `#REF!` printed as blank: the picture loses a series and says
/// nothing, so a chart that was plotting three things plots two and looks
/// finished (`CHT-08`). The legend names it instead.
#[must_use]
pub fn resolve(
    workbook: &Workbook,
    sheet_index: usize,
    chart: &ChartView,
) -> (Vec<String>, Vec<ResolvedSeries>) {
    let cats = chart
        .series
        .first()
        .and_then(|s| s.categories.as_deref())
        .map(|r| ref_text(workbook, sheet_index, r))
        .unwrap_or_default();
    let mut series: Vec<ResolvedSeries> = chart
        .series
        .iter()
        .map(|se| {
            // Resolved **once**. This used to call `ref_cells` to ask whether
            // the reference named anything and then `ref_numbers`, which
            // resolves it again — so every series was parsed twice and its
            // whole address list built twice, the second copy thrown away
            // unread.
            let strip = ref_strip(workbook, sheet_index, &se.values);
            ResolvedSeries {
                name: se.name.clone(),
                values: strip
                    .map(|s| strip_numbers(workbook, s))
                    .unwrap_or_default(),
                broken: strip.is_none(),
                truncated: strip.map_or(0, |s| s.truncated),
                // A per-series kind only means anything where the chart has
                // groups to combine. A pie has none, so a `Line` on one of its
                // series is ignored rather than drawn over the slices.
                kind: match se.kind {
                    Some(k) if combinable(k) && combinable(chart.kind) => k,
                    _ => chart.kind,
                },
                secondary_axis: se.secondary_axis,
                data_labels: se.data_labels,
                slot: 0,
            }
        })
        .filter(|s| s.broken || s.values.iter().any(Option::is_some))
        .collect();
    // After the filter, so the palette is the one the legend draws — which is
    // the behaviour that was already here, now written down rather than implied
    // by an `enumerate` at each use.
    for (i, s) in series.iter_mut().enumerate() {
        s.slot = i;
    }
    (cats, series)
}

/// Whether `kind` can share one plot area with a different kind.
///
/// The combination family, and only it. A pie has no axes to share; a scatter's
/// horizontal axis is a value axis rather than a category axis, so its points
/// do not land on the same x positions as a column's. Mixing either with a
/// column would draw a picture that is not a chart of anything.
fn combinable(kind: ChartKind) -> bool {
    matches!(
        kind,
        ChartKind::Bar | ChartKind::Column | ChartKind::Line | ChartKind::Area
    )
}

/// The value range an axis has to cover, always including zero so a bar's
/// length is proportional to its value.
#[must_use]
pub fn value_extent(series: &[ResolvedSeries]) -> (f64, f64) {
    extent_of(&series.iter().collect::<Vec<_>>(), None)
}

/// The percent-stacked axis, in percent rather than in fractions.
///
/// The axis labels are the display list's only statement of what the numbers
/// are, and it has no percent format — so `0` and `100` say "a share of the
/// whole" where `0` and `1` would read as a chart of ones.
const PERCENT_FULL: f64 = 100.0;

/// The value range an axis has to cover for `series` under `grouping`.
///
/// **Stacking changes the extent, not only the geometry.** A stacked column of
/// 100 and 140 reaches 240, and an axis measured on the individual values
/// would clip it at 140 — which is how a stacked chart drawn on a clustered
/// extent comes out with bars running off the top of the plot rather than
/// merely looking wrong.
fn extent_of(series: &[&ResolvedSeries], grouping: Option<ChartGrouping>) -> (f64, f64) {
    let (mut lo, mut hi) = (0.0f64, 0.0f64);
    match grouping {
        // Normalised, so the axis is the whole and nothing else.
        Some(ChartGrouping::PercentStacked) => hi = PERCENT_FULL,
        Some(ChartGrouping::Stacked) => {
            let points = series.iter().map(|s| s.values.len()).max().unwrap_or(0);
            for i in 0..points {
                // Positive and negative stack away from zero in opposite
                // directions, so the two run in separate totals — one signed
                // sum would let a negative shorten the positive stack it is
                // drawn nowhere near.
                let (mut up, mut down) = (0.0f64, 0.0f64);
                for v in series.iter().filter_map(|s| s.values.get(i)).flatten() {
                    if *v >= 0.0 { up += *v } else { down += *v }
                }
                lo = lo.min(down);
                hi = hi.max(up);
            }
        }
        _ => {
            for s in series {
                for v in s.values.iter().flatten() {
                    lo = lo.min(*v);
                    hi = hi.max(*v);
                }
            }
        }
    }
    // A flat series would otherwise divide by zero when it is scaled.
    if lo == hi {
        hi = lo + 1.0;
    }
    (lo, hi)
}

/// The grouping that applies to `kind`, or `None` where the kind has no groups.
///
/// A `Stacked` on a pie or a scatter is meaningless, and this is where that is
/// decided rather than discovered: the model may carry one, and the plotter
/// ignores it.
fn grouping_for(kind: ChartKind, grouping: Option<ChartGrouping>) -> Option<ChartGrouping> {
    let g = grouping?;
    match kind {
        ChartKind::Bar | ChartKind::Column | ChartKind::Line | ChartKind::Area
            if g.is_stacked() =>
        {
            Some(g)
        }
        _ => None,
    }
}

/// A plot rectangle in twips, kept in floating point while it is whittled down
/// by the title, the axis titles and the legend, exactly as the canvas does.
#[derive(Debug, Clone, Copy)]
struct Plot {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// The smallest plot worth drawing: below this the canvas gives up, and so does
/// this, rather than emitting geometry nobody can read.
const MIN_PLOT: f64 = 20.0 * PX;

fn round(v: f64) -> i64 {
    if v.is_finite() { v.round() as i64 } else { 0 }
}

fn point(x: f64, y: f64) -> Point {
    Point {
        x: round(x),
        y: round(y),
    }
}

/// The box one line of chart text is placed in: the canvas draws from a point
/// with `textBaseline = "top"`, and the display list carries a rectangle the
/// backend centres in, so the box is one line of `size_px` starting at `y`.
#[derive(Debug, Clone, Copy)]
struct Line {
    x: f64,
    y: f64,
    w: f64,
    size_px: f64,
    bold: bool,
}

/// A line box `size_px` tall, in the weight everything but a title uses.
fn line(x: f64, y: f64, w: f64, size_px: f64) -> Line {
    Line {
        x,
        y,
        w,
        size_px,
        bold: false,
    }
}

fn text_at(at: Line, content: String, align: Align, color: &str) -> PaintItem {
    PaintItem::Text {
        rect: Rect {
            x: round(at.x),
            y: round(at.y),
            w: round(at.w),
            h: round(at.size_px),
        },
        content,
        align,
        color: Some(color.to_owned()),
        bold: at.bold,
        italic: false,
        font_name: None,
        font_pt: Some(pt(at.size_px)),
    }
}

/// The rectangle the legend gets, and how its entries run.
#[derive(Debug, Clone, Copy)]
struct LegendBox {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    /// Stacked down a side, rather than run across the foot.
    rows: bool,
}

/// The point size the canvas draws legend text at, in CSS pixels.
const LEGEND_PT: f64 = 10.0;

/// The suffix a broken series' legend entry carries.
///
/// The cell spelling, on purpose: a reader who has seen `#REF!` in the grid
/// already knows what it means, and a chart that invents its own word for the
/// same condition teaches two.
const BROKEN_SUFFIX: &str = " (#REF!)";

/// The suffix a series the read bound cut short carries.
///
/// The picture's own report. Layout has no compatibility report to write into
/// — that belongs to import — so the only place a reader can be told that this
/// series is a prefix of itself is the legend, which is where `CHT-08` already
/// puts `#REF!` for the same reason.
const TRUNCATED_SUFFIX: &str = " (truncated)";

/// A series' displayed name, which is its position when it has none.
///
/// The canvas's `s.name || \`Series ${i + 1}\``, kept: a blank name must
/// measure as the text that will be drawn, or the box is sized for nothing and
/// the label runs out of it.
///
/// A broken series is named **and** marked. Naming it is what stops the picture
/// losing a series in silence; marking it is what stops the reader taking an
/// empty slot for an empty range (`CHT-08`).
fn legend_label(series: &ResolvedSeries, index: usize) -> String {
    let mut label = if series.name.is_empty() {
        format!("Series {}", index + 1)
    } else {
        series.name.clone()
    };
    if series.broken {
        label.push_str(BROKEN_SUFFIX);
    }
    if series.truncated > 0 {
        label.push_str(TRUNCATED_SUFFIX);
    }
    label
}

/// How wide a legend label is, in twips.
///
/// `casual-calc-text` measures the bundled face; the canvas measures the
/// browser's `system-ui` with `measureText`. They differ a little, and that is
/// the same difference the two already have for every other string. What
/// matters is that this is measured in the face the PNG *draws* it in, so the
/// box fits its contents — which is what the plot rectangle is computed from.
fn label_width(label: &str) -> f64 {
    f64::from(casual_calc_text::advance_width(
        label,
        LEGEND_PT as f32,
        false,
        false,
        None,
    )) * PX
}

/// Reserve the legend's side of the frame, shrinking `plot` to what is left.
///
/// A line-for-line port of `legendBox` in `webapp/editor.js`, including the
/// order it mutates `plot` in: the legend comes off the frame *before* the axis
/// titles do, or the two disagree about what is left.
///
/// `None` when the frame is too small to give it one — a legend that leaves no
/// room for the plot has cost more than it explains.
fn legend_box(side: &str, series: &[ResolvedSeries], w: f64, plot: &mut Plot) -> Option<LegendBox> {
    let widest = series
        .iter()
        .enumerate()
        .map(|(i, s)| label_width(&legend_label(s, i)))
        .fold(24.0 * PX, f64::max);

    if side == "r" || side == "l" || side == "tr" {
        // `Math.floor(w * 0.4)` in CSS pixels, floored there and not in twips.
        let cap = (w / PX * 0.4).floor() * PX;
        let width = (widest + 22.0 * PX).min(cap);
        if plot.w - width < 40.0 * PX {
            return None;
        }
        plot.w -= width;
        let left = if side == "l" {
            plot.x
        } else {
            plot.x + plot.w + 6.0 * PX
        };
        if side == "l" {
            plot.x += width;
        }
        return Some(LegendBox {
            x: left,
            y: plot.y,
            w: width,
            h: plot.h,
            rows: true,
        });
    }

    let height = 14.0 * PX;
    if plot.h - height < 40.0 * PX {
        return None;
    }
    plot.h -= height;
    let top_edge = if side == "t" {
        plot.y
    } else {
        plot.y + plot.h + 2.0 * PX
    };
    if side == "t" {
        plot.y += height;
    }
    Some(LegendBox {
        x: plot.x,
        y: top_edge,
        w: plot.w,
        h: height,
        rows: false,
    })
}

/// A swatch and a name per series, stacked down the side or run across the foot.
///
/// The port of `drawLegend`, including its overflow rule: a row past the bottom
/// of the box stops the list rather than drawing over the plot.
fn draw_legend(
    list: &mut DisplayList,
    workbook: &Workbook,
    series: &[ResolvedSeries],
    at: LegendBox,
) {
    let palette = series_colors(workbook, series.len());
    let mut cursor_x = at.x;
    for (i, s) in series.iter().enumerate() {
        let label = legend_label(s, i);
        let cy = if at.rows {
            at.y + 8.0 * PX + (i as f64) * 14.0 * PX
        } else {
            at.y + at.h / 2.0
        };
        let cx = if at.rows { at.x } else { cursor_x };
        if at.rows && cy > at.y + at.h {
            return;
        }
        let swatch = 8.0 * PX;
        list.items.push(PaintItem::Polygon {
            points: vec![
                point(cx, cy - 4.0 * PX),
                point(cx + swatch, cy - 4.0 * PX),
                point(cx + swatch, cy + 4.0 * PX),
                point(cx, cy + 4.0 * PX),
            ],
            fill: palette
                .get(i)
                .cloned()
                .unwrap_or_else(|| CHART_MUTED.to_owned()),
        });
        let width = label_width(&label);
        // Never wider than the legend has. The canvas clips the chart to its
        // frame and the display list has no clip primitive (see the module
        // docs), so a name longer than its column would otherwise be given a
        // box reaching over the plot. Bounding it here keeps the *geometry*
        // honest even where the backend cannot clip.
        let room = (at.x + at.w - (cx + 12.0 * PX)).max(1.0);
        // `textBaseline = "middle"` on the canvas; the display list centres text
        // in the box it is given, so the box is one line centred on `cy`.
        list.items.push(text_at(
            line(
                cx + 12.0 * PX,
                cy - LEGEND_PT * PX / 2.0,
                width.max(1.0).min(room),
                LEGEND_PT * PX,
            ),
            label,
            Align::Left,
            CHART_FG,
        ));
        cursor_x = cx + 12.0 * PX + width + 10.0 * PX;
    }
}

/// Build one chart's geometry into `list`, inside the twip rectangle `frame`.
///
/// `frame` is the anchored rectangle the caller resolved from the chart's cells
/// and EMU offsets — the same rectangle a picture gets, for the same reason.
pub fn push_chart(
    list: &mut DisplayList,
    workbook: &Workbook,
    sheet_index: usize,
    chart: &ChartView,
    frame: Rect,
) {
    if frame.w <= 0 || frame.h <= 0 {
        return;
    }
    let (x, y, w, h) = (
        frame.x as f64,
        frame.y as f64,
        frame.w as f64,
        frame.h as f64,
    );

    // Ground, then outline. The outline is inset by half its own width so it
    // falls inside the frame, which is what the canvas's `x + 0.5` offset
    // achieves for a one-pixel stroke.
    list.items.push(PaintItem::Polygon {
        points: vec![
            point(x, y),
            point(x + w, y),
            point(x + w, y + h),
            point(x, y + h),
        ],
        fill: CHART_BG.to_owned(),
    });
    let half = PX / 2.0;
    list.items.push(PaintItem::Polyline {
        points: vec![
            point(x + half, y + half),
            point(x + w - half, y + half),
            point(x + w - half, y + h - half),
            point(x + half, y + h - half),
            point(x + half, y + half),
        ],
        width: round(PX),
        color: CHART_BORDER.to_owned(),
    });

    let mut top = y + 6.0 * PX;
    if !chart.title.is_empty() {
        list.items.push(text_at(
            Line {
                bold: true,
                ..line(x, top, w, 12.0 * PX)
            },
            chart.title.clone(),
            Align::Center,
            CHART_FG,
        ));
        top += 18.0 * PX;
    }

    let (cats, series) = resolve(workbook, sheet_index, chart);
    if series.is_empty() || series.iter().all(|s| s.broken) {
        // Honest rather than blank: the chart exists, its data did not resolve.
        // The two cases are told apart, because they are different faults — an
        // empty range is a chart waiting for numbers, and a broken reference is
        // a chart that used to have them (`CHT-08`).
        let note = if series.is_empty() {
            "no data"
        } else {
            "series reference broken (#REF!)"
        };
        list.items.push(text_at(
            line(x + 8.0 * PX, top, w - 8.0 * PX, 11.0 * PX),
            note.to_owned(),
            Align::Left,
            CHART_MUTED,
        ));
        return;
    }

    let mut plot = Plot {
        x: x + 34.0 * PX,
        y: top,
        w: w - 44.0 * PX,
        h: y + h - top - 18.0 * PX,
    };
    // The legend takes its side out of the plot before anything is drawn, or the
    // bars run underneath it — and before the axis titles, which is the order
    // the canvas uses. Two or more series without one are unreadable: three
    // rectangles and no way to tell which is which.
    let legend = chart
        .legend
        .as_deref()
        .and_then(|side| legend_box(side, &series, w, &mut plot));
    if plot.w < MIN_PLOT || plot.h < MIN_PLOT {
        return;
    }
    if let Some(at) = legend {
        draw_legend(list, workbook, &series, at);
    }
    if !chart.x_title.is_empty() {
        list.items.push(text_at(
            line(plot.x, y + h - 11.0 * PX - 5.0 * PX, plot.w, 10.0 * PX),
            chart.x_title.clone(),
            Align::Center,
            CHART_MUTED,
        ));
        plot.h -= 11.0 * PX;
    }
    if !chart.y_title.is_empty() {
        // Reserved but not drawn: rotated text has no display-list variant. The
        // reservation is what keeps the plot rectangle equal to the canvas's.
        plot.x += 10.0 * PX;
        plot.w -= 10.0 * PX;
    }
    if plot.w < MIN_PLOT || plot.h < MIN_PLOT {
        return;
    }

    match chart.kind {
        ChartKind::Pie | ChartKind::Doughnut => push_pie(list, workbook, chart, &series, plot),
        ChartKind::Line
        | ChartKind::Area
        | ChartKind::Scatter
        | ChartKind::Bar
        | ChartKind::Column => {
            push_xy(list, workbook, chart, &cats, &series, plot);
        }
        ChartKind::Unsupported => {
            // The same note the canvas leaves, in the same place: visibly
            // incomplete rather than silently wrong, which is what
            // `ChartKind::Unsupported` is documented to be.
            list.items.push(text_at(
                line(plot.x, plot.y, plot.w, 11.0 * PX),
                "unsupported chart not drawn".to_owned(),
                Align::Left,
                CHART_MUTED,
            ));
        }
    }
}

/// One series and the axis it is measured against.
///
/// A chart may have two value axes, and a bar's height is `(hi - v) / (hi -
/// lo)` of the plot for **its** axis. Carrying the scale beside the series is
/// what lets one bar plot draw series measured against two different extents
/// without a second pass over the geometry.
#[derive(Debug, Clone, Copy)]
struct Scaled<'a> {
    series: &'a ResolvedSeries,
    lo: f64,
    hi: f64,
    /// Where this axis' zero sits, which is what a bar is measured from so a
    /// negative value draws downward.
    zero_y: f64,
}

impl Scaled<'_> {
    /// Where a value sits in the plot, vertically.
    fn y(&self, plot: Plot, v: f64) -> f64 {
        plot.y + plot.h * ((self.hi - v) / (self.hi - self.lo))
    }
}

/// The most points a series will label. Above it, labels are not drawn.
///
/// **Part of the feature, not a follow-up.** A data label is a
/// [`PaintItem::Text`] per point, so labels roughly double a bar plot's item
/// count and add text shaping to a path `CHT-06` had to cap in the first place.
/// Two hundred labels do not fit across a 400 px plot in any case — they would
/// be two pixels apart — so the cap costs nothing a reader could have used, and
/// the chart says so rather than quietly drawing fewer.
pub const MAX_LABEL_POINTS: usize = 200;

/// Everything with a category axis and a value axis: bars, columns, lines,
/// areas and scatters, in any combination.
///
/// One function rather than the two this used to dispatch between, because a
/// **combination** chart is not one kind or the other — the bars and the lines
/// share a plot, a category axis and a set of x positions, and drawing them in
/// two independent passes would give each its own extent and its own bar width.
///
/// The **secondary axis** is measured here for the same reason. On one shared
/// extent, a margin percentage beside revenue in millions is 0.000058 px of a
/// 200 px plot: the series is drawn, and it is invisible.
fn push_xy(
    list: &mut DisplayList,
    workbook: &Workbook,
    chart: &ChartView,
    cats: &[String],
    series: &[ResolvedSeries],
    plot: Plot,
) {
    let primary: Vec<&ResolvedSeries> = series.iter().filter(|s| !s.secondary_axis).collect();
    let secondary: Vec<&ResolvedSeries> = series.iter().filter(|s| s.secondary_axis).collect();
    // A chart whose every series asks for the secondary axis has one axis, not
    // an empty primary one: "secondary" is a relation, and there is nothing
    // here for it to be secondary to.
    let (primary, secondary) = if primary.is_empty() {
        (secondary, Vec::new())
    } else {
        (primary, secondary)
    };

    let grouping = grouping_for(chart.kind, chart.grouping);
    let (lo, hi) = extent_of(&primary, grouping);
    let zero_y = push_axes(list, plot, lo, hi);
    let mut scaled: Vec<Scaled> = primary
        .iter()
        .map(|s| Scaled {
            series: s,
            lo,
            hi,
            zero_y,
        })
        .collect();
    if !secondary.is_empty() {
        let (lo2, hi2) = extent_of(&secondary, grouping);
        let zero2 = push_secondary_axis(list, plot, lo2, hi2);
        scaled.extend(secondary.iter().map(|s| Scaled {
            series: s,
            lo: lo2,
            hi: hi2,
            zero_y: zero2,
        }));
    }
    // Back into plot order, so the display list does not depend on which axis a
    // series happens to be on — two charts differing only in that would
    // otherwise emit their items in different orders.
    scaled.sort_by_key(|s| s.series.slot);

    let points = series.iter().map(|s| s.values.len()).max().unwrap_or(0);
    let bars: Vec<Scaled> = scaled
        .iter()
        .filter(|s| matches!(s.series.kind, ChartKind::Bar | ChartKind::Column))
        .copied()
        .collect();
    let lines: Vec<Scaled> = scaled
        .iter()
        .filter(|s| !matches!(s.series.kind, ChartKind::Bar | ChartKind::Column))
        .copied()
        .collect();

    if !bars.is_empty() {
        push_bars(list, workbook, &bars, plot, grouping, points, series.len());
    }
    if !lines.is_empty() {
        push_line(list, workbook, &lines, plot, points, series.len());
    }
    push_category_labels(list, chart.kind, cats, plot, points);
}

/// The second value axis, on the right, returning its zero line's y.
///
/// No zero *line* is drawn for it: the primary axis already drew one across the
/// plot, and a second horizontal rule at a different height reads as data.
fn push_secondary_axis(list: &mut DisplayList, plot: Plot, lo: f64, hi: f64) -> f64 {
    list.items.push(PaintItem::Polyline {
        points: vec![
            point(plot.x + plot.w, plot.y),
            point(plot.x + plot.w, plot.y + plot.h),
        ],
        width: round(PX),
        color: CHART_AXIS.to_owned(),
    });
    let label_w = 34.0 * PX;
    for (value, cy) in [(hi, plot.y + 4.0 * PX), (lo, plot.y + plot.h - 4.0 * PX)] {
        list.items.push(text_at(
            line(plot.x + plot.w + 3.0 * PX, cy - 4.5 * PX, label_w, 9.0 * PX),
            axis_label(value),
            Align::Left,
            CHART_MUTED,
        ));
    }
    plot.y + plot.h * (hi / (hi - lo))
}

/// A point's value, beside the mark that draws it.
///
/// Nothing is drawn past [`MAX_LABEL_POINTS`]; the caller decides that once for
/// the whole plot rather than per point, so a chart either labels its series or
/// says it did not.
fn push_label(list: &mut DisplayList, x: f64, y: f64, value: f64) {
    let width = 40.0 * PX;
    list.items.push(text_at(
        line(x - width / 2.0, y - 11.0 * PX, width, 9.0 * PX),
        axis_label(value),
        Align::Center,
        CHART_FG,
    ));
}

/// The value axis and the zero line, returning the zero line's y.
///
/// Both axes are one polyline each rather than one path with two subpaths: the
/// display list has no subpaths, and two items cost nothing.
fn push_axes(list: &mut DisplayList, plot: Plot, lo: f64, hi: f64) -> f64 {
    let zero_y = plot.y + plot.h * (hi / (hi - lo));
    list.items.push(PaintItem::Polyline {
        points: vec![point(plot.x, plot.y), point(plot.x, plot.y + plot.h)],
        width: round(PX),
        color: CHART_AXIS.to_owned(),
    });
    list.items.push(PaintItem::Polyline {
        points: vec![point(plot.x, zero_y), point(plot.x + plot.w, zero_y)],
        width: round(PX),
        color: CHART_AXIS.to_owned(),
    });
    // The extremes, right-aligned in a box ending three pixels left of the
    // axis — which is where the canvas ends the *text*, not the box.
    //
    // A backend insets a right-aligned run from its box's edge by its own
    // padding, so the label lands a couple of twips left of where the canvas
    // puts it. Compensating here would mean writing the backend's inset into
    // layout, and [`PaintItem::DataBar`](crate::PaintItem::DataBar) already
    // settled that argument the other way: the inset is the backend's
    // business, and a device quantity copied into layout is a copy that drifts.
    // So the small difference is left, and named.
    let label_w = 34.0 * PX;
    for (value, cy) in [(hi, plot.y + 4.0 * PX), (lo, plot.y + plot.h - 4.0 * PX)] {
        list.items.push(text_at(
            line(
                plot.x - 3.0 * PX - label_w,
                cy - 4.5 * PX,
                label_w,
                9.0 * PX,
            ),
            axis_label(value),
            Align::Right,
            CHART_MUTED,
        ));
    }
    zero_y
}

/// An axis extreme as the canvas prints it: two decimal places at most, and no
/// trailing `.0` — `Math.round(v * 100) / 100` through `String`.
fn axis_label(v: f64) -> String {
    let rounded = (v * 100.0).round() / 100.0;
    let mut s = format!("{rounded}");
    if s.ends_with(".0") {
        s.truncate(s.len() - 2);
    }
    s
}

/// The most bar polygons one bar or column plot will emit, whatever its frame
/// and whatever its data.
///
/// `bar_groups`'s geometric bound is already `0.35 ×` the plot's width in
/// pixels, so this only binds a chart anchored over a frame thousands of pixels
/// wide. It is here as a **resource bound** rather than a picture decision, on
/// the same rule every parser in this workspace follows: a size the caller
/// controls needs a ceiling the caller does not.
pub const MAX_BAR_POLYGONS: usize = 2048;

/// How many category groups a bar plot of `points` points can actually draw.
///
/// Below the group width where `bar_w` hits its `.max(PX)` clamp, `bw` — the
/// polygon's body, `bar_w - PX` — is **zero**, and the plot emits one
/// zero-width polygon per point per series: six series over a thousand rows in
/// a 400 px frame was 6,000 polygons of which every single one drew nothing
/// (`CHT-06`). So the bound is the group count at which a bar still has a body
/// of at least one pixel: `bar_w >= 2 · PX`, hence
/// `groups <= plot_w · 0.7 / (2 · series · PX)`.
///
/// The points above the bound are **not discarded** — see [`push_bars`], which
/// merges them into a bucket drawn from the bucket's minimum to its maximum.
fn bar_groups(plot_w: f64, series: usize, points: usize) -> usize {
    let series = series.max(1);
    let resolvable = plot_w * 0.7 / (2.0 * series as f64 * PX);
    let resolvable = if resolvable.is_finite() && resolvable >= 1.0 {
        resolvable.floor() as usize
    } else {
        1
    };
    points.min(resolvable).min(MAX_BAR_POLYGONS / series).max(1)
}

/// The half-open point range group `g` of `groups` covers, over `points` points.
///
/// Integer arithmetic on purpose: `groups == points` must give exactly
/// `[g, g + 1)`, or the uncapped plot would stop being the plot it was.
fn bucket(g: usize, groups: usize, points: usize) -> (usize, usize) {
    (g * points / groups, (g + 1) * points / groups)
}

/// The bar and column plot.
///
/// **Stacking is a change to the arithmetic, not to the item count.** A stacked
/// bar is the same polygon at a different `y0`: every category gets one column
/// the full width of its group, and each series' band is measured from the top
/// of the one before it rather than from the axis. Clustered is unchanged —
/// bands side by side, each from the axis.
fn push_bars(
    list: &mut DisplayList,
    workbook: &Workbook,
    series: &[Scaled],
    plot: Plot,
    grouping: Option<ChartGrouping>,
    points: usize,
    plot_series: usize,
) {
    let stacked = grouping.is_some_and(ChartGrouping::is_stacked);
    let percent = grouping == Some(ChartGrouping::PercentStacked);
    if points == 0 {
        return;
    }
    // Whether the plot labels its points at all, decided once for the whole
    // plot: a chart that labelled its first two hundred points and stopped
    // would read as data that ends.
    let label = series.iter().any(|s| s.series.data_labels) && points <= MAX_LABEL_POINTS;
    // **The bound, and what it costs.** Past what the plot can resolve, points
    // are bucketed and each bucket draws one rectangle per series spanning that
    // bucket's minimum to its maximum. No value is dropped and no outlier is
    // hidden: the tallest bar in a bucket is exactly the top of the rectangle
    // and the deepest is exactly its bottom, so the ink covers the same values
    // the individual bars covered. What is given up is *horizontal* resolution
    // — which point inside the bucket held which value — below the width of one
    // bar, which is under a pixel and could not be read either way (`CHT-06`).
    // The **plot's** ceiling, shared across every series on it — the rule
    // `MAX_LINE_POINTS` and `MAX_SCATTER_MARKERS` already state. A combination
    // chart's bars must not get the whole allowance and leave nothing for its
    // lines, and a stacked plot must not resolve more groups than a clustered
    // one of the same data because it happens to draw them in one lane.
    let groups = bar_groups(plot.w, plot_series, points);
    let group_w = plot.w / groups as f64;
    // Every "1" in the canvas's arithmetic is one *pixel*; in twips it is
    // fifteen. Getting that wrong is invisible at a glance and a whole pixel
    // wide in the picture, which is how a bar came out a pixel too wide here.
    //
    // **Stacked columns share one bar.** The width is the group's rather than
    // the group's divided by the series count, which is the visible difference
    // between a stacked chart and a clustered one before any value is plotted.
    let lanes = if stacked { 1 } else { series.len() };
    let bar_w = (group_w * 0.7 / lanes as f64).max(PX);
    for g in 0..groups {
        let (from, to) = bucket(g, groups, points);
        // Where the next band starts, tracked separately above and below the
        // axis: positive and negative stack away from zero in opposite
        // directions. `lo` and `hi` are the bucket's own extremes, kept apart
        // so a bucket holding several points still covers exactly the values
        // its individual bars covered (`CHT-06`'s rule, carried into stacking).
        let (mut up_lo, mut up_hi, mut down_lo, mut down_hi) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        // What the whole category adds up to, for the percent-stacked scale.
        let total: f64 = if percent {
            series
                .iter()
                .filter_map(|s| bucket_span(s.series, from, to))
                .map(|(min, max)| (min.abs()).max(max.abs()))
                .sum()
        } else {
            0.0
        };
        for (si, s) in series.iter().enumerate() {
            let Some((min, max)) = bucket_span(s.series, from, to) else {
                continue;
            };
            // Normalised to a share of the category, which is what
            // `percentStacked` means and what makes every column reach the top.
            let (min, max) = if percent {
                if total <= 0.0 {
                    continue;
                }
                (min / total * PERCENT_FULL, max / total * PERCENT_FULL)
            } else {
                (min, max)
            };
            let (base_lo, base_hi) = if !stacked {
                (0.0, 0.0)
            } else if max >= 0.0 {
                (up_lo, up_hi)
            } else {
                (down_lo, down_hi)
            };
            // **The band runs from the base to the base plus the value**, not
            // between the bucket's two extremes: with one point in the bucket
            // `min == max`, and measuring the band between them would make
            // every stacked bar zero high. Where a bucket does hold several
            // points, the four candidate edges are taken together, so the ink
            // still covers exactly what the individual bands covered
            // (`CHT-06`'s rule).
            let (from_v, to_v) = if stacked {
                let edges = [base_lo, base_hi, base_lo + min, base_hi + max];
                (
                    edges.iter().copied().fold(f64::INFINITY, f64::min),
                    edges.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                )
            } else {
                (min, max)
            };
            if stacked {
                if max >= 0.0 {
                    up_lo = base_lo + min;
                    up_hi = base_hi + max;
                } else {
                    down_lo = base_lo + min;
                    down_hi = base_hi + max;
                }
            }
            // A stacked band is measured from the top of the one before it; a
            // clustered bar from the axis. Both are a rectangle between two
            // values, which is why one expression covers them: the clustered
            // case has a base of zero, and `s.zero_y` is where zero sits.
            let lane = if stacked { 0 } else { si };
            let bx = plot.x + g as f64 * group_w + group_w * 0.15 + lane as f64 * bar_w;
            let edges = [s.y(plot, from_v), s.y(plot, to_v), s.zero_y];
            let (y0, y1) = if stacked {
                // Never clamped to the axis: a band that does not touch zero is
                // exactly what stacking is.
                let a = edges[0].min(edges[1]);
                let b = edges[0].max(edges[1]);
                (a, b)
            } else {
                // A negative value draws downward from the zero line, which is
                // why the rectangle is measured from it rather than from the
                // axis. With one point in the bucket `top == bottom` and this
                // is the single bar it always was.
                (
                    edges[0].min(edges[1]).min(s.zero_y),
                    edges[0].max(edges[1]).max(s.zero_y),
                )
            };
            let bh = {
                let d = y1 - y0;
                // A zero value still shows a one-pixel mark on the axis rather
                // than nothing, so a zero and a gap look different.
                if d == 0.0 { PX } else { d }
            };
            let bw = bar_w - PX;
            list.items.push(PaintItem::Polygon {
                points: vec![
                    point(bx, y0),
                    point(bx + bw, y0),
                    point(bx + bw, y0 + bh),
                    point(bx, y0 + bh),
                ],
                fill: series_color(workbook, s.series.slot),
            });
            if label && s.series.data_labels {
                // The value the file holds, not the stacked or normalised
                // position: a label that read `240` on a band worth `140`
                // would be a second wrong picture on top of the first.
                push_label(list, bx + bw / 2.0, y0, max);
            }
        }
    }
}

/// One bucket's lowest and highest value in `series`, or `None` when it holds
/// no value at all.
fn bucket_span(series: &ResolvedSeries, from: usize, to: usize) -> Option<(f64, f64)> {
    // Clamped, not `get(from..to)`: the point count is the *longest* series',
    // so a shorter one has buckets that overrun its end, and an out-of-bounds
    // slice would return `None` for the whole bucket — silently dropping the
    // points at its start that do exist.
    let end = to.min(series.values.len());
    let mut span: Option<(f64, f64)> = None;
    for v in series.values[from.min(end)..end].iter().flatten() {
        span = Some(match span {
            None => (*v, *v),
            Some((min, max)) => (min.min(*v), max.max(*v)),
        });
    }
    span
}

/// The most points one polyline will carry, whatever the series' length.
///
/// A **byte** bound, which is the one `CHT-06` did not need. A line plot has
/// always been bounded in *items* — one polyline per series — and that is what
/// made its cost invisible: six series over 10,000 rows is 12 display-list
/// items and **1,181,094 bytes of JSON across the wasm boundary, every frame**,
/// which is larger than the 722 KB the uncapped bar plot was fixed for. An
/// item count is not a size.
///
/// Two points per pixel column is the geometric bound (see
/// `push_polyline`); this is the resource ceiling on top of it, for a chart
/// anchored over a frame thousands of pixels wide.
///
/// The ceiling is the **plot's**, shared out across its series, as
/// [`MAX_BAR_POLYGONS`] is. A per-series ceiling is not a bound on the picture:
/// six series each honouring 4,096 is 24,576, and the frame pays for the plot
/// rather than for a series.
pub const MAX_LINE_POINTS: usize = 4096;

/// The most markers one scatter plot will emit, across all of its series.
///
/// A scatter plot's ink is a marker at a position, so `CHT-06`'s min-max
/// bucket does not apply: a bar's rectangle runs from zero to its value and
/// covers everything between, and a marker covers four pixels and nothing else.
/// The bound is therefore on *positions* rather than on values — see
/// `push_markers`.
///
/// Shared out across the series for the reason [`MAX_LINE_POINTS`] gives, and
/// this is not a detail: with a per-series ceiling of 2,048 a six-series plot
/// measured **11,940 markers and 1.4 MB of JSON**, which is a cap that has not
/// capped anything a frame budget can feel.
pub const MAX_SCATTER_MARKERS: usize = 4096;

/// The most wedges one pie or doughnut will emit. See `push_pie`.
pub const MAX_PIE_WEDGES: usize = 1024;

/// The scatter marker's side, in CSS pixels: two markers whose centres are
/// closer than this overlap by more than half.
const MARKER_PX: f64 = 4.0;

/// The line, area and scatter plot.
///
/// `points` and `plot_series` are the **plot's**, not this call's: in a
/// combination chart the lines share their x positions with the bars beside
/// them, and share the item ceiling with them too.
fn push_line(
    list: &mut DisplayList,
    workbook: &Workbook,
    series: &[Scaled],
    plot: Plot,
    points: usize,
    plot_series: usize,
) {
    let step = if points > 1 {
        plot.w / (points - 1) as f64
    } else {
        0.0
    };
    let label = series.iter().any(|s| s.series.data_labels) && points <= MAX_LABEL_POINTS;
    for s in series {
        let frame = Frame {
            plot,
            step,
            lo: s.lo,
            hi: s.hi,
            series: plot_series.max(1),
        };
        let color = series_color(workbook, s.series.slot);
        if s.series.kind == ChartKind::Scatter {
            push_markers(list, s.series, &color, frame);
        } else {
            push_polyline(list, s.series, &color, frame);
        }
        if label && s.series.data_labels {
            for (i, v) in s.series.values.iter().enumerate() {
                if let Some(v) = v {
                    push_label(list, frame.x(i), frame.y(*v), *v);
                }
            }
        }
    }
}

/// Everything a series plot needs that is the same for every series on it.
///
/// One struct rather than five parameters: the value extent, the horizontal
/// step and how many series share the plot's item ceiling all belong to the
/// *plot* rather than to a series, and threading them one by one gave two
/// functions eight arguments each.
#[derive(Debug, Clone, Copy)]
struct Frame {
    plot: Plot,
    /// Twips between one point's x and the next's.
    step: f64,
    /// The value extent the axis covers.
    lo: f64,
    hi: f64,
    /// How many series divide this plot's item ceiling.
    series: usize,
}

impl Frame {
    /// Where a value sits in the plot, vertically.
    fn y(&self, v: f64) -> f64 {
        self.plot.y + self.plot.h * ((self.hi - v) / (self.hi - self.lo))
    }

    /// Where point `i` sits, horizontally.
    fn x(&self, i: usize) -> f64 {
        self.plot.x + i as f64 * self.step
    }
}

/// The lowest and highest point of one series inside one pixel column.
#[derive(Debug, Clone, Copy)]
struct Column {
    at: i64,
    /// `(point index, y)` of the smallest y in the column, and of the largest.
    top: (usize, f64),
    bottom: (usize, f64),
}

/// One series as polylines, at most two points per pixel column (`CHT-11`).
///
/// **What the bound preserves.** Within one pixel column a polyline's ink is
/// the vertical span between its lowest and its highest point there; every
/// point between them is drawn over. Emitting exactly those two, in the order
/// they occur, paints the same column of pixels. So no spike is lost and no
/// value is dropped — this is `CHT-06`'s min-max bucket again, and it is sound
/// here for the same reason it was sound for bars and is *not* sound for a
/// marker or a wedge.
///
/// **What it gives up.** The order of the wiggle inside one pixel column, and
/// the sub-pixel x of the two survivors. Neither is on the screen.
///
/// **What does not change.** With one point per column — which is every chart
/// whose series is no longer than its plot is wide — each column's top and
/// bottom are the same point, one point is emitted, and the polyline is the
/// one it always was.
fn push_polyline(list: &mut DisplayList, series: &ResolvedSeries, color: &str, frame: Frame) {
    // One pixel, or wider if a pixel would put more than this series' share of
    // the plot's ceiling into one line.
    let allowance = (MAX_LINE_POINTS / frame.series / 2).max(1);
    let pitch = (frame.plot.w / allowance as f64).max(PX);
    let mut run: Vec<Point> = Vec::new();
    let mut open: Option<Column> = None;
    for (i, v) in series.values.iter().enumerate() {
        let Some(v) = v else {
            // A gap breaks the line rather than being bridged across, which is
            // the canvas's `started = false`; here that means closing the open
            // column first — its points belong to the run that is ending — and
            // then starting a new polyline.
            flush_column(&mut run, open.take(), frame);
            flush_run(list, &mut run, color);
            continue;
        };
        let y = frame.y(*v);
        // Clamped to the last column, not merely divided into one. The final
        // point sits at exactly `plot.w`, which divides to one column *past*
        // the allowance and made the ceiling two points per series short of
        // being a ceiling — 4,098 emitted against a stated 4,096.
        let at = ((i as f64 * frame.step / pitch) as i64).min(allowance as i64 - 1);
        match &mut open {
            Some(c) if c.at == at => {
                if y < c.top.1 {
                    c.top = (i, y);
                }
                if y > c.bottom.1 {
                    c.bottom = (i, y);
                }
            }
            _ => {
                flush_column(&mut run, open.take(), frame);
                open = Some(Column {
                    at,
                    top: (i, y),
                    bottom: (i, y),
                });
            }
        }
    }
    flush_column(&mut run, open.take(), frame);
    flush_run(list, &mut run, color);
}

/// Append a closed column's survivors to the open run, in point order.
fn flush_column(run: &mut Vec<Point>, column: Option<Column>, frame: Frame) {
    let Some(c) = column else {
        return;
    };
    let at = |(i, y): (usize, f64)| point(frame.x(i), y);
    if c.top.0 == c.bottom.0 {
        run.push(at(c.top));
        return;
    }
    let (first, second) = if c.top.0 < c.bottom.0 {
        (c.top, c.bottom)
    } else {
        (c.bottom, c.top)
    };
    run.push(at(first));
    run.push(at(second));
}

/// One scatter series as markers, one per distinguishable position (`CHT-11`).
///
/// **Why not `CHT-06`'s bucket.** A bar's ink is a rectangle from zero to its
/// value, so merging two bars into their min-max span covers exactly what the
/// two covered. A marker's ink is four pixels around its own position and
/// nothing in between, so a min-max span would be a rectangle the data never
/// drew — and dropping every nth point would be the outlier-hiding lie
/// `CHT-06` was careful to avoid. The bound has to be on *positions*.
///
/// **What the bound preserves.** The plot is divided into cells one marker
/// wide, and the first point to land in a cell draws the marker for it. Since a
/// marker is `MARKER_PX` across, two points in one cell were already drawing
/// the same opaque square in the same place. Every point still contributes ink
/// within one cell of where it was; the cloud's outline, its extremes and its
/// empty regions are unchanged, and nothing is dropped from a place that would
/// otherwise be blank.
///
/// **What it gives up.** Overplotting *density* — after this you cannot tell
/// one point in a cell from fifty. You could not before either: the markers are
/// opaque, identical and in the same place, so the fiftieth painted exactly
/// what the first did.
///
/// **The cell is widened** past one marker if a marker-fine grid would hold
/// more than this series' share of [`MAX_SCATTER_MARKERS`] — a resource ceiling
/// on the same rule as [`MAX_BAR_POLYGONS`]. Widening merges rather than drops,
/// so the guarantee above survives it with a larger "one cell", and the cost is
/// stated plainly: **the merge radius grows with the series count**, because
/// the ceiling belongs to the plot and six series share it six ways. At a
/// 400x300 plot that is a 6-pixel cell for one series and 13 for six.
fn push_markers(list: &mut DisplayList, series: &ResolvedSeries, color: &str, frame: Frame) {
    let plot = frame.plot;
    let area = (plot.w / PX) * (plot.h / PX);
    let allowance = (MAX_SCATTER_MARKERS / frame.series).max(1);
    let ceiling = (area / allowance as f64).sqrt().ceil();
    let mut pitch = MARKER_PX.max(if ceiling.is_finite() {
        ceiling
    } else {
        MARKER_PX
    }) * PX;
    let cells = |extent: f64, pitch: f64| ((extent / pitch).ceil() as usize).max(1);
    let (mut nx, mut ny) = (cells(plot.w, pitch), cells(plot.h, pitch));
    // A pitch taken from the plot's *area* bounds the cell count only for a
    // plot near square. A frame a thousand pixels wide and twenty tall holds
    // far more cells than its area suggests, so the ceiling would not be one —
    // widen until it is. Terminates: the pitch doubles and the count at least
    // halves, ending at a single cell.
    while nx.saturating_mul(ny) > allowance && (nx > 1 || ny > 1) {
        pitch *= 2.0;
        nx = cells(plot.w, pitch);
        ny = cells(plot.h, pitch);
    }
    // Emission follows the data's order, not the grid's, so the display list is
    // deterministic — a hash set's iteration order would not be.
    let mut drawn = vec![false; nx.saturating_mul(ny)];
    for v in series.values.iter().enumerate() {
        let (i, Some(v)) = v else {
            continue;
        };
        let px = frame.x(i);
        let py = frame.y(*v);
        let cx = (((px - plot.x) / pitch) as usize).min(nx - 1);
        let cy = (((py - plot.y) / pitch) as usize).min(ny - 1);
        let cell = cy * nx + cx;
        if drawn[cell] {
            continue;
        }
        drawn[cell] = true;
        // A point marker: four pixels square, centred on the value.
        let half = MARKER_PX / 2.0 * PX;
        list.items.push(PaintItem::Polygon {
            points: vec![
                point(px - half, py - half),
                point(px + half, py - half),
                point(px + half, py + half),
                point(px - half, py + half),
            ],
            fill: color.to_owned(),
        });
    }
}

/// Emit the accumulated run as one polyline and start a new one.
fn flush_run(list: &mut DisplayList, run: &mut Vec<Point>, color: &str) {
    if run.len() >= 2 {
        list.items.push(PaintItem::Polyline {
            points: core::mem::take(run),
            // 1.8 px, the canvas's `lineWidth`.
            width: round(1.8 * PX),
            color: color.to_owned(),
        });
    } else {
        run.clear();
    }
}

fn push_pie(
    list: &mut DisplayList,
    workbook: &Workbook,
    chart: &ChartView,
    series: &[ResolvedSeries],
    plot: Plot,
) {
    // A pie plots one series, and only its positive values: a negative slice
    // has no meaning in a part-of-a-whole picture.
    let values: Vec<f64> = series[0]
        .values
        .iter()
        .flatten()
        .copied()
        .filter(|v| *v > 0.0)
        .collect();
    let total: f64 = values.iter().sum();
    if total <= 0.0 {
        return;
    }
    let cx = plot.x + plot.w / 2.0;
    let cy = plot.y + plot.h / 2.0;
    let r = (plot.w.min(plot.h) / 2.0 - 4.0 * PX).max(6.0 * PX);
    let inner = if chart.kind == ChartKind::Doughnut {
        r * 0.55
    } else {
        0.0
    };
    let least = min_sweep(r);
    // Twelve o'clock, clockwise, as Excel starts.
    let mut from = 0.0f64;
    // The open group: how much of the total it holds, and which of its values
    // is the largest — the one whose colour it will be drawn in.
    let (mut held, mut lead, mut lead_v) = (0.0f64, 0usize, f64::NEG_INFINITY);
    let mut last: Option<usize> = None;
    for (i, v) in values.iter().enumerate() {
        held += v;
        if *v > lead_v {
            (lead, lead_v) = (i, *v);
        }
        let sweep = held / total * 360.0;
        if sweep < least {
            continue;
        }
        last = Some(list.items.len());
        list.items.push(PaintItem::Wedge {
            center: point(cx, cy),
            radius: round(r),
            inner_radius: round(inner),
            from,
            sweep,
            fill: series_color(workbook, lead),
        });
        from += sweep;
        (held, lead, lead_v) = (0.0, i + 1, f64::NEG_INFINITY);
    }
    // **The tail joins the wedge before it rather than becoming one.** What is
    // left after the loop is by construction below the threshold, so drawing it
    // separately would put back exactly the hairline the merge exists to
    // remove — and would make the wedge count `MAX_PIE_WEDGES + 1`, so the
    // ceiling would be off by one from what it says. Extending the last wedge
    // to close the circle keeps the total at 360 and the count at the bound.
    if held > 0.0 {
        match last.and_then(|at| list.items.get_mut(at)) {
            Some(PaintItem::Wedge {
                from: at, sweep, ..
            }) => *sweep = 360.0 - *at,
            // Nothing was emitted at all, which needs every value to sit under
            // the threshold — only reachable if the threshold is a whole turn.
            _ => list.items.push(PaintItem::Wedge {
                center: point(cx, cy),
                radius: round(r),
                inner_radius: round(inner),
                from: 0.0,
                sweep: 360.0,
                fill: series_color(workbook, lead),
            }),
        }
    }
}

/// The narrowest wedge worth drawing, in degrees, for a pie of radius `r`.
///
/// **Why not `CHT-06`'s bucket, and what this one is.** A wedge's ink is an
/// *angle*, so merging by min and max would be meaningless — but a wedge's
/// angle is also **contiguous and ordered**, which a marker's position is not.
/// So adjacent wedges can be merged, and that is what `push_pie` does: the
/// merged wedge occupies exactly the angle its members occupied, in the same
/// place, and the total still closes at 360 degrees.
///
/// The threshold is two pixels of arc at the rim. Below it a wedge is a
/// hairline: a pie of 500 slices in a 300-pixel frame gives each slice about
/// 1.9 pixels of arc, and one of 10,000 gives it a tenth of a pixel — which is
/// how a pie came to emit 10,002 display-list items and 1.4 MB of JSON for a
/// picture with no visible divisions in it at all.
///
/// **What merging preserves.** Every value's angular position — each is still
/// inside the wedge it was merged into — the order around the circle, and the
/// total. **What it gives up.** The boundary between merged neighbours, and
/// the colour of every member of a group but its largest. Both were already
/// below two pixels of arc, which is to say neither was on the screen.
fn min_sweep(r: f64) -> f64 {
    let circumference = 2.0 * core::f64::consts::PI * r;
    let by_pixel = if circumference > 0.0 {
        360.0 * (2.0 * PX) / circumference
    } else {
        360.0
    };
    // And never fine enough to exceed the resource ceiling, whatever the frame.
    by_pixel.max(360.0 / MAX_PIE_WEDGES as f64)
}

/// Category labels under the plot, thinned to whatever fits: overlapping labels
/// are less readable than fewer of them.
///
/// The thinning rule is arithmetic on the plot width and the point count, not a
/// measurement, so it ports exactly. The ten-character truncation counts
/// `char`s rather than UTF-16 units as the canvas does — the two differ only
/// for text outside the basic plane.
fn push_category_labels(
    list: &mut DisplayList,
    kind: ChartKind,
    cats: &[String],
    plot: Plot,
    points: usize,
) {
    if cats.is_empty() || kind == ChartKind::Scatter || points == 0 {
        return;
    }
    let every = (((points as f64 * 34.0 * PX) / plot.w).ceil() as usize).max(1);
    let mut i = 0;
    while i < points {
        let Some(label) = cats.get(i).filter(|l| !l.is_empty()) else {
            i += every;
            continue;
        };
        let px = if points > 1 && kind != ChartKind::Column && kind != ChartKind::Bar {
            plot.x + (i as f64 * plot.w) / (points - 1) as f64
        } else {
            plot.x + (i as f64 + 0.5) * (plot.w / points as f64)
        };
        let shown = if label.chars().count() > 10 {
            let mut t: String = label.chars().take(9).collect();
            t.push('…');
            t
        } else {
            label.clone()
        };
        // Centred on `px`; the box is one group wide so a long label has room.
        let box_w = (plot.w / points as f64).max(34.0 * PX);
        list.items.push(text_at(
            line(
                px - box_w / 2.0,
                plot.y + plot.h + 3.0 * PX,
                box_w,
                9.0 * PX,
            ),
            shown,
            Align::Center,
            CHART_MUTED,
        ));
        i += every;
    }
}
