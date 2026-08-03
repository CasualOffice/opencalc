//! Package admission errors, each carrying a stable diagnostic code.
//! See `docs/20-ERROR-CODE-REGISTRY.md`.

use core::fmt;

/// A reason a package was refused, or a part could not be read. Every variant
/// maps to a stable `OC-PKG-*` code via [`PackageError::code`]. All refusals are
/// *clean*: on any error the package is not partially admitted.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackageError {
    /// Input exceeds `max_input_bytes`.
    InputTooLarge { size: u64, limit: u64 },
    /// Entry count exceeds `max_entries`.
    TooManyEntries { count: usize, limit: usize },
    /// Total uncompressed size exceeds `max_total_uncompressed`.
    ExpansionTooLarge { total: u64, limit: u64 },
    /// Uncompressed:compressed ratio exceeds `max_expansion_ratio`.
    ExpansionRatioExceeded { ratio: u64, limit: u64 },
    /// An entry path is longer than `max_path_bytes`.
    PathTooLong { len: usize, limit: usize },
    /// An entry path is unsafe (traversal, absolute, or drive-qualified).
    UnsafePath { path: String },
    /// The input is not a valid ZIP/OPC package.
    NotAPackage,
    /// A requested part does not exist in the package.
    PartNotFound { name: String },
}

impl PackageError {
    /// The stable diagnostic code for this error (`docs/20`).
    pub fn code(&self) -> &'static str {
        match self {
            PackageError::InputTooLarge { .. } => "OC-PKG-0001",
            PackageError::TooManyEntries { .. } => "OC-PKG-0002",
            PackageError::ExpansionTooLarge { .. }
            | PackageError::ExpansionRatioExceeded { .. } => "OC-PKG-0003",
            PackageError::PathTooLong { .. } | PackageError::UnsafePath { .. } => "OC-PKG-0004",
            PackageError::NotAPackage => "OC-PKG-0005",
            PackageError::PartNotFound { .. } => "OC-PKG-0006",
        }
    }
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] ", self.code())?;
        match self {
            PackageError::InputTooLarge { size, limit } => {
                write!(f, "input too large: {size} bytes exceeds limit {limit}")
            }
            PackageError::TooManyEntries { count, limit } => {
                write!(f, "too many entries: {count} exceeds limit {limit}")
            }
            PackageError::ExpansionTooLarge { total, limit } => {
                write!(
                    f,
                    "expanded size too large: {total} bytes exceeds limit {limit}"
                )
            }
            PackageError::ExpansionRatioExceeded { ratio, limit } => {
                write!(
                    f,
                    "expansion ratio too high: {ratio}:1 exceeds limit {limit}:1"
                )
            }
            PackageError::PathTooLong { len, limit } => {
                write!(f, "entry path too long: {len} bytes exceeds limit {limit}")
            }
            PackageError::UnsafePath { path } => write!(f, "unsafe entry path: {path:?}"),
            PackageError::NotAPackage => write!(f, "not a valid ZIP/OPC package"),
            PackageError::PartNotFound { name } => write!(f, "part not found: {name:?}"),
        }
    }
}

impl std::error::Error for PackageError {}
