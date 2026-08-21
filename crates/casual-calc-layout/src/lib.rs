//! `casual-calc-layout` — grid geometry, viewport virtualization, and the
//! backend-neutral display list.
//!
//! Phase 1C, increment 1: the **cumulative offset index** ([`Axis`]) maps
//! between line indices and twip positions; [`layout_viewport`] lays out only
//! the visible window of a sheet — the basis for 60 fps on a million-cell grid
//! (the `docs/30` performance targets). The invariant is that the viewport
//! output equals the full-layout output restricted to that window (gated by
//! tests).
//!
//! Layout reads the model's **cached cell values** only — it never invokes the
//! calc engine. Glyph shaping is deferred to the render backend (Phase 1D); this
//! layer emits text as a string plus its cell rectangle. Number-format-aware
//! display text is a later increment (currently the raw value is shown).
//!
//! See `docs/42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md`.

mod axis;
pub mod chart;
pub mod chart_data;
pub mod conditional;
mod display;
mod geometry;
mod numfmt;
pub mod table_style;

pub use axis::Axis;
pub use display::{Align, BorderLine, DisplayList, PaintItem, Point, Rect};
// Moved to `casual-calc-text` (`RND-11`) and re-exported unchanged, so it is
// still reached as `casual_calc_layout::substitute` by everything that already
// did — and is still the one substitution table, now somewhere the faces
// themselves can see it.
pub use casual_calc_text::substitution::{
    BundledFamily, PICKER_FAMILIES, Substitute, SubstituteKind, css_stack, substitute,
};
pub use geometry::{DEFAULT_COL_WIDTH, DEFAULT_ROW_HEIGHT, GridGeometry};
pub use numfmt::{
    adjust_format_decimals, format_general, format_number, format_number_1904,
    format_number_colored, format_text,
};

use casual_calc_model::{
    BorderEdge, Borders, Cell, CellRange, CellRef, CellValue, Emu, Sheet, Style, Workbook,
};

/// A scrolled viewport rectangle, in twips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    /// Left scroll position.
    pub x: i64,
    /// Top scroll position.
    pub y: i64,
    /// Visible width.
    pub width: i64,
    /// Visible height.
    pub height: i64,
}

/// The visible line ranges (inclusive) for a viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleRange {
    /// First/last visible row (inclusive).
    pub rows: (u32, u32),
    /// First/last visible column (inclusive).
    pub cols: (u32, u32),
}

/// How many leading rows and columns stay pinned while the rest scrolls.
///
/// Mirrors [`SheetView::frozen_rows`](casual_calc_model::SheetView::frozen_rows)
/// and its column counterpart. The default is no freeze, which is why every
/// existing caller keeps the behaviour it had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Freeze {
    /// Rows pinned at the top.
    pub rows: u32,
    /// Columns pinned at the left.
    pub cols: u32,
}

impl Freeze {
    /// Whether anything is pinned at all.
    #[must_use]
    pub fn is_none(&self) -> bool {
        self.rows == 0 && self.cols == 0
    }
}

/// One region of a split viewport: what it looks at, and where it sits.
///
/// A frozen sheet is not one window onto the grid but up to four, each
/// scrolling on its own axes. The pane owns a plain [`Viewport`], so
/// everything downstream — [`layout_viewport`], the display list, the render
/// backend — works on it unchanged; only the composition is new.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pane {
    /// The window into the sheet this pane shows.
    pub viewport: Viewport,
    /// Where the pane's top-left sits within the whole image, in twips.
    pub origin: (i64, i64),
}

/// Split a viewport into the panes a freeze divides it into.
///
/// Returns the panes in painter's order — corner, top band, left band, body —
/// omitting any the freeze leaves no room for. With no freeze the result is a
/// single pane equal to `viewport`, so a frozen-pane-aware renderer is exactly
/// the old renderer on an unfrozen sheet.
///
/// `viewport.x`/`.y` keep meaning an absolute content offset. The scrolling
/// panes clamp it to the first unfrozen line: scrolling back past the freeze
/// would show the pinned lines a second time, next to themselves.
#[must_use]
pub fn panes(geometry: &GridGeometry, viewport: &Viewport, freeze: Freeze) -> Vec<Pane> {
    let width = viewport.width.max(0);
    let height = viewport.height.max(0);

    // A freeze wider than the image leaves the scrolling region no room. Clamp
    // rather than hand a pane a negative size: the frozen band is the part the
    // author asked to always see, so it is the part that survives.
    let frozen_w = geometry.columns.offset(freeze.cols).clamp(0, width);
    let frozen_h = geometry.rows.offset(freeze.rows).clamp(0, height);

    let scroll_x = if freeze.cols > 0 {
        viewport.x.max(geometry.columns.offset(freeze.cols))
    } else {
        viewport.x
    };
    let scroll_y = if freeze.rows > 0 {
        viewport.y.max(geometry.rows.offset(freeze.rows))
    } else {
        viewport.y
    };

    let mut out = Vec::with_capacity(4);
    let mut push = |x: i64, y: i64, origin: (i64, i64), w: i64, h: i64| {
        if w > 0 && h > 0 {
            out.push(Pane {
                viewport: Viewport {
                    x,
                    y,
                    width: w,
                    height: h,
                },
                origin,
            });
        }
    };

    let body_w = width - frozen_w;
    let body_h = height - frozen_h;
    push(0, 0, (0, 0), frozen_w, frozen_h);
    push(scroll_x, 0, (frozen_w, 0), body_w, frozen_h);
    push(0, scroll_y, (0, frozen_h), frozen_w, body_h);
    push(scroll_x, scroll_y, (frozen_w, frozen_h), body_w, body_h);
    out
}

/// Compute the inclusive row/column ranges intersecting `viewport`, in
/// O(overrides) — independent of how many cells are populated.
pub fn visible_range(geometry: &GridGeometry, viewport: &Viewport) -> VisibleRange {
    let first_col = geometry.columns.line_at(viewport.x);
    let last_col = geometry.columns.line_at(viewport.x + viewport.width.max(0));
    let first_row = geometry.rows.line_at(viewport.y);
    let last_row = geometry.rows.line_at(viewport.y + viewport.height.max(0));
    VisibleRange {
        rows: (first_row, last_row),
        cols: (first_col, last_col),
    }
}

/// Lay out only the cells intersecting `viewport` into a display list.
pub fn layout_viewport(
    workbook: &Workbook,
    sheet_index: usize,
    geometry: &GridGeometry,
    viewport: &Viewport,
) -> DisplayList {
    let range = visible_range(geometry, viewport);
    layout_range(workbook, sheet_index, geometry, range)
}

/// Lay out every populated cell of the sheet (the reference full layout).
pub fn layout_full(
    workbook: &Workbook,
    sheet_index: usize,
    geometry: &GridGeometry,
) -> DisplayList {
    let range = VisibleRange {
        rows: (0, u32::MAX),
        cols: (0, u32::MAX),
    };
    layout_range(workbook, sheet_index, geometry, range)
}

fn layout_range(
    workbook: &Workbook,
    sheet_index: usize,
    geometry: &GridGeometry,
    range: VisibleRange,
) -> DisplayList {
    let mut list = DisplayList::new();
    let Some(sheet) = workbook.sheets.get(sheet_index) else {
        return list;
    };

    // A merged range is one cell that happens to be large. Resolved before the
    // cells are walked, because it changes both what is drawn (one rectangle,
    // not N) and what is skipped (everything the range covers).
    let merged = visible_merges(sheet, range);

    // Conditional formatting, resolved here so the headless renderer sees what
    // the canvas does (`RND-05`). Skipped entirely when the sheet has no rules,
    // which is almost every sheet: `range_stats` scans a rule's whole range,
    // and paying for that on a sheet with nothing to show would be a cost every
    // frame for nothing.
    let (cf_stats, cf_order) = if sheet.conditional_formats.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        (
            sheet
                .conditional_formats
                .iter()
                .map(|cf| conditional::range_stats(workbook, sheet, cf))
                .collect(),
            conditional::priority_order(sheet),
        )
    };
    let effect_at = |at: casual_calc_model::CellRef, cell: &Cell| {
        if cf_order.is_empty() {
            return conditional::CellEffect::default();
        }
        conditional::effect_for(
            sheet,
            &cf_stats,
            &cf_order,
            at.row,
            at.col,
            &cell.value,
            &display_text(workbook, cell),
        )
    };

    // The anchors first, in the order `visible_merges` fixed, so the display
    // list stays deterministic. They cannot overlap each other or an unmerged
    // cell, so painting them before the rest costs nothing.
    for merge in &merged {
        let rect = merge_rect(geometry, merge);
        let cell = sheet.cells.get(merge.start);
        // The region carries the anchor's fill, so the backend paints ground
        // and outline in one place and cannot order them wrongly. The anchor's
        // text and border follow as their own items, on top.
        let merged_fill = cell
            .map(|c| effect_at(merge.start, c))
            .and_then(|e| e.fill)
            .or_else(|| {
                cell.and_then(|c| c.style)
                    .and_then(|id| workbook.styles.get(id))
                    .and_then(|s| s.fill_color.clone())
            });
        list.items.push(PaintItem::MergedRegion {
            rect,
            fill: merged_fill,
        });
        if let Some(cell) = cell {
            let effect = effect_at(merge.start, cell);
            push_cell(
                &mut list,
                workbook,
                cell,
                rect,
                Background::AlreadyPainted,
                &effect,
            );
        }
    }

    for (at, cell) in sheet.cells.row_band(range.rows.0, range.rows.1) {
        if at.col < range.cols.0 || at.col > range.cols.1 {
            continue;
        }
        // Covered by a merge — including its own anchor, which the pass above
        // has already drawn at the full size. A covered cell may still hold a
        // value (Excel keeps them, and so does the writer); a merge means it is
        // not shown, not that it is gone.
        if merged.iter().any(|m| covers(m, at)) {
            continue;
        }
        let rect = Rect {
            x: geometry.columns.offset(at.col),
            y: geometry.rows.offset(at.row),
            w: geometry.columns.size(at.col),
            h: geometry.rows.size(at.row),
        };
        let effect = effect_at(at, cell);
        push_cell(&mut list, workbook, cell, rect, Background::Paint, &effect);
    }

    // Pictures last, because they float over the grid rather than sitting in
    // it: a drawing anchored across four cells covers whatever those cells
    // hold. Emitted with the cells they overlap, they would be painted away by
    // the next cell's background.
    push_images(&mut list, sheet, geometry, range);
    // Charts after pictures, matching the canvas, which draws `drawImages`
    // before `drawCharts`. Two drawings overlapping is rare and their order is
    // not recorded anywhere in the file, so the one thing that matters is that
    // both renderers pick the same one.
    push_charts(&mut list, workbook, sheet_index, sheet, geometry, range);
    list
}

/// EMUs in one twip: 914,400 to the inch against 1,440, which divides exactly.
const EMU_PER_TWIP: i64 = 635;

/// A drawing offset in twips. Truncating toward zero, deterministically — the
/// remainder is at most a twip, which is 1/20 of a point and below any device
/// this renders to.
fn emu_to_twips(emu: i64) -> i64 {
    emu / EMU_PER_TWIP
}

/// The twip rectangle a drawing frame occupies, from its anchor cells **and**
/// its EMU offsets.
///
/// The offsets are not decoration. A picture's edges land wherever they were
/// dragged, which is almost never on a gridline; a rectangle taken from the
/// anchor cells alone snaps every drawing to whole cells, which moves it and
/// resizes it at once.
///
/// [`ImageView::anchor`](casual_calc_model::ImageView::anchor) is inclusive
/// while OOXML's `<xdr:to>` is exclusive, so the far edge is the leading edge of
/// the line *after* the last covered one — and
/// [`to_offset`](casual_calc_model::ImageView::to_offset) is measured from
/// exactly there, which is why it is added to `offset(end + 1)` rather than to
/// `offset(end)`.
fn frame_rect(geometry: &GridGeometry, anchor: &CellRange, from: Emu, to: Emu) -> Rect {
    let x = geometry
        .columns
        .offset(anchor.start.col)
        .saturating_add(emu_to_twips(from.x));
    let y = geometry
        .rows
        .offset(anchor.start.row)
        .saturating_add(emu_to_twips(from.y));
    let right = geometry
        .columns
        .offset(anchor.end.col.saturating_add(1))
        .saturating_add(emu_to_twips(to.x));
    let bottom = geometry
        .rows
        .offset(anchor.end.row.saturating_add(1))
        .saturating_add(emu_to_twips(to.y));
    Rect {
        x,
        y,
        w: right.saturating_sub(x).max(0),
        h: bottom.saturating_sub(y).max(0),
    }
}

/// Emit the sheet's pictures whose frames reach into `range`.
///
/// Filtered by the **frame**, not by the anchor cell: a picture is routinely
/// larger than the window, so its anchor is off screen precisely when it is
/// most visible. Testing containment instead would make a large drawing
/// disappear as it was scrolled into.
///
/// A frame with no area is skipped rather than emitted and reported as undrawn.
/// That is not loss — it is a picture on rows or columns the author hid, and
/// Excel does not draw it either; naming it would fill a compatibility report
/// with things nobody asked to see.
fn push_images(
    list: &mut DisplayList,
    sheet: &Sheet,
    geometry: &GridGeometry,
    range: VisibleRange,
) {
    if sheet.images.is_empty() {
        return;
    }
    let win_x0 = geometry.columns.offset(range.cols.0);
    let win_x1 = geometry.columns.offset(range.cols.1.saturating_add(1));
    let win_y0 = geometry.rows.offset(range.rows.0);
    let win_y1 = geometry.rows.offset(range.rows.1.saturating_add(1));

    for image in &sheet.images {
        // **An authored size beats a measured one, and only one anchor kind
        // measures.** A `twoCellAnchor` says where the far corner is, and that
        // rectangle *is* the picture — including a distortion the author made
        // by dragging a handle, which must be reproduced. `oneCellAnchor` and
        // `absoluteAnchor` carry `<xdr:ext>` instead, and the importer used to
        // discard it and substitute a nominal eight columns by fifteen rows.
        //
        // Scaling a picture to fill a *guessed* rectangle applies a fabricated
        // aspect ratio: tolerable for a chart, which redraws itself into
        // whatever box it lands in, and visibly wrong for a photograph
        // (`RND-13`). So where the file stated a size, that is the size.
        let rect = match image.extent {
            Some(ext) => Rect {
                x: geometry
                    .columns
                    .offset(image.anchor.start.col)
                    .saturating_add(emu_to_twips(image.from_offset.x)),
                y: geometry
                    .rows
                    .offset(image.anchor.start.row)
                    .saturating_add(emu_to_twips(image.from_offset.y)),
                w: emu_to_twips(ext.x).max(0),
                h: emu_to_twips(ext.y).max(0),
            },
            None => frame_rect(geometry, &image.anchor, image.from_offset, image.to_offset),
        };
        if rect.w == 0 || rect.h == 0 {
            continue;
        }
        let intersects = rect.x < win_x1
            && rect.x.saturating_add(rect.w) > win_x0
            && rect.y < win_y1
            && rect.y.saturating_add(rect.h) > win_y0;
        if !intersects {
            continue;
        }
        list.items.push(PaintItem::Image {
            rect,
            part: image.part.clone(),
        });
    }
}

/// Emit the sheet's charts whose frames reach into `range`.
///
/// The window test is the frame's, for the reason [`push_images`] gives: a
/// chart is routinely larger than the viewport, so its anchor cell is off
/// screen exactly when the chart is most visible.
///
/// A chart's frame comes from the anchor **and** its EMU offsets, like a
/// picture's — and unlike a picture's, it is never taken from an authored
/// extent, because a chart has none: it redraws itself into whatever box it
/// lands in, which is the difference `RND-13` turned on.
fn push_charts(
    list: &mut DisplayList,
    workbook: &Workbook,
    sheet_index: usize,
    sheet: &Sheet,
    geometry: &GridGeometry,
    range: VisibleRange,
) {
    if sheet.charts.is_empty() {
        return;
    }
    let win_x0 = geometry.columns.offset(range.cols.0);
    let win_x1 = geometry.columns.offset(range.cols.1.saturating_add(1));
    let win_y0 = geometry.rows.offset(range.rows.0);
    let win_y1 = geometry.rows.offset(range.rows.1.saturating_add(1));

    for chart in &sheet.charts {
        let rect = frame_rect(geometry, &chart.anchor, chart.from_offset, chart.to_offset);
        if rect.w == 0 || rect.h == 0 {
            continue;
        }
        let intersects = rect.x < win_x1
            && rect.x.saturating_add(rect.w) > win_x0
            && rect.y < win_y1
            && rect.y.saturating_add(rect.h) > win_y0;
        if !intersects {
            continue;
        }
        chart::push_chart(list, workbook, sheet_index, chart, rect);
    }
}

/// Whether `at` falls inside `merge`.
fn covers(merge: &CellRange, at: CellRef) -> bool {
    at.row >= merge.start.row
        && at.row <= merge.end.row
        && at.col >= merge.start.col
        && at.col <= merge.end.col
}

/// The union rectangle of a merged range, in twips.
fn merge_rect(geometry: &GridGeometry, merge: &CellRange) -> Rect {
    let x = geometry.columns.offset(merge.start.col);
    let y = geometry.rows.offset(merge.start.row);
    Rect {
        x,
        y,
        // From the left edge of the first column to the left edge of the one
        // past the last, which is the width however the columns in between are
        // sized — and correct for a hidden column, which is simply zero wide.
        w: geometry.columns.offset(merge.end.col.saturating_add(1)) - x,
        h: geometry.rows.offset(merge.end.row.saturating_add(1)) - y,
    }
}

/// The sheet's merged ranges that intersect `range`, in a deterministic order.
///
/// Intersecting, **not** contained: a merge anchored above or to the left of the
/// window still shows inside it, and virtualization that dropped it would make a
/// merged block appear and disappear as it was scrolled past — the anchor is
/// often off screen precisely because the block is wide.
///
/// Linear in the sheet's merge count, and the result is linear in the merges
/// *in view*, which is bounded by the cells in view. That is what makes the
/// containment scan in `layout_range` affordable; if a sheet ever carries
/// enough merges for it to matter, an interval index goes here rather than
/// there.
fn visible_merges(sheet: &Sheet, range: VisibleRange) -> Vec<CellRange> {
    let mut out: Vec<CellRange> = sheet
        .merges
        .iter()
        .filter(|m| {
            m.start.row <= range.rows.1
                && m.end.row >= range.rows.0
                && m.start.col <= range.cols.1
                && m.end.col >= range.cols.0
        })
        .copied()
        .collect();
    // The model does not promise an order for `merges`, and a display list that
    // depends on one is a golden test that fails when a file is re-saved.
    out.sort_by_key(|m| (m.start.row, m.start.col, m.end.row, m.end.col));
    out.dedup();
    out
}

/// Whether [`push_cell`] should emit the cell's background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Background {
    /// An ordinary cell: emit it.
    Paint,
    /// A merged anchor, whose fill the [`PaintItem::MergedRegion`] already
    /// carries. Emitting it again would paint over the region's outline.
    AlreadyPainted,
}

/// Emit one cell's paint items into `list` at `rect`.
///
/// Shared by the merged and unmerged paths so a merged cell is styled, aligned
/// and bordered exactly as it would be unmerged — only its rectangle differs.
fn push_cell(
    list: &mut DisplayList,
    workbook: &Workbook,
    cell: &Cell,
    rect: Rect,
    background: Background,
    effect: &conditional::CellEffect,
) {
    let style = cell.style.and_then(|id| workbook.styles.get(id));

    // Painter's order per cell: fill behind, then text, then border on top.
    //
    // A matching rule's fill wins over the cell's own — that is what
    // conditional formatting *is*, and the order matters: taking the style's
    // first would paint the rule away.
    if background == Background::Paint
        && let Some(fill) = effect
            .fill
            .clone()
            .or_else(|| style.and_then(|s| s.fill_color.clone()))
    {
        list.items.push(PaintItem::CellBackground {
            rect,
            fill: Some(fill),
        });
    }

    // A data bar goes **in front of the cell background and behind the text**,
    // and both halves of that are load-bearing. Emitted before the background
    // it would be painted away by an opaque fill; emitted after the text it
    // would cover the number it exists to annotate, and the cell would read as
    // a coloured smear. It is pushed for a merged anchor too — there the
    // `MergedRegion` above is the background, and it has already been laid
    // down, so the bar still lands on top of the fill and under the text.
    if let Some((fraction, color)) = &effect.data_bar {
        list.items.push(PaintItem::DataBar {
            rect,
            fraction: *fraction,
            color: color.clone(),
        });
    }

    let content = display_text(workbook, cell);
    if !content.is_empty() {
        // Effective font: the cell's own, else the workbook default.
        let font_name = style
            .and_then(|s| s.font_name.clone())
            .or_else(|| workbook.default_font_name.clone());
        let font_pt = style
            .and_then(|s| s.font_size_hp)
            .or(workbook.default_font_size_hp)
            .map(|hp| hp as f32 / 2.0);
        list.items.push(PaintItem::Text {
            rect,
            content,
            align: align_for(&cell.value),
            color: effect
                .font_color
                .clone()
                .or_else(|| style.and_then(|s| s.font_color.clone())),
            bold: effect.bold || style.is_some_and(|s| s.bold),
            italic: style.is_some_and(|s| s.italic),
            font_name,
            font_pt,
        });
    }

    if let Some(border) = style.and_then(|s| border_item(rect, s)) {
        list.items.push(border);
    }
}

/// Build a [`PaintItem::CellBorder`] from a style's borders, or `None` if the
/// style carries no border edges.
fn border_item(rect: Rect, style: &Style) -> Option<PaintItem> {
    let borders: &Borders = style.border.as_ref()?;
    if borders.is_empty() {
        return None;
    }
    Some(PaintItem::CellBorder {
        rect,
        left: borders.left.as_ref().map(border_line),
        right: borders.right.as_ref().map(border_line),
        top: borders.top.as_ref().map(border_line),
        bottom: borders.bottom.as_ref().map(border_line),
    })
}

/// Resolve a model [`BorderEdge`] to a paint-ready [`BorderLine`], mapping the
/// raw OOXML line-style token to a deterministic pixel width.
fn border_line(edge: &BorderEdge) -> BorderLine {
    BorderLine {
        width: border_width(&edge.style),
        color: edge.color.clone(),
    }
}

/// The pixel width for an OOXML border line-style token. Unknown tokens fall
/// back to a thin (1px) line so any style still paints.
fn border_width(token: &str) -> u32 {
    match token {
        "hair" | "thin" | "dashed" | "dotted" | "dashDot" | "dashDotDot" => 1,
        "medium" | "mediumDashed" | "mediumDashDot" | "mediumDashDotDot" | "slantDashDot" => 2,
        "thick" | "double" => 3,
        _ => 1,
    }
}

fn align_for(value: &CellValue) -> Align {
    match value {
        CellValue::Number(_) | CellValue::Bool(_) | CellValue::Error(_) => Align::Right,
        _ => Align::Left,
    }
}

/// The colour a cell's number format asks its own output to be drawn in
/// (`#,##0;[Red]-#,##0`), as `RRGGBB`. `None` when the format names no colour,
/// or the value is not numeric — a format colour applies to the number it
/// formats. Overrides the style's font colour, as in Excel.
#[must_use]
pub fn display_color(workbook: &Workbook, cell: &Cell) -> Option<&'static str> {
    let CellValue::Number(n) = &cell.value else {
        return None;
    };
    numfmt::format_number_colored(*n, cell_number_format(workbook, cell)?).1
}

/// The display string for a cell's cached value, applying the cell's
/// number-format code (if any) to numeric values.
pub fn display_text(workbook: &Workbook, cell: &Cell) -> String {
    match &cell.value {
        CellValue::Empty => String::new(),
        CellValue::Number(n) => match cell_number_format(workbook, cell) {
            // The epoch is a property of the workbook, so it has to come from
            // here — a date rendered under the wrong one is out by 1462 days.
            Some(code) if workbook.date1904 => numfmt::format_number_1904(*n, code),
            Some(code) => format_number(*n, code),
            None => format_general(*n),
        },
        CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        CellValue::Error(e) => e.to_string(),
        CellValue::SharedString(id) | CellValue::InlineString(id) => {
            let text = workbook.strings.get(*id).unwrap_or_default().to_owned();
            // A format's text section applies to text values (`@" kg"`), and
            // the stock Text format is exactly this case.
            match cell_number_format(workbook, cell) {
                Some(code) => numfmt::format_text(&text, code).unwrap_or(text),
                None => text,
            }
        }
    }
}

/// The number-format code in force on a cell, or `None` for General.
///
/// Public because `CELL("format")` has to report the same code the renderer
/// draws with; resolving it twice is how the two come to disagree.
pub fn cell_number_format<'a>(workbook: &'a Workbook, cell: &Cell) -> Option<&'a str> {
    let style = workbook.styles.get(cell.style?)?;
    style.number_format.as_deref()
}

#[cfg(test)]
mod tests;
