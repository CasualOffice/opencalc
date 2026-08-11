//! `casual-calc-formula` — the formula *language*: tokenizer, parser, AST, A1
//! reference algebra, and a canonical pretty-printer.
//!
//! This crate is available from Phase 1A: import parses `<f>` text into an
//! [`Expr`] and the model stores it (the reserved calc seam). It contains **no
//! evaluation** — that is `casual-calc-eval` in Phase 2. Keeping parsing here,
//! below the engine, is what lets import and the transaction layer build and
//! rewrite formulas without depending on the calc engine.
//!
//! Current subset: literals (number/bool/text/error), A1 cell references with
//! `$` anchors and sheet qualification (`Sheet2!A1`, `'My Sheet'!B2`), cell
//! ranges (`A1:B2`), defined names, function calls, unary `+`/`-`/`%`, and the
//! binary arithmetic/comparison/concat operators with correct precedence.
//! Deferred: R1C1, full row/column ranges (`A:A`), 3-D refs, structured (table)
//! references, and union/intersection.
//!
//! See `docs/40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md`.

mod ast;
mod error;
pub mod future;
mod lex;
mod parse;
mod print;
mod reference;
mod refscan;
mod rewrite;

pub use ast::{BinaryOp, Expr, UnaryOp};
pub use error::FormulaError;
pub use future::{
    FUTURE_FUNCTIONS, is_future_function, qualify_bound_names, qualify_future_functions,
    strip_bound_name_prefixes, strip_future_prefixes,
};
pub use parse::parse;
pub use reference::{CellReference, MAX_COL, MAX_ROW, column_to_letters, parse_a1};
pub use refscan::{RefSpan, reference_spans};
pub use rewrite::{rename_sheet_references, shift_references};

#[cfg(test)]
mod tests;
