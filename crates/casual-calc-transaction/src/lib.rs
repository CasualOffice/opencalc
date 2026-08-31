//! `casual-calc-transaction` — atomic, invertible edit operations.
//!
//! Increment 1 (the Edit dimension): the closed cell-level operation set. Every
//! [`apply`] returns the **inverse** operation, so undo/redo is inverse replay
//! and never a separate implementation that can drift. All mutation of the model
//! flows through here — the transaction contract in
//! `docs/24-TRANSACTION-AND-EDIT-SEMANTICS.md`.
//!
//! Covered now: set a cell's value / style, set or clear a whole cell, and an
//! atomic [`Operation::Batch`]. Structural ops (insert/delete rows & columns with
//! formula-reference rewriting) are the next increment.

use std::collections::{BTreeMap, BTreeSet};

use casual_calc_formula::{Expr, rename_sheet_references};
use casual_calc_model::{
    AutoFilter, AxisSizing, Cell, CellComment, CellRange, CellRef, CellValue, ChartView,
    ConditionalFormat, DataValidation, DefinedName, Hyperlink, PivotTable, PrintSetup,
    RetainedPart, RetainedRel, Sheet, SheetProtection, SheetView, SheetVisibility, SortState,
    StyleId, Table, Workbook,
};

pub mod protocol;
pub mod restore;
pub mod session;
#[cfg(test)]
mod session_tests;
mod structural;
pub mod transform;
#[cfg(test)]
mod transform_move_tests;
#[cfg(test)]
mod transform_tests;
pub mod version;
#[cfg(test)]
mod version_tests;
pub mod wire;

pub use structural::{Axis, defined_names_after_move, repointed_after_move};

/// An error applying an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TxnError {
    /// The target sheet does not exist.
    SheetNotFound {
        /// The sheet index.
        index: usize,
    },
}

impl TxnError {
    /// The stable diagnostic code (`docs/20`).
    pub fn code(&self) -> &'static str {
        "OC-TXN-0001"
    }
}

impl core::fmt::Display for TxnError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TxnError::SheetNotFound { index } => {
                write!(f, "[{}] sheet {index} not found", self.code())
            }
        }
    }
}

impl std::error::Error for TxnError {}

/// Everything on a sheet that is keyed by position rather than by cell: merges,
/// axis sizing, the hidden sets, the frozen bands, the autofilter, and the
/// outline.
///
/// Travelling as one bundle is what makes these undoable together — a delete
/// that drops a merge, a custom height and an outline level cannot recover them
/// by re-inserting an empty band, so its inverse carries a pre-mutation snapshot
/// of the lot. It also means adding another positional field is a change in one
/// place rather than at every construction site.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetMetadata {
    /// Merged ranges.
    pub merges: Vec<CellRange>,
    /// Column widths.
    pub columns: AxisSizing,
    /// Row heights.
    pub rows: AxisSizing,
    /// Hidden rows.
    pub hidden_rows: BTreeSet<u32>,
    /// Hidden columns.
    pub hidden_cols: BTreeSet<u32>,
    /// View state (frozen panes, zoom, gridline/header visibility).
    pub view: SheetView,
    /// The autofilter, or `None` when the sheet has none.
    pub auto_filter: Option<AutoFilter>,
    /// Rows the autofilter hides. Travels with the filter so undo restores the
    /// rules and the rows they hid together.
    ///
    /// **This set is indexed by row, so anything that reorders rows owes it an
    /// answer.** The structural operations give one: an
    /// insert, a delete and a move all reindex it under the same map they apply
    /// to the cells, so a hidden row stays hidden and stays attached to the data
    /// that made it hidden.
    ///
    /// A **sort is a row permutation that does not go through those operations**
    /// — it is composed elsewhere as a batch of [`Operation::SetCell`] plus one
    /// [`Operation::SetSheetMetadata`] — and it does not currently give that
    /// answer. `DATA-SORT-01` in `docs/14-EXECUTION-TRACKER.md` is **open**:
    /// sorting a filtered range moves the data and leaves this set naming the
    /// rows it named before, so the filter goes on hiding a row that now holds
    /// different values. This paragraph records where the invariant lives and
    /// that it is currently broken on that one path; it does not claim the
    /// engine keeps it.
    pub filter_hidden: BTreeSet<u32>,
    /// Outline nesting level per row.
    #[serde(deserialize_with = "casual_calc_model::int_keys::deserialize")]
    pub row_outline_levels: BTreeMap<u32, u8>,
    /// Outline nesting level per column.
    #[serde(deserialize_with = "casual_calc_model::int_keys::deserialize")]
    pub col_outline_levels: BTreeMap<u32, u8>,
    /// Rows whose outline group is collapsed.
    pub collapsed_rows: BTreeSet<u32>,
    /// Columns whose outline group is collapsed.
    pub collapsed_cols: BTreeSet<u32>,
    /// Data-validation rules.
    ///
    /// The five fields below are here because they had **no** reversible
    /// operation at all, so editing a validation, a conditional format, a
    /// comment, a sheet's visibility or its protection wrote straight to the
    /// workbook. Undo then reversed whatever preceded it — the last cell edit —
    /// which destroys work the user did not ask to lose. Folding them into the
    /// one bundle that already has a proven inverse fixes all five at once,
    /// rather than adding five operations that each need their own inverse to
    /// get right.
    ///
    /// This paragraph used to open by calling them "not positional", meaning
    /// only that undo was the reason they joined the bundle. Read as a claim
    /// about the grid it is false and was costly: a validation, a conditional
    /// format, a comment and a hyperlink each name cells, and for as long as
    /// the sentence stood nothing shifted them on an insert or a delete
    /// (FID-25). Positional is what the grid says, not what the field list is
    /// sorted by.
    pub validations: Vec<DataValidation>,
    /// Conditional-formatting rules.
    pub conditional_formats: Vec<ConditionalFormat>,
    /// Comment threads.
    pub comments: Vec<CellComment>,
    /// Tab visibility.
    pub visibility: SheetVisibility,
    /// Sheet protection.
    pub protection: Option<SheetProtection>,
    /// Hyperlinks.
    pub hyperlinks: Vec<Hyperlink>,
    /// Tables (ListObjects).
    pub tables: Vec<Table>,
    /// Charts anchored on the sheet.
    ///
    /// Here for the same reason as the pivots below: a chart is edited through
    /// the panel, and without a reversible operation those edits wrote straight
    /// to the workbook, so undo reversed whatever preceded them instead.
    pub charts: Vec<ChartView>,
    /// Pivot table definitions.
    ///
    /// The definition, not the report: the report is ordinary cells and travels
    /// as the cell operations beside this bundle, so one undo takes back both
    /// the layout change and the figures it produced.
    pub pivots: Vec<PivotTable>,
    /// Page setup, margins, header/footer and manual breaks.
    pub print: PrintSetup,
    /// The record of how a range was last sorted.
    pub sort_state: Option<SortState>,
}

/// Which fields of a [`SheetMetadata`] bundle an operation actually changes.
///
/// The bundle travels whole because that is what makes its inverse correct, but
/// "whole" and "changed" are different questions, and only the second one is
/// answerable by looking at the op. Recording it matters for two reasons.
///
/// **Undo stops over-reaching.** Installing twenty-three fields to change one
/// means undo puts twenty-three back, so it reverses edits the user never asked
/// it to.
///
/// **Concurrent edits stop destroying each other.** Under the operational
/// transform in [ADR-011](../../../docs/56-COLLABORATION-CONCURRENCY-DESIGN.md),
/// two ops that touch disjoint fields must merge rather than one overwriting
/// the other. Without a mask there is no way to tell disjoint from
/// conflicting: one person resizing a column and another adding a comment
/// produce two indistinguishable whole-sheet bundles, and the later one wins
/// entirely. That is silent data loss, which this project does not do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct SheetFields(u32);

impl SheetFields {
    /// No fields — an operation that changes nothing.
    pub const NONE: Self = Self(0);
    /// Every field, including any added later.
    pub const ALL: Self = Self(u32::MAX);

    /// Whether every field in `other` is present here.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Fields present in either.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Fields present in both — how an op's declared intent is narrowed to what
    /// it turned out to change.
    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Whether any two operations touch a common field, and so cannot simply be
    /// merged.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Whether this changes nothing.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Generates the field constants, the diff and the masked install from **one**
/// list, so the three cannot disagree.
///
/// Written as a macro rather than by hand for a specific reason: this bundle
/// has grown from six fields to twenty-three, and every growth added a line to
/// `capture` and a line to `install`. A mask adds two more places to forget,
/// and a field silently missing from the diff is an edit that never merges and
/// never conflicts — it just vanishes under concurrency. The list is written
/// once.
macro_rules! sheet_fields {
    ($($bit:expr, $konst:ident, $field:ident;)+) => {
        impl SheetFields {
            $(
                #[doc = concat!("The `", stringify!($field), "` field.")]
                pub const $konst: Self = Self(1 << $bit);
            )+
        }

        impl SheetMetadata {
            /// The fields in which this bundle differs from `base`.
            #[must_use]
            pub fn diff(&self, base: &Self) -> SheetFields {
                let mut changed = SheetFields::NONE;
                $(
                    if self.$field != base.$field {
                        changed = changed.union(SheetFields::$konst);
                    }
                )+
                changed
            }

            /// Install only the masked fields, returning the sheet's prior state.
            ///
            /// The returned bundle is a full snapshot so it remains a valid
            /// inverse on its own, but only the masked fields are meaningful —
            /// the inverse carries the same mask.
            pub fn install_masked(self, sheet: &mut Sheet, mask: SheetFields) -> Self {
                let previous = Self::capture(sheet);
                $(
                    if mask.contains(SheetFields::$konst) {
                        sheet.$field = self.$field;
                    }
                )+
                previous
            }
        }
    };
}

sheet_fields! {
    0,  MERGES,              merges;
    1,  COLUMNS,             columns;
    2,  ROWS,                rows;
    3,  HIDDEN_ROWS,         hidden_rows;
    4,  HIDDEN_COLS,         hidden_cols;
    5,  VIEW,                view;
    6,  AUTO_FILTER,         auto_filter;
    7,  FILTER_HIDDEN,       filter_hidden;
    8,  ROW_OUTLINE_LEVELS,  row_outline_levels;
    9,  COL_OUTLINE_LEVELS,  col_outline_levels;
    10, COLLAPSED_ROWS,      collapsed_rows;
    11, COLLAPSED_COLS,      collapsed_cols;
    12, VALIDATIONS,         validations;
    13, CONDITIONAL_FORMATS, conditional_formats;
    14, COMMENTS,            comments;
    15, VISIBILITY,          visibility;
    16, PROTECTION,          protection;
    17, HYPERLINKS,          hyperlinks;
    18, TABLES,              tables;
    19, CHARTS,              charts;
    20, PIVOTS,              pivots;
    21, PRINT,               print;
    22, SORT_STATE,          sort_state;
}

impl SheetMetadata {
    /// A snapshot of a sheet's positional metadata.
    pub fn capture(sheet: &Sheet) -> Self {
        Self {
            merges: sheet.merges.clone(),
            columns: sheet.columns.clone(),
            rows: sheet.rows.clone(),
            hidden_rows: sheet.hidden_rows.clone(),
            hidden_cols: sheet.hidden_cols.clone(),
            view: sheet.view,
            auto_filter: sheet.auto_filter.clone(),
            filter_hidden: sheet.filter_hidden.clone(),
            row_outline_levels: sheet.row_outline_levels.clone(),
            col_outline_levels: sheet.col_outline_levels.clone(),
            collapsed_rows: sheet.collapsed_rows.clone(),
            collapsed_cols: sheet.collapsed_cols.clone(),
            validations: sheet.validations.clone(),
            conditional_formats: sheet.conditional_formats.clone(),
            comments: sheet.comments.clone(),
            visibility: sheet.visibility,
            protection: sheet.protection.clone(),
            hyperlinks: sheet.hyperlinks.clone(),
            tables: sheet.tables.clone(),
            charts: sheet.charts.clone(),
            pivots: sheet.pivots.clone(),
            print: sheet.print.clone(),
            sort_state: sheet.sort_state.clone(),
        }
    }

    /// Install every field, returning what was there before — the exact
    /// inverse. Equivalent to [`Self::install_masked`] with
    /// [`SheetFields::ALL`].
    pub fn install(self, sheet: &mut Sheet) -> Self {
        Self {
            merges: std::mem::replace(&mut sheet.merges, self.merges),
            columns: std::mem::replace(&mut sheet.columns, self.columns),
            rows: std::mem::replace(&mut sheet.rows, self.rows),
            hidden_rows: std::mem::replace(&mut sheet.hidden_rows, self.hidden_rows),
            hidden_cols: std::mem::replace(&mut sheet.hidden_cols, self.hidden_cols),
            view: std::mem::replace(&mut sheet.view, self.view),
            auto_filter: std::mem::replace(&mut sheet.auto_filter, self.auto_filter),
            filter_hidden: std::mem::replace(&mut sheet.filter_hidden, self.filter_hidden),
            row_outline_levels: std::mem::replace(
                &mut sheet.row_outline_levels,
                self.row_outline_levels,
            ),
            col_outline_levels: std::mem::replace(
                &mut sheet.col_outline_levels,
                self.col_outline_levels,
            ),
            collapsed_rows: std::mem::replace(&mut sheet.collapsed_rows, self.collapsed_rows),
            collapsed_cols: std::mem::replace(&mut sheet.collapsed_cols, self.collapsed_cols),
            validations: std::mem::replace(&mut sheet.validations, self.validations),
            conditional_formats: std::mem::replace(
                &mut sheet.conditional_formats,
                self.conditional_formats,
            ),
            comments: std::mem::replace(&mut sheet.comments, self.comments),
            visibility: std::mem::replace(&mut sheet.visibility, self.visibility),
            protection: std::mem::replace(&mut sheet.protection, self.protection),
            hyperlinks: std::mem::replace(&mut sheet.hyperlinks, self.hyperlinks),
            tables: std::mem::replace(&mut sheet.tables, self.tables),
            charts: std::mem::replace(&mut sheet.charts, self.charts),
            pivots: std::mem::replace(&mut sheet.pivots, self.pivots),
            print: std::mem::replace(&mut sheet.print, self.print),
            sort_state: std::mem::replace(&mut sheet.sort_state, self.sort_state),
        }
    }
}

/// The retained bytes an inverse has to put back.
///
/// A chart imported from a file keeps its original XML as a retained part,
/// because the model does not describe a chart completely enough to rebuild it.
/// Removing the chart must remove that part — leaving it would put the deleted
/// chart's XML back into the saved file, with a relationship still reaching it.
///
/// Only an **inverse** ever carries these. The forward direction needs no bytes
/// at all: which parts died is derivable from the bundle, so every replica
/// works it out from the operation it already receives. That is what makes this
/// converge without a wire-format change — and it is why the field is
/// `skip_serializing_if` empty, so an ordinary edit serialises exactly as before.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetainedBytes {
    /// The parts themselves.
    pub parts: Vec<RetainedPart>,
    /// The relationships that reached them.
    pub rels: Vec<RetainedRel>,
}

impl RetainedBytes {
    /// Whether there is nothing to put back.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty() && self.rels.is_empty()
    }
}

/// Why an undo was refused, and what to say about it.
///
/// Carried rather than reduced to a string because the caller has to *name*
/// what stopped it — a refusal a user cannot act on is the failure mode this
/// policy exists to avoid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WouldDiscard {
    /// The sheet the band is on.
    pub sheet: usize,
    /// Whether the band is rows or columns.
    pub axis: Axis,
    /// The first line of the band.
    pub at: u32,
    /// How many lines.
    pub count: u32,
    /// A cell inside it that holds work, for the message.
    pub occupied: CellRef,
    /// How many cells in the band hold work.
    pub cells: usize,
}

/// Whether undoing `op` would delete somebody else's work (docs/69).
///
/// **Only an inverse that deletes a band can do this.** Undoing a *delete*
/// re-inserts one and is additive; undoing an *insert* removes rows that were
/// created empty, and anything in them now arrived afterwards.
///
/// That is also what makes the check need no authorship, which the model does
/// not carry. The stack is last-in-first-out: by the time an insert is the
/// operation being undone, everything this session did after it has already
/// been undone, so a cell still standing in the band was written by somebody
/// else. Coarse in the direction the policy asks for — it refuses a little more
/// often than strictly necessary, and every extra refusal is a case where the
/// user is told to look rather than one where data disappears.
#[must_use]
pub fn undo_would_discard(workbook: &Workbook, op: &Operation) -> Option<WouldDiscard> {
    let (sheet, axis, at, count) = match op {
        Operation::DeleteRows { sheet, at, count } => (*sheet, Axis::Row, *at, *count),
        Operation::DeleteColumns { sheet, at, count } => (*sheet, Axis::Col, *at, *count),
        // A batch is exactly its members; the first blocked one blocks it.
        Operation::Batch(ops) => {
            return ops.iter().find_map(|op| undo_would_discard(workbook, op));
        }
        _ => return None,
    };
    if count == 0 {
        return None;
    }
    let target = workbook.sheets.get(sheet)?;
    let end = at.saturating_add(count);
    let inside = |at_ref: CellRef| match axis {
        Axis::Row => at_ref.row >= at && at_ref.row < end,
        Axis::Col => at_ref.col >= at && at_ref.col < end,
    };

    let mut found: Option<CellRef> = None;
    let mut cells = 0usize;
    for (at_ref, cell) in target.cells.iter() {
        if inside(at_ref) && !cell.is_blank() {
            cells += 1;
            // The topmost-leftmost, so the message names a stable cell rather
            // than whichever the store happened to yield first.
            if found.is_none_or(|best| (at_ref.row, at_ref.col) < (best.row, best.col)) {
                found = Some(at_ref);
            }
        }
    }
    found.map(|occupied| WouldDiscard {
        sheet,
        axis,
        at,
        count,
        occupied,
        cells,
    })
}

/// Remove the retained parts at `paths`, and the relationships reaching them.
///
/// Returns what was taken, so the inverse can put it back. A relationship is
/// matched by the file its target names: the target is relative to the part
/// that declares it (`../charts/chart1.xml` from a drawing), so comparing whole
/// paths would match nothing.
fn take_retained(workbook: &mut Workbook, paths: &BTreeSet<String>) -> RetainedBytes {
    if paths.is_empty() {
        return RetainedBytes::default();
    }
    let files: BTreeSet<&str> = paths
        .iter()
        .map(|p| p.rsplit('/').next().unwrap_or(p.as_str()))
        .collect();

    let mut taken = RetainedBytes::default();
    let mut keep = Vec::with_capacity(workbook.retained_parts.len());
    for part in std::mem::take(&mut workbook.retained_parts) {
        if paths.contains(&part.path) {
            taken.parts.push(part);
        } else {
            keep.push(part);
        }
    }
    workbook.retained_parts = keep;

    let mut keep = Vec::with_capacity(workbook.retained_rels.len());
    for rel in std::mem::take(&mut workbook.retained_rels) {
        let names = rel
            .target
            .rsplit('/')
            .next()
            .is_some_and(|file| files.contains(file));
        if names {
            taken.rels.push(rel);
        } else {
            keep.push(rel);
        }
    }
    workbook.retained_rels = keep;
    taken
}

/// The atomic edit operations, **closed at a given `PROTOCOL_VERSION`** rather
/// than closed for all time (`ADR-024`).
///
/// This said "a closed set" until `DOC-039` went looking, and the history says
/// otherwise: `MoveColumns`, `MoveRows` and `MoveRange` joined on 2026-08-29,
/// three weeks after the rest and long after Phase 5 shipped on top of it. The
/// weaker claim is the true one and is also the one operational transform needs
/// — totality *per version*, never immutability across time.
///
/// Adding a variant is therefore a **hard** wire break: an old client cannot
/// read the tag, so it is refused rather than misled. That is the opposite of an
/// additive field on `SheetMetadata`, which is the *quiet* break — a new variant
/// announces itself, a new field does not.
///
/// Two obligations come with a new variant, both enforced rather than asserted.
/// It must be invertible: the inverse of any operation is expressible as a
/// `SetCell` (or a `Batch` of them), which is what lets an undo travel as an
/// ordinary edit. And it must arrive with its transform row — `transform_tests`
/// pins the complete refusal surface, so a pair that starts refusing fails a
/// test that names it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Operation {
    /// Replace a whole cell (or clear it with `None`). This is the primitive and
    /// the universal inverse form.
    SetCell {
        /// Sheet index.
        sheet: usize,
        /// Cell address.
        at: CellRef,
        /// The new cell, or `None` to clear.
        cell: Option<Cell>,
    },
    /// Set a cell's value, preserving its style and clearing any formula.
    SetValue {
        /// Sheet index.
        sheet: usize,
        /// Cell address.
        at: CellRef,
        /// The new value.
        value: CellValue,
    },
    /// Set (or clear) a cell's style, preserving its value and formula.
    SetStyle {
        /// Sheet index.
        sheet: usize,
        /// Cell address.
        at: CellRef,
        /// The new style, or `None` for the default.
        style: Option<StyleId>,
    },
    /// Clear a cell entirely.
    ClearCell {
        /// Sheet index.
        sheet: usize,
        /// Cell address.
        at: CellRef,
    },
    /// Set (or clear, with `None`) a column's explicit width in twips.
    SetColumnWidth {
        /// Sheet index.
        sheet: usize,
        /// Zero-based column.
        col: u32,
        /// The new width (twips), or `None` to revert to the sheet default.
        width: Option<i64>,
    },
    /// Set (or clear, with `None`) a row's explicit height in twips.
    SetRowHeight {
        /// Sheet index.
        sheet: usize,
        /// Zero-based row.
        row: u32,
        /// The new height (twips), or `None` to revert to the sheet default.
        height: Option<i64>,
    },
    /// Insert `count` blank rows at row `at`, shifting rows on/after `at` down
    /// and rewriting formula references that target this sheet.
    InsertRows {
        /// Sheet index.
        sheet: usize,
        /// Zero-based row the inserted band begins at.
        at: u32,
        /// Number of rows to insert.
        count: u32,
    },
    /// Delete `count` rows starting at row `at`, shifting rows on/after
    /// `at + count` up and rewriting formula references that target this sheet
    /// (references onto a deleted row become `#REF!`).
    DeleteRows {
        /// Sheet index.
        sheet: usize,
        /// Zero-based row the deleted band begins at.
        at: u32,
        /// Number of rows to delete.
        count: u32,
    },
    /// Insert `count` blank columns at column `at`, shifting columns on/after
    /// `at` right and rewriting formula references that target this sheet.
    InsertColumns {
        /// Sheet index.
        sheet: usize,
        /// Zero-based column the inserted band begins at.
        at: u32,
        /// Number of columns to insert.
        count: u32,
    },
    /// Delete `count` columns starting at column `at`, shifting columns on/after
    /// `at + count` left and rewriting formula references that target this sheet
    /// (references onto a deleted column become `#REF!`).
    DeleteColumns {
        /// Sheet index.
        sheet: usize,
        /// Zero-based column the deleted band begins at.
        at: u32,
        /// Number of columns to delete.
        count: u32,
    },
    /// Move the columns `[at, at + count)` so they sit immediately before
    /// column `before` — Excel's cut-and-insert, which is what dragging a
    /// column header does. **Not a swap**: the columns between the band and its
    /// destination close up behind it and re-open in front of it.
    ///
    /// `before` is in **pre-move** coordinates, because that is what the host
    /// has: the drop indicator sits between two columns the user can currently
    /// see. `before` inside the band, or at either of its edges, is a drop onto
    /// itself and changes nothing.
    ///
    /// # What follows the move, and what does not
    ///
    /// A line move is a **permutation** of the axis: nothing is created,
    /// nothing is destroyed, and no reference it rewrites ever becomes
    /// `#REF!`. Everything below is mapped by that permutation, with ranges
    /// following the rule in `map_span_move`.
    ///
    /// **Follows:** cell values, styles and formulas; every formula reference
    /// in the workbook, including cross-sheet and `$`-anchored ones; defined
    /// names; chart series reference strings; column widths / row heights; the
    /// hidden sets; outline levels and collapse flags; the rows an autofilter
    /// hides; merges; data validations; conditional formats; hyperlinks;
    /// comments; the autofilter's range; tables (range, filter range, and the
    /// **column list** when the move is a reorder within the table); chart
    /// frames; pivot anchors, report blocks and sources, including pivots on
    /// other sheets; image anchors.
    ///
    /// **Does not follow:** the frozen-pane counts, deliberately — they count
    /// pinned lines rather than naming them, and reordering does not change how
    /// many are pinned. A table whose column *membership* changes (a column
    /// dragged out of it, or a foreign one dropped in) keeps its old column
    /// list, since inventing or discarding a `TableColumn` is a decision this
    /// pass should not make silently. `Expr::StructuredRef` is never rewritten,
    /// as with an insert or a delete: it names columns rather than addresses.
    /// And there is **no operational transform** for this operation — see
    /// [`transform`], which refuses rather than guessing.
    MoveColumns {
        /// Sheet index.
        sheet: usize,
        /// Zero-based first column of the moving band.
        at: u32,
        /// How many columns move.
        count: u32,
        /// The column the band lands in front of, in pre-move coordinates.
        before: u32,
    },
    /// [`Operation::MoveColumns`] on the other axis — dragging a row header.
    MoveRows {
        /// Sheet index.
        sheet: usize,
        /// Zero-based first row of the moving band.
        at: u32,
        /// How many rows move.
        count: u32,
        /// The row the band lands above, in pre-move coordinates.
        before: u32,
    },
    /// Move the rectangle `from` so its top-left corner lands on `to`, leaving
    /// the source empty — dragging a selection's border.
    ///
    /// The destination rectangle is **overwritten**, including the parts of it
    /// the source had nothing in: a move carries the whole block, blanks
    /// included, which is what makes it a move rather than a merge of two
    /// blocks. A destination that would run off the grid changes nothing —
    /// clamping would silently drop the block's far edge.
    ///
    /// # What follows the move, and what does not
    ///
    /// **Follows:** the block's cells, whose formulas travel *verbatim* (a cut
    /// does not change what a cell means, only where it lives); every formula
    /// elsewhere in the workbook that named a moved cell, and every defined
    /// name that did — the same [`repointed_after_move`] /
    /// [`defined_names_after_move`] pair the clipboard's cut uses, so a drag
    /// and a cut cannot disagree; merges wholly inside the block, with any
    /// merge under the destination destroyed as a paste destroys it;
    /// validations, conditional formats, hyperlinks and comments wholly inside
    /// the block.
    ///
    /// **Does not follow:** tables, autofilters, charts, pivots and images,
    /// none of which is moved by dragging cells out from under it; row heights
    /// and column widths, which belong to the lines rather than to the block,
    /// as in Excel. A validation, format or link that only *partly* overlaps
    /// the block stays where it is — half of what it describes is leaving, and
    /// splitting one is a bigger decision than a drag should make. Moving a
    /// block to **another sheet** is not expressible here (one `sheet` index);
    /// the clipboard's cut/paste covers that. And there is **no operational
    /// transform** — see [`transform`].
    MoveRange {
        /// Sheet index.
        sheet: usize,
        /// The rectangle to lift.
        from: CellRange,
        /// Where its top-left corner lands.
        to: CellRef,
    },
    /// Replace a sheet's position-indexed metadata wholesale: merged ranges,
    /// column widths, row heights, hidden row/column sets, and frozen-pane
    /// counts. This is the universal inverse form for the metadata half of a
    /// structural insert/delete — a delete that drops merges, sizing, hidden
    /// lines, or freeze bands cannot recover them by re-inserting an empty band,
    /// so its inverse carries a pre-mutation snapshot and this op restores it.
    SetSheetMetadata {
        /// Sheet index.
        sheet: usize,
        /// The bundle to install. Boxed: it is by far the largest payload here,
        /// and an unboxed variant would pad every `SetCell` on the undo stack up
        /// to its size.
        data: Box<SheetMetadata>,
        /// Which fields to install. Callers may pass [`SheetFields::ALL`] and
        /// let [`apply`] narrow it to what actually differs; the inverse always
        /// carries the narrowed set, so undo touches only what the op touched.
        changed: SheetFields,
        /// Retained bytes to put back with this bundle.
        ///
        /// Empty on every forward edit, and populated by [`apply`] only on the
        /// inverse of one that removed an imported chart — see [`RetainedBytes`]
        /// for why the forward direction needs nothing.
        #[serde(default, skip_serializing_if = "RetainedBytes::is_empty")]
        restore: RetainedBytes,
    },
    /// Insert a fully-formed sheet at position `index`, shifting later sheets
    /// right. The caller assigns the sheet's id and name; the inverse removes it.
    /// `index` is clamped to the end, so appending is `index == sheets.len()`.
    InsertSheet {
        /// Position to insert at (clamped to the current sheet count).
        index: usize,
        /// The sheet to insert.
        sheet: Box<Sheet>,
    },
    /// Remove the sheet at `index`. The inverse re-inserts the removed sheet at
    /// the same position, so a delete is fully recoverable.
    RemoveSheet {
        /// Position of the sheet to remove.
        index: usize,
    },
    /// Rename the sheet at `index`. The inverse restores the prior name.
    RenameSheet {
        /// Position of the sheet to rename.
        index: usize,
        /// The new name.
        name: String,
    },
    /// Move the sheet at `from` to position `to` (tab reorder). The inverse
    /// moves it back.
    MoveSheet {
        /// Current position.
        from: usize,
        /// Destination position.
        to: usize,
    },
    /// Set (or clear, with `None`) a sheet's tab color. The inverse restores the
    /// prior color.
    SetTabColor {
        /// Sheet index.
        sheet: usize,
        /// The new tab color (`RRGGBB`), or `None` to clear.
        color: Option<String>,
    },
    /// Replace the workbook's whole defined-name table wholesale. The
    /// universal inverse form for defining, renaming, or deleting a name —
    /// each swaps in the new list and carries the prior list back as its own
    /// inverse, mirroring [`Operation::SetSheetMetadata`].
    SetDefinedNames(Vec<DefinedName>),
    /// A group applied atomically, with a single combined inverse.
    Batch(Vec<Operation>),
}

impl Operation {
    /// A metadata change whose extent [`apply`] works out for itself.
    ///
    /// The ergonomic path is "capture the sheet, change one field, submit", and
    /// the caller genuinely does not know which field it changed by the time it
    /// submits — it handed a mutable bundle to whatever edited it. Declaring
    /// `ALL` and letting `apply` narrow to the real difference is therefore not
    /// a shortcut; it is the only place with both the old and new state in hand.
    #[must_use]
    pub fn set_sheet_metadata(sheet: usize, data: SheetMetadata) -> Self {
        Self::SetSheetMetadata {
            sheet,
            data: Box::new(data),
            changed: SheetFields::ALL,
            // A forward edit never carries bytes: `apply` derives which retained
            // parts died from the bundle itself.
            restore: RetainedBytes::default(),
        }
    }

    /// Narrow this operation's declared intent to what it actually changes,
    /// against the state it was written on.
    ///
    /// [`apply`] does this internally, which is enough for undo. It is **not**
    /// enough for the collaboration transform, which reads the mask without
    /// ever seeing a workbook: an op still claiming [`SheetFields::ALL`] looks
    /// like it contends with every concurrent metadata edit, and one of them
    /// gets discarded for nothing.
    ///
    /// So an operation is narrowed **before it enters the protocol** — before
    /// it is sent, logged, or transformed. Doing it here rather than inside
    /// `transform` is forced: this needs the state the op was written against,
    /// and by the time two ops meet, that state is gone.
    #[must_use]
    pub fn narrowed(self, workbook: &Workbook) -> Self {
        match self {
            Self::SetSheetMetadata {
                sheet,
                data,
                changed,
                restore,
            } => {
                let effective = workbook.sheets.get(sheet).map_or(changed, |target| {
                    changed.intersection(data.diff(&SheetMetadata::capture(target)))
                });
                Self::SetSheetMetadata {
                    sheet,
                    data,
                    changed: effective,
                    restore,
                }
            }
            Self::Batch(ops) => Self::Batch(
                ops.into_iter()
                    .map(|op| op.narrowed(workbook))
                    .collect::<Vec<_>>(),
            ),
            other => other,
        }
    }

    /// Which sheet-metadata fields this operation changes, if any.
    ///
    /// The collaboration layer's transform needs this to tell two concurrent
    /// edits apart: disjoint sets merge, overlapping ones are resolved by
    /// server order. Everything that is not a metadata change reports
    /// [`SheetFields::NONE`], since it cannot collide on this axis at all.
    #[must_use]
    pub fn sheet_fields(&self) -> SheetFields {
        match self {
            Self::SetSheetMetadata { changed, .. } => *changed,
            // These write the same state the bundle's sizing fields do, just
            // one line at a time. Reporting `NONE` would say a column resize
            // cannot collide with a bundle that replaces every column width,
            // which is exactly backwards.
            Self::SetColumnWidth { .. } => SheetFields::COLUMNS,
            Self::SetRowHeight { .. } => SheetFields::ROWS,
            Self::Batch(ops) => ops
                .iter()
                .fold(SheetFields::NONE, |acc, op| acc.union(op.sheet_fields())),
            _ => SheetFields::NONE,
        }
    }
}

/// Whether an operation **provably** changes nothing, so the undo stack should
/// not carry it.
///
/// Conservative on purpose: it answers "can this be shown to be a no-op without
/// consulting the workbook", and `false` for anything else. Setting a cell to
/// the value it already holds is also a no-op, and this does not say so —
/// proving it needs the state the op was written against, which is gone by the
/// time the inverse comes back. Reporting only what is certain keeps a real
/// edit from being silently dropped, which is the far worse failure of the two.
///
/// [`SheetFields`] is what makes the sheet-metadata case provable at all: an
/// inverse comes back from [`apply`] narrowed to the fields that actually
/// differed, so an empty mask *is* the proof.
fn changes_nothing(op: &Operation) -> bool {
    match op {
        Operation::SetSheetMetadata { changed, .. } => *changed == SheetFields::NONE,
        // Vacuously for an empty batch, and by induction otherwise: a batch is
        // exactly its members, so one whose every member does nothing does
        // nothing.
        Operation::Batch(ops) => ops.iter().all(changes_nothing),
        _ => false,
    }
}

/// A short, human name for an operation, for undo/redo labels.
///
/// Deliberately coarse: a label's job is to say which action is about to be
/// reversed, not to describe it exactly. Naming the inverse of a delete
/// "insert" would be accurate and useless.
fn describe_op(op: &Operation) -> &'static str {
    match op {
        Operation::SetCell { .. } | Operation::SetValue { .. } => "cell edit",
        Operation::ClearCell { .. } => "clear cells",
        Operation::SetStyle { .. } => "formatting",
        Operation::SetColumnWidth { .. } => "column width",
        Operation::SetRowHeight { .. } => "row height",
        Operation::SetTabColor { .. } => "tab colour",
        Operation::SetSheetMetadata { .. } => "sheet change",
        Operation::InsertRows { .. } => "insert rows",
        Operation::DeleteRows { .. } => "delete rows",
        Operation::InsertColumns { .. } => "insert columns",
        Operation::DeleteColumns { .. } => "delete columns",
        Operation::MoveColumns { .. } => "move columns",
        Operation::MoveRows { .. } => "move rows",
        Operation::MoveRange { .. } => "move cells",
        Operation::InsertSheet { .. } => "add sheet",
        Operation::RemoveSheet { .. } => "remove sheet",
        Operation::RenameSheet { .. } => "rename sheet",
        Operation::MoveSheet { .. } => "move sheet",
        Operation::SetDefinedNames(_) => "defined names",
        Operation::Batch(ops) => ops.first().map_or("change", describe_op),
    }
}

/// Apply `op` to `workbook`, returning the inverse operation.
///
/// A `Batch` is all-or-nothing: if any member fails, the already-applied members
/// are rolled back before the error is returned.
pub fn apply(workbook: &mut Workbook, op: Operation) -> Result<Operation, TxnError> {
    match op {
        Operation::SetCell { sheet, at, cell } => {
            let previous = replace_cell(workbook, sheet, at, cell)?;
            Ok(inverse_of(sheet, at, previous))
        }
        Operation::SetValue { sheet, at, value } => {
            let previous = current_cell(workbook, sheet, at)?;
            let new_cell = Cell {
                value,
                style: previous.as_ref().and_then(|c| c.style),
                formula: None,
                ..Cell::default()
            };
            let cell = (!new_cell.is_blank()).then_some(new_cell);
            let restored = replace_cell(workbook, sheet, at, cell)?;
            Ok(inverse_of(sheet, at, restored))
        }
        Operation::SetStyle { sheet, at, style } => {
            let previous = current_cell(workbook, sheet, at)?;
            let mut new_cell = previous.unwrap_or_default();
            new_cell.style = style;
            let cell = (!new_cell.is_blank()).then_some(new_cell);
            let restored = replace_cell(workbook, sheet, at, cell)?;
            Ok(inverse_of(sheet, at, restored))
        }
        Operation::ClearCell { sheet, at } => {
            let previous = replace_cell(workbook, sheet, at, None)?;
            Ok(inverse_of(sheet, at, previous))
        }
        Operation::SetColumnWidth { sheet, col, width } => {
            let target = workbook
                .sheets
                .get_mut(sheet)
                .ok_or(TxnError::SheetNotFound { index: sheet })?;
            let previous = set_axis_override(&mut target.columns, col, width);
            Ok(Operation::SetColumnWidth {
                sheet,
                col,
                width: previous,
            })
        }
        Operation::SetRowHeight { sheet, row, height } => {
            let target = workbook
                .sheets
                .get_mut(sheet)
                .ok_or(TxnError::SheetNotFound { index: sheet })?;
            let previous = set_axis_override(&mut target.rows, row, height);
            Ok(Operation::SetRowHeight {
                sheet,
                row,
                height: previous,
            })
        }
        Operation::InsertRows { sheet, at, count } => {
            structural::insert(workbook, sheet, Axis::Row, at, count)
        }
        Operation::DeleteRows { sheet, at, count } => {
            structural::delete(workbook, sheet, Axis::Row, at, count)
        }
        Operation::InsertColumns { sheet, at, count } => {
            structural::insert(workbook, sheet, Axis::Col, at, count)
        }
        Operation::DeleteColumns { sheet, at, count } => {
            structural::delete(workbook, sheet, Axis::Col, at, count)
        }
        Operation::MoveColumns {
            sheet,
            at,
            count,
            before,
        } => structural::move_lines(workbook, sheet, Axis::Col, at, count, before),
        Operation::MoveRows {
            sheet,
            at,
            count,
            before,
        } => structural::move_lines(workbook, sheet, Axis::Row, at, count, before),
        Operation::MoveRange { sheet, from, to } => {
            structural::move_range(workbook, sheet, from, to)
        }
        Operation::SetSheetMetadata {
            sheet,
            data,
            changed,
            restore,
        } => {
            let target = workbook
                .sheets
                .get_mut(sheet)
                .ok_or(TxnError::SheetNotFound { index: sheet })?;
            // Which retained chart parts this sheet was holding on to, before
            // the bundle replaces its chart list.
            let held: BTreeSet<String> = target
                .charts
                .iter()
                .filter_map(|c| c.part.clone())
                .collect();
            // Narrow the declared intent to what the bundle actually differs in.
            // A caller that passes `ALL` — which is most of them, because the
            // ergonomic path is "capture the sheet, change one field, submit" —
            // gets a precise op for free, and the inverse inherits it. That is
            // what stops undo reversing twenty-two fields nobody touched, and
            // what lets two concurrent edits to different fields merge.
            let effective = changed.intersection(data.diff(&SheetMetadata::capture(target)));
            let previous = data.install_masked(target, effective);

            // Put back anything this operation carries. Undoing a chart removal
            // restores the chart *and* the bytes the model cannot rebuild.
            for part in restore.parts {
                if !workbook.retained_parts.iter().any(|p| p.path == part.path) {
                    workbook.retained_parts.push(part);
                }
            }
            for rel in restore.rels {
                if !workbook
                    .retained_rels
                    .iter()
                    .any(|r| r.source == rel.source && r.id == rel.id)
                {
                    workbook.retained_rels.push(rel);
                }
            }

            // And take away anything this sheet was the last to reference.
            //
            // Derived rather than sent: every replica computes the same set from
            // the same bundle, which is what makes a chart deletion converge
            // without the operation carrying any bytes. Only paths this sheet
            // *held* are candidates, so a retained part belonging to anything
            // else — an external link, a pivot cache — is never in scope.
            let live: BTreeSet<&str> = workbook
                .sheets
                .iter()
                .flat_map(|s| s.charts.iter())
                .filter_map(|c| c.part.as_deref())
                .collect();
            let dead: BTreeSet<String> = held
                .into_iter()
                .filter(|path| !live.contains(path.as_str()))
                .collect();
            let dropped = take_retained(workbook, &dead);

            Ok(Operation::SetSheetMetadata {
                sheet,
                data: Box::new(previous),
                changed: effective,
                restore: dropped,
            })
        }
        Operation::InsertSheet { index, sheet } => {
            let at = index.min(workbook.sheets.len());
            workbook.sheets.insert(at, *sheet);
            Ok(Operation::RemoveSheet { index: at })
        }
        Operation::RemoveSheet { index } => {
            if index >= workbook.sheets.len() {
                return Err(TxnError::SheetNotFound { index });
            }
            // A chart series names its sheet by *name*, so the sheet going away
            // does not break the reference — it leaves it pointing at a name
            // nothing answers to, and at whatever answers to it next. Collapse
            // those to `#REF!`, the same spelling a `DeleteRows` already writes
            // into a series whose rows it took (`CHT-08`).
            //
            // The snapshots are taken *before* the removal, at pre-removal sheet
            // indices, which is what the inverse restores them at: the batch
            // re-inserts the sheet first, so every later index is the one it was.
            let gone = workbook.sheets[index].name.clone();
            let charting = structural::sheets_charting(workbook, &gone);
            let restores: Vec<Operation> = charting
                .into_iter()
                .filter(|at| *at != index)
                .map(|at| structural::snapshot_metadata(workbook, at))
                .collect();
            let removed = workbook.sheets.remove(index);
            structural::break_series_naming(workbook, &gone);
            let reinsert = Operation::InsertSheet {
                index,
                sheet: Box::new(removed),
            };
            if restores.is_empty() {
                // The common case keeps the plain inverse it always had, so a
                // removal that broke nothing is still one operation to undo.
                return Ok(reinsert);
            }
            let mut ops = Vec::with_capacity(restores.len() + 1);
            ops.push(reinsert);
            ops.extend(restores);
            Ok(Operation::Batch(ops))
        }
        Operation::RenameSheet { index, name } => {
            let previous = {
                let target = workbook
                    .sheets
                    .get_mut(index)
                    .ok_or(TxnError::SheetNotFound { index })?;
                std::mem::replace(&mut target.name, name.clone())
            };
            // Follow the rename in every cross-sheet reference (`Old!A1` ->
            // `New!A1`) so a referenced sheet's formulas don't silently break.
            // The inverse renames back and this same pass reverses the rewrite.
            if previous != name {
                rename_sheet_in_formulas(workbook, &previous, &name);
            }
            Ok(Operation::RenameSheet {
                index,
                name: previous,
            })
        }
        Operation::MoveSheet { from, to } => {
            let count = workbook.sheets.len();
            if from >= count {
                return Err(TxnError::SheetNotFound { index: from });
            }
            if to >= count {
                return Err(TxnError::SheetNotFound { index: to });
            }
            let sheet = workbook.sheets.remove(from);
            workbook.sheets.insert(to, sheet);
            // Removing `from` then inserting at `to` is undone by removing `to`
            // then inserting at `from`.
            Ok(Operation::MoveSheet { from: to, to: from })
        }
        Operation::SetTabColor { sheet, color } => {
            let target = workbook
                .sheets
                .get_mut(sheet)
                .ok_or(TxnError::SheetNotFound { index: sheet })?;
            let previous = std::mem::replace(&mut target.tab_color, color);
            Ok(Operation::SetTabColor {
                sheet,
                color: previous,
            })
        }
        Operation::SetDefinedNames(names) => {
            let previous = std::mem::replace(&mut workbook.defined_names, names);
            Ok(Operation::SetDefinedNames(previous))
        }
        Operation::Batch(ops) => {
            let mut inverses = Vec::with_capacity(ops.len());
            for member in ops {
                match apply(workbook, member) {
                    Ok(inverse) => inverses.push(inverse),
                    Err(err) => {
                        while let Some(inv) = inverses.pop() {
                            let _ = apply(workbook, inv);
                        }
                        return Err(err);
                    }
                }
            }
            inverses.reverse();
            Ok(Operation::Batch(inverses))
        }
    }
}

/// Set or clear one axis override, returning the previous value (for the inverse).
fn set_axis_override(axis: &mut AxisSizing, index: u32, size: Option<i64>) -> Option<i64> {
    let previous = axis.sizes.get(&index).copied();
    match size {
        Some(value) => {
            axis.sizes.insert(index, value);
        }
        None => {
            axis.sizes.remove(&index);
        }
    }
    previous
}

/// Rewrite every workbook formula that references sheet `old` (by name) so it
/// points at `new`. Only formulas that actually change are re-stored, mirroring
/// the structural row/column rewrite pass.
///
/// **Defined names are formulas too**, and this pass used to walk only the
/// cells — so renaming a sheet left every name that referred to it pointing at
/// a sheet no longer in the workbook, with nothing said. The same shape as
/// `FID-24`, which fixed the row/column rewrite for defined names and did not
/// reach the rename. Found by the TP1 property the first time its seed carried
/// a defined name: an `InsertRows` and a `RenameSheet` on the same sheet
/// settled the name at `$C$4` in one order and `$C$3` in the other, because in
/// the second order the insert no longer recognised the sheet the stale
/// qualifier still named.
fn rename_sheet_in_formulas(workbook: &mut Workbook, old: &str, new: &str) {
    let mut jobs: Vec<(usize, CellRef, Expr)> = Vec::new();
    for (idx, sheet) in workbook.sheets.iter().enumerate() {
        for (addr, cell) in sheet.cells.iter() {
            if let Some(handle) = cell.formula
                && let Some(expr) = workbook.formula(handle)
            {
                let mut rewritten = expr.clone();
                if rename_sheet_references(&mut rewritten, old, new) {
                    jobs.push((idx, addr, rewritten));
                }
            }
        }
    }
    for (idx, addr, expr) in jobs {
        let handle = workbook.store_formula(expr);
        let store = &mut workbook.sheets[idx].cells;
        if let Some(existing) = store.get(addr) {
            let mut updated = existing.clone();
            updated.formula = Some(handle);
            store.set(addr, updated);
        }
    }
    for name in &mut workbook.defined_names {
        rename_sheet_references(&mut name.formula, old, new);
    }
}

fn inverse_of(sheet: usize, at: CellRef, previous: Option<Cell>) -> Operation {
    Operation::SetCell {
        sheet,
        at,
        cell: previous,
    }
}

fn current_cell(workbook: &Workbook, sheet: usize, at: CellRef) -> Result<Option<Cell>, TxnError> {
    let sheet = workbook
        .sheets
        .get(sheet)
        .ok_or(TxnError::SheetNotFound { index: sheet })?;
    Ok(sheet.cells.get(at).cloned())
}

fn replace_cell(
    workbook: &mut Workbook,
    sheet: usize,
    at: CellRef,
    cell: Option<Cell>,
) -> Result<Option<Cell>, TxnError> {
    let sheet = workbook
        .sheets
        .get_mut(sheet)
        .ok_or(TxnError::SheetNotFound { index: sheet })?;
    let previous = sheet.cells.get(at).cloned();
    match cell {
        Some(cell) => sheet.cells.set(at, cell),
        None => {
            sheet.cells.clear(at);
        }
    }
    Ok(previous)
}

/// Paired undo/redo stacks over [`apply`]. The host keeps one of these per
/// document session.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Operation>,
    redo: Vec<Operation>,
    /// How many undo entries to keep. `None` is unbounded.
    ///
    /// Unbounded is right for a short session and wrong for a long one: an
    /// entry holds a whole inverse operation, and the inverse of a metadata
    /// edit is a snapshot of the sheet's metadata — so a day of formatting
    /// tweaks accumulates a copy of the sheet's tables, validations, comments
    /// and conditional formats per tweak, none of which is ever released.
    depth: Option<usize>,
    /// How many edits have ever been applied, counting up and never down.
    ///
    /// A host compares this against the value it saw when it last saved, to
    /// answer "is there unsaved work?" — the question behind a close warning.
    ///
    /// Deliberately **not** `undo.len()`. That stack is bounded, so past the
    /// bound it stops growing, and a document edited past its cap would compare
    /// equal to its save point and be reported *clean* while dirty. This can
    /// only ever err the other way: undoing back to the save point still counts
    /// as having edited, so the warning appears when it need not have. A
    /// needless warning costs a click; the other mistake costs the document.
    applied: u64,
}

impl History {
    /// A new, empty history with no bound on how far back undo reaches.
    pub fn new() -> Self {
        Self::default()
    }

    /// A new, empty history keeping at most `depth` undo entries.
    pub fn with_depth(depth: Option<usize>) -> Self {
        Self {
            depth,
            ..Self::default()
        }
    }

    /// Apply `op`, recording its inverse for undo and clearing the redo stack.
    ///
    /// An operation that **changed nothing** is applied and then forgotten: no
    /// undo entry, and the redo stack survives. This is the same rule a refused
    /// edit follows in [`WorkbookSession::edit`](../casual_calc_sdk/struct.WorkbookSession.html#method.edit)
    /// — "must leave no trace, or undo has an entry that undoes nothing" — and
    /// it was not being kept. The editor calls `session_table_autoexpand` after
    /// **every** cell commit, which submits a whole-sheet bundle that almost
    /// always differs in nothing; each one landed on the stack, so the first
    /// Ctrl+Z after typing a value appeared to do nothing at all and the second
    /// did the work. A user pressing undo and seeing no change does not press
    /// it again — they conclude undo is broken, which it was.
    pub fn apply(&mut self, workbook: &mut Workbook, op: Operation) -> Result<(), TxnError> {
        let inverse = apply(workbook, op)?;
        // Before the redo stack is cleared, because a keystroke that changed
        // nothing is not a new edit and must not discard what redo was holding.
        if changes_nothing(&inverse) {
            return Ok(());
        }
        self.applied += 1;
        self.undo.push(inverse);
        // Oldest first: the entry least likely to be wanted is the one furthest
        // back. A depth of zero means no undo at all, which is a legitimate
        // thing for a batch host to ask for.
        if let Some(depth) = self.depth {
            let excess = self.undo.len().saturating_sub(depth);
            self.undo.drain(..excess);
        }
        self.redo.clear();
        Ok(())
    }

    /// Forget everything, making the current state the one nothing precedes.
    ///
    /// For the moment a document *becomes* the document: after a host has
    /// populated a fresh workbook, or after opening a file. Neither is an edit
    /// the user made, and leaving them on the stack lets Ctrl+Z walk backwards
    /// out of the document they were given and into an empty sheet.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// How many edits have ever been applied, counting up and never down.
    ///
    /// A host compares this against the value it saw when it last saved, to
    /// answer "is there unsaved work?" — the question behind a close warning.
    ///
    /// Deliberately not the undo stack's depth. That stack is bounded, so past
    /// the bound it stops growing, and a document edited past its cap would
    /// compare equal to its save point and be reported *clean* while dirty.
    /// This can only err the other way: undo counts as an edit too, so undoing
    /// back to the save point still reports a difference. A needless warning
    /// costs a click; the other mistake costs the document.
    pub fn edits_applied(&self) -> u64 {
        self.applied
    }

    /// Count a change this history did not make (`FID-39`).
    ///
    /// For a caller that changes the workbook behind the history's back — the
    /// SDK's `apply_raw` and `workbook_mut`, and the collaborative receive path
    /// that goes through the latter. Those bypass the undo *stack* on purpose;
    /// bypassing the **dirty signal** was never part of it, and
    /// [`Self::edits_applied`] promises a host that the number answers "is
    /// there unsaved work?".
    ///
    /// Deliberately does not touch either stack. A change nobody recorded an
    /// inverse for is not undoable, and inventing a stack entry for it would
    /// let Ctrl+Z walk into a state the history cannot reconstruct.
    pub fn note_foreign_change(&mut self) {
        self.applied += 1;
    }

    /// Whether there is anything to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is anything to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// A short description of what undo would reverse, for a menu label.
    ///
    /// The stack holds *inverses*, so this describes the operation that would be
    /// applied — which is the right thing to name: "Undo delete rows" is what the
    /// user is about to get back.
    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo.last().map(describe_op)
    }

    /// The operation undo would apply, without applying it.
    ///
    /// The stack holds inverses, so this is the thing whose effect a policy has
    /// to judge — and it has to be judged *before* it runs, because the state it
    /// would destroy is the evidence.
    #[must_use]
    pub fn peek_undo(&self) -> Option<&Operation> {
        self.undo.last()
    }

    /// Likewise for redo.
    pub fn redo_label(&self) -> Option<&'static str> {
        self.redo.last().map(describe_op)
    }

    /// Undo the most recent operation, returning **what it actually applied**.
    ///
    /// The return value is the point, and it used to be `()`. An undo changes
    /// the document exactly as an edit does, so a collaborating host has to send
    /// it; with nothing returned there was nothing to send, and undo was
    /// local-only. One participant reverted while the server and every peer kept
    /// the edit — a divergence that never heals, because nothing afterwards
    /// disagrees loudly enough to notice.
    ///
    /// `None` when there was nothing to undo, which is not an error.
    pub fn undo(&mut self, workbook: &mut Workbook) -> Result<Option<Operation>, TxnError> {
        let Some(op) = self.undo.pop() else {
            return Ok(None);
        };
        // Cloned because `apply` consumes it and gives back the *inverse*; what
        // a host must transmit is the operation that ran.
        let applied = op.clone();
        let inverse = apply(workbook, op)?;
        // An undo is an edit (`FID-39`). `edits_applied`'s doc comment always
        // said so and reasoned from it — that this counter "can only err the
        // other way" — while this line was missing, so the mistake it rules out
        // was the one being made: undo an edit that was already *saved* and the
        // document moves away from disk with the number the host compares
        // standing still, so no close warning is shown.
        self.applied += 1;
        self.redo.push(inverse);
        Ok(Some(applied))
    }

    /// Redo the most recently undone operation, returning what it applied.
    ///
    /// A redo is a fresh intention rather than the cancellation of one, and it
    /// travels the same way for the same reason.
    pub fn redo(&mut self, workbook: &mut Workbook) -> Result<Option<Operation>, TxnError> {
        let Some(op) = self.redo.pop() else {
            return Ok(None);
        };
        let applied = op.clone();
        let inverse = apply(workbook, op)?;
        self.applied += 1;
        self.undo.push(inverse);
        Ok(Some(applied))
    }
}

#[cfg(test)]
mod tests;

/// Which cells an operation **writes**, for attribution (`HIST-02`).
///
/// Deliberately narrower than [`recalc_plan`], which answers a different
/// question. A formula whose result changed because a precedent moved was not
/// *edited* by anybody — Excel does not credit the author of `A1` with every
/// cell that sums it, and neither should this. So only the addresses an
/// operation writes directly are returned.
///
/// Structural operations return nothing on purpose. Inserting a row moves a
/// thousand cells without authoring any of them, and stamping the mover's name
/// across a sheet they did not write is worse than saying nothing: it is
/// confident and wrong, where silence is merely incomplete.
#[must_use]
pub fn written_cells(op: &Operation) -> Vec<(usize, CellRef)> {
    let mut out = Vec::new();
    collect_written(op, &mut out);
    out
}

fn collect_written(op: &Operation, out: &mut Vec<(usize, CellRef)>) {
    match op {
        Operation::SetCell { sheet, at, .. }
        | Operation::SetValue { sheet, at, .. }
        | Operation::ClearCell { sheet, at, .. } => out.push((*sheet, *at)),
        // A style change is an edit of the cell's appearance and is attributed:
        // "who made this red" is the same question as "who typed this".
        Operation::SetStyle { sheet, at, .. } => out.push((*sheet, *at)),
        Operation::Batch(ops) => {
            for inner in ops {
                collect_written(inner, out);
            }
        }
        _ => {}
    }
}
