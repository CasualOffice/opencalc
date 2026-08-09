//! A precedence-climbing (Pratt) parser over the token stream.

use crate::ast::{BinaryOp, Expr, UnaryOp};
use crate::error::FormulaError;
use crate::lex::{Token, tokenize};
use crate::reference::{CellReference, parse_a1};

/// Parse a formula body (the text after a leading `=`) into an [`Expr`].
pub fn parse(input: &str) -> Result<Expr, FormulaError> {
    let tokens = tokenize(input)?;
    let mut parser = Parser { tokens, pos: 0 };
    let expr = parser.parse_expr(0)?;
    if parser.pos != parser.tokens.len() {
        return Err(FormulaError::TrailingInput);
    }
    Ok(expr)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
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
        let mut left = self.parse_prefix()?;
        while let Some((op, lbp, rbp)) = self.peek().and_then(binary_op) {
            if lbp < min_bp {
                break;
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
                let reference = reference_from(&word, Some(name))?;
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
                Ok(Expr::Function {
                    name: word.to_ascii_uppercase(),
                    args,
                })
            }
            Some(Token::Bang) => {
                self.advance();
                let target = self.expect_word()?;
                let reference = reference_from(&target, Some(word))?;
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
                None => Ok(Expr::Name(word)),
            },
        }
    }

    fn maybe_range(&mut self, first: CellReference) -> Result<Expr, FormulaError> {
        if self.peek() == Some(&Token::Colon) {
            self.advance();
            let second = self.parse_ref_operand()?;
            Ok(Expr::Range(first, second))
        } else {
            Ok(Expr::Reference(first))
        }
    }

    fn parse_ref_operand(&mut self) -> Result<CellReference, FormulaError> {
        match self.advance() {
            Some(Token::Word(word)) => reference_from(&word, None),
            Some(Token::QuotedSheet(name)) => {
                self.expect(&Token::Bang)?;
                let word = self.expect_word()?;
                reference_from(&word, Some(name))
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
            args.push(self.parse_expr(0)?);
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
