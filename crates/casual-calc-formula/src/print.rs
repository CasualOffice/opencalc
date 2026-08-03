//! Canonical pretty-printing of an [`Expr`] back to formula text.
//!
//! The output is fully parenthesized around operators, so `parse(print(e)) == e`
//! for every parseable `e` (a Phase 1A round-trip gate). It is canonical, not
//! minimal; a minimal printer can replace it later without changing the AST.

use core::fmt;

use crate::ast::{BinaryOp, Expr, UnaryOp};

fn binary_symbol(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Subtract => "-",
        BinaryOp::Multiply => "*",
        BinaryOp::Divide => "/",
        BinaryOp::Power => "^",
        BinaryOp::Concat => "&",
        BinaryOp::Equal => "=",
        BinaryOp::NotEqual => "<>",
        BinaryOp::Less => "<",
        BinaryOp::LessEqual => "<=",
        BinaryOp::Greater => ">",
        BinaryOp::GreaterEqual => ">=",
    }
}

fn write_string_literal(f: &mut fmt::Formatter<'_>, text: &str) -> fmt::Result {
    f.write_str("\"")?;
    f.write_str(&text.replace('"', "\"\""))?;
    f.write_str("\"")
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Number(n) => write!(f, "{n}"),
            Expr::Bool(b) => f.write_str(if *b { "TRUE" } else { "FALSE" }),
            Expr::Text(s) => write_string_literal(f, s),
            Expr::Error(s) => f.write_str(s),
            Expr::Reference(r) => write!(f, "{r}"),
            Expr::Range(a, b) => write!(f, "{a}:{b}"),
            Expr::Name(name) => f.write_str(name),
            Expr::Unary { op, operand } => match op {
                UnaryOp::Negate => write!(f, "(-{operand})"),
                UnaryOp::Plus => write!(f, "(+{operand})"),
                UnaryOp::Percent => write!(f, "({operand}%)"),
            },
            Expr::Binary { op, left, right } => {
                write!(f, "({left}{}{right})", binary_symbol(*op))
            }
            Expr::Function { name, args } => {
                f.write_str(name)?;
                f.write_str("(")?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{arg}")?;
                }
                f.write_str(")")
            }
        }
    }
}
