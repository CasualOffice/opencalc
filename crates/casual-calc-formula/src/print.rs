//! Printing an [`Expr`] back to formula text, with the parentheses the reader
//! needs and no others.
//!
//! `parse(print(e)) == e` for every parseable `e` — a Phase 1A round-trip gate,
//! and the property that makes the bracket rules below correct rather than
//! merely plausible.
//!
//! # Why minimal, and not the fully-parenthesized printer this used to be
//!
//! This is what the user sees. A cell's formula is not stored as the text that
//! was typed — it is parsed, and the formula bar prints the tree back — so the
//! printer *is* the formula as far as anyone editing it is concerned. Printing
//! every operator in brackets turned `=1+2*3` into `=(1+(2*3))` the moment the
//! cell was reselected, and `=SUM(A1:A9)/COUNT(A1:A9)` into something with four
//! more brackets than it was written with. It is still the same formula and it
//! still computes the same answer, which is exactly why it went unnoticed: the
//! version that came back was correct, and nobody had typed it.
//!
//! It reaches the file too, so a workbook saved from here and opened in Excel
//! shows the rewritten text, permanently. A spreadsheet that quietly edits your
//! formulas is not a viable alternative to one that does not.
//!
//! # The rules
//!
//! Mirrored from the parser's binding powers in [`crate::parse`], because
//! "minimal" is only meaningful against a specific grammar. A child needs
//! brackets exactly when leaving them off would let the parser re-associate it:
//!
//! - a **left** child, when it binds no tighter on its right than its parent
//!   does on its left — which keeps `(2^3)^2` bracketed, since `^` is
//!   right-associative and `2^3^2` would come back as `2^(3^2)`;
//! - a **right** child, when it binds looser on its left than its parent does
//!   on its right — which keeps `1-(2-3)` bracketed and lets `1+2*3` go bare.
//!
//! Prefix `-` and `+` bind tighter than every binary operator (so `-2^2` is
//! `(-2)^2`, as in Excel), and postfix `%` binds tighter still.

use core::fmt;

use crate::ast::{BinaryOp, Expr, UnaryOp};

/// The prefix operators' binding power, from [`crate::parse`].
const PREFIX_BP: u8 = 50;

/// What binds tighter than any operator: a literal, a reference, a call — and
/// postfix `%`, which the parser reads before it considers any operator at all.
const ATOM_BP: u8 = u8::MAX;

/// The (left, right) binding powers the parser gives a binary operator.
///
/// Kept identical to `binary_op` in [`crate::parse`]; the round-trip gate is
/// what holds the two in step, since any disagreement shows up as an expression
/// that does not survive being printed and read back.
fn binary_bp(op: BinaryOp) -> (u8, u8) {
    match op {
        BinaryOp::Equal
        | BinaryOp::NotEqual
        | BinaryOp::Less
        | BinaryOp::LessEqual
        | BinaryOp::Greater
        | BinaryOp::GreaterEqual => (5, 6),
        BinaryOp::Concat => (10, 11),
        BinaryOp::Add | BinaryOp::Subtract => (20, 21),
        BinaryOp::Multiply | BinaryOp::Divide => (30, 31),
        // Equal powers, which is what makes it right-associative.
        BinaryOp::Power => (40, 40),
    }
}

/// How tightly an expression binds on each side, as the parser sees it.
fn bp(expr: &Expr) -> (u8, u8) {
    match expr {
        Expr::Binary { op, .. } => binary_bp(*op),
        // Postfix, so the parser has already taken it before any operator is
        // considered: it never needs brackets to hold together.
        Expr::Unary {
            op: UnaryOp::Percent,
            ..
        } => (ATOM_BP, ATOM_BP),
        Expr::Unary { .. } => (PREFIX_BP, PREFIX_BP),
        _ => (ATOM_BP, ATOM_BP),
    }
}

/// Write `child`, bracketed only if `needs` says the parser would otherwise
/// read it differently.
fn write_child(f: &mut fmt::Formatter<'_>, child: &Expr, needs: bool) -> fmt::Result {
    if needs {
        write!(f, "({child})")
    } else {
        write!(f, "{child}")
    }
}

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
            // Verbatim: the point of keeping it is that it goes back out
            // exactly as it came in.
            Expr::Raw(text) => write!(f, "{text}"),
            // Prints as nothing, which is exactly how it was written.
            Expr::Empty => Ok(()),
            Expr::Call { callee, args } => {
                write!(f, "{callee}(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{a}")?;
                }
                f.write_str(")")
            }
            Expr::Name(name) => f.write_str(name),
            Expr::StructuredRef { table, spec } => {
                if let Some(table) = table {
                    f.write_str(table)?;
                }
                write!(f, "[{spec}]")
            }
            Expr::Unary { op, operand } => match op {
                // The operand was parsed at `PREFIX_BP`, so anything binding
                // looser than that — every binary operator — would be cut short
                // without brackets: `-(1+2)` cannot be written `-1+2`.
                UnaryOp::Negate => {
                    f.write_str("-")?;
                    write_child(f, operand, bp(operand).0 < PREFIX_BP)
                }
                UnaryOp::Plus => {
                    f.write_str("+")?;
                    write_child(f, operand, bp(operand).0 < PREFIX_BP)
                }
                // Postfix, and the parser applies it to a *primary*, so its
                // operand must be one. `(-a)%` is not `-a%`: the second is a
                // negated percentage.
                UnaryOp::Percent => {
                    write_child(f, operand, bp(operand).1 < ATOM_BP)?;
                    f.write_str("%")
                }
            },
            Expr::Binary { op, left, right } => {
                let (lbp, rbp) = binary_bp(*op);
                write_child(f, left, bp(left).1 <= lbp)?;
                f.write_str(binary_symbol(*op))?;
                write_child(f, right, bp(right).0 < rbp)
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
