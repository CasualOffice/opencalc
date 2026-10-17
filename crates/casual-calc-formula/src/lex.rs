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
    /// `{`, opening an array constant.
    LBrace,
    /// `}`
    RBrace,
    /// `;`, the row separator inside an array constant. Excel's file grammar
    /// is the en-US one, so `;` means nothing else.
    Semicolon,
    /// `#` written straight after a reference or a name: the spilled-range
    /// operator, `A1#`. Anywhere else `#` starts an error literal.
    Hash,
    /// `@`, the implicit-intersection operator. A structured reference's own
    /// `[@Col]` is inside its brackets and never lexes as this.
    At,
}

const KNOWN_ERRORS: &[&str] = &[
    "#REF!", "#VALUE!", "#DIV/0!", "#N/A", "#NAME?", "#NULL!", "#NUM!", "#SPILL!",
    // A pending custom function's placeholder (`MACRO-04`), so `#BUSY!` typed
    // or read back is the error rather than a name.
    "#BUSY!",
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
            '{' => {
                tokens.push(Token::LBrace);
                i += 1;
            }
            '}' => {
                tokens.push(Token::RBrace);
                i += 1;
            }
            ';' => {
                tokens.push(Token::Semicolon);
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
            // Straight after a word character — `A1#`, `Sheet2!B3#`, `Name#` —
            // `#` is the spill operator. An error literal never follows a word
            // without an operator or a bracket between them, so `#N/A`,
            // `IF(A1,#REF!)` and `A1#+#SPILL!` all lex as they always did.
            '#' if i > 0 && is_word_char(chars[i - 1]) => {
                tokens.push(Token::Hash);
                i += 1;
            }
            '@' => {
                tokens.push(Token::At);
                i += 1;
            }
            '#' => {
                let (error, next) = lex_error(&chars, i)?;
                tokens.push(Token::Error(error));
                i = next;
            }
            // A row number beside a colon is one side of a whole-row reference
            // (`1:1`, `5:10`, `Sheet1!3:3`), not a number (XL-FEAT-016). Lexed
            // as a word, as `$1` already is, so the parser treats the two
            // spellings alike. Only a pure digit run qualifies: `1.5:2` is still
            // a number and then a syntax error, as in Excel.
            c if c.is_ascii_digit() && row_beside_colon(&chars, i).is_some() => {
                let end = row_beside_colon(&chars, i).unwrap_or(i);
                tokens.push(Token::Word(chars[i..end].iter().collect()));
                i = end;
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

/// The end of a digit run starting at `start`, when the run is a whole word of
/// digits touching a colon on either side — a row number in `1:1`.
fn row_beside_colon(chars: &[char], start: usize) -> Option<usize> {
    let mut end = start;
    while end < chars.len() && is_word_char(chars[end]) {
        end += 1;
    }
    let digits = chars[start..end].iter().all(char::is_ascii_digit);
    let after = chars.get(end) == Some(&':');
    let before = start > 0 && chars[start - 1] == ':';
    (digits && (after || before)).then_some(end)
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

/// The error literal starting at `start`: the longest known one the text
/// begins with.
///
/// Matched against the list rather than by scanning a run of the characters
/// errors are made of, because `/` is one of those characters and is also
/// division: the scan read `#N/A/2` as the single unknown token `#N/A/`, so
/// a formula dividing an error refused to parse — and the printer writes
/// exactly that text for `(#N/A)/2` (found by fuzzing the round trip).
fn lex_error(chars: &[char], start: usize) -> Result<(String, usize), FormulaError> {
    KNOWN_ERRORS
        .iter()
        .filter(|known| {
            let len = known.chars().count();
            chars.len() >= start + len
                && known.chars().eq(chars[start..start + len].iter().copied())
        })
        .max_by_key(|known| known.len())
        .map(|known| ((*known).to_owned(), start + known.chars().count()))
        .ok_or(FormulaError::UnexpectedChar('#'))
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
    // A literal past the largest double (`1e400`) is not a number: it would
    // become infinity and print as `inf` — a name — so the formula would not
    // read back, and inside an array constant would not read at all. Refused
    // here, as Excel refuses it (found by fuzzing the round trip).
    match text.parse::<f64>() {
        Ok(n) if n.is_finite() => Ok((n, i)),
        _ => Err(FormulaError::UnexpectedToken(text)),
    }
}
