//! Worksheets. A shell for Phase 0 — geometry, merges, views, and the reserved
//! per-sheet dependency-edge table (`docs/22-NORMALIZED-SCHEMA.md`) are added as
//! import matures.

use serde::{Deserialize, Serialize};

use crate::ids::SheetId;
use crate::store::CellStore;

/// One worksheet: an identity, a name, and its sparse cell grid.
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
}

impl Sheet {
    /// A new empty sheet.
    pub fn new(id: SheetId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            cells: CellStore::new(),
        }
    }
}
