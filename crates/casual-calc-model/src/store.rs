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

    /// Move the first `n` cells (row-major) out into a store of their own.
    ///
    /// For [`crate::transfer`], which hands a sheet over in bounded parts.
    pub(crate) fn split_first(&mut self, n: usize) -> CellStore {
        if n >= self.cells.len() {
            return std::mem::take(self);
        }
        let Some(at) = self.cells.keys().nth(n).copied() else {
            return std::mem::take(self);
        };
        let rest = self.cells.split_off(&at);
        CellStore {
            cells: std::mem::replace(&mut self.cells, rest),
        }
    }

    /// The first and last populated addresses, row-major.
    pub(crate) fn bounds(&self) -> Option<(CellRef, CellRef)> {
        Some((*self.cells.keys().next()?, *self.cells.keys().next_back()?))
    }

    /// Move every cell of `other` into this store; a cell already here at the
    /// same address is replaced.
    pub(crate) fn absorb(&mut self, mut other: CellStore) {
        self.cells.append(&mut other.cells);
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

    /// Excel's *current region* around `seed`: the rectangle bounded by blank
    /// rows and blank columns, what Ctrl+Shift+* selects and what Sort, Ctrl+A,
    /// Create Table and Go To Special › Current region act on (`XL-UX-175`).
    ///
    /// Grown from `seed` until the ring of cells around it — the row above and
    /// below, the column left and right, *corners included* — holds nothing.
    /// A cell counts when it has a value or a formula; a style alone does not.
    /// The growth runs to a fixed point: a block found by walking right can
    /// reach further up than the walk up did, and stopping after one pass of
    /// each direction left a ragged block short of its own data.
    ///
    /// `None` when nothing in or around `seed` has content — Excel's region
    /// is then the selection itself, which no caller wants as a block.
    #[must_use]
    pub fn current_region(&self, seed: CellRange) -> Option<CellRange> {
        let (mut r0, mut c0) = (seed.start.row, seed.start.col);
        let (mut r1, mut c1) = (
            seed.end.row.min(GRID_MAX_ROW),
            seed.end.col.min(GRID_MAX_COL),
        );
        let content = |c: &Cell| !c.value.is_empty() || c.formula.is_some();
        // One row's cells in [a, b]: a range query on the row-major map.
        let row_has = |r: u32, a: u32, b: u32| {
            self.cells
                .range(CellRef::new(r, a)..=CellRef::new(r, b))
                .any(|(_, c)| content(c))
        };
        let mut grew = false;
        loop {
            let before = (r0, c0, r1, c1);
            let (ca, cb) = (c0.saturating_sub(1), (c1 + 1).min(GRID_MAX_COL));
            while r0 > 0 && row_has(r0 - 1, ca, cb) {
                r0 -= 1;
            }
            while r1 < GRID_MAX_ROW && row_has(r1 + 1, ca, cb) {
                r1 += 1;
            }
            // The columns with content in the band, found in one pass over the
            // band rather than a point lookup per row per column step.
            let (ra, rb) = (r0.saturating_sub(1), (r1 + 1).min(GRID_MAX_ROW));
            let mut cols = std::collections::BTreeSet::new();
            for (at, c) in self.row_band(ra, rb) {
                if content(c) {
                    cols.insert(at.col);
                }
            }
            while c0 > 0 && cols.contains(&(c0 - 1)) {
                c0 -= 1;
            }
            while c1 < GRID_MAX_COL && cols.contains(&(c1 + 1)) {
                c1 += 1;
            }
            if (r0, c0, r1, c1) == before {
                break;
            }
            grew = true;
        }
        let seeded = self
            .row_band(seed.start.row, seed.end.row)
            .any(|(at, c)| at.col >= seed.start.col && at.col <= seed.end.col && content(c));
        (grew || seeded).then(|| CellRange::new(CellRef::new(r0, c0), CellRef::new(r1, c1)))
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

#[cfg(test)]
mod current_region_tests {
    use super::*;
    use crate::{CellValue, StyleId};

    fn store(cells: &[(u32, u32)]) -> CellStore {
        let mut s = CellStore::new();
        for &(r, c) in cells {
            s.set(CellRef::new(r, c), Cell::value(CellValue::Number(1.0)));
        }
        s
    }
    fn at(r: u32, c: u32) -> CellRange {
        CellRange::new(CellRef::new(r, c), CellRef::new(r, c))
    }
    fn rect(r0: u32, c0: u32, r1: u32, c1: u32) -> Option<CellRange> {
        Some(CellRange::new(CellRef::new(r0, c0), CellRef::new(r1, c1)))
    }

    #[test]
    fn a_blank_column_bounds_the_region() {
        // A1:B4 and D1:E4, column C blank between them.
        let mut cells = Vec::new();
        for r in 0..4 {
            cells.extend([(r, 0), (r, 1), (r, 3), (r, 4)]);
        }
        let s = store(&cells);
        assert_eq!(s.current_region(at(1, 1)), rect(0, 0, 3, 1));
        assert_eq!(s.current_region(at(2, 4)), rect(0, 3, 3, 4));
    }

    #[test]
    fn a_blank_row_bounds_the_region() {
        // A title in A1, a blank row 2, the block B3:C6.
        let mut cells = vec![(0, 0)];
        for r in 2..6 {
            cells.extend([(r, 1), (r, 2)]);
        }
        let s = store(&cells);
        assert_eq!(s.current_region(at(4, 2)), rect(2, 1, 5, 2));
        assert_eq!(s.current_region(at(0, 0)), rect(0, 0, 0, 0));
    }

    #[test]
    fn growth_runs_to_a_fixed_point() {
        // A1:C1, then C2:C4 hanging down from its right end. Walking up, down,
        // left and right once from A1 reaches C1 but not C2:C4 below it: the
        // walk down ran while the block was still only A1 wide.
        let s = store(&[(0, 0), (0, 1), (0, 2), (1, 2), (2, 2), (3, 2)]);
        assert_eq!(s.current_region(at(0, 0)), rect(0, 0, 3, 2));
        assert_eq!(s.current_region(at(3, 2)), rect(0, 0, 3, 2));
    }

    #[test]
    fn a_diagonal_neighbour_joins_the_region() {
        let s = store(&[(0, 0), (1, 1)]);
        assert_eq!(s.current_region(at(0, 0)), rect(0, 0, 1, 1));
    }

    #[test]
    fn an_empty_cell_takes_the_region_it_touches_and_an_isolated_one_has_none() {
        let s = store(&[(0, 0), (0, 1)]);
        assert_eq!(s.current_region(at(0, 2)), rect(0, 0, 0, 2));
        assert_eq!(s.current_region(at(5, 5)), None);
        assert_eq!(CellStore::new().current_region(at(0, 0)), None);
    }

    #[test]
    fn a_style_alone_is_not_content() {
        let mut s = store(&[(0, 0)]);
        s.set(
            CellRef::new(0, 1),
            Cell {
                style: Some(StyleId::at(0)),
                ..Cell::default()
            },
        );
        assert_eq!(s.current_region(at(0, 0)), rect(0, 0, 0, 0));
    }

    #[test]
    fn a_partial_selection_grows_to_its_block() {
        let mut cells = Vec::new();
        for r in 0..4 {
            cells.extend([(r, 0), (r, 1), (r, 3)]);
        }
        let s = store(&cells);
        let scores = CellRange::new(CellRef::new(1, 1), CellRef::new(3, 1));
        assert_eq!(s.current_region(scores), rect(0, 0, 3, 1));
    }

    #[test]
    fn the_grid_edge_is_a_boundary_not_an_overflow() {
        let s = store(&[
            (GRID_MAX_ROW, GRID_MAX_COL),
            (GRID_MAX_ROW - 1, GRID_MAX_COL),
        ]);
        assert_eq!(
            s.current_region(at(GRID_MAX_ROW, GRID_MAX_COL)),
            rect(GRID_MAX_ROW - 1, GRID_MAX_COL, GRID_MAX_ROW, GRID_MAX_COL)
        );
    }
}
