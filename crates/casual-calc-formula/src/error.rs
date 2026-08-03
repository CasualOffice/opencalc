//! Formula parse errors. See `docs/20-ERROR-CODE-REGISTRY.md`.

use core::fmt;

/// A formula could not be tokenized or parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FormulaError {
    /// An unexpected character during tokenizing.
    UnexpectedChar(char),
    /// A string literal was not closed.
    UnterminatedString,
    /// A quoted sheet name was not closed.
    UnterminatedSheet,
    /// The parser hit an unexpected token or ran out of input.
    UnexpectedToken(String),
    /// Trailing input remained after a complete expression.
    TrailingInput,
    /// A reference was malformed.
    InvalidReference(String),
}

impl FormulaError {
    /// The stable diagnostic code (`docs/20`).
    pub fn code(&self) -> &'static str {
        "OC-FML-0001"
    }
}

impl fmt::Display for FormulaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] ", self.code())?;
        match self {
            FormulaError::UnexpectedChar(c) => write!(f, "unexpected character {c:?}"),
            FormulaError::UnterminatedString => write!(f, "unterminated string literal"),
            FormulaError::UnterminatedSheet => write!(f, "unterminated quoted sheet name"),
            FormulaError::UnexpectedToken(t) => write!(f, "unexpected token: {t}"),
            FormulaError::TrailingInput => write!(f, "trailing input after expression"),
            FormulaError::InvalidReference(r) => write!(f, "invalid reference: {r:?}"),
        }
    }
}

impl std::error::Error for FormulaError {}
