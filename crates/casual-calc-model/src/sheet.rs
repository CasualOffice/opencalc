//! Worksheets. Definition tables (styles, numbering, defined names, notes,
//! theme) live on the workbook; the sheet holds its grid, merges, and view.
//! See `docs/22-NORMALIZED-SCHEMA.md`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::ids::SheetId;
use crate::store::{CellRange, CellStore};

/// Per-axis sizing (column widths or row heights), in twips: an optional default
/// plus per-line overrides. Empty means "use the engine default".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AxisSizing {
    /// Default line size (twips) for this axis, if the sheet sets one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<i64>,
    /// Explicit per-line sizes (twips), keyed by zero-based index.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sizes: BTreeMap<u32, i64>,
}

impl AxisSizing {
    /// Whether nothing is set (no default, no overrides).
    pub fn is_empty(&self) -> bool {
        self.default.is_none() && self.sizes.is_empty()
    }

    /// The size (twips) of `index`, falling back to `default` then `fallback`.
    pub fn size(&self, index: u32, fallback: i64) -> i64 {
        self.sizes
            .get(&index)
            .copied()
            .or(self.default)
            .unwrap_or(fallback)
    }
}

/// A sheet's view state: the frozen (pinned) row/column bands and zoom level.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SheetView {
    /// Number of rows frozen at the top.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub frozen_rows: u32,
    /// Number of columns frozen at the left.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub frozen_cols: u32,
    /// Zoom magnification as a whole percentage (`100` = normal). `0` means the
    /// view uses the application default, so no explicit `zoomScale` is written.
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub zoom: u16,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

fn is_zero_u16(value: &u16) -> bool {
    *value == 0
}

impl SheetView {
    /// Whether the view is at its default (nothing frozen, default zoom).
    pub fn is_default(&self) -> bool {
        self.frozen_rows == 0 && self.frozen_cols == 0 && self.zoom == 0
    }
}

/// Outline (row/column grouping) properties from `<sheetPr><outlinePr/>`: where
/// group summary rows/columns sit relative to their detail. Both flags default
/// to `true` (summary below a row group, right of a column group), matching the
/// OOXML defaults, so an untouched sheet writes no `<outlinePr>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutlinePr {
    /// Whether a group's summary row sits below its detail rows.
    pub summary_below: bool,
    /// Whether a group's summary column sits to the right of its detail columns.
    pub summary_right: bool,
}

impl Default for OutlinePr {
    fn default() -> Self {
        Self {
            summary_below: true,
            summary_right: true,
        }
    }
}

impl OutlinePr {
    /// Whether both flags are at their OOXML defaults (summary below/right).
    pub fn is_default(&self) -> bool {
        self.summary_below && self.summary_right
    }
}

/// One worksheet: an identity, a name, its sparse cell grid, merged ranges, and
/// view state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Sheet {
    /// Stable sheet identity.
    pub id: SheetId,
    /// Display name (tab label).
    pub name: String,
    /// The populated cells.
    #[serde(default, skip_serializing_if = "CellStore::is_empty")]
    pub cells: CellStore,
    /// Merged cell ranges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merges: Vec<CellRange>,
    /// View state (frozen panes).
    #[serde(default, skip_serializing_if = "SheetView::is_default")]
    pub view: SheetView,
    /// Column widths (twips).
    #[serde(default, skip_serializing_if = "AxisSizing::is_empty")]
    pub columns: AxisSizing,
    /// Row heights (twips).
    #[serde(default, skip_serializing_if = "AxisSizing::is_empty")]
    pub rows: AxisSizing,
    /// Hidden rows, by zero-based index.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub hidden_rows: BTreeSet<u32>,
    /// Hidden columns, by zero-based index.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub hidden_cols: BTreeSet<u32>,
    /// Outline (grouping) nesting level per row, by zero-based index. Sparse:
    /// only rows with a non-zero level appear.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub row_outline_levels: BTreeMap<u32, u8>,
    /// Outline (grouping) nesting level per column, by zero-based index. Sparse:
    /// only columns with a non-zero level appear.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub col_outline_levels: BTreeMap<u32, u8>,
    /// Rows whose outline group is collapsed, by zero-based index.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub collapsed_rows: BTreeSet<u32>,
    /// Columns whose outline group is collapsed, by zero-based index.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub collapsed_cols: BTreeSet<u32>,
    /// Outline summary-position properties (`<outlinePr>`).
    #[serde(default, skip_serializing_if = "OutlinePr::is_default")]
    pub outline: OutlinePr,
    /// Tab color as an `RRGGBB` hex string (no `#`), if the tab is colored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_color: Option<String>,
}

impl Sheet {
    /// A new empty sheet.
    pub fn new(id: SheetId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            cells: CellStore::new(),
            merges: Vec::new(),
            view: SheetView::default(),
            columns: AxisSizing::default(),
            rows: AxisSizing::default(),
            hidden_rows: BTreeSet::new(),
            hidden_cols: BTreeSet::new(),
            row_outline_levels: BTreeMap::new(),
            col_outline_levels: BTreeMap::new(),
            collapsed_rows: BTreeSet::new(),
            collapsed_cols: BTreeSet::new(),
            outline: OutlinePr::default(),
            tab_color: None,
        }
    }
}
