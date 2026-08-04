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
mod geometry;
mod numfmt;

pub use axis::Axis;
pub use display::{Align, DisplayList, PaintItem, Rect};
pub use geometry::{DEFAULT_COL_WIDTH, DEFAULT_ROW_HEIGHT, GridGeometry};
pub use numfmt::{format_general, format_number};

use casual_calc_model::{Cell, CellValue, Workbook};

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
        let content = display_text(workbook, cell);
        if content.is_empty() {
            continue;
        }
        list.items.push(PaintItem::Text {
            rect,
            content,
            align: align_for(&cell.value),
        });
    }
    list
}

fn align_for(value: &CellValue) -> Align {
    match value {
        CellValue::Number(_) | CellValue::Bool(_) | CellValue::Error(_) => Align::Right,
        _ => Align::Left,
    }
}

/// The display string for a cell's cached value, applying the cell's
/// number-format code (if any) to numeric values.
pub fn display_text(workbook: &Workbook, cell: &Cell) -> String {
    match &cell.value {
        CellValue::Empty => String::new(),
        CellValue::Number(n) => match cell_number_format(workbook, cell) {
            Some(code) => format_number(*n, code),
            None => format_general(*n),
        },
        CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        CellValue::Error(e) => e.to_string(),
        CellValue::SharedString(id) | CellValue::InlineString(id) => {
            workbook.strings.get(*id).unwrap_or_default().to_owned()
        }
    }
}

fn cell_number_format<'a>(workbook: &'a Workbook, cell: &Cell) -> Option<&'a str> {
    let style = workbook.styles.get(cell.style?)?;
    style.number_format.as_deref()
}

#[cfg(test)]
mod tests;
