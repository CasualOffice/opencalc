//! Worksheets. Definition tables (styles, numbering, defined names, notes,
//! theme) live on the workbook; the sheet holds its grid, merges, and view.
//! See `docs/22-NORMALIZED-SCHEMA.md`.

use serde::{Deserialize, Serialize};

use crate::ids::SheetId;
use crate::store::{CellRange, CellStore};

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
        }
    }
}
