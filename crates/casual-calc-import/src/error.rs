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
    /// The document asked for more of something than one document may have.
    ///
    /// Counted across the **whole workbook**, not per part — see
    /// [`SpreadsheetLimits`](casual_calc_ooxml::SpreadsheetLimits). Refused
    /// rather than truncated: docs/21 requires failing closed, and a workbook
    /// admitted with some of its cells missing is a file that will be saved back
    /// over the original with the rest gone.
    OverBudget {
        /// Which budget ran out.
        what: Overrun,
        /// The ceiling it passed.
        limit: usize,
    },
}

/// Which document-scale budget an import ran past.
///
/// One variant per registered code, because docs/20 allocates a code per
/// condition and an operator reading a log needs to know *which* limit a file
/// met — "too big" is not something anybody can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overrun {
    /// Populated cells across every sheet. The code docs/20 reserved for this
    /// before anything emitted it.
    PopulatedCells,
    /// Entries in the shared-string table.
    SharedStrings,
    /// Defined names in the workbook.
    DefinedNames,
    /// Merged ranges across every sheet.
    MergedRanges,
}

impl Overrun {
    /// The stable diagnostic code (`docs/20`).
    pub fn code(self) -> &'static str {
        match self {
            Overrun::PopulatedCells => "OC-IMP-0003",
            Overrun::SharedStrings => "OC-IMP-0004",
            Overrun::DefinedNames => "OC-IMP-0005",
            Overrun::MergedRanges => "OC-IMP-0006",
        }
    }

    /// What to call it in a sentence.
    pub fn what(self) -> &'static str {
        match self {
            Overrun::PopulatedCells => "populated cells",
            Overrun::SharedStrings => "shared strings",
            Overrun::DefinedNames => "defined names",
            Overrun::MergedRanges => "merged ranges",
        }
    }
}

impl ImportError {
    /// The stable diagnostic code for this error (`docs/20`).
    pub fn code(&self) -> &'static str {
        match self {
            ImportError::Ooxml(err) => err.code(),
            ImportError::Model(err) => err.code(),
            ImportError::MissingPart { .. } => "OC-IMP-0001",
            ImportError::OverBudget { what, .. } => what.code(),
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
            ImportError::OverBudget { what, limit } => write!(
                f,
                "[{}] this document has more {} than one document may have (limit {limit}); \
                 it was refused rather than partly loaded",
                what.code(),
                what.what(),
            ),
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
