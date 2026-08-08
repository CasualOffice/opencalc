//! Locate the cell references *inside formula text*, with their character
//! spans.
//!
//! The parser turns text into an AST and throws the text positions away, which
//! is what an editor needs: to tint `B2:D9` in a formula and outline that block
//! on the grid, it must know **where in the string** the reference sits. A host
//! that re-derived this with its own regex would drift from what the engine
//! actually parses — the same name is a reference here and a function call one
//! character later (`SUM(` vs `SUM`), and neither is a reference inside a
//! string literal. So the scan lives next to the parser and ships with it.
//!
//! Positions are **character** indices (not bytes), because that is what a DOM
//! text field counts in.

use crate::reference::{CellReference, parse_a1};

/// One reference found in formula text: where it sits in the string, and the
/// cell block it covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefSpan {
    /// Character index of the first character of the reference.
    pub start: usize,
    /// Character index one past its last character.
    pub end: usize,
    /// Qualifying sheet name, if the reference carried one.
    pub sheet: Option<String>,
    /// Top row of the covered block (zero-based).
    pub row0: u32,
    /// Left column of the covered block (zero-based).
    pub col0: u32,
    /// Bottom row, inclusive. Equal to `row0` for a single cell.
    pub row1: u32,
    /// Right column, inclusive. Equal to `col0` for a single cell.
    pub col1: u32,
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.'
}

/// Every cell reference in `text`, in order, with its character span.
///
/// `text` may include the leading `=`. String literals are skipped, a word
/// followed by `(` is a function call rather than a reference, and `A1:B2` is
/// reported as one span covering the whole range.
#[must_use]
pub fn reference_spans(text: &str) -> Vec<RefSpan> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<RefSpan> = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            // Skip a "…" literal (doubled quotes escape a quote inside it).
            '"' => {
                i += 1;
                while i < chars.len() {
                    if chars[i] == '"' {
                        if chars.get(i + 1) == Some(&'"') {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            // A '…'!ref sheet qualifier: consume the quoted name and let the
            // reference after the `!` be picked up with this as its sheet.
            '\'' => {
                let start = i;
                let mut name = String::new();
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\'' {
                        if chars.get(i + 1) == Some(&'\'') {
                            name.push('\'');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    name.push(chars[i]);
                    i += 1;
                }
                if chars.get(i) == Some(&'!') {
                    i += 1;
                    scan_from(&chars, &mut i, start, Some(name), &mut out);
                }
            }
            c if is_word_char(c) => {
                let start = i;
                let word_end = word_end(&chars, i);
                // `Sheet1!A1` — an unquoted qualifier.
                if chars.get(word_end) == Some(&'!') {
                    let name: String = chars[start..word_end].iter().collect();
                    i = word_end + 1;
                    scan_from(&chars, &mut i, start, Some(name), &mut out);
                } else {
                    i = start;
                    scan_from(&chars, &mut i, start, None, &mut out);
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// Read the reference (or range) beginning at `*i`, recording it as spanning
/// from `span_start` (which may be earlier, when a sheet qualifier preceded
/// it). Advances `*i` past whatever was consumed.
fn scan_from(
    chars: &[char],
    i: &mut usize,
    span_start: usize,
    sheet: Option<String>,
    out: &mut Vec<RefSpan>,
) {
    let first_start = *i;
    let first_end = word_end(chars, first_start);
    if first_end == first_start {
        *i = first_start + 1;
        return;
    }
    let first: String = chars[first_start..first_end].iter().collect();
    // A name followed by `(` is a function call, never a reference.
    if chars.get(first_end) == Some(&'(') {
        *i = first_end;
        return;
    }
    let Some(a) = parse_a1(&first) else {
        *i = first_end;
        return;
    };
    // `A1:B2` — a range, provided the far side parses too.
    if chars.get(first_end) == Some(&':') {
        let second_start = first_end + 1;
        let second_end = word_end(chars, second_start);
        let second: String = chars[second_start..second_end].iter().collect();
        if let Some(b) = parse_a1(&second) {
            out.push(span_of(span_start, second_end, sheet, &a, &b));
            *i = second_end;
            return;
        }
    }
    out.push(span_of(span_start, first_end, sheet, &a, &a));
    *i = first_end;
}

fn span_of(
    start: usize,
    end: usize,
    sheet: Option<String>,
    a: &CellReference,
    b: &CellReference,
) -> RefSpan {
    RefSpan {
        start,
        end,
        sheet,
        row0: a.row.min(b.row),
        col0: a.col.min(b.col),
        row1: a.row.max(b.row),
        col1: a.col.max(b.col),
    }
}

/// Index one past the run of word characters starting at `from`.
fn word_end(chars: &[char], from: usize) -> usize {
    let mut i = from;
    while i < chars.len() && is_word_char(chars[i]) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str) -> Vec<(usize, usize, u32, u32, u32, u32)> {
        reference_spans(text)
            .into_iter()
            .map(|r| (r.start, r.end, r.row0, r.col0, r.row1, r.col1))
            .collect()
    }

    #[test]
    fn finds_cells_and_ranges_with_their_spans() {
        // =SUM(B2:D9)+A1
        assert_eq!(
            spans("=SUM(B2:D9)+A1"),
            vec![(5, 10, 1, 1, 8, 3), (12, 14, 0, 0, 0, 0)]
        );
    }

    #[test]
    fn function_names_are_not_references() {
        // LOG10 parses as a cell reference on its own, but the `(` makes it a
        // call — the classic false positive a host-side regex would hit.
        assert_eq!(spans("=LOG10(A1)"), vec![(7, 9, 0, 0, 0, 0)]);
        // …and standing alone it *is* a reference (Excel agrees): column LOG,
        // row 10.
        assert_eq!(spans("=LOG10+1"), vec![(1, 6, 9, 8508, 9, 8508)]);
    }

    #[test]
    fn string_literals_are_skipped() {
        assert_eq!(spans(r#"=IF(A1="B2",C3,"D4")"#), vec![
            (4, 6, 0, 0, 0, 0),
            (12, 14, 2, 2, 2, 2),
        ]);
        // A doubled quote inside a literal does not end it.
        assert_eq!(spans(r#"="say ""A1"" now"&B2"#), vec![(18, 20, 1, 1, 1, 1)]);
    }

    #[test]
    fn anchors_and_reversed_ranges_normalize() {
        let r = &reference_spans("=$B$7")[0];
        assert_eq!((r.start, r.end, r.row0, r.col0), (1, 5, 6, 1));
        // D9:B2 covers the same block as B2:D9.
        let r = &reference_spans("=D9:B2")[0];
        assert_eq!((r.row0, r.col0, r.row1, r.col1), (1, 1, 8, 3));
    }

    #[test]
    fn sheet_qualifiers_are_carried_and_included_in_the_span() {
        let r = &reference_spans("=Sheet2!A1")[0];
        assert_eq!((r.start, r.end), (1, 10));
        assert_eq!(r.sheet.as_deref(), Some("Sheet2"));
        let r = &reference_spans("='My Sheet'!B2:C3")[0];
        assert_eq!((r.start, r.end), (1, 17));
        assert_eq!(r.sheet.as_deref(), Some("My Sheet"));
        assert_eq!((r.row0, r.col0, r.row1, r.col1), (1, 1, 2, 2));
    }

    #[test]
    fn plain_text_and_numbers_yield_nothing() {
        assert!(reference_spans("=1+2*3").is_empty());
        assert!(reference_spans("=TODAY()").is_empty());
        assert!(reference_spans("not a formula").is_empty());
    }
}
