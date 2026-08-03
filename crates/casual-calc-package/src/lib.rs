//! `casual-calc-package` — format-neutral, security-bounded ZIP/OPC package
//! admission and part reads.
//!
//! This is the substrate every OPC-based format sits on: it admits an archive
//! under explicit, non-bypassable [`PackageLimits`] (size, entry count,
//! expansion ratio, path safety), then exposes on-demand, size-capped part
//! reads. It backs both `.xlsx` (via `casual-calc-ooxml`) and `.ods` (via
//! `casual-calc-ods`). Delimited text formats (CSV/TSV/PSV) are *not* packages
//! and do not pass through this crate — they are handled by `casual-calc-io`
//! adapters.
//!
//! See `docs/28-XLSX-PACKAGE-READER.md` and `docs/21-PARSER-LIMITS.md`.

mod error;
mod limits;
mod package;
mod path;

pub use error::PackageError;
pub use limits::PackageLimits;
pub use package::{EntryInfo, Package};
pub use path::is_safe_part_path;

#[cfg(test)]
mod tests;
