//! Grid geometry: the column and row axes and their default sizes.

use casual_calc_model::Sheet;

use crate::axis::Axis;

/// Default column width in twips (~64 px at 96 dpi).
pub const DEFAULT_COL_WIDTH: i64 = 960;
/// Default row height in twips (~20 px at 96 dpi).
pub const DEFAULT_ROW_HEIGHT: i64 = 300;

/// The column and row geometry of a sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridGeometry {
    /// Column widths.
    pub columns: Axis,
    /// Row heights.
    pub rows: Axis,
}

impl Default for GridGeometry {
    fn default() -> Self {
        Self {
            columns: Axis::uniform(DEFAULT_COL_WIDTH),
            rows: Axis::uniform(DEFAULT_ROW_HEIGHT),
        }
    }
}

impl GridGeometry {
    /// The geometry implied by a sheet's column widths and row heights, falling
    /// back to the engine defaults where the sheet sets none.
    pub fn for_sheet(sheet: &Sheet) -> Self {
        let col_default = sheet.columns.default.unwrap_or(DEFAULT_COL_WIDTH);
        let row_default = sheet.rows.default.unwrap_or(DEFAULT_ROW_HEIGHT);
        // Hidden lines collapse to zero size (listed after explicit sizes so a
        // hidden line always wins over any width/height it may also carry).
        let cols = sheet
            .columns
            .sizes
            .iter()
            .map(|(&k, &v)| (k, v))
            .chain(sheet.hidden_cols.iter().map(|&c| (c, 0)));
        let rows = sheet
            .rows
            .sizes
            .iter()
            .map(|(&k, &v)| (k, v))
            .chain(sheet.hidden_rows.iter().map(|&r| (r, 0)))
            .chain(sheet.filter_hidden.iter().map(|&r| (r, 0)));
        Self {
            columns: Axis::with_sizes(col_default, cols),
            rows: Axis::with_sizes(row_default, rows),
        }
    }
}
