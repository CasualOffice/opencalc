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
mod display;
pub mod font_substitution;
mod geometry;
mod numfmt;
pub mod table_style;

pub use axis::Axis;
pub use display::{Align, BorderLine, DisplayList, PaintItem, Rect};
pub use font_substitution::{
    BundledFamily, PICKER_FAMILIES, Substitute, SubstituteKind, css_stack, substitute,
};
pub use geometry::{DEFAULT_COL_WIDTH, DEFAULT_ROW_HEIGHT, GridGeometry};
pub use numfmt::{
    adjust_format_decimals, format_general, format_number, format_number_1904,
    format_number_colored, format_text,
};

use casual_calc_model::{BorderEdge, Borders, Cell, CellValue, Style, Workbook};

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

    for (at, cell) in sheet.cells.row_band(range.rows.0, range.rows.1) {
        if at.col < range.cols.0 || at.col > range.cols.1 {
            continue;
        }
        let rect = Rect {
            x: geometry.columns.offset(at.col),
            y: geometry.rows.offset(at.row),
            w: geometry.columns.size(at.col),
            h: geometry.rows.size(at.row),
        };
        let style = cell.style.and_then(|id| workbook.styles.get(id));

        // Painter's order per cell: fill behind, then text, then border on top.
        if let Some(fill) = style.and_then(|s| s.fill_color.clone()) {
            list.items.push(PaintItem::CellBackground {
                rect,
                fill: Some(fill),
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
                color: style.and_then(|s| s.font_color.clone()),
                bold: style.is_some_and(|s| s.bold),
                italic: style.is_some_and(|s| s.italic),
                font_name,
                font_pt,
            });
        }

        if let Some(border) = style.and_then(|s| border_item(rect, s)) {
            list.items.push(border);
        }
    }
    list
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
