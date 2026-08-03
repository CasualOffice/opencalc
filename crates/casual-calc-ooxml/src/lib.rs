//! `casual-calc-ooxml` — security-bounded SpreadsheetML (OPC) package
//! inspection.
//!
//! On top of the bounded [`casual_calc_package`] substrate, this crate resolves
//! the SpreadsheetML OPC graph — root relationships, the workbook part, its
//! `<sheets>`, and the workbook relationships that map each `r:id` to a
//! worksheet part — under per-part XML element/depth limits. It exposes the
//! *shape* of the package; mapping into the model is `casual-calc-import`.
//!
//! See `docs/28-XLSX-PACKAGE-READER.md` and `docs/21-PARSER-LIMITS.md`.

mod discovery;
mod error;
mod limits;
mod opc;

pub use discovery::{SheetEntry, SpreadsheetPackage};
pub use error::OoxmlError;
pub use limits::OoxmlLimits;
pub use opc::{Relationship, SheetRef, parse_relationships, parse_sheet_refs, resolve_target};

#[cfg(test)]
mod tests;
