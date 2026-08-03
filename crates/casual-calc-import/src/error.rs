//! Import errors. See `docs/20-ERROR-CODE-REGISTRY.md`.

use core::fmt;

use casual_calc_model::ModelError;
use casual_calc_ooxml::OoxmlError;

/// An error importing a SpreadsheetML package into the model.
#[derive(Debug)]
#[non_exhaustive]
pub enum ImportError {
    /// A package-inspection error.
    Ooxml(OoxmlError),
    /// The resulting model failed validation.
    Model(ModelError),
    /// A required part was missing.
    MissingPart { name: String },
}

impl ImportError {
    /// The stable diagnostic code for this error (`docs/20`).
    pub fn code(&self) -> &'static str {
        match self {
            ImportError::Ooxml(err) => err.code(),
            ImportError::Model(err) => err.code(),
            ImportError::MissingPart { .. } => "OC-IMP-0001",
        }
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Ooxml(err) => write!(f, "{err}"),
            ImportError::Model(err) => write!(f, "{err}"),
            ImportError::MissingPart { name } => {
                write!(f, "[OC-IMP-0001] required part missing: {name:?}")
            }
        }
    }
}

impl std::error::Error for ImportError {}

impl From<OoxmlError> for ImportError {
    fn from(err: OoxmlError) -> Self {
        ImportError::Ooxml(err)
    }
}

impl From<ModelError> for ImportError {
    fn from(err: ModelError) -> Self {
        ImportError::Model(err)
    }
}
