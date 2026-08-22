//! Structural row/column insert & delete with formula-reference rewriting.
//!
//! Inserting or deleting whole rows/columns has three coupled effects that must
//! stay in lock-step or the model silently corrupts:
//!
//! 1. **Geometry** — every populated cell on or after the insertion/deletion
//!    band shifts, and the sheet's [`CellStore`] is rebuilt at the new
//!    coordinates.
//! 2. **Position-indexed metadata** — everything else on the sheet that is keyed
//!    by row/column index shifts the same way: merged ranges, explicit column
//!    widths / row heights, the hidden row/column sets, and the frozen-pane
//!    counts. Miss any of these and a merge, custom height, or freeze silently
//!    slides out from under the cells it belonged to.
//! 3. **References** — every formula *anywhere in the workbook* that targets the
//!    affected sheet has its cell/range references rewritten so they keep
//!    pointing at the same logical cells (or collapse to `#REF!` when their
//!    target is deleted).
//!
//! A reference *targets* the operation's sheet `S` when it is explicitly
//! qualified with `S`'s name, or when it is unqualified and lives in a formula
//! whose home sheet *is* `S`. Cross-sheet references to `S` from other sheets
//! shift too.
//!
//! Both operations return an exact inverse, so undo is inverse replay (see
//! [`crate::apply`]). Insert's inverse is the matching delete — exact because an
//! insert opens an empty band no reference or metadata index can point into, so
//! the matching delete shifts everything back with nothing to drop. Delete's
//! inverse is a [`Operation::Batch`] of the re-insert, a
//! [`Operation::SetSheetMetadata`] carrying the pre-delete metadata snapshot
//! (the re-insert alone cannot resurrect a merge or custom height the delete
//! dropped), plus `SetCell` restores of every cell and formula the delete could
//! have touched — all snapshotted *before* mutation.

use std::collections::{BTreeMap, BTreeSet};

use casual_calc_formula::Expr;
use casual_calc_formula::restore_at;
use casual_calc_formula::stored::{ABSOLUTE, Origin, StoredRef};
use casual_calc_model::{
    AutoFilter, AxisSizing, CellComment, CellRange, CellRef, CellStore, ChartView,
    ConditionalFormat, DataValidation, DefinedName, Hyperlink, PivotTable, Sheet, Table,
    TableColumn, Workbook,
};

use crate::{Operation, TxnError};

/// Which axis a structural operation runs along.
///
/// Public because [`WouldDiscard`](crate::WouldDiscard) names one, and a caller
/// refusing an undo has to say whether it was rows or columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Rows — the vertical axis; coordinate is `CellRef::row` / `CellReference::row`.
    Row,
    /// Columns — the horizontal axis; coordinate is `CellRef::col` / `CellReference::col`.
    Col,
}

impl Axis {
    /// The along-axis coordinate of a stored cell address.
    fn coord(self, at: CellRef) -> u32 {
        match self {
            Axis::Row => at.row,
            Axis::Col => at.col,
        }
    }

    /// A copy of `at` with its along-axis coordinate replaced by `value`.
    fn with_coord(self, at: CellRef, value: u32) -> CellRef {
        match self {
            Axis::Row => CellRef::new(value, at.col),
            Axis::Col => CellRef::new(at.row, value),
        }
    }

    /// The along-axis coordinate of a formula reference.
    ///
    /// Read from a tree in the **absolute form**, where a stored reference's
    /// `i64` offset from `(0, 0)` is the address itself. The caller converts on
    /// the way in and back on the way out, so everything here goes on working
    /// in addresses.
    fn ref_coord(self, reference: &StoredRef) -> u32 {
        let raw = match self {
            Axis::Row => reference.row,
            Axis::Col => reference.col,
        };
        u32::try_from(raw).unwrap_or(0)
    }

    /// Set the along-axis coordinate of a formula reference in place.
    fn set_ref_coord(self, reference: &mut StoredRef, value: u32) {
        match self {
            Axis::Row => reference.row = i64::from(value),
            Axis::Col => reference.col = i64::from(value),
        }
    }
}

/// Whether a rewrite is shifting for an insert or a delete.
#[derive(Debug, Clone, Copy)]
enum ShiftKind {
    Insert,
    Delete,
}

/// Apply an insert of `count` lines at `at` along `axis` on sheet `sheet`.
/// Returns the inverse (the matching delete).
pub(crate) fn insert(
    workbook: &mut Workbook,
    sheet: usize,
    axis: Axis,
    at: u32,
    count: u32,
) -> Result<Operation, TxnError> {
    let target = sheet_name(workbook, sheet)?;
    shift_cells_insert(workbook, sheet, axis, at, count);
    shift_metadata_insert(&mut workbook.sheets[sheet], axis, at, count);
    rewrite_all_formulas(workbook, &target, axis, ShiftKind::Insert, at, count);
    rewrite_defined_names(workbook, &target, axis, ShiftKind::Insert, at, count);
    shift_drawings_and_pivot_sources(workbook, sheet, axis, ShiftKind::Insert, at, count);
    rewrite_chart_series(workbook, &target, axis, ShiftKind::Insert, at, count);
    Ok(delete_op(sheet, axis, at, count))
}

/// Apply a delete of `count` lines starting at `at` along `axis` on sheet
/// `sheet`. Returns the inverse: a [`Operation::Batch`] of the re-insert plus
/// `SetCell` restores captured *before* mutation.
pub(crate) fn delete(
    workbook: &mut Workbook,
    sheet: usize,
    axis: Axis,
    at: u32,
    count: u32,
) -> Result<Operation, TxnError> {
    let target = sheet_name(workbook, sheet)?;
    // Snapshot everything the delete can change, before we mutate anything. The
    // metadata snapshot is taken as a whole because a delete can drop a merge or
    // a hidden line outright — a shift alone is not invertible — so undo restores
    // the exact pre-delete metadata rather than trying to un-shift it.
    let restores = snapshot_for_delete(workbook, sheet, axis, at);
    let metadata_restore = snapshot_metadata(workbook, sheet);
    shift_cells_delete(workbook, sheet, axis, at, count);
    shift_metadata_delete(&mut workbook.sheets[sheet], axis, at, count);
    rewrite_all_formulas(workbook, &target, axis, ShiftKind::Delete, at, count);
    rewrite_defined_names(workbook, &target, axis, ShiftKind::Delete, at, count);
    shift_drawings_and_pivot_sources(workbook, sheet, axis, ShiftKind::Delete, at, count);
    rewrite_chart_series(workbook, &target, axis, ShiftKind::Delete, at, count);

    // Inverse order: re-open the band (restores cell geometry), overwrite the
    // metadata with its pre-delete snapshot, then restore the touched cells.
    let mut ops = Vec::with_capacity(restores.len() + 2);
    ops.push(insert_op(sheet, axis, at, count));
    ops.push(metadata_restore);
    ops.extend(restores);
    Ok(Operation::Batch(ops))
}

/// Clone the target sheet's name, erroring if the sheet index is out of range.
fn sheet_name(workbook: &Workbook, sheet: usize) -> Result<String, TxnError> {
    workbook
        .sheets
        .get(sheet)
        .map(|s| s.name.clone())
        .ok_or(TxnError::SheetNotFound { index: sheet })
}

/// The `Insert*` operation for `axis`.
fn insert_op(sheet: usize, axis: Axis, at: u32, count: u32) -> Operation {
    match axis {
        Axis::Row => Operation::InsertRows { sheet, at, count },
        Axis::Col => Operation::InsertColumns { sheet, at, count },
    }
}

/// The `Delete*` operation for `axis`.
fn delete_op(sheet: usize, axis: Axis, at: u32, count: u32) -> Operation {
    match axis {
        Axis::Row => Operation::DeleteRows { sheet, at, count },
        Axis::Col => Operation::DeleteColumns { sheet, at, count },
    }
}

/// Re-store the formulas of cells that are about to move, so each goes on
/// naming the cells it named.
///
/// **A cell that moves is doing what a cut does** (`PERF-11`, and the reason
/// this exists at all): a stored tree's references are offsets from the cell
/// holding it, so carrying that cell somewhere else changes what they point at
/// unless the offsets are re-measured. `restore_at` is the same primitive the
/// clipboard uses for a cut.
///
/// Two passes, because re-storing needs the workbook while rebuilding needs the
/// cell store, and they cannot both be borrowed. The map is keyed by the cell's
/// **old** address, which is what the rebuild still has in hand.
///
/// Returns the new handle for each moved formula. Cells that do not move, and
/// cells with no formula, are absent — nothing about them changes.
fn restore_moved_formulas(
    workbook: &mut Workbook,
    sheet: usize,
    moves: &[(CellRef, CellRef)],
) -> BTreeMap<CellRef, casual_calc_model::FormulaHandle> {
    let planned: Vec<(CellRef, Expr)> = moves
        .iter()
        .filter(|(from, to)| from != to)
        .filter_map(|(from, to)| {
            let handle = workbook.sheets[sheet].cells.get(*from)?.formula?;
            let expr = workbook.formula(handle)?;
            Some((
                *from,
                restore_at(
                    expr,
                    Origin::at(from.row, from.col),
                    Origin::at(to.row, to.col),
                ),
            ))
        })
        .collect();

    planned
        .into_iter()
        .map(|(from, expr)| (from, workbook.store_formula(expr)))
        .collect()
}

/// Rebuild the sheet's cell store, shifting every cell on or after `at` by
/// `+count` along `axis`. The sheet index was already validated by the caller.
fn shift_cells_insert(workbook: &mut Workbook, sheet: usize, axis: Axis, at: u32, count: u32) {
    let destination = |addr: CellRef| {
        let coord = axis.coord(addr);
        if coord >= at {
            axis.with_coord(addr, coord.saturating_add(count))
        } else {
            addr
        }
    };
    let moves: Vec<(CellRef, CellRef)> = workbook.sheets[sheet]
        .cells
        .iter()
        .map(|(addr, _)| (addr, destination(addr)))
        .collect();
    let restored = restore_moved_formulas(workbook, sheet, &moves);

    let store = &mut workbook.sheets[sheet].cells;
    let old = std::mem::take(store);
    let mut rebuilt = CellStore::new();
    for (addr, cell) in old.iter() {
        let mut cell = cell.clone();
        if let Some(handle) = restored.get(&addr) {
            cell.formula = Some(*handle);
        }
        rebuilt.set(destination(addr), cell);
    }
    *store = rebuilt;
}

/// Rebuild the sheet's cell store, dropping cells in the deleted band
/// `[at, at+count)` and shifting cells past it back by `count` along `axis`.
fn shift_cells_delete(workbook: &mut Workbook, sheet: usize, axis: Axis, at: u32, count: u32) {
    let end = at.saturating_add(count);
    let moves: Vec<(CellRef, CellRef)> = workbook.sheets[sheet]
        .cells
        .iter()
        .filter(|(addr, _)| axis.coord(*addr) >= end)
        .map(|(addr, _)| (addr, axis.with_coord(addr, axis.coord(addr) - count)))
        .collect();
    let restored = restore_moved_formulas(workbook, sheet, &moves);

    let store = &mut workbook.sheets[sheet].cells;
    let old = std::mem::take(store);
    let mut rebuilt = CellStore::new();
    for (addr, cell) in old.iter() {
        let coord = axis.coord(addr);
        if coord < at {
            rebuilt.set(addr, cell.clone());
        } else if coord >= end {
            let mut cell = cell.clone();
            if let Some(handle) = restored.get(&addr) {
                cell.formula = Some(*handle);
            }
            rebuilt.set(axis.with_coord(addr, coord - count), cell);
        }
        // Cells inside the deleted band are dropped.
    }
    *store = rebuilt;
}

// ---------------------------------------------------------------------------
// Position-indexed metadata (merges, sizing, hidden lines, frozen panes).
// ---------------------------------------------------------------------------

/// Capture the sheet's current position-indexed metadata as a
/// [`Operation::SetSheetMetadata`]. Used as a delete's inverse so undo reinstates
/// merges, sizing, hidden lines, and frozen panes the delete may have dropped —
/// re-inserting an empty band cannot resurrect them.
fn snapshot_metadata(workbook: &Workbook, sheet: usize) -> Operation {
    // `ALL` and not a narrower set on purpose: this is a *pre*-mutation
    // snapshot, taken before anyone knows which fields the delete will drop.
    // `apply` narrows it against the post-delete sheet when the undo runs,
    // which is the only moment both states exist.
    Operation::set_sheet_metadata(
        sheet,
        crate::SheetMetadata::capture(&workbook.sheets[sheet]),
    )
}

/// The along-axis sizing, hidden set, and frozen-count fields for `axis`. All
/// three are disjoint fields, so returning them together is a sound split borrow.
/// Everything a structural shift has to move, reachable the same way whether it
/// lives on a [`Sheet`] or in a [`crate::SheetMetadata`] bundle.
///
/// The two carry identical positional state — the bundle exists precisely to be
/// a snapshot of it — and both need the same shift: the sheet when rows are
/// inserted, the bundle when a pending metadata edit has to be rebased past
/// someone else's insert
/// ([ADR-011](../../../docs/56-COLLABORATION-CONCURRENCY-DESIGN.md)). A trait
/// rather than two copies, because a shift implemented twice is a shift that
/// disagrees with itself the first time a field is added.
pub(crate) trait Positional {
    fn merges_mut(&mut self) -> &mut Vec<CellRange>;
    /// The sheet's tables. Position-indexed like everything else here: a table
    /// is a *range*, and a range that does not move when rows move stops
    /// describing the data it names.
    fn tables_mut(&mut self) -> &mut Vec<Table>;
    fn auto_filter_mut(&mut self) -> &mut Option<AutoFilter>;
    fn filter_hidden_mut(&mut self) -> &mut BTreeSet<u32>;
    /// Validations, highlights, comments and links — **also position-indexed**,
    /// and every one of them a range or an address that stops describing its
    /// data the moment rows move under it.
    ///
    /// They were in the metadata bundle already, so a delete has always
    /// restored them; what none of them had was a *shift*. Insert a row above a
    /// validated block and the rule went on policing the rows the data had
    /// left: the row that moved out kept the constraint and the row that moved
    /// in was unguarded. Same shape as the table defect above, same silence —
    /// no error, no compatibility report, a saved file that looks right.
    fn validations_mut(&mut self) -> &mut Vec<DataValidation>;
    fn conditional_formats_mut(&mut self) -> &mut Vec<ConditionalFormat>;
    fn comments_mut(&mut self) -> &mut Vec<CellComment>;
    fn hyperlinks_mut(&mut self) -> &mut Vec<Hyperlink>;
    /// Charts anchored on this sheet. Only the **frame** moves here: a chart's
    /// series are reference strings and need a workbook to resolve sheet names
    /// against, which this trait deliberately does not have.
    fn charts_mut(&mut self) -> &mut Vec<ChartView>;
    /// Pivots held by this sheet. Only `anchor` and `output` move here — they
    /// are the two fields that live on *this* sheet. `source` lives on
    /// `source_sheet`, which may be another one entirely, so it is shifted at
    /// workbook level where that comparison can actually be made.
    fn pivots_mut(&mut self) -> &mut Vec<PivotTable>;
    /// Sizing, hidden lines, the freeze boundary, outline levels and collapse
    /// flags for one axis.
    fn axis_mut(
        &mut self,
        axis: Axis,
    ) -> (
        &mut AxisSizing,
        &mut BTreeSet<u32>,
        &mut u32,
        &mut BTreeMap<u32, u8>,
        &mut BTreeSet<u32>,
    );
}

macro_rules! impl_positional {
    ($t:ty) => {
        impl Positional for $t {
            fn merges_mut(&mut self) -> &mut Vec<CellRange> {
                &mut self.merges
            }
            fn tables_mut(&mut self) -> &mut Vec<Table> {
                &mut self.tables
            }
            fn auto_filter_mut(&mut self) -> &mut Option<AutoFilter> {
                &mut self.auto_filter
            }
            fn filter_hidden_mut(&mut self) -> &mut BTreeSet<u32> {
                &mut self.filter_hidden
            }
            fn validations_mut(&mut self) -> &mut Vec<DataValidation> {
                &mut self.validations
            }
            fn conditional_formats_mut(&mut self) -> &mut Vec<ConditionalFormat> {
                &mut self.conditional_formats
            }
            fn comments_mut(&mut self) -> &mut Vec<CellComment> {
                &mut self.comments
            }
            fn hyperlinks_mut(&mut self) -> &mut Vec<Hyperlink> {
                &mut self.hyperlinks
            }
            fn charts_mut(&mut self) -> &mut Vec<ChartView> {
                &mut self.charts
            }
            fn pivots_mut(&mut self) -> &mut Vec<PivotTable> {
                &mut self.pivots
            }
            fn axis_mut(
                &mut self,
                axis: Axis,
            ) -> (
                &mut AxisSizing,
                &mut BTreeSet<u32>,
                &mut u32,
                &mut BTreeMap<u32, u8>,
                &mut BTreeSet<u32>,
            ) {
                match axis {
                    Axis::Row => (
                        &mut self.rows,
                        &mut self.hidden_rows,
                        &mut self.view.frozen_rows,
                        &mut self.row_outline_levels,
                        &mut self.collapsed_rows,
                    ),
                    Axis::Col => (
                        &mut self.columns,
                        &mut self.hidden_cols,
                        &mut self.view.frozen_cols,
                        &mut self.col_outline_levels,
                        &mut self.collapsed_cols,
                    ),
                }
            }
        }
    };
}

impl_positional!(Sheet);
impl_positional!(crate::SheetMetadata);

/// Bump `cell`'s along-axis coordinate by `count` if it sits on or after `at`.
fn insert_coord(axis: Axis, cell: &mut CellRef, at: u32, count: u32) {
    let coord = axis.coord(*cell);
    if coord >= at {
        *cell = axis.with_coord(*cell, coord.saturating_add(count));
    }
}

/// Shift all position-indexed metadata for an insert of `count` lines at `at`:
/// every index on or after `at` moves up by `count`, and a freeze boundary that
/// the insert falls *before* grows to keep the same lines pinned.
pub(crate) fn shift_metadata_insert(sheet: &mut impl Positional, axis: Axis, at: u32, count: u32) {
    // Merges: each endpoint moves independently, so a merge straddling `at`
    // grows (its start stays, its end shifts down) — matching spreadsheets.
    for merge in sheet.merges_mut().iter_mut() {
        insert_coord(axis, &mut merge.start, at, count);
        insert_coord(axis, &mut merge.end, at, count);
    }
    // Validations, highlights and links are ranges and move exactly like
    // merges; a comment names one cell and moves like one endpoint.
    for validation in sheet.validations_mut() {
        insert_coord(axis, &mut validation.range.start, at, count);
        insert_coord(axis, &mut validation.range.end, at, count);
    }
    for format in sheet.conditional_formats_mut() {
        insert_coord(axis, &mut format.range.start, at, count);
        insert_coord(axis, &mut format.range.end, at, count);
    }
    for link in sheet.hyperlinks_mut() {
        insert_coord(axis, &mut link.range.start, at, count);
        insert_coord(axis, &mut link.range.end, at, count);
    }
    for comment in sheet.comments_mut() {
        insert_coord(axis, &mut comment.at, at, count);
    }
    // The autofilter's header range moves like a merge: both endpoints shift
    // independently, so an insert inside the range grows it to cover the new rows.
    if let Some(filter) = sheet.auto_filter_mut() {
        insert_coord(axis, &mut filter.range.start, at, count);
        insert_coord(axis, &mut filter.range.end, at, count);
    }
    // Filter-hidden rows are position-indexed too; miss this and an insert leaves
    // the wrong rows collapsed.
    if axis == Axis::Row {
        reindex_set(sheet.filter_hidden_mut(), |k| {
            Some(if k >= at { k.saturating_add(count) } else { k })
        });
    }
    // A chart's frame and a pivot's report block are ranges on this sheet and
    // move like a merge. A chart drawn over rows 5 to 10 must still be over the
    // same data after two rows go in above it, not two rows higher than it.
    for chart in sheet.charts_mut().iter_mut() {
        insert_coord(axis, &mut chart.anchor.start, at, count);
        insert_coord(axis, &mut chart.anchor.end, at, count);
    }
    for pivot in sheet.pivots_mut().iter_mut() {
        insert_coord(axis, &mut pivot.anchor, at, count);
        if let Some(output) = pivot.output.as_mut() {
            insert_coord(axis, &mut output.start, at, count);
            insert_coord(axis, &mut output.end, at, count);
        }
    }
    // Tables move exactly like merges, and were the one position-indexed thing
    // this function never touched. Insert a row above a table and its range,
    // banding, filter buttons and column list stayed on the old row numbers, so
    // `SUM(Table1[Amount])` read the header text row and dropped the last
    // record — a wrong number in a saved file, with no error and no report.
    for table in sheet.tables_mut().iter_mut() {
        let start = axis.coord(table.range.start);
        let end_before = axis.coord(table.range.end);
        insert_coord(axis, &mut table.range.start, at, count);
        insert_coord(axis, &mut table.range.end, at, count);
        // A table filters independently of the sheet, so it has its own range
        // to move; leaving it behind puts the filter buttons on the wrong
        // columns.
        if let Some(filter) = table.auto_filter.as_mut() {
            insert_coord(axis, &mut filter.range.start, at, count);
            insert_coord(axis, &mut filter.range.end, at, count);
        }
        // Widening a table sideways needs a *column* as well as the room for
        // it: `columns` is indexed left to right and every structured reference
        // resolves through it, so a range one wider than the list silently
        // shifts every column's meaning by one. Only an insert strictly inside
        // widens — one at the table's own first column pushes the whole table
        // right instead, which is what `at > start` distinguishes.
        if axis == Axis::Col && at > start && at <= end_before {
            let offset = (at - start) as usize;
            let first_id = table.columns.iter().map(|c| c.id).max().unwrap_or(0) + 1;
            for (i, id) in (first_id..).take(count as usize).enumerate() {
                let name = unused_column_name(&table.columns);
                table.columns.insert(
                    offset + i,
                    TableColumn {
                        id,
                        name,
                        totals_row_function: None,
                        totals_row_label: None,
                        calculated_column_formula: None,
                        totals_row_formula: None,
                    },
                );
            }
        }
    }
    let (sizing, hidden, frozen, outline, collapsed) = sheet.axis_mut(axis);
    let shift = |k: u32| Some(if k >= at { k.saturating_add(count) } else { k });
    reindex_map(&mut sizing.sizes, shift);
    reindex_set(hidden, shift);
    // The outline is position-indexed like everything else here, and was being
    // left behind: insert three rows above a group and its levels and collapse
    // flags stayed on the rows the group used to occupy, so the group silently
    // detached from its own rows.
    reindex_map(outline, shift);
    reindex_set(collapsed, shift);
    // Inserting inside (or above) the frozen band extends it; inserting exactly
    // at the boundary (`at == *frozen`) or below leaves the freeze alone.
    if at < *frozen {
        *frozen = frozen.saturating_add(count);
    }
}

/// Shift all position-indexed metadata for a delete of the band `[at, at+count)`:
/// indices in the band are dropped, indices past it move down by `count`, a merge
/// wholly inside the band is removed and one straddling it is clamped, and a
/// freeze boundary loses however many of its pinned lines fell in the band.
pub(crate) fn shift_metadata_delete(sheet: &mut impl Positional, axis: Axis, at: u32, count: u32) {
    sheet.merges_mut().retain_mut(|merge| {
        let lo = axis.coord(merge.start);
        let hi = axis.coord(merge.end);
        match map_range_delete(lo, hi, at, count) {
            None => false,
            Some((new_lo, new_hi)) => {
                merge.start = axis.with_coord(merge.start, new_lo);
                merge.end = axis.with_coord(merge.end, new_hi);
                true
            }
        }
    });
    // The same clamp-or-drop a merge gets: a range wholly inside the deleted
    // band goes with it, one straddling it is clamped.
    let clamp_range = |range: &mut casual_calc_model::CellRange| {
        let lo = axis.coord(range.start);
        let hi = axis.coord(range.end);
        match map_range_delete(lo, hi, at, count) {
            None => false,
            Some((new_lo, new_hi)) => {
                range.start = axis.with_coord(range.start, new_lo);
                range.end = axis.with_coord(range.end, new_hi);
                true
            }
        }
    };
    sheet
        .validations_mut()
        .retain_mut(|v| clamp_range(&mut v.range));
    sheet
        .conditional_formats_mut()
        .retain_mut(|f| clamp_range(&mut f.range));
    sheet
        .hyperlinks_mut()
        .retain_mut(|l| clamp_range(&mut l.range));
    // A comment names one cell: it is dropped with its row and moves with the
    // rows below.
    let band_end = at.saturating_add(count);
    sheet.comments_mut().retain_mut(|comment| {
        let coord = axis.coord(comment.at);
        if coord >= at && coord < band_end {
            return false;
        }
        if coord >= band_end {
            comment.at = axis.with_coord(comment.at, coord - count);
        }
        true
    });

    let end = at.saturating_add(count);
    // Clamp the autofilter's range the way a straddling merge is clamped, and
    // drop the filter outright if the delete takes the whole range with it.
    if let Some(filter) = sheet.auto_filter_mut() {
        let lo = axis.coord(filter.range.start);
        let hi = axis.coord(filter.range.end);
        match map_range_delete(lo, hi, at, count) {
            Some((new_lo, new_hi)) => {
                filter.range.start = axis.with_coord(filter.range.start, new_lo);
                filter.range.end = axis.with_coord(filter.range.end, new_hi);
            }
            None => *sheet.auto_filter_mut() = None,
        }
    }
    // A chart whose frame is entirely inside the deleted band goes with it, the
    // way a merge does; one that straddles the band shrinks. Same for a pivot's
    // report block — except that losing the *extent* must not lose the pivot,
    // which still has a definition and can be refreshed again, so `output`
    // clears to `None` rather than dropping the whole table.
    let clamp = |range: &mut CellRange| -> bool {
        let lo = axis.coord(range.start);
        let hi = axis.coord(range.end);
        match map_range_delete(lo, hi, at, count) {
            None => false,
            Some((new_lo, new_hi)) => {
                range.start = axis.with_coord(range.start, new_lo);
                range.end = axis.with_coord(range.end, new_hi);
                true
            }
        }
    };
    sheet
        .charts_mut()
        .retain_mut(|chart| clamp(&mut chart.anchor));
    for pivot in sheet.pivots_mut().iter_mut() {
        let coord = axis.coord(pivot.anchor);
        if coord >= end {
            pivot.anchor = axis.with_coord(pivot.anchor, coord - count);
        } else if coord >= at {
            // The anchor cell itself was deleted. The report has to start
            // somewhere, and the band's first surviving line is where the rows
            // below it have moved to.
            pivot.anchor = axis.with_coord(pivot.anchor, at);
        }
        if let Some(output) = pivot.output.as_mut()
            && !clamp(output)
        {
            pivot.output = None;
        }
    }
    // Tables are clamped the way a straddling merge is, and dropped outright
    // when the delete takes the whole range — the counterpart of the insert
    // above, and missing for the same reason it was.
    sheet.tables_mut().retain_mut(|table| {
        let start = axis.coord(table.range.start);
        let end_before = axis.coord(table.range.end);
        let Some((new_lo, new_hi)) = map_range_delete(start, end_before, at, count) else {
            return false;
        };
        table.range.start = axis.with_coord(table.range.start, new_lo);
        table.range.end = axis.with_coord(table.range.end, new_hi);
        if let Some(filter) = table.auto_filter.as_mut() {
            let lo = axis.coord(filter.range.start);
            let hi = axis.coord(filter.range.end);
            match map_range_delete(lo, hi, at, count) {
                Some((lo, hi)) => {
                    filter.range.start = axis.with_coord(filter.range.start, lo);
                    filter.range.end = axis.with_coord(filter.range.end, hi);
                }
                None => table.auto_filter = None,
            }
        }
        // Drop the columns the delete took with it, keyed by where each one
        // *was*. Doing this by position rather than by count matters when the
        // deleted band only partly overlaps the table: the survivors have to be
        // the ones outside the band, not the first or last N.
        if axis == Axis::Col {
            let mut column = start;
            table.columns.retain(|_| {
                let keep = !(column >= at && column < end);
                column += 1;
                keep
            });
        }
        true
    });
    if axis == Axis::Row {
        reindex_set(sheet.filter_hidden_mut(), |k| {
            map_index_delete(k, at, end, count)
        });
    }
    let (sizing, hidden, frozen, outline, collapsed) = sheet.axis_mut(axis);
    let shift = |k: u32| map_index_delete(k, at, end, count);
    reindex_map(&mut sizing.sizes, shift);
    reindex_set(hidden, shift);
    reindex_map(outline, shift);
    reindex_set(collapsed, shift);
    // Only the pinned lines that actually fell in the band reduce the freeze.
    if at < *frozen {
        let removed = end.min(*frozen).saturating_sub(at);
        *frozen = frozen.saturating_sub(removed);
    }
}

/// A `Column{n}` name no column in `existing` already uses.
///
/// Excel names a column it creates for you this way, and the name is not
/// cosmetic: it is what a structured reference resolves through, so two columns
/// sharing one would make `Table[Column2]` ambiguous.
fn unused_column_name(existing: &[TableColumn]) -> String {
    let mut n = existing.len() + 1;
    loop {
        let candidate = format!("Column{n}");
        if !existing.iter().any(|c| c.name == candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Map a single index through a delete of `[at, end)`: `None` (dropped) if it is
/// in the band, shifted down by `count` if past it, unchanged if before it.
fn map_index_delete(index: u32, at: u32, end: u32, count: u32) -> Option<u32> {
    if index < at {
        Some(index)
    } else if index >= end {
        Some(index - count)
    } else {
        None
    }
}

/// Rebuild a sizing/height map under an index remapping, dropping keys the
/// remapper returns `None` for. Values ride along with their (possibly moved)
/// key; a collision keeps the last-written value in ascending key order.
fn reindex_map<V>(map: &mut BTreeMap<u32, V>, remap: impl Fn(u32) -> Option<u32>) {
    let taken = std::mem::take(map);
    for (key, value) in taken {
        if let Some(new_key) = remap(key) {
            map.insert(new_key, value);
        }
    }
}

/// Rebuild an index set under an index remapping, dropping keys the remapper
/// returns `None` for.
fn reindex_set(set: &mut BTreeSet<u32>, remap: impl Fn(u32) -> Option<u32>) {
    let taken = std::mem::take(set);
    for key in taken {
        if let Some(new_key) = remap(key) {
            set.insert(new_key);
        }
    }
}

/// Snapshot, as `SetCell` restore ops, every cell a delete could disturb: on the
/// target sheet, cells on/after `at` (they move or vanish) plus any formula
/// cell (its references may be rewritten); on every other sheet, formula cells
/// (their cross-sheet references may be rewritten). Captured before mutation so
/// each op restores the exact original cell, including its original formula
/// handle.
fn snapshot_for_delete(workbook: &Workbook, sheet: usize, axis: Axis, at: u32) -> Vec<Operation> {
    let mut restores = Vec::new();
    for (idx, s) in workbook.sheets.iter().enumerate() {
        for (addr, cell) in s.cells.iter() {
            let touched = if idx == sheet {
                axis.coord(addr) >= at || cell.formula.is_some()
            } else {
                cell.formula.is_some()
            };
            if touched {
                restores.push(Operation::SetCell {
                    sheet: idx,
                    at: addr,
                    cell: Some(cell.clone()),
                });
            }
        }
    }
    restores
}

/// A formula cell scheduled for reference rewriting.
struct RewriteJob {
    sheet: usize,
    at: CellRef,
    home: String,
    expr: Expr,
}

/// Context for rewriting one formula's references.
struct RewriteCtx<'a> {
    /// Name of the sheet the structural op runs on.
    target: &'a str,
    /// Home sheet of the formula being rewritten (resolves unqualified refs).
    home: &'a str,
    axis: Axis,
    kind: ShiftKind,
    at: u32,
    count: u32,
}

/// Rewrite the references of every formula in the workbook that targets
/// `target`. Formulas are collected first (immutable borrow), then transformed
/// and re-stored; only formulas that actually change get a new arena entry and
/// an updated cell handle. Cached values are intentionally left stale for the
/// caller to recalculate.
fn rewrite_all_formulas(
    workbook: &mut Workbook,
    target: &str,
    axis: Axis,
    kind: ShiftKind,
    at: u32,
    count: u32,
) {
    let mut jobs: Vec<RewriteJob> = Vec::new();
    for (idx, s) in workbook.sheets.iter().enumerate() {
        for (addr, cell) in s.cells.iter() {
            if let Some(handle) = cell.formula
                && let Some(expr) = workbook.formula(handle)
            {
                jobs.push(RewriteJob {
                    sheet: idx,
                    at: addr,
                    home: s.name.clone(),
                    expr: expr.clone(),
                });
            }
        }
    }

    for job in jobs {
        let ctx = RewriteCtx {
            target,
            home: &job.home,
            axis,
            kind,
            at,
            count,
        };
        // **In and out of the absolute form.** The rewrite below works in
        // addresses — "is this reference at or past the insertion point" is a
        // question about an address, not an offset — so the tree is resolved
        // against its own cell first and re-stored after. The rewrite itself is
        // unchanged by `PERF-11`, which is the point: what moving a formula
        // does to its meaning is handled where the cells move, not here.
        let origin = Origin::at(job.at.row, job.at.col);
        let absolute = restore_at(&job.expr, origin, ABSOLUTE);
        let rewritten = restore_at(&rewrite_expr(&absolute, &ctx), ABSOLUTE, origin);
        if rewritten != job.expr {
            let handle = workbook.store_formula(rewritten);
            let store = &mut workbook.sheets[job.sheet].cells;
            if let Some(existing) = store.get(job.at) {
                let mut updated = existing.clone();
                updated.formula = Some(handle);
                store.set(job.at, updated);
            }
        }
    }
}

/// Rewrite the references inside every defined name.
///
/// `structural.rs` shifts merges, sizing, hidden sets, the freeze boundary,
/// outline levels, tables and autofilters — everything position-indexed except
/// this, until `FID-24`. A name pointing at `A10` still pointed at `A10` after
/// five rows were inserted above it, so every formula using that name silently
/// read different cells. Silent is the whole of the problem: nothing is marked
/// `#REF!`, nothing is refused, the number just changes.
///
/// **A workbook-scoped name has no home sheet**, and that matters for the
/// unqualified references the rewrite decides by home. Excel writes a
/// workbook-scoped `refersTo` fully qualified (`Sheet1!$A$10`), so an
/// unqualified one is already unusual; treating its home as no sheet at all
/// means such a reference is left alone rather than rewritten against a sheet
/// picked arbitrarily. Under-rewriting an oddity beats rewriting it wrongly.
fn rewrite_defined_names(
    workbook: &mut Workbook,
    target: &str,
    axis: Axis,
    kind: ShiftKind,
    at: u32,
    count: u32,
) {
    let jobs: Vec<(usize, Expr, String)> = workbook
        .defined_names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let home = name
                .sheet
                .and_then(|id| workbook.sheets.iter().find(|s| s.id == id))
                .map(|s| s.name.clone())
                .unwrap_or_default();
            (i, name.formula.clone(), home)
        })
        .collect();

    for (i, expr, home) in jobs {
        let ctx = RewriteCtx {
            target,
            home: &home,
            axis,
            kind,
            at,
            count,
        };
        // A defined name has no holding cell, so its tree is already the
        // absolute form and needs no conversion.
        let rewritten = rewrite_expr(&expr, &ctx);
        if rewritten != expr {
            workbook.defined_names[i].formula = rewritten;
        }
    }
}

/// Shift what `shift_metadata_*` cannot reach from a single sheet.
///
/// Two things fall outside it. **Images** are on `Sheet` but not in the
/// metadata bundle, so they have no place on [`Positional`] — the trait is
/// implemented for both and a method one of them cannot answer would be a lie.
/// **A pivot's `source`** lives on `source_sheet`, which is very often *not*
/// the sheet holding the pivot; deciding whether it moves means comparing sheet
/// identities, and a lone `&mut impl Positional` has none to compare.
fn shift_drawings_and_pivot_sources(
    workbook: &mut Workbook,
    sheet: usize,
    axis: Axis,
    kind: ShiftKind,
    at: u32,
    count: u32,
) {
    let target_id = workbook.sheets[sheet].id;

    // Images sit on the sheet the insert ran on, and move exactly like a chart
    // frame: both endpoints on an insert, clamped-or-dropped on a delete.
    let images = &mut workbook.sheets[sheet].images;
    match kind {
        ShiftKind::Insert => {
            for image in images.iter_mut() {
                insert_coord(axis, &mut image.anchor.start, at, count);
                insert_coord(axis, &mut image.anchor.end, at, count);
            }
        }
        ShiftKind::Delete => images.retain_mut(|image| {
            let lo = axis.coord(image.anchor.start);
            let hi = axis.coord(image.anchor.end);
            match map_range_delete(lo, hi, at, count) {
                None => false,
                Some((new_lo, new_hi)) => {
                    image.anchor.start = axis.with_coord(image.anchor.start, new_lo);
                    image.anchor.end = axis.with_coord(image.anchor.end, new_hi);
                    true
                }
            }
        }),
    }

    // A pivot anywhere in the workbook whose records live on the target sheet
    // has to follow them. Miss this and the pivot still refreshes — over a
    // rectangle that now starts on the header row, or ends one record short.
    for other in workbook.sheets.iter_mut() {
        for pivot in other.pivots.iter_mut() {
            if pivot.source_sheet != target_id {
                continue;
            }
            match kind {
                ShiftKind::Insert => {
                    insert_coord(axis, &mut pivot.source.start, at, count);
                    insert_coord(axis, &mut pivot.source.end, at, count);
                }
                ShiftKind::Delete => {
                    let lo = axis.coord(pivot.source.start);
                    let hi = axis.coord(pivot.source.end);
                    // A source deleted out of existence leaves the definition
                    // in place with an empty rectangle rather than silently
                    // removing a pivot the user still sees on the sheet.
                    if let Some((new_lo, new_hi)) = map_range_delete(lo, hi, at, count) {
                        pivot.source.start = axis.with_coord(pivot.source.start, new_lo);
                        pivot.source.end = axis.with_coord(pivot.source.end, new_hi);
                    }
                }
            }
        }
    }
}

/// Shift a **pending metadata bundle's** chart series and pivot sources.
///
/// [`shift_metadata_insert`] and [`shift_metadata_delete`] move everything that
/// is a position — including a chart's frame and a pivot's report block — from
/// a lone `&mut impl Positional`. Two things need more than that: a chart's
/// series is a reference *string*, so deciding whether `Sheet1!$D$2` names the
/// sheet being shifted needs that sheet's **name**; and a pivot's `source` is
/// on `source_sheet`, so deciding whether it moves needs its **id**.
///
/// The transform is a pure function over operations and an operation carries
/// only a sheet *index*, which is why this was left undone by `FID-26` and
/// filed as `FID-28`. It does not need the wire to carry identity on every
/// operation, though — the transform's callers hold the workbook, so they pass
/// what the index means (`target_name`, `target_id`) and the shift happens
/// here, next to the one `apply` performs.
pub(crate) struct BundleShift<'a> {
    /// The sheet the structural operation ran on — what a qualified series
    /// reference must name to be moved.
    pub target_name: &'a str,
    /// That sheet's identity, which is what a pivot's `source_sheet` is
    /// compared against.
    pub target_id: casual_calc_model::SheetId,
    /// The sheet the bundle itself belongs to, which resolves an *unqualified*
    /// reference.
    pub home_name: &'a str,
    pub axis: Axis,
    pub inserting: bool,
    pub at: u32,
    pub count: u32,
}

pub(crate) fn shift_bundle_references(data: &mut crate::SheetMetadata, shift: &BundleShift<'_>) {
    let BundleShift {
        target_name,
        target_id,
        home_name,
        axis,
        inserting,
        at,
        count,
    } = *shift;
    let kind = if inserting {
        ShiftKind::Insert
    } else {
        ShiftKind::Delete
    };
    let ctx = RewriteCtx {
        target: target_name,
        home: home_name,
        axis,
        kind,
        at,
        count,
    };
    for chart in data.charts.iter_mut() {
        for series in chart.series.iter_mut() {
            if let Some(shifted) = shift_reference_text(&series.values, &ctx) {
                series.values = shifted;
            }
            if let Some(text) = series.categories.as_ref()
                && let Some(shifted) = shift_reference_text(text, &ctx)
            {
                series.categories = Some(shifted);
            }
        }
    }
    for pivot in data.pivots.iter_mut() {
        if pivot.source_sheet != target_id {
            continue;
        }
        match kind {
            ShiftKind::Insert => {
                insert_coord(axis, &mut pivot.source.start, at, count);
                insert_coord(axis, &mut pivot.source.end, at, count);
            }
            ShiftKind::Delete => {
                let lo = axis.coord(pivot.source.start);
                let hi = axis.coord(pivot.source.end);
                if let Some((new_lo, new_hi)) = map_range_delete(lo, hi, at, count) {
                    pivot.source.start = axis.with_coord(pivot.source.start, new_lo);
                    pivot.source.end = axis.with_coord(pivot.source.end, new_hi);
                }
            }
        }
    }
}

/// Shift the reference strings naming each chart series' categories and values.
///
/// A series is stored as the text OOXML uses — `Sheet1!$D$2:$D$11` — not as a
/// range, so it cannot be nudged like an anchor: it is parsed, put through the
/// same [`rewrite_expr`] every cell formula and defined name goes through, and
/// printed back. Text that will not parse is left exactly as it was, on the
/// same rule [`Expr::Raw`] follows: a reference that cannot be understood
/// cannot be shifted without guessing, and a guess here corrupts a chart.
///
/// A chart read from a file also has a retained part holding these same
/// references, and that part is what the writer emits. Shifting the model
/// therefore fixes what is drawn on screen but not yet what is saved — see
/// `FID-27`, which is that half.
fn rewrite_chart_series(
    workbook: &mut Workbook,
    target: &str,
    axis: Axis,
    kind: ShiftKind,
    at: u32,
    count: u32,
) {
    for sheet in workbook.sheets.iter_mut() {
        let home = sheet.name.clone();
        let ctx = RewriteCtx {
            target,
            home: &home,
            axis,
            kind,
            at,
            count,
        };
        for chart in sheet.charts.iter_mut() {
            for series in chart.series.iter_mut() {
                if let Some(shifted) = shift_reference_text(&series.values, &ctx) {
                    series.values = shifted;
                }
                if let Some(text) = series.categories.as_ref()
                    && let Some(shifted) = shift_reference_text(text, &ctx)
                {
                    series.categories = Some(shifted);
                }
            }
        }
    }
}

/// Parse `text` as a reference, shift it, and print it back — `None` when it
/// does not parse or the shift leaves it unchanged, so an untouched series
/// keeps its original spelling rather than being re-emitted in ours.
fn shift_reference_text(text: &str, ctx: &RewriteCtx) -> Option<String> {
    let expr = casual_calc_formula::parse(text).ok()?;
    let rewritten = rewrite_expr(&expr, ctx);
    (rewritten != expr).then(|| rewritten.to_string())
}

/// Recursively rewrite references within an expression.
fn rewrite_expr(expr: &Expr, ctx: &RewriteCtx) -> Expr {
    match expr {
        // A structured reference names columns, not addresses, so inserting or
        // deleting rows leaves it alone — that insulation is the point of using
        // one. Excel likewise never rewrites `Sales[Amount]`.
        Expr::StructuredRef { .. } => expr.clone(),
        // Unparsed text cannot be rewritten without understanding it, and
        // guessing at its references would corrupt what was preserved.
        Expr::Raw(_) | Expr::Empty => expr.clone(),
        Expr::Reference(reference) => rewrite_reference(reference, ctx),
        Expr::Range(a, b) => rewrite_range(a, b, ctx),
        Expr::Unary { op, operand } => Expr::Unary {
            op: *op,
            operand: Box::new(rewrite_expr(operand, ctx)),
        },
        Expr::Binary { op, left, right } => Expr::Binary {
            op: *op,
            left: Box::new(rewrite_expr(left, ctx)),
            right: Box::new(rewrite_expr(right, ctx)),
        },
        Expr::Function { name, args } => Expr::Function {
            name: name.clone(),
            args: args.iter().map(|arg| rewrite_expr(arg, ctx)).collect(),
        },
        // Both halves hold references that move with an insert or delete: the
        // callee may be a LAMBDA body full of them.
        Expr::Call { callee, args } => Expr::Call {
            callee: Box::new(rewrite_expr(callee, ctx)),
            args: args.iter().map(|arg| rewrite_expr(arg, ctx)).collect(),
        },
        // Literals and defined names carry no cell coordinates.
        Expr::Number(_) | Expr::Bool(_) | Expr::Text(_) | Expr::Error(_) | Expr::Name(_) => {
            expr.clone()
        }
    }
}

/// Whether a reference targets the operation's sheet: qualified with its name,
/// or unqualified inside a formula whose home sheet is the target.
fn targets(reference: &StoredRef, ctx: &RewriteCtx) -> bool {
    match &reference.sheet {
        Some(name) => name == ctx.target,
        None => ctx.home == ctx.target,
    }
}

/// The `#REF!` error expression a collapsed reference becomes.
fn ref_error() -> Expr {
    Expr::Error("#REF!".to_owned())
}

/// Rewrite a single (non-range) cell reference.
fn rewrite_reference(reference: &StoredRef, ctx: &RewriteCtx) -> Expr {
    if !targets(reference, ctx) {
        return Expr::Reference(reference.clone());
    }
    let coord = ctx.axis.ref_coord(reference);
    match ctx.kind {
        ShiftKind::Insert => {
            let mut shifted = reference.clone();
            if coord >= ctx.at {
                ctx.axis
                    .set_ref_coord(&mut shifted, coord.saturating_add(ctx.count));
            }
            Expr::Reference(shifted)
        }
        ShiftKind::Delete => {
            let end = ctx.at.saturating_add(ctx.count);
            if coord >= ctx.at && coord < end {
                // The referenced line was deleted.
                ref_error()
            } else if coord >= end {
                let mut shifted = reference.clone();
                ctx.axis.set_ref_coord(&mut shifted, coord - ctx.count);
                Expr::Reference(shifted)
            } else {
                Expr::Reference(reference.clone())
            }
        }
    }
}

/// Rewrite a range. Targeting is decided from the first endpoint (the qualifier
/// on a sheet-qualified range); both endpoints then move together.
fn rewrite_range(a: &StoredRef, b: &StoredRef, ctx: &RewriteCtx) -> Expr {
    if !targets(a, ctx) {
        return Expr::Range(a.clone(), b.clone());
    }
    // A whole-column range already covers every row, so inserting or deleting
    // rows cannot change it — and shifting its placeholder bound would turn
    // `A:A` into a range that no longer starts at row 1. The same holds for a
    // whole-row range against column edits.
    let open_on_axis = match ctx.axis {
        Axis::Row => a.row_implicit || b.row_implicit,
        Axis::Col => a.col_implicit || b.col_implicit,
    };
    if open_on_axis {
        return Expr::Range(a.clone(), b.clone());
    }
    match ctx.kind {
        ShiftKind::Insert => {
            Expr::Range(shift_endpoint_insert(a, ctx), shift_endpoint_insert(b, ctx))
        }
        ShiftKind::Delete => rewrite_range_delete(a, b, ctx),
    }
}

/// Shift one range endpoint for an insert: bump its along-axis coordinate if it
/// is on or after the insertion point.
fn shift_endpoint_insert(endpoint: &StoredRef, ctx: &RewriteCtx) -> StoredRef {
    let mut shifted = endpoint.clone();
    let coord = ctx.axis.ref_coord(endpoint);
    if coord >= ctx.at {
        ctx.axis
            .set_ref_coord(&mut shifted, coord.saturating_add(ctx.count));
    }
    shifted
}

/// Rewrite a range for a delete, clamping endpoints that fall in the deleted
/// band and collapsing to `#REF!` when nothing survives.
fn rewrite_range_delete(a: &StoredRef, b: &StoredRef, ctx: &RewriteCtx) -> Expr {
    let ca = ctx.axis.ref_coord(a);
    let cb = ctx.axis.ref_coord(b);
    let (lo, hi, a_is_lo) = if ca <= cb {
        (ca, cb, true)
    } else {
        (cb, ca, false)
    };
    match map_range_delete(lo, hi, ctx.at, ctx.count) {
        None => ref_error(),
        Some((new_lo, new_hi)) => {
            let mut na = a.clone();
            let mut nb = b.clone();
            let (lo_ref, hi_ref) = if a_is_lo {
                (&mut na, &mut nb)
            } else {
                (&mut nb, &mut na)
            };
            ctx.axis.set_ref_coord(lo_ref, new_lo);
            ctx.axis.set_ref_coord(hi_ref, new_hi);
            Expr::Range(na, nb)
        }
    }
}

/// Map a normalized `[lo, hi]` coordinate span through a delete of the band
/// `[at, at+count)`, returning the surviving span or `None` for `#REF!`.
///
/// A deleted low endpoint clamps up to `at` (the line the band collapses onto);
/// a deleted high endpoint clamps down to `at-1` (the last kept line before the
/// band). Kept endpoints past the band shift back by `count`. If the whole span
/// is inside the band — or the mapping would invert — nothing survives.
fn map_range_delete(lo: u32, hi: u32, at: u32, count: u32) -> Option<(u32, u32)> {
    let end = at.saturating_add(count);
    let lo_deleted = lo >= at && lo < end;
    let hi_deleted = hi >= at && hi < end;
    if lo_deleted && hi_deleted {
        // The entire span lies within the deleted band (covers `at == 0` too).
        return None;
    }
    let new_lo = if lo < at {
        lo
    } else if lo_deleted {
        at
    } else {
        lo - count
    };
    let new_hi = if hi < at {
        hi
    } else if hi_deleted {
        // `hi_deleted` with `lo` kept implies `lo < at`, hence `at >= 1`.
        at - 1
    } else {
        hi - count
    };
    if new_lo > new_hi {
        return None;
    }
    Some((new_lo, new_hi))
}

/// Formulas that must be repointed because the cells they name were **moved**.
///
/// Cutting a block and pasting it elsewhere moves those cells. Excel then
/// rewrites every *other* formula that pointed at them so it follows — `=A1*2`
/// becomes `=C3*2` when `A1` is cut to `C3`. Only the moved cells' own formulas
/// were handled here (correctly: a cut travels verbatim, because the cell did
/// not change what it means, only where it lives). Everything pointing *at*
/// them kept its old address and silently began reading whatever moved in
/// underneath (`UX-CUT-03`).
///
/// Returned rather than applied, so the caller can fold these into the same
/// operation batch as the paste — one undo step, as a move should be.
///
/// # A range that only partly overlaps the block is left alone
///
/// `=SUM(A1:A10)` when `A1:A5` is cut has no correct rewrite: moving one
/// endpoint changes which cells the range covers, and leaving it changes what
/// it reads. Excel keeps the range and lets it read whatever is there now,
/// which at least preserves the shape the author wrote. Silently resizing
/// somebody's range would be the worse of the two.
#[must_use]
pub fn repointed_after_move(
    workbook: &Workbook,
    moved_sheet: &str,
    block: (u32, u32, u32, u32),
    delta: (i64, i64),
) -> Vec<(usize, CellRef, Expr)> {
    let mut out = Vec::new();
    for (index, sheet) in workbook.sheets.iter().enumerate() {
        for (at, cell) in sheet.cells.iter() {
            let Some(handle) = cell.formula else { continue };
            let Some(expr) = workbook.formula(handle) else {
                continue;
            };
            // **In and out of the absolute form**, as the structural rewrite
            // does: "does this reference name a cell inside the moved block"
            // is a question about addresses, and a stored tree holds offsets.
            let origin = Origin::at(at.row, at.col);
            let absolute = restore_at(expr, origin, ABSOLUTE);
            let moved = restore_at(
                &move_expr(&absolute, moved_sheet, &sheet.name, block, delta),
                ABSOLUTE,
                origin,
            );
            if moved != *expr {
                out.push((index, at, moved));
            }
        }
    }
    out
}

fn move_expr(
    expr: &Expr,
    moved_sheet: &str,
    home: &str,
    block: (u32, u32, u32, u32),
    delta: (i64, i64),
) -> Expr {
    match expr {
        // Same reasoning as the insert/delete rewrite: a structured reference
        // names columns rather than addresses, and unparsed text cannot be
        // rewritten without understanding it.
        Expr::StructuredRef { .. } | Expr::Raw(_) | Expr::Empty => expr.clone(),
        Expr::Reference(reference) => {
            match moved_reference(reference, moved_sheet, home, block, delta) {
                Some(moved) => Expr::Reference(moved),
                None => expr.clone(),
            }
        }
        // Both endpoints, or neither. See the note above.
        Expr::Range(a, b) => {
            let (Some(a2), Some(b2)) = (
                moved_reference(a, moved_sheet, home, block, delta),
                moved_reference(b, moved_sheet, home, block, delta),
            ) else {
                return expr.clone();
            };
            Expr::Range(a2, b2)
        }
        Expr::Unary { op, operand } => Expr::Unary {
            op: *op,
            operand: Box::new(move_expr(operand, moved_sheet, home, block, delta)),
        },
        Expr::Binary { op, left, right } => Expr::Binary {
            op: *op,
            left: Box::new(move_expr(left, moved_sheet, home, block, delta)),
            right: Box::new(move_expr(right, moved_sheet, home, block, delta)),
        },
        Expr::Function { name, args } => Expr::Function {
            name: name.clone(),
            args: args
                .iter()
                .map(|a| move_expr(a, moved_sheet, home, block, delta))
                .collect(),
        },
        Expr::Call { callee, args } => Expr::Call {
            callee: Box::new(move_expr(callee, moved_sheet, home, block, delta)),
            args: args
                .iter()
                .map(|a| move_expr(a, moved_sheet, home, block, delta))
                .collect(),
        },
        _ => expr.clone(),
    }
}

/// The reference's new address, if it named a cell inside the moved block.
///
/// A reference reaches the moved sheet either by naming it or by being
/// unqualified in a formula that lives there — the same rule the insert and
/// delete rewrite uses, so the two cannot disagree about what "this sheet"
/// means.
fn moved_reference(
    reference: &StoredRef,
    moved_sheet: &str,
    home: &str,
    (r0, c0, r1, c1): (u32, u32, u32, u32),
    (dr, dc): (i64, i64),
) -> Option<StoredRef> {
    let reaches = match reference.sheet.as_deref() {
        Some(named) => named == moved_sheet,
        None => home == moved_sheet,
    };
    if !reaches {
        return None;
    }
    // A whole-row or whole-column reference names no single cell, so it cannot
    // be inside a block.
    if reference.row_implicit || reference.col_implicit {
        return None;
    }
    // Compared as addresses: this is reached with a tree in the absolute form,
    // where a stored reference's offset from `(0, 0)` is the address.
    let (rrow, rcol) = (reference.row, reference.col);
    if rrow < i64::from(r0) || rrow > i64::from(r1) || rcol < i64::from(c0) || rcol > i64::from(c1)
    {
        return None;
    }
    // **`$` anchoring does not exempt it.** An absolute reference is about what
    // a *copy* does to it, not about whether the cell it names may move: `$A$1`
    // still means A1, and if A1 has gone to C3 then it means C3 now. Excel
    // moves both.
    let mut moved = reference.clone();
    moved.row = i64::from(u32::try_from(rrow + dr).ok()?);
    moved.col = i64::from(u32::try_from(rcol + dc).ok()?);
    Some(moved)
}

/// Defined names that must be repointed because the cells they name were moved.
///
/// The sibling of [`repointed_after_move`] for names rather than formulas.
/// `FID-24` made an insert or a delete shift defined names; a **cut** left them
/// behind, so `Rate` went on meaning `$A$1` after `$A$1` had gone to `G6` and
/// every formula using the name silently read the wrong cell. A name is the
/// indirection people reach for precisely so they do not have to track
/// addresses, which makes it the worst place for an address to go stale.
///
/// Returns the whole list when anything changed, because that is the shape
/// `Operation::SetDefinedNames` takes and inverts.
#[must_use]
pub fn defined_names_after_move(
    workbook: &Workbook,
    moved_sheet: &str,
    block: (u32, u32, u32, u32),
    delta: (i64, i64),
) -> Option<Vec<DefinedName>> {
    let mut names = workbook.defined_names.clone();
    let mut changed = false;
    for name in &mut names {
        // A sheet-scoped name's unqualified references mean its own sheet; a
        // workbook-scoped one has no home, so only an explicitly qualified
        // reference can reach the moved sheet. Same rule as the insert/delete
        // rewrite, deliberately.
        let home = name
            .sheet
            .and_then(|id| workbook.sheets.iter().find(|s| s.id == id))
            .map(|s| s.name.as_str())
            .unwrap_or_default();
        let moved = move_expr(&name.formula, moved_sheet, home, block, delta);
        if moved != name.formula {
            name.formula = moved;
            changed = true;
        }
    }
    changed.then_some(names)
}
