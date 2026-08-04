//! Runtime values produced by evaluation, with Excel-style coercions.

use casual_calc_model::{CellValue, ErrorValue, StringTable};

/// A computed value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// An empty/blank reference.
    Empty,
    /// A number.
    Number(f64),
    /// A boolean.
    Bool(bool),
    /// Text.
    Text(String),
    /// An error value that propagates.
    Error(ErrorValue),
}

/// Format a number the way the engine stringifies it (used by `&` and text ops).
pub fn number_to_text(n: f64) -> String {
    format!("{n}")
}

impl Value {
    /// Coerce to a number (empty → 0, bool → 0/1, text → parsed), or an error.
    pub fn as_number(&self) -> Result<f64, ErrorValue> {
        match self {
            Value::Empty => Ok(0.0),
            Value::Number(n) => Ok(*n),
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Value::Text(s) => s.trim().parse::<f64>().map_err(|_| ErrorValue::Value),
            Value::Error(e) => Err(*e),
        }
    }

    /// Coerce to text (numbers formatted, bool → TRUE/FALSE), or an error.
    pub fn as_text(&self) -> Result<String, ErrorValue> {
        match self {
            Value::Empty => Ok(String::new()),
            Value::Number(n) => Ok(number_to_text(*n)),
            Value::Bool(b) => Ok(if *b { "TRUE" } else { "FALSE" }.to_owned()),
            Value::Text(s) => Ok(s.clone()),
            Value::Error(e) => Err(*e),
        }
    }

    /// Coerce to a truth value for conditionals.
    pub fn as_bool(&self) -> Result<bool, ErrorValue> {
        match self {
            Value::Empty => Ok(false),
            Value::Number(n) => Ok(*n != 0.0),
            Value::Bool(b) => Ok(*b),
            Value::Text(s) if s.eq_ignore_ascii_case("true") => Ok(true),
            Value::Text(s) if s.eq_ignore_ascii_case("false") => Ok(false),
            Value::Text(_) => Err(ErrorValue::Value),
            Value::Error(e) => Err(*e),
        }
    }

    /// The error carried by this value, if any (for propagation).
    pub fn as_error(&self) -> Option<ErrorValue> {
        match self {
            Value::Error(e) => Some(*e),
            _ => None,
        }
    }
}

/// Read a stored cell value into a runtime [`Value`], resolving interned strings.
pub fn value_from_cell(value: &CellValue, strings: &StringTable) -> Value {
    match value {
        CellValue::Empty => Value::Empty,
        CellValue::Number(n) => Value::Number(*n),
        CellValue::Bool(b) => Value::Bool(*b),
        CellValue::Error(e) => Value::Error(*e),
        CellValue::SharedString(id) | CellValue::InlineString(id) => {
            Value::Text(strings.get(*id).unwrap_or_default().to_owned())
        }
    }
}
