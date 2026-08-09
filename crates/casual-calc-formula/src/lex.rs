//! Tokenizer for Excel-style formula text (without the leading `=`).

use crate::error::FormulaError;

/// A formula token.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// A numeric literal.
    Number(f64),
    /// A string literal (already unescaped).
    Text(String),
    /// An error literal (`#REF!`, …).
    Error(String),
    /// A bare word: a function name, defined name, or A1 reference.
    Word(String),
    /// A single-quoted sheet name (already unescaped).
    QuotedSheet(String),
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `^`
    Caret,
    /// `&`
    Amp,
    /// A structured reference's bracketed specifier, with the outer brackets
    /// stripped: `Sales[Amount]` lexes as `Word("Sales")` then
    /// `Brackets("Amount")`.
    ///
    /// Lexed as one token rather than as punctuation because the contents are
    /// not an expression: they can hold spaces, `#` keywords and nested
    /// brackets (`[[#Headers],[Amount]]`), and tokenizing them individually
    /// would make the grammar ambiguous with array literals.
    Brackets(String),
    /// `%`
    Percent,
    /// `=`
    Eq,
    /// `<>`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `,`
    Comma,
    /// `:`
    Colon,
    /// `!`
    Bang,
}

const KNOWN_ERRORS: &[&str] = &[
    "#REF!", "#VALUE!", "#DIV/0!", "#N/A", "#NAME?", "#NULL!", "#NUM!", "#SPILL!",
];

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.'
}

/// Tokenize `input`, returning the token stream or a lexer error.
pub fn tokenize(input: &str) -> Result<Vec<Token>, FormulaError> {
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut tokens = Vec::new();

    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\r' | '\n' => i += 1,
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            ':' => {
                tokens.push(Token::Colon);
                i += 1;
            }
            '[' => {
                // Scan to the matching close, tracking depth so a nested
                // specifier is captured whole.
                let mut depth = 0usize;
                let start = i + 1;
                let mut end = None;
                while i < chars.len() {
                    match chars[i] {
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(i);
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                let Some(end) = end else {
                    return Err(FormulaError::UnexpectedToken(
                        "unterminated structured reference".to_owned(),
                    ));
                };
                tokens.push(Token::Brackets(chars[start..end].iter().collect()));
                i = end + 1;
            }
            '!' => {
                tokens.push(Token::Bang);
                i += 1;
            }
            '+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                i += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            '/' => {
                tokens.push(Token::Slash);
                i += 1;
            }
            '^' => {
                tokens.push(Token::Caret);
                i += 1;
            }
            '&' => {
                tokens.push(Token::Amp);
                i += 1;
            }
            '%' => {
                tokens.push(Token::Percent);
                i += 1;
            }
            '=' => {
                tokens.push(Token::Eq);
                i += 1;
            }
            '<' => {
                if chars.get(i + 1) == Some(&'=') {
                    tokens.push(Token::Le);
                    i += 2;
                } else if chars.get(i + 1) == Some(&'>') {
                    tokens.push(Token::Ne);
                    i += 2;
                } else {
                    tokens.push(Token::Lt);
                    i += 1;
                }
            }
            '>' => {
                if chars.get(i + 1) == Some(&'=') {
                    tokens.push(Token::Ge);
                    i += 2;
                } else {
                    tokens.push(Token::Gt);
                    i += 1;
                }
            }
            '"' => {
                let (text, next) = lex_string(&chars, i)?;
                tokens.push(Token::Text(text));
                i = next;
            }
            '\'' => {
                let (name, next) = lex_quoted_sheet(&chars, i)?;
                tokens.push(Token::QuotedSheet(name));
                i = next;
            }
            '#' => {
                let (error, next) = lex_error(&chars, i)?;
                tokens.push(Token::Error(error));
                i = next;
            }
            c if c.is_ascii_digit() || (c == '.' && next_is_digit(&chars, i)) => {
                let (number, next) = lex_number(&chars, i)?;
                tokens.push(Token::Number(number));
                i = next;
            }
            c if is_word_char(c) => {
                let start = i;
                while i < chars.len() && is_word_char(chars[i]) {
                    i += 1;
                }
                tokens.push(Token::Word(chars[start..i].iter().collect()));
            }
            other => return Err(FormulaError::UnexpectedChar(other)),
        }
    }

    Ok(tokens)
}

fn next_is_digit(chars: &[char], i: usize) -> bool {
    chars.get(i + 1).is_some_and(char::is_ascii_digit)
}

fn lex_string(chars: &[char], start: usize) -> Result<(String, usize), FormulaError> {
    let mut i = start + 1;
    let mut text = String::new();
    while i < chars.len() {
        if chars[i] == '"' {
            if chars.get(i + 1) == Some(&'"') {
                text.push('"');
                i += 2;
            } else {
                return Ok((text, i + 1));
            }
        } else {
            text.push(chars[i]);
            i += 1;
        }
    }
    Err(FormulaError::UnterminatedString)
}

fn lex_quoted_sheet(chars: &[char], start: usize) -> Result<(String, usize), FormulaError> {
    let mut i = start + 1;
    let mut name = String::new();
    while i < chars.len() {
        if chars[i] == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                name.push('\'');
                i += 2;
            } else {
                return Ok((name, i + 1));
            }
        } else {
            name.push(chars[i]);
            i += 1;
        }
    }
    Err(FormulaError::UnterminatedSheet)
}

fn lex_error(chars: &[char], start: usize) -> Result<(String, usize), FormulaError> {
    let mut i = start + 1;
    while i < chars.len()
        && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '/' | '?' | '!' | '_'))
    {
        i += 1;
    }
    let token: String = chars[start..i].iter().collect();
    if KNOWN_ERRORS.contains(&token.as_str()) {
        Ok((token, i))
    } else {
        Err(FormulaError::UnexpectedChar('#'))
    }
}

fn lex_number(chars: &[char], start: usize) -> Result<(f64, usize), FormulaError> {
    let mut i = start;
    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
        i += 1;
    }
    // Optional scientific exponent.
    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
        let mut j = i + 1;
        if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
            j += 1;
        }
        if j < chars.len() && chars[j].is_ascii_digit() {
            i = j;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
        }
    }
    let text: String = chars[start..i].iter().collect();
    text.parse::<f64>()
        .map(|n| (n, i))
        .map_err(|_| FormulaError::UnexpectedToken(text))
}
