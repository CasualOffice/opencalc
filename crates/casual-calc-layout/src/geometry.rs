//! Grid geometry: the column and row axes and their default sizes.

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
