//! A precedence-climbing (Pratt) parser over the token stream.

use crate::ast::{BinaryOp, Expr, UnaryOp};
use crate::error::FormulaError;
use crate::lex::{Token, tokenize};
use crate::reference::{CellReference, parse_a1, parse_a1_axis};
use crate::stored::StoredRef;

/// How deep an expression may nest before the parser refuses it.
///
/// The parser is recursive descent, so nesting depth is stack depth, and stack
/// exhaustion is not an error — it is `SIGABRT`, which no `Result` can carry
/// and no caller can catch. The limit therefore has to be here rather than in
/// whatever is calling.
///
/// **Sixty-four is measured, and happens to be Excel's own limit too.**
///
/// The first attempt used 256, reasoning that it matched the evaluator's
/// `MAX_DEPTH` and was four times Excel's documented ceiling. It aborted a test
/// thread. Measuring each shape in its own process — a stack overflow aborts
/// rather than unwinding, so it cannot be observed from inside — gave this, on
/// a megabyte of stack in a debug build:
///
/// | shape | dies at |
/// | --- | --- |
/// | `SUM(SUM(…))` | between 64 and 128 |
/// | `((((…))))` | between 128 and 192 |
/// | `----…` | past 256 |
///
/// Function nesting is the worst because it costs the most frames per level, so
/// it sets the bound. Sixty-four survives it with room, and is exactly what
/// Excel permits — which means the limit costs no compatibility at all.
///
/// The number is **sixty-five** because this counts *expression levels* and the
/// outermost expression is one of them: Excel's sixty-four levels of nesting sit
/// underneath it. Writing 64 here and calling it Excel-compatible would reject
/// `SUM(` nested exactly sixty-four deep, which Excel accepts — an off-by-one a
/// test caught.
///
/// Note this is **not** the evaluator's `MAX_DEPTH`, and they should not be
/// forced to agree: the evaluator's bounds a chain of *cell references*
/// (`A1`→`B1`→`C1`…), which no formula's own shape constrains.
pub const MAX_DEPTH: u32 = 65;

/// How long a chain of same-level operators may be — `1+1+1+…`.
///
/// A separate bound from [`MAX_DEPTH`], because it is a separate crash. The
/// parser handles a left-associative chain in a **loop**, so no amount of it
/// recurses and the depth counter never rises — but the *tree* it builds is a
/// left spine as long as the chain, and `Expr`'s `Drop` and `Display` are both
/// recursive. A quarter of a million terms therefore parses cleanly and then
/// aborts the process on the way out of scope, which is a stranger failure than
/// the one [`MAX_DEPTH`] prevents and reachable the same three ways.
///
/// Five hundred and twelve is **measured, not chosen**. The first attempt at
/// this used four thousand, reasoning from the format — SpreadsheetML caps a
/// formula at 8,192 characters, so a chain cannot hold many more operators than
/// that. It passed on the main thread, with eight megabytes of stack, and
/// aborted the moment a *test* thread ran it with two. A megabyte, which is
/// what a WebAssembly thread gets, dies at a thousand in a debug build.
///
/// So the bound is the largest that survives print-and-drop on the smallest
/// stack this code runs on, in the least favourable build. It is below what the
/// format permits, and that is a deliberate trade: a chain longer than this
/// gets a clear error, where before it got `SIGABRT`. Anyone summing five
/// hundred cells with `+` wants `SUM` anyway.
pub const MAX_CHAIN: u32 = 512;

/// Parse a formula body (the text after a leading `=`) into an [`Expr`].
pub fn parse(input: &str) -> Result<Expr, FormulaError> {
    let tokens = tokenize(input)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        depth: 0,
    };
    let expr = parser.parse_expr(0)?;
    if parser.pos != parser.tokens.len() {
        return Err(FormulaError::TrailingInput);
    }
    Ok(expr)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// How deep the recursion currently is. Every path that recurses passes
    /// through `parse_expr`, so counting there counts all of them.
    depth: u32,
}

const PREFIX_BP: u8 = 50;

fn binary_op(token: &Token) -> Option<(BinaryOp, u8, u8)> {
    // (op, left binding power, recurse power). Right-assoc `^` recurses at its own bp.
    Some(match token {
        Token::Eq => (BinaryOp::Equal, 5, 6),
        Token::Ne => (BinaryOp::NotEqual, 5, 6),
        Token::Lt => (BinaryOp::Less, 5, 6),
        Token::Le => (BinaryOp::LessEqual, 5, 6),
        Token::Gt => (BinaryOp::Greater, 5, 6),
        Token::Ge => (BinaryOp::GreaterEqual, 5, 6),
        Token::Amp => (BinaryOp::Concat, 10, 11),
        Token::Plus => (BinaryOp::Add, 20, 21),
        Token::Minus => (BinaryOp::Subtract, 20, 21),
        Token::Star => (BinaryOp::Multiply, 30, 31),
        Token::Slash => (BinaryOp::Divide, 30, 31),
        Token::Caret => (BinaryOp::Power, 40, 40),
        _ => return None,
    })
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn expect(&mut self, want: &Token) -> Result<(), FormulaError> {
        match self.advance() {
            Some(ref got) if got == want => Ok(()),
            Some(got) => Err(FormulaError::UnexpectedToken(format!("{got:?}"))),
            None => Err(FormulaError::UnexpectedToken("end of input".to_owned())),
        }
    }

    fn parse_expr(&mut self, min_bp: u8) -> Result<Expr, FormulaError> {
        // Every recursive path — a bracketed subexpression, a function
        // argument, an operand — comes back through here, so one counter
        // covers them all.
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return Err(FormulaError::TooDeep { limit: MAX_DEPTH });
        }
        let parsed = self.parse_expr_inner(min_bp);
        self.depth -= 1;
        parsed
    }

    fn parse_expr_inner(&mut self, min_bp: u8) -> Result<Expr, FormulaError> {
        let mut left = self.parse_prefix()?;
        let mut chain: u32 = 0;
        while let Some((op, lbp, rbp)) = self.peek().and_then(binary_op) {
            if lbp < min_bp {
                break;
            }
            // Each turn of this loop adds a level to the left spine of the tree
            // — no recursion here, and a tree whose recursive `Drop` would
            // exhaust the stack all the same.
            chain += 1;
            if chain > MAX_CHAIN {
                return Err(FormulaError::TooDeep { limit: MAX_CHAIN });
            }
            self.advance();
            let right = self.parse_expr(rbp)?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_prefix(&mut self) -> Result<Expr, FormulaError> {
        match self.peek() {
            Some(Token::Minus) => {
                self.advance();
                let operand = self.parse_expr(PREFIX_BP)?;
                Ok(Expr::Unary {
                    op: UnaryOp::Negate,
                    operand: Box::new(operand),
                })
            }
            Some(Token::Plus) => {
                self.advance();
                let operand = self.parse_expr(PREFIX_BP)?;
                Ok(Expr::Unary {
                    op: UnaryOp::Plus,
                    operand: Box::new(operand),
                })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, FormulaError> {
        let mut expr = self.parse_primary()?;
        while self.peek() == Some(&Token::Percent) {
            self.advance();
            expr = Expr::Unary {
                op: UnaryOp::Percent,
                operand: Box::new(expr),
            };
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, FormulaError> {
        match self.advance() {
            Some(Token::Number(n)) => Ok(Expr::Number(n)),
            Some(Token::Text(s)) => Ok(Expr::Text(s)),
            Some(Token::Error(s)) => Ok(Expr::Error(s)),
            Some(Token::LParen) => {
                let expr = self.parse_expr(0)?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Some(Token::QuotedSheet(name)) => {
                self.expect(&Token::Bang)?;
                let word = self.expect_word()?;
                let reference = self.reference_or_axis(&word, Some(name))?;
                self.maybe_range(reference)
            }
            // A bare `[Amount]`, which is how a formula inside the table
            // refers to its own columns.
            Some(Token::Brackets(spec)) => Ok(Expr::StructuredRef { table: None, spec }),
            Some(Token::Word(word)) => self.parse_word(word),
            Some(other) => Err(FormulaError::UnexpectedToken(format!("{other:?}"))),
            None => Err(FormulaError::UnexpectedToken("end of input".to_owned())),
        }
    }

    fn parse_word(&mut self, word: String) -> Result<Expr, FormulaError> {
        match self.peek() {
            Some(Token::LParen) => {
                self.advance();
                let args = self.parse_args()?;
                self.expect(&Token::RParen)?;
                let call = Expr::Function {
                    name: word.to_ascii_uppercase(),
                    args,
                };
                // `LAMBDA(x, x+1)(5)` — a call on what the call returned.
                self.maybe_invoke(call)
            }
            Some(Token::Bang) => {
                self.advance();
                let target = self.expect_word()?;
                let reference = self.reference_or_axis(&target, Some(word))?;
                self.maybe_range(reference)
            }
            // `Sales[Amount]` — a name followed immediately by a specifier.
            Some(Token::Brackets(_)) => {
                let Some(Token::Brackets(spec)) = self.advance() else {
                    unreachable!("peeked a Brackets token")
                };
                Ok(Expr::StructuredRef {
                    table: Some(word),
                    spec,
                })
            }
            _ if word.eq_ignore_ascii_case("TRUE") => Ok(Expr::Bool(true)),
            _ if word.eq_ignore_ascii_case("FALSE") => Ok(Expr::Bool(false)),
            _ => match parse_a1(&word) {
                Some(reference) => self.maybe_range(reference),
                // `A:A` is a whole-column reference, but a bare `A` is a
                // defined name. Only a following colon tells them apart, so the
                // axis form is tried nowhere else.
                None if self.peek() == Some(&Token::Colon) => match parse_a1_axis(&word, false) {
                    Some(reference) => self.maybe_range(reference),
                    None => Ok(Expr::Name(word)),
                },
                None => Ok(Expr::Name(word)),
            },
        }
    }

    /// Wrap `callee` in as many invocations as follow it.
    ///
    /// A loop rather than a single check, because `LAMBDA(x, LAMBDA(y, x+y))(1)(2)`
    /// is legal and currying is the reason LAMBDA returns a LAMBDA at all.
    fn maybe_invoke(&mut self, mut callee: Expr) -> Result<Expr, FormulaError> {
        while self.peek() == Some(&Token::LParen) {
            self.advance();
            let args = self.parse_args()?;
            self.expect(&Token::RParen)?;
            callee = Expr::Call {
                callee: Box::new(callee),
                args,
            };
        }
        Ok(callee)
    }

    /// A reference, or a range when a colon follows.
    ///
    /// Produced in the **absolute form** — stored against `(0, 0)`, where an
    /// offset and an address are the same number — because the parser reads
    /// text and cannot know which cell will hold it.
    /// [`Workbook::store_formula_at`] re-stores it against the cell that does.
    fn maybe_range(&mut self, first: CellReference) -> Result<Expr, FormulaError> {
        let first = StoredRef::absolute(&first);
        if self.peek() == Some(&Token::Colon) {
            self.advance();
            let second = StoredRef::absolute(&self.parse_ref_operand()?);
            Ok(Expr::Range(first, second))
        } else {
            Ok(Expr::Reference(first))
        }
    }

    /// A reference that may name only one axis, when a colon follows it.
    ///
    /// The colon is the whole test: without it `A` is a name and `$1` is not a
    /// reference at all.
    fn reference_or_axis(
        &mut self,
        word: &str,
        sheet: Option<String>,
    ) -> Result<CellReference, FormulaError> {
        if self.peek() == Some(&Token::Colon)
            && let Some(mut reference) = parse_a1_axis(word, false)
        {
            reference.sheet = sheet;
            return Ok(reference);
        }
        reference_from(word, sheet)
    }

    fn parse_ref_operand(&mut self) -> Result<CellReference, FormulaError> {
        match self.advance() {
            // The closing side of a range: an axis token here takes the *last*
            // row or column, so `A:A` spans the column rather than one cell.
            Some(Token::Word(word)) => end_reference_from(&word, None),
            Some(Token::QuotedSheet(name)) => {
                self.expect(&Token::Bang)?;
                let word = self.expect_word()?;
                end_reference_from(&word, Some(name))
            }
            Some(other) => Err(FormulaError::UnexpectedToken(format!("{other:?}"))),
            None => Err(FormulaError::UnexpectedToken("end of input".to_owned())),
        }
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>, FormulaError> {
        let mut args = Vec::new();
        if self.peek() == Some(&Token::RParen) {
            return Ok(args);
        }
        loop {
            // A hole between commas is an omitted argument, not a syntax
            // error: `XLOOKUP(x, a, b, , -1)` skips `if_not_found`.
            let omitted = matches!(self.peek(), Some(Token::Comma) | Some(Token::RParen));
            args.push(if omitted {
                Expr::Empty
            } else {
                self.parse_expr(0)?
            });
            match self.peek() {
                Some(Token::Comma) => {
                    self.advance();
                }
                _ => break,
            }
        }
        Ok(args)
    }

    fn expect_word(&mut self) -> Result<String, FormulaError> {
        match self.advance() {
            Some(Token::Word(word)) => Ok(word),
            Some(other) => Err(FormulaError::UnexpectedToken(format!("{other:?}"))),
            None => Err(FormulaError::UnexpectedToken("end of input".to_owned())),
        }
    }
}

fn reference_from(word: &str, sheet: Option<String>) -> Result<CellReference, FormulaError> {
    let mut reference =
        parse_a1(word).ok_or_else(|| FormulaError::InvalidReference(word.to_owned()))?;
    reference.sheet = sheet;
    Ok(reference)
}

/// The closing side of a range, where an unnamed axis takes its last index.
fn end_reference_from(word: &str, sheet: Option<String>) -> Result<CellReference, FormulaError> {
    let mut reference = parse_a1(word)
        .or_else(|| parse_a1_axis(word, true))
        .ok_or_else(|| FormulaError::InvalidReference(word.to_owned()))?;
    reference.sheet = sheet;
    Ok(reference)
}
