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

use casual_calc_model::{ChartKind, ChartView, Workbook};

use crate::chart_data::{ref_numbers, ref_text};
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

/// `n` series colours, cycling the workbook's own theme accents.
///
/// The workbook's, not a palette invented here — a chart should match the file
/// it came from. [`Workbook::theme_slot`] already falls back to the stock
/// Office accent for a slot this file does not define, which is what the canvas
/// does by filtering the slice and substituting its own list.
#[must_use]
pub fn series_colors(workbook: &Workbook, n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            workbook
                .theme_slot(ACCENT_SLOTS[i % ACCENT_SLOTS.len()])
                .to_owned()
        })
        .collect()
}

/// One series, resolved to the numbers it plots.
#[derive(Debug, Clone)]
pub struct ResolvedSeries {
    /// The series name as displayed, empty when it has none.
    pub name: String,
    /// Its points, in order; `None` is a gap rather than a zero.
    pub values: Vec<Option<f64>>,
}

/// A chart's series and category labels, resolved against the cached values.
///
/// A series with no value that resolved is dropped rather than plotted as
/// zeroes — a chart of flat zeroes looks like data, which is worse than a chart
/// with one series missing. That rule is the canvas's, kept.
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
    let series = chart
        .series
        .iter()
        .map(|se| ResolvedSeries {
            name: se.name.clone(),
            values: ref_numbers(workbook, sheet_index, &se.values),
        })
        .filter(|s| s.values.iter().any(Option::is_some))
        .collect();
    (cats, series)
}

/// The value range an axis has to cover, always including zero so a bar's
/// length is proportional to its value.
#[must_use]
pub fn value_extent(series: &[ResolvedSeries]) -> (f64, f64) {
    let mut lo = 0.0f64;
    let mut hi = 0.0f64;
    for s in series {
        for v in s.values.iter().flatten() {
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
    }
    // A flat series would otherwise divide by zero when it is scaled.
    if lo == hi {
        hi = lo + 1.0;
    }
    (lo, hi)
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

/// A series' displayed name, which is its position when it has none.
///
/// The canvas's `s.name || \`Series ${i + 1}\``, kept: a blank name must
/// measure as the text that will be drawn, or the box is sized for nothing and
/// the label runs out of it.
fn legend_label(series: &ResolvedSeries, index: usize) -> String {
    if series.name.is_empty() {
        format!("Series {}", index + 1)
    } else {
        series.name.clone()
    }
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
    if series.is_empty() {
        // Honest rather than blank: the chart exists, its data did not resolve.
        list.items.push(text_at(
            line(x + 8.0 * PX, top, w - 8.0 * PX, 11.0 * PX),
            "no data".to_owned(),
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
        ChartKind::Line | ChartKind::Area | ChartKind::Scatter => {
            push_line(list, workbook, chart.kind, &cats, &series, plot);
        }
        ChartKind::Bar | ChartKind::Column => {
            push_bars(list, workbook, chart.kind, &cats, &series, plot);
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

fn push_bars(
    list: &mut DisplayList,
    workbook: &Workbook,
    kind: ChartKind,
    cats: &[String],
    series: &[ResolvedSeries],
    plot: Plot,
) {
    let (lo, hi) = value_extent(series);
    let zero_y = push_axes(list, plot, lo, hi);
    let cols = series_colors(workbook, series.len());
    let points = series.iter().map(|s| s.values.len()).max().unwrap_or(0);
    if points == 0 {
        return;
    }
    let group_w = plot.w / points as f64;
    // Every "1" in the canvas's arithmetic is one *pixel*; in twips it is
    // fifteen. Getting that wrong is invisible at a glance and a whole pixel
    // wide in the picture, which is how a bar came out a pixel too wide here.
    let bar_w = (group_w * 0.7 / series.len() as f64).max(PX);
    for i in 0..points {
        for (si, s) in series.iter().enumerate() {
            let Some(Some(v)) = s.values.get(i) else {
                continue;
            };
            let bx = plot.x + i as f64 * group_w + group_w * 0.15 + si as f64 * bar_w;
            let bar_top = plot.y + plot.h * ((hi - v) / (hi - lo));
            // A negative value draws downward from the zero line, which is why
            // the rectangle is measured from it rather than from the axis.
            let y0 = bar_top.min(zero_y);
            let bh = {
                let d = (zero_y - bar_top).abs();
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
                fill: cols[si].clone(),
            });
        }
    }
    push_category_labels(list, kind, cats, plot, points);
}

fn push_line(
    list: &mut DisplayList,
    workbook: &Workbook,
    kind: ChartKind,
    cats: &[String],
    series: &[ResolvedSeries],
    plot: Plot,
) {
    let (lo, hi) = value_extent(series);
    push_axes(list, plot, lo, hi);
    let cols = series_colors(workbook, series.len());
    let points = series.iter().map(|s| s.values.len()).max().unwrap_or(0);
    let step = if points > 1 {
        plot.w / (points - 1) as f64
    } else {
        0.0
    };
    for (si, s) in series.iter().enumerate() {
        // A gap breaks the line rather than being bridged across, which is the
        // canvas's `started = false`; here that means starting a new polyline.
        let mut run: Vec<Point> = Vec::new();
        for (i, v) in s.values.iter().enumerate() {
            let Some(v) = v else {
                flush_run(list, &mut run, &cols[si]);
                continue;
            };
            let px = plot.x + i as f64 * step;
            let py = plot.y + plot.h * ((hi - v) / (hi - lo));
            if kind == ChartKind::Scatter {
                // A point marker: four pixels square, centred on the value.
                list.items.push(PaintItem::Polygon {
                    points: vec![
                        point(px - 2.0 * PX, py - 2.0 * PX),
                        point(px + 2.0 * PX, py - 2.0 * PX),
                        point(px + 2.0 * PX, py + 2.0 * PX),
                        point(px - 2.0 * PX, py + 2.0 * PX),
                    ],
                    fill: cols[si].clone(),
                });
                continue;
            }
            run.push(point(px, py));
        }
        flush_run(list, &mut run, &cols[si]);
    }
    push_category_labels(list, kind, cats, plot, points);
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
    let cols = series_colors(workbook, values.len());
    let cx = plot.x + plot.w / 2.0;
    let cy = plot.y + plot.h / 2.0;
    let r = (plot.w.min(plot.h) / 2.0 - 4.0 * PX).max(6.0 * PX);
    let inner = if chart.kind == ChartKind::Doughnut {
        r * 0.55
    } else {
        0.0
    };
    // Twelve o'clock, clockwise, as Excel starts.
    let mut from = 0.0f64;
    for (i, v) in values.iter().enumerate() {
        let sweep = v / total * 360.0;
        list.items.push(PaintItem::Wedge {
            center: point(cx, cy),
            radius: round(r),
            inner_radius: round(inner),
            from,
            sweep,
            fill: cols[i].clone(),
        });
        from += sweep;
    }
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
