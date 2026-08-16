//! The sparse cell grid.
//!
//! The public API here (point access, ordered iteration, blank-eviction) is the
//! stable contract; the internal representation is a tuning decision. This shell
//! uses an ordered `BTreeMap` keyed row-major, which already gives deterministic
//! iteration and zero cost for empty cells. The row-blocked tile layout in
//! `docs/23-CELL-STORE-REPRESENTATION.md` is a later performance implementation
//! *behind this same API* — swapping it in is not an API change.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::cell::Cell;

/// A cell address within a sheet. Ordering is row-major, matching the
/// deterministic snapshot iteration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CellRef {
    /// Zero-based row.
    pub row: u32,
    /// Zero-based column.
    pub col: u32,
}

/// The last addressable row, zero-based: 2^20 rows.
///
/// `docs/21-PARSER-LIMITS.md` has listed "Max rows / columns — 2^20 rows x 2^14
/// cols" as a non-bypassable admission limit since Phase 0, and until FID-18
/// **no constant anywhere in `crates/` said so**. A worksheet naming
/// `<row r="4294967295">` imported unbounded, was written straight back, and
/// produced a package Excel and LibreOffice refuse to open. A documented limit
/// nothing in the code enforces is not a limit, so the number lives here, beside
/// the type it bounds, and every layer that has to decide whether an address is
/// real asks [`CellRef::in_grid`] rather than carrying its own copy.
pub const GRID_MAX_ROW: u32 = (1 << 20) - 1;
/// The last addressable column, zero-based: 2^14 columns. See [`GRID_MAX_ROW`].
pub const GRID_MAX_COL: u32 = (1 << 14) - 1;

impl CellRef {
    /// A reference to `(row, col)`.
    pub fn new(row: u32, col: u32) -> Self {
        Self { row, col }
    }

    /// Whether this address exists in an ECMA-376 grid.
    ///
    /// `CellRef` is two `u32`s because the grid is 2^20 x 2^14 and packing it
    /// tighter buys nothing; that width is a representation detail, not a
    /// licence to address 4 billion rows.
    #[must_use]
    pub fn in_grid(&self) -> bool {
        self.row <= GRID_MAX_ROW && self.col <= GRID_MAX_COL
    }
}

/// A rectangular range of cells (inclusive), e.g. a merged region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CellRange {
    /// Top-left corner.
    pub start: CellRef,
    /// Bottom-right corner.
    pub end: CellRef,
}

impl CellRange {
    /// A range from `start` to `end`, normalized so `start` is the top-left.
    pub fn new(a: CellRef, b: CellRef) -> Self {
        Self {
            start: CellRef::new(a.row.min(b.row), a.col.min(b.col)),
            end: CellRef::new(a.row.max(b.row), a.col.max(b.col)),
        }
    }

    /// Whether the whole rectangle exists in an ECMA-376 grid.
    ///
    /// Only `end` can fail once the range is normalized, but both corners are
    /// checked: a caller that built a `CellRange` by hand rather than through
    /// [`Self::new`] would otherwise slip past.
    #[must_use]
    pub fn in_grid(&self) -> bool {
        self.start.in_grid() && self.end.in_grid()
    }
}

/// One stored cell with its address; the on-wire form of a [`CellStore`] entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredCell {
    row: u32,
    col: u32,
    cell: Cell,
}

/// The sparse grid of populated cells for one sheet.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(from = "Vec<StoredCell>", into = "Vec<StoredCell>")]
pub struct CellStore {
    cells: BTreeMap<CellRef, Cell>,
}

impl CellStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of populated cells.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether no cells are populated.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Get the cell at `at`, if populated.
    pub fn get(&self, at: CellRef) -> Option<&Cell> {
        self.cells.get(&at)
    }

    /// Set the cell at `at`. A blank cell is evicted rather than stored, so
    /// empty cells never cost memory.
    pub fn set(&mut self, at: CellRef, cell: Cell) {
        if cell.is_blank() {
            self.cells.remove(&at);
        } else {
            self.cells.insert(at, cell);
        }
    }

    /// Remove the cell at `at`, returning it if it was populated.
    pub fn clear(&mut self, at: CellRef) -> Option<Cell> {
        self.cells.remove(&at)
    }

    /// Iterate populated cells in deterministic row-major order.
    pub fn iter(&self) -> impl Iterator<Item = (CellRef, &Cell)> {
        self.cells.iter().map(|(r, c)| (*r, c))
    }

    /// The highest populated row, or `None` when the sheet is empty.
    ///
    /// O(log n): the map is keyed row-major, so the last key is the last row.
    /// A whole-column reference like `A:A` needs exactly this — it is what
    /// stops the range from spanning all 1,048,576 rows.
    #[must_use]
    pub fn last_row(&self) -> Option<u32> {
        self.cells.keys().next_back().map(|at| at.row)
    }

    /// The highest populated column within `[first_row, last_row]`.
    ///
    /// Scoped to the band rather than the sheet because the map is not ordered
    /// by column: answering for the whole sheet would mean a full scan, and a
    /// whole-row reference like `$1:$2` only ever needs its own rows.
    #[must_use]
    pub fn last_col_in_rows(&self, first_row: u32, last_row: u32) -> Option<u32> {
        self.row_band(first_row, last_row)
            .map(|(at, _)| at.col)
            .max()
    }

    /// Iterate populated cells whose row is in `[first_row, last_row]`, in
    /// row-major order. Cost is proportional to the cells in that band, not to
    /// the whole sheet — the basis for O(visible) viewport layout.
    pub fn row_band(
        &self,
        first_row: u32,
        last_row: u32,
    ) -> impl Iterator<Item = (CellRef, &Cell)> {
        let start = CellRef::new(first_row, 0);
        let end = CellRef::new(last_row.saturating_add(1), 0);
        self.cells.range(start..end).map(|(r, c)| (*r, c))
    }
}

impl From<Vec<StoredCell>> for CellStore {
    fn from(entries: Vec<StoredCell>) -> Self {
        let mut cells = BTreeMap::new();
        for entry in entries {
            let at = CellRef::new(entry.row, entry.col);
            if !entry.cell.is_blank() {
                cells.insert(at, entry.cell);
            }
        }
        Self { cells }
    }
}

impl From<CellStore> for Vec<StoredCell> {
    fn from(store: CellStore) -> Self {
        store
            .cells
            .into_iter()
            .map(|(at, cell)| StoredCell {
                row: at.row,
                col: at.col,
                cell,
            })
            .collect()
    }
}
