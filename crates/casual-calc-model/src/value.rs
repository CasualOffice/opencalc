//! Cell values and error values. See `docs/22-NORMALIZED-SCHEMA.md`,
//! `docs/17-GLOSSARY.md`.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::ids::StringId;

/// A spreadsheet error value — the calc-visible errors a cell can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorValue {
    /// `#REF!`
    Ref,
    /// `#VALUE!`
    Value,
    /// `#DIV/0!`
    Div0,
    /// `#N/A`
    Na,
    /// `#NAME?`
    Name,
    /// `#NULL!`
    Null,
    /// `#NUM!`
    Num,
    /// `#SPILL!`
    Spill,
}

impl fmt::Display for ErrorValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let token = match self {
            ErrorValue::Ref => "#REF!",
            ErrorValue::Value => "#VALUE!",
            ErrorValue::Div0 => "#DIV/0!",
            ErrorValue::Na => "#N/A",
            ErrorValue::Name => "#NAME?",
            ErrorValue::Null => "#NULL!",
            ErrorValue::Num => "#NUM!",
            ErrorValue::Spill => "#SPILL!",
        };
        f.write_str(token)
    }
}

/// The value stored in (or cached for) a cell.
///
/// For a literal cell this *is* the value; for a formula cell it is the last
/// computed (cached) result — layout and render read only this, never the calc
/// engine. Strings are interned via [`StringId`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CellValue {
    /// No value.
    #[default]
    Empty,
    /// A number.
    Number(f64),
    /// A boolean.
    Bool(bool),
    /// An interned shared string.
    SharedString(StringId),
    /// An interned string emitted inline (round-trips the original choice).
    InlineString(StringId),
    /// An error value.
    Error(ErrorValue),
}

impl CellValue {
    /// Whether this value is [`CellValue::Empty`].
    pub fn is_empty(&self) -> bool {
        matches!(self, CellValue::Empty)
    }
}
