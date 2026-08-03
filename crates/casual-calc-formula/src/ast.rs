//! The formula AST. Serializable so the model can store it in a per-workbook
//! arena (`docs/22-NORMALIZED-SCHEMA.md`). Evaluation lives in the calc engine
//! (`casual-calc-eval`, Phase 2).

use serde::{Deserialize, Serialize};

use crate::reference::CellReference;

/// A prefix/postfix unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnaryOp {
    /// Prefix `-`.
    Negate,
    /// Prefix `+`.
    Plus,
    /// Postfix `%`.
    Percent,
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BinaryOp {
    /// `+`
    Add,
    /// `-`
    Subtract,
    /// `*`
    Multiply,
    /// `/`
    Divide,
    /// `^`
    Power,
    /// `&`
    Concat,
    /// `=`
    Equal,
    /// `<>`
    NotEqual,
    /// `<`
    Less,
    /// `<=`
    LessEqual,
    /// `>`
    Greater,
    /// `>=`
    GreaterEqual,
}

/// A parsed formula expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Expr {
    /// A numeric literal.
    Number(f64),
    /// A boolean literal (`TRUE`/`FALSE`).
    Bool(bool),
    /// A string literal.
    Text(String),
    /// An error literal (`#REF!`, …).
    Error(String),
    /// A cell reference.
    Reference(CellReference),
    /// A range between two references (`A1:B2`).
    Range(CellReference, CellReference),
    /// A defined name.
    Name(String),
    /// A unary operation.
    Unary {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        operand: Box<Expr>,
    },
    /// A binary operation.
    Binary {
        /// The operator.
        op: BinaryOp,
        /// The left operand.
        left: Box<Expr>,
        /// The right operand.
        right: Box<Expr>,
    },
    /// A function call.
    Function {
        /// The function name (upper-cased).
        name: String,
        /// The arguments.
        args: Vec<Expr>,
    },
}
