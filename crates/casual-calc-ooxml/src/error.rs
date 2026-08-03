//! SpreadsheetML inspection errors. See `docs/20-ERROR-CODE-REGISTRY.md`.

use core::fmt;

use casual_calc_package::PackageError;

/// An error inspecting a SpreadsheetML package.
#[derive(Debug)]
#[non_exhaustive]
pub enum OoxmlError {
    /// The underlying package could not be admitted or read.
    Package(PackageError),
    /// A part's XML was malformed.
    MalformedXml(String),
    /// XML element count exceeded the limit.
    TooManyElements { limit: usize },
    /// XML nesting depth exceeded the limit.
    TooDeep { limit: usize },
    /// A required part is missing.
    MissingPart { name: String },
    /// A relationship could not be resolved.
    UnresolvableRelationship { id: String },
}

impl OoxmlError {
    /// The stable diagnostic code for this error (`docs/20`).
    pub fn code(&self) -> &'static str {
        match self {
            OoxmlError::Package(err) => err.code(),
            OoxmlError::MalformedXml(_) => "OC-XML-0004",
            OoxmlError::TooManyElements { .. } => "OC-XML-0001",
            OoxmlError::TooDeep { .. } => "OC-XML-0002",
            OoxmlError::MissingPart { .. } => "OC-IMP-0001",
            OoxmlError::UnresolvableRelationship { .. } => "OC-IMP-0002",
        }
    }
}

impl fmt::Display for OoxmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] ", self.code())?;
        match self {
            OoxmlError::Package(err) => write!(f, "{err}"),
            OoxmlError::MalformedXml(msg) => write!(f, "malformed XML: {msg}"),
            OoxmlError::TooManyElements { limit } => {
                write!(f, "XML element count exceeds limit {limit}")
            }
            OoxmlError::TooDeep { limit } => write!(f, "XML nesting exceeds depth limit {limit}"),
            OoxmlError::MissingPart { name } => write!(f, "required part missing: {name:?}"),
            OoxmlError::UnresolvableRelationship { id } => {
                write!(f, "unresolvable relationship: {id:?}")
            }
        }
    }
}

impl std::error::Error for OoxmlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OoxmlError::Package(err) => Some(err),
            _ => None,
        }
    }
}

impl From<PackageError> for OoxmlError {
    fn from(err: PackageError) -> Self {
        OoxmlError::Package(err)
    }
}
