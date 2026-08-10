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
    ConditionalFormat, DataValidation, DefinedName, Hyperlink, PivotTable, PrintSetup, Sheet,
    SheetProtection, SheetView, SheetVisibility, SortState, StyleId, Table, Workbook,
};

mod structural;

use structural::Axis;

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
#[derive(Debug, Clone, PartialEq, Default)]
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
    pub filter_hidden: BTreeSet<u32>,
    /// Outline nesting level per row.
    pub row_outline_levels: BTreeMap<u32, u8>,
    /// Outline nesting level per column.
    pub col_outline_levels: BTreeMap<u32, u8>,
    /// Rows whose outline group is collapsed.
    pub collapsed_rows: BTreeSet<u32>,
    /// Columns whose outline group is collapsed.
    pub collapsed_cols: BTreeSet<u32>,
    /// Data-validation rules.
    ///
    /// The five fields below are not positional like the ones above; they are
    /// here because they had **no** reversible operation at all, so editing a
    /// validation, a conditional format, a comment, a sheet's visibility or its
    /// protection wrote straight to the workbook. Undo then reversed whatever
    /// preceded it — the last cell edit — which destroys work the user did not
    /// ask to lose. Folding them into the one bundle that already has a proven
    /// inverse fixes all five at once, rather than adding five operations that
    /// each need their own inverse to get right.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

/// A closed set of atomic edit operations. Every operation is invertible; the
/// inverse of any operation is expressible as a `SetCell` (or a `Batch` of them).
#[derive(Debug, Clone, PartialEq)]
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
            Self::Batch(ops) => ops
                .iter()
                .fold(SheetFields::NONE, |acc, op| acc.union(op.sheet_fields())),
            _ => SheetFields::NONE,
        }
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
        Operation::SetSheetMetadata {
            sheet,
            data,
            changed,
        } => {
            let target = workbook
                .sheets
                .get_mut(sheet)
                .ok_or(TxnError::SheetNotFound { index: sheet })?;
            // Narrow the declared intent to what the bundle actually differs in.
            // A caller that passes `ALL` — which is most of them, because the
            // ergonomic path is "capture the sheet, change one field, submit" —
            // gets a precise op for free, and the inverse inherits it. That is
            // what stops undo reversing twenty-two fields nobody touched, and
            // what lets two concurrent edits to different fields merge.
            let effective = changed.intersection(data.diff(&SheetMetadata::capture(target)));
            let previous = data.install_masked(target, effective);
            Ok(Operation::SetSheetMetadata {
                sheet,
                data: Box::new(previous),
                changed: effective,
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
            let removed = workbook.sheets.remove(index);
            Ok(Operation::InsertSheet {
                index,
                sheet: Box::new(removed),
            })
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
    pub fn apply(&mut self, workbook: &mut Workbook, op: Operation) -> Result<(), TxnError> {
        let inverse = apply(workbook, op)?;
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

    /// Likewise for redo.
    pub fn redo_label(&self) -> Option<&'static str> {
        self.redo.last().map(describe_op)
    }

    /// Undo the most recent operation.
    pub fn undo(&mut self, workbook: &mut Workbook) -> Result<(), TxnError> {
        if let Some(op) = self.undo.pop() {
            let inverse = apply(workbook, op)?;
            self.redo.push(inverse);
        }
        Ok(())
    }

    /// Redo the most recently undone operation.
    pub fn redo(&mut self, workbook: &mut Workbook) -> Result<(), TxnError> {
        if let Some(op) = self.redo.pop() {
            let inverse = apply(workbook, op)?;
            self.undo.push(inverse);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
