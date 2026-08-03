//! The per-cell record and its flags. Kept compact and fixed-shape — this is
//! the structure multiplied by ~1M in the capacity target
//! (`docs/23-CELL-STORE-REPRESENTATION.md`, `docs/30-PERFORMANCE-AND-CAPACITY-TARGETS.md`).

use serde::{Deserialize, Serialize};

use crate::ids::{FormulaHandle, StyleId};
use crate::value::CellValue;

/// Packed per-cell flags. Reserved bits (`SPILL_ANCHOR`, `SPILL_CHILD`) are set
/// by the calc engine in Phase 2; `DIRTY` is set by the transaction layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CellFlags(u8);

impl CellFlags {
    /// The cached value is stale and awaits recalculation.
    pub const DIRTY: u8 = 1 << 0;
    /// This cell is the origin of a spilled dynamic array.
    pub const SPILL_ANCHOR: u8 = 1 << 1;
    /// This cell is filled by a neighboring spill anchor.
    pub const SPILL_CHILD: u8 = 1 << 2;

    /// Empty flags.
    pub fn new() -> Self {
        Self(0)
    }

    /// Whether no flags are set (used to skip serialization).
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Whether all bits in `flag` are set.
    pub fn contains(&self, flag: u8) -> bool {
        self.0 & flag == flag
    }

    /// Set the bits in `flag`.
    pub fn insert(&mut self, flag: u8) {
        self.0 |= flag;
    }

    /// Clear the bits in `flag`.
    pub fn remove(&mut self, flag: u8) {
        self.0 &= !flag;
    }
}

/// A single cell: its cached/literal value, an optional interned style, an
/// optional formula handle (the reserved calc seam), and packed flags.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cell {
    /// The cached or literal value.
    pub value: CellValue,
    /// The interned style, if any (absent = workbook default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<StyleId>,
    /// The formula AST handle, if this cell holds a formula (reserved calc seam;
    /// evaluated in Phase 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<FormulaHandle>,
    /// Packed flags.
    #[serde(default, skip_serializing_if = "CellFlags::is_empty")]
    pub flags: CellFlags,
}

impl Cell {
    /// A literal-value cell with no style, formula, or flags.
    pub fn value(value: CellValue) -> Self {
        Self {
            value,
            ..Self::default()
        }
    }

    /// Whether this cell is empty in every field (and so need not be stored).
    pub fn is_blank(&self) -> bool {
        self.value.is_empty()
            && self.style.is_none()
            && self.formula.is_none()
            && self.flags.is_empty()
    }
}
