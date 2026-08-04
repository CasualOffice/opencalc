//! Worksheets. Definition tables (styles, numbering, defined names, notes,
//! theme) live on the workbook; the sheet holds its grid, merges, and view.
//! See `docs/22-NORMALIZED-SCHEMA.md`.

use std::collections::BTreeMap;

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

/// A sheet's view state: the frozen (pinned) row/column bands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SheetView {
    /// Number of rows frozen at the top.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub frozen_rows: u32,
    /// Number of columns frozen at the left.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub frozen_cols: u32,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

impl SheetView {
    /// Whether the view is at its default (nothing frozen).
    pub fn is_default(&self) -> bool {
        self.frozen_rows == 0 && self.frozen_cols == 0
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
        }
    }
}
