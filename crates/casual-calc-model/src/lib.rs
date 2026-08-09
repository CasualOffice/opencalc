//! `casual-calc-model` — the normalized, in-memory workbook.
//!
//! This is the authoritative representation a workbook is imported into, edited
//! through, calculated over, laid out from, and written back from. It is
//! deliberately sparse and compact (the 1M-cell target) and carries the
//! **reserved calc seams** — [`cell::Cell::formula`], the cached
//! [`cell::Cell::value`], and the [`cell::CellFlags`] spill bits — so the
//! Phase 2 calc engine adds behavior, not schema.
//!
//! Snapshots serialize deterministically (fixed field order, ordered cell
//! store) so golden files are byte-stable.
//!
//! See `docs/22-NORMALIZED-SCHEMA.md` and `docs/23-CELL-STORE-REPRESENTATION.md`.

mod cell;
mod defined_name;
mod error;
mod ids;
mod sheet;
mod store;
mod strings;
mod style;
mod value;
mod workbook;

pub use cell::{Cell, CellFlags};
pub use defined_name::DefinedName;
pub use error::ModelError;
pub use ids::{
    DefinedNameId, FormulaHandle, Id, IdGenerator, NumberFormatId, SheetId, StringId, StyleId,
};
pub use sheet::{
    AutoFilter, AxisSizing, CellComment, CfRule, CommentReply, ConditionalFormat, CustomFilter,
    DataValidation, DvKind, DvOperator, FilterOp, FilterRule, Hyperlink, OutlinePr, PrintSetup,
    Sheet, SheetProtection, SheetView, SheetVisibility, wildcard_match,
};
pub use store::{CellRange, CellRef, CellStore};
pub use strings::StringTable;
pub use style::{
    BorderEdge, Borders, GradientFill, GradientStop, HAlign, RunFont, Style, StyleTable, TextRun,
    ThemeTint, Underline, VAlign, VertAlign, from_micro, to_micro,
};
pub use value::{CellValue, ErrorValue};
pub use workbook::{NamedCellStyle, SCHEMA_VERSION, STOCK_THEME_SLOTS, Workbook};

#[cfg(test)]
mod tests;
