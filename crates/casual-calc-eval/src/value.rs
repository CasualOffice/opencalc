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
    /// A function value: a `LAMBDA` together with the bindings it closed over.
    ///
    /// First-class because a LAMBDA that returns a LAMBDA has to carry the
    /// outer parameter with it — `LAMBDA(x, LAMBDA(y, x+y))(3)(4)` is 7 only if
    /// the inner one still knows what `x` was. The same representation is what
    /// lets `MAP` and `REDUCE` take a function as an argument.
    Lambda(std::rc::Rc<LambdaValue>),
    /// A rectangular block of values, row-major.
    ///
    /// Produced by the functions that answer with a *shape* rather than a
    /// number — TRANSPOSE, MMULT and their kin. Where a scalar is wanted, an
    /// array collapses to its top-left element, which is Excel's rule for
    /// implicit intersection and keeps every existing consumer working without
    /// knowing arrays exist.
    Array {
        /// Row count; always at least one for a non-empty array.
        rows: usize,
        /// Column count.
        cols: usize,
        /// `rows × cols` values in row-major order.
        cells: Vec<Value>,
    },
}

impl Value {
    /// The top-left element, or the value itself when it is not an array.
    ///
    /// Excel's implicit intersection: a formula that wants one number and is
    /// handed a block takes the corner rather than failing, which is what lets
    /// `=TRANSPOSE(A1:B2)+1` mean something.
    pub fn scalar(self) -> Value {
        match self {
            Value::Array { cells, .. } => cells.into_iter().next().unwrap_or(Value::Empty),
            other => other,
        }
    }
}

/// A `LAMBDA`'s parameters, body, and the scope it captured.
#[derive(Clone, Debug, PartialEq)]
pub struct LambdaValue {
    /// Parameter names, in order.
    pub params: Vec<String>,
    /// The expression to evaluate once the parameters are bound.
    pub body: casual_calc_formula::Expr,
    /// Bindings visible where the LAMBDA was written, captured by value.
    ///
    /// By value rather than by reference: the scope it was written in is gone
    /// by the time it is called, and a lambda that read whatever happened to be
    /// bound at call time would be a different function each time.
    pub captured: Vec<(String, Value)>,
}

/// Format a number the way the engine stringifies it (used by `&` and text ops).
pub fn number_to_text(n: f64) -> String {
    format!("{n}")
}

impl Value {
    // An array reaching a coercion means a block was used where one value was
    // wanted. Excel takes the top-left rather than failing — the same implicit
    // intersection `eval_expr` applies — so the coercions agree with it instead
    // of introducing a second rule.
    /// Coerce to a number (empty → 0, bool → 0/1, text → parsed), or an error.
    pub fn as_number(&self) -> Result<f64, ErrorValue> {
        match self {
            // A function is not a value of this kind, and there is no
            // sensible coercion — Excel says #VALUE! too.
            Value::Lambda(_) => Err(ErrorValue::Value),
            Value::Array { cells, .. } => cells.first().map_or(Ok(0.0), Value::as_number),
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
            // A function is not a value of this kind, and there is no
            // sensible coercion — Excel says #VALUE! too.
            Value::Lambda(_) => Err(ErrorValue::Value),
            Value::Array { cells, .. } => cells
                .first()
                .map_or_else(|| Ok(String::new()), Value::as_text),
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
            // A function is not a value of this kind, and there is no
            // sensible coercion — Excel says #VALUE! too.
            Value::Lambda(_) => Err(ErrorValue::Value),
            Value::Array { cells, .. } => cells.first().map_or(Ok(false), Value::as_bool),
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
