//! Export errors. See `docs/20-ERROR-CODE-REGISTRY.md`.

use core::fmt;

/// An error writing a workbook to a `.xlsx` package.
#[derive(Debug)]
#[non_exhaustive]
pub enum ExportError {
    /// A ZIP/IO failure while packaging.
    Package(String),
}

impl ExportError {
    /// The stable diagnostic code (`docs/20`).
    pub fn code(&self) -> &'static str {
        "OC-EXP-0001"
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Package(msg) => write!(f, "[{}] packaging failed: {msg}", self.code()),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<zip::result::ZipError> for ExportError {
    fn from(err: zip::result::ZipError) -> Self {
        ExportError::Package(err.to_string())
    }
}

impl From<std::io::Error> for ExportError {
    fn from(err: std::io::Error) -> Self {
        ExportError::Package(err.to_string())
    }
}
