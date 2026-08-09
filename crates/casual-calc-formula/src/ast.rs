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
    /// Text this parser cannot read, kept exactly as written.
    ///
    /// Only reachable where dropping the expression would lose data that the
    /// file legitimately contains — today, a `definedName`'s `refersTo`. A name
    /// whose target failed to parse used to be discarded outright, so every
    /// workbook carrying `Print_Titles` (`Sheet1!$1:$2`, a whole-row reference
    /// this parser does not yet support) lost it on save.
    ///
    /// It prints back verbatim and evaluates to `#NAME?`: the file survives the
    /// round trip, and a formula that depends on it fails visibly rather than
    /// quietly resolving to something else.
    Raw(String),
    /// A structured (table) reference: `Sales[Amount]`, or `[Amount]` inside
    /// the table itself.
    ///
    /// The specifier is kept as written rather than resolved to a range at
    /// parse time, because resolving needs the table's geometry — which columns
    /// exist, where the header and totals rows are — and the parser has no
    /// access to the workbook. The evaluator resolves it; the printer renders
    /// it back unchanged, so a formula survives a round trip even when no table
    /// of that name is present.
    StructuredRef {
        /// The table name, absent for a reference from inside the table.
        table: Option<String>,
        /// The bracketed specifier, without the outer brackets.
        spec: String,
    },
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
