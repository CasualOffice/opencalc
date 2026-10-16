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

/// How many elements an array constant (`{1,2;3,4}`) may hold.
///
/// A constant is untrusted input — typed, imported, or arriving on the wire —
/// and it is materialised whole the moment it is evaluated. Excel bounds a
/// formula at 8,192 characters, which cannot hold more than about 4,096
/// one-digit elements; this is four times that, so no formula Excel can write
/// is refused, and it is far below the evaluator's own array ceiling.
pub const MAX_ARRAY_CONSTANT: usize = 16_384;

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

/// The function `A1#` is stored as: `_xlfn.ANCHORARRAY(A1)` in the file.
pub const ANCHORARRAY: &str = "ANCHORARRAY";

/// The function `@x` is stored as: `_xlfn.SINGLE(x)` in the file.
pub const SINGLE: &str = "SINGLE";

fn binary_op(token: &Token) -> Option<(BinaryOp, u8, u8)> {
    // (op, left binding power, recurse power). Every operator is left-associative,
    // `^` included: Excel reads `2^3^2` as `(2^3)^2` = 64 (XL-FEAT-135).
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
        Token::Caret => (BinaryOp::Power, 40, 41),
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
            // `@A1:A5`, `@SUM(…)`: implicit intersection of the operand that
            // follows, stored — as Excel stores it — as `SINGLE(…)`. It takes
            // a primary (with its `%` and `#`), never a prefixed expression,
            // so `@@A1` and `@-A1` are refused and the recursion is bounded.
            Some(Token::At) => {
                self.advance();
                let operand = self.parse_postfix()?;
                Ok(Expr::Function {
                    name: SINGLE.to_owned(),
                    args: vec![operand],
                })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, FormulaError> {
        let mut expr = self.parse_primary()?;
        // `A1#`: the range the anchor spills into, stored — as Excel stores it
        // — as `ANCHORARRAY(A1)`. Only a single cell or a name can anchor a
        // spill; `A1:B2#` and `SUM(A1)#` are not references Excel accepts.
        if self.peek() == Some(&Token::Hash) {
            let anchors = match &expr {
                Expr::Reference(r) => !r.row_implicit && !r.col_implicit,
                Expr::Name(_) => true,
                _ => false,
            };
            if !anchors {
                return Err(FormulaError::UnexpectedToken(
                    "# after something that is not a cell or a name".to_owned(),
                ));
            }
            self.advance();
            expr = Expr::Function {
                name: ANCHORARRAY.to_owned(),
                args: vec![expr],
            };
        }
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
            Some(Token::LBrace) => self.parse_array(),
            Some(Token::LParen) => {
                let expr = self.parse_expr(0)?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Some(Token::QuotedSheet(name)) => {
                self.expect(&Token::Bang)?;
                // `'Jan 2026:Dec 2026'!B5` — a colon cannot be part of a sheet
                // name (Excel forbids it), so one inside the quotes is the 3-D
                // span, written as one quoted pair.
                if let Some((first, last)) = name.split_once(':') {
                    if first.is_empty() || last.is_empty() {
                        return Err(FormulaError::InvalidReference(name));
                    }
                    return self.parse_3d(first.to_owned(), last.to_owned());
                }
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
        // `Sheet1:Sheet3!A1`: two sheet names and a bang. Looked for first,
        // because `Sheet1` on its own would otherwise be tried as a reference.
        if self.peek() == Some(&Token::Colon)
            && matches!(self.tokens.get(self.pos + 1), Some(Token::Word(_)))
            && self.tokens.get(self.pos + 2) == Some(&Token::Bang)
        {
            self.advance();
            let Some(Token::Word(last)) = self.advance() else {
                unreachable!("peeked a word")
            };
            self.advance();
            return self.parse_3d(word, last);
        }
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

    /// The area of a 3-D reference, after `first:last!`: a cell or a range on
    /// each sheet of the span. The far corner names no sheet of its own.
    fn parse_3d(&mut self, first: String, last: String) -> Result<Expr, FormulaError> {
        let word = self.expect_word()?;
        let corner = self.reference_or_axis(&word, None)?;
        let (start, end) = match self.maybe_range(corner)? {
            Expr::Reference(start) => (start, None),
            Expr::Range(start, end) if end.sheet.is_none() => (start, Some(end)),
            other => {
                return Err(FormulaError::InvalidReference(format!(
                    "{first}:{last}!{other}"
                )));
            }
        };
        Ok(crate::ast::Reference3d {
            first,
            last,
            start,
            end,
        }
        .into_expr())
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
        if self.peek() == Some(&Token::Colon) {
            self.advance();
            let second = self.parse_ref_operand()?;
            // Both ends must be the same shape: two cells, two columns or two
            // rows. Excel refuses `A1:3` and `A:B2`, and a mixed range has no
            // sensible meaning to evaluate (XL-FEAT-016).
            if (first.row_implicit, first.col_implicit)
                != (second.row_implicit, second.col_implicit)
            {
                return Err(FormulaError::InvalidReference(format!("{first}:{second}")));
            }
            let (first, second) = (StoredRef::absolute(&first), StoredRef::absolute(&second));
            Ok(Expr::Range(first, second))
        } else {
            Ok(Expr::Reference(StoredRef::absolute(&first)))
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

    /// The rest of an array constant, after its `{`.
    ///
    /// One pass, element by element: the cost is linear in the constant and the
    /// count is checked as each element is read, so an oversized constant is
    /// refused before it is built rather than after.
    fn parse_array(&mut self) -> Result<Expr, FormulaError> {
        let mut rows: Vec<Vec<Expr>> = Vec::new();
        let mut row: Vec<Expr> = Vec::new();
        let mut count = 0usize;
        loop {
            row.push(self.array_element()?);
            count += 1;
            if count > MAX_ARRAY_CONSTANT {
                return Err(FormulaError::TooLarge {
                    limit: MAX_ARRAY_CONSTANT,
                });
            }
            let end = match self.advance() {
                Some(Token::Comma) => continue,
                Some(Token::Semicolon) => false,
                Some(Token::RBrace) => true,
                Some(other) => {
                    return Err(FormulaError::UnexpectedToken(format!(
                        "{other:?} in an array constant"
                    )));
                }
                None => {
                    return Err(FormulaError::UnexpectedToken(
                        "end of input in an array constant".to_owned(),
                    ));
                }
            };
            // Every row as wide as the first: Excel has no ragged constant.
            if rows.first().is_some_and(|first| first.len() != row.len()) {
                return Err(FormulaError::UnexpectedToken(
                    "array constant rows of different lengths".to_owned(),
                ));
            }
            rows.push(std::mem::take(&mut row));
            if end {
                return Ok(Expr::Array(rows));
            }
        }
    }

    /// One element of an array constant: a number (optionally negated), text,
    /// a logical or an error value — and nothing else, as in Excel.
    fn array_element(&mut self) -> Result<Expr, FormulaError> {
        match self.advance() {
            Some(Token::Number(n)) => Ok(Expr::Number(n)),
            Some(Token::Minus) => match self.advance() {
                Some(Token::Number(n)) => Ok(Expr::Number(-n)),
                other => Err(FormulaError::UnexpectedToken(format!(
                    "-{other:?} in an array constant"
                ))),
            },
            Some(Token::Text(s)) => Ok(Expr::Text(s)),
            Some(Token::Error(s)) => Ok(Expr::Error(s)),
            Some(Token::Word(w)) if w.eq_ignore_ascii_case("TRUE") => Ok(Expr::Bool(true)),
            Some(Token::Word(w)) if w.eq_ignore_ascii_case("FALSE") => Ok(Expr::Bool(false)),
            other => Err(FormulaError::UnexpectedToken(format!(
                "{other:?} in an array constant"
            ))),
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
