//! The formula AST. Serializable so the model can store it in a per-workbook
//! arena (`docs/22-NORMALIZED-SCHEMA.md`). Evaluation lives in the calc engine
//! (`casual-calc-eval`, Phase 2).

use serde::{Deserialize, Serialize};

use crate::stored::StoredRef;

/// A prefix/postfix unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// A cell reference **as stored**: relative to the cell holding this
    /// formula, unless `$`-anchored (`PERF-11`).
    ///
    /// Relative is what lets one tree serve a whole filled-down column: `A1*2`
    /// in `B1` and `A2*2` in `B2` are one shape — one column left — so the
    /// arena keeps one tree rather than one per row.
    ///
    /// Resolve against the holding cell with [`StoredRef::resolve`] to get an
    /// address. The type is what makes that impossible to forget, which is the
    /// mitigation: a delta read as an address does not crash, it computes a
    /// plausible wrong answer in a spreadsheet.
    ///
    /// A tree at [`ABSOLUTE`](crate::stored::ABSOLUTE) is the absolute form —
    /// what the parser produces and what a snapshot carries.
    Reference(StoredRef),
    /// A range between two stored references (`A1:B2`).
    Range(StoredRef, StoredRef),
    /// A defined name.
    Name(String),
    /// Calling the result of an expression: `LAMBDA(x, x+1)(5)`.
    ///
    /// Distinct from [`Self::Function`], which names a builtin. A `LAMBDA`
    /// written inline and invoked immediately is how the feature is taught and
    /// how it is tested before being given a name, so the parser has to accept
    /// a call where a value is expected.
    Call {
        /// What is being called — a `LAMBDA` expression or a defined name.
        callee: Box<Expr>,
        /// The arguments.
        args: Vec<Expr>,
    },
    /// An argument left out: `XLOOKUP(x, a, b, , -1)`.
    ///
    /// Excel allows any optional argument to be skipped this way, and files
    /// written elsewhere rely on it. Modelled as an expression rather than by
    /// shortening the argument list, because the *position* is what carries the
    /// meaning — dropping the hole would shift every later argument one place
    /// left and silently change which parameter they are.
    Empty,
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

impl Expr {
    /// A structural fingerprint, for interning identical formulas.
    ///
    /// [`Expr`] cannot derive `Hash`, because it carries an `f64` and floats
    /// have no total equality — `NaN != NaN`, and `0.0 == -0.0` while their
    /// bits differ. So the numbers are hashed **by their bits**, which makes
    /// this a fingerprint of the written formula rather than of its
    /// mathematical value: `=0` and `=-0` fingerprint apart, and correctly, as
    /// they are different text that must round-trip differently.
    ///
    /// Only ever a *hint*. Equality still decides — a collision costs a
    /// comparison, never a wrong answer — which is what lets this be a cheap
    /// hash rather than a canonical serialisation.
    ///
    /// Written as an exhaustive match on purpose: adding a variant without
    /// deciding how it fingerprints will not compile, where a derive or a
    /// catch-all arm would silently make every new variant collide.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        use std::hash::Hasher;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.hash_into(&mut hasher);
        hasher.finish()
    }

    fn hash_into(&self, state: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        // The discriminant first, so `Text("1")` and `Name("1")` differ.
        std::mem::discriminant(self).hash(state);
        match self {
            Expr::Number(n) => n.to_bits().hash(state),
            Expr::Bool(b) => b.hash(state),
            Expr::Text(s) | Expr::Error(s) | Expr::Name(s) | Expr::Raw(s) => s.hash(state),
            Expr::Reference(r) => r.hash(state),
            Expr::Range(a, b) => {
                a.hash(state);
                b.hash(state);
            }
            Expr::Call { callee, args } => {
                callee.hash_into(state);
                args.len().hash(state);
                for arg in args {
                    arg.hash_into(state);
                }
            }
            Expr::Empty => {}
            Expr::StructuredRef { table, spec } => {
                table.hash(state);
                spec.hash(state);
            }
            Expr::Unary { op, operand } => {
                op.hash(state);
                operand.hash_into(state);
            }
            Expr::Binary { op, left, right } => {
                op.hash(state);
                left.hash_into(state);
                right.hash_into(state);
            }
            Expr::Function { name, args } => {
                name.hash(state);
                args.len().hash(state);
                for arg in args {
                    arg.hash_into(state);
                }
            }
        }
    }
}
