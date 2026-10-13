//! Reading a structured reference's specifier — the text between a table
//! name's outer brackets — into the rows and columns it selects.
//!
//! The parser keeps the specifier as written (so it round-trips to xlsx
//! byte-for-byte); this is the one place it is given meaning. The grammar is
//! Excel's:
//!
//! - `Amount` — one column, data rows.
//! - `#All` / `#Data` / `#Headers` / `#Totals` / `#This Row` — a special item,
//!   every column.
//! - `@`, `@Amount`, `@[Unit Price]`, `@[A]:[C]` — the formula's own row
//!   (`@` is the UI spelling of `[#This Row]`).
//! - `[#Data],[Amount]`, `[#Headers],[#Data],[A]:[C]`, `[A]:[C]` — a list of
//!   bracketed items: special items joined by commas, then at most one column
//!   or column range.
//!
//! Inside a bracketed item `'` escapes the next character (`'[`, `']`, `'#`,
//! `''`), which is how a column name holding a bracket is written.

/// Which rows of the table a specifier selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rows {
    All,
    Data,
    Headers,
    Totals,
    HeadersData,
    DataTotals,
    /// The formula's own row, intersected with the data rows.
    ThisRow,
}

/// A specifier read into its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Spec {
    pub rows: Rows,
    /// The first and last column named, or `None` for every column.
    pub columns: Option<(String, String)>,
}

/// Read a specifier; `None` when it is not one Excel accepts, which the caller
/// reports as `#REF!`.
pub(crate) fn parse_spec(spec: &str) -> Option<Spec> {
    let s = spec.trim();
    if let Some(rest) = s.strip_prefix('@') {
        let rest = rest.trim();
        let columns = if rest.is_empty() {
            None
        } else if rest.starts_with('[') {
            let (special, columns) = parse_items(rest)?;
            if !special.is_empty() {
                return None;
            }
            columns
        } else {
            Some((rest.to_owned(), rest.to_owned()))
        };
        return Some(Spec {
            rows: Rows::ThisRow,
            columns,
        });
    }
    if s.is_empty() {
        return Some(Spec {
            rows: Rows::Data,
            columns: None,
        });
    }
    if !s.starts_with('[') {
        // A single bare item: a special item or a column name.
        return if s.starts_with('#') {
            Some(Spec {
                rows: rows_of(&[special(s)?])?,
                columns: None,
            })
        } else {
            Some(Spec {
                rows: Rows::Data,
                columns: Some((s.to_owned(), s.to_owned())),
            })
        };
    }
    let (specials, columns) = parse_items(s)?;
    Some(Spec {
        rows: rows_of(&specials)?,
        columns,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Special {
    All,
    Data,
    Headers,
    Totals,
    ThisRow,
}

fn special(item: &str) -> Option<Special> {
    let lower = item.trim().to_ascii_lowercase();
    Some(match lower.as_str() {
        "#all" => Special::All,
        "#data" => Special::Data,
        "#headers" => Special::Headers,
        "#totals" => Special::Totals,
        "#this row" => Special::ThisRow,
        _ => return None,
    })
}

/// The rows a set of special items selects. Only the combinations Excel
/// accepts — each contiguous — are allowed; an empty set is the data body.
fn rows_of(specials: &[Special]) -> Option<Rows> {
    use Special as S;
    let has = |x: S| specials.contains(&x);
    let mut unique = specials.to_vec();
    unique.dedup();
    Some(match unique.len() {
        0 => Rows::Data,
        1 => match unique[0] {
            S::All => Rows::All,
            S::Data => Rows::Data,
            S::Headers => Rows::Headers,
            S::Totals => Rows::Totals,
            S::ThisRow => Rows::ThisRow,
        },
        2 if has(S::Headers) && has(S::Data) => Rows::HeadersData,
        2 if has(S::Data) && has(S::Totals) => Rows::DataTotals,
        _ => return None,
    })
}

enum Piece {
    Item(String),
    Comma,
    Colon,
}

/// Split `[a],[b]:[c]` into its items and separators, unescaping each item.
fn pieces(s: &str) -> Option<Vec<Piece>> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            c if c.is_whitespace() => i += 1,
            ',' => {
                out.push(Piece::Comma);
                i += 1;
            }
            ':' => {
                out.push(Piece::Colon);
                i += 1;
            }
            '[' => {
                i += 1;
                let mut item = String::new();
                loop {
                    match chars.get(i)? {
                        '\'' => {
                            item.push(*chars.get(i + 1)?);
                            i += 2;
                        }
                        ']' => {
                            i += 1;
                            break;
                        }
                        '[' => return None,
                        &c => {
                            item.push(c);
                            i += 1;
                        }
                    }
                }
                out.push(Piece::Item(item));
            }
            _ => return None,
        }
    }
    Some(out)
}

type Columns = Option<(String, String)>;

/// Read a bracketed item list: special items first, then at most one column or
/// column range, all separated by commas.
fn parse_items(s: &str) -> Option<(Vec<Special>, Columns)> {
    let pieces = pieces(s)?;
    let mut specials = Vec::new();
    let mut columns: Columns = None;
    let mut i = 0;
    while i < pieces.len() {
        if i > 0 {
            match pieces[i] {
                Piece::Comma => i += 1,
                _ => return None,
            }
        }
        let Some(Piece::Item(item)) = pieces.get(i) else {
            return None;
        };
        i += 1;
        if item.trim_start().starts_with('#') {
            // Special items precede the column, never follow it.
            if columns.is_some() {
                return None;
            }
            specials.push(special(item)?);
            continue;
        }
        if columns.is_some() {
            return None;
        }
        let first = item.clone();
        let last = if let Some(Piece::Colon) = pieces.get(i) {
            let Some(Piece::Item(end)) = pieces.get(i + 1) else {
                return None;
            };
            if end.trim_start().starts_with('#') {
                return None;
            }
            i += 2;
            end.clone()
        } else {
            first.clone()
        };
        columns = Some((first, last));
    }
    Some((specials, columns))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(a: &str, b: &str) -> Columns {
        Some((a.to_owned(), b.to_owned()))
    }

    #[test]
    fn specifiers_read_into_rows_and_columns() {
        let cases = [
            ("Amount", Rows::Data, cols("Amount", "Amount")),
            ("", Rows::Data, None),
            ("#All", Rows::All, None),
            ("#This Row", Rows::ThisRow, None),
            ("@", Rows::ThisRow, None),
            ("@Qty", Rows::ThisRow, cols("Qty", "Qty")),
            (
                "@[Unit Price]",
                Rows::ThisRow,
                cols("Unit Price", "Unit Price"),
            ),
            ("@[A]:[C]", Rows::ThisRow, cols("A", "C")),
            ("[#This Row],[Qty]", Rows::ThisRow, cols("Qty", "Qty")),
            ("[#Data],[A]:[C]", Rows::Data, cols("A", "C")),
            (
                "[#Headers],[#Data],[Qty]",
                Rows::HeadersData,
                cols("Qty", "Qty"),
            ),
            ("[#Data],[#Totals]", Rows::DataTotals, None),
            ("[A]:[C]", Rows::Data, cols("A", "C")),
            (" [#Data] , [Qty] ", Rows::Data, cols("Qty", "Qty")),
            (
                "[Price '[USD']]",
                Rows::Data,
                cols("Price [USD]", "Price [USD]"),
            ),
        ];
        for (text, rows, columns) in cases {
            assert_eq!(parse_spec(text), Some(Spec { rows, columns }), "{text:?}");
        }
    }

    #[test]
    fn specifiers_excel_rejects_are_none() {
        for text in [
            "[#Headers],[#Totals]",
            "[#All],[#Data]",
            "[#This Row],[#Data]",
            "[Qty],[#Data]",
            "[A],[B]",
            "#Nope",
            "[#Data],[Qty",
            "@[#Data]",
        ] {
            assert_eq!(parse_spec(text), None, "{text:?}");
        }
    }
}
