//! Where a hyperlink goes (`XL-FEAT-139`).
//!
//! Two questions a host asks before it can follow a link, answered here rather
//! than in the host so every host answers them the same way:
//!
//! - **Which cells are in a link's location?** [`resolve_location`] reads the
//!   in-document half of a link — `Sheet2!B3`, `'It''s'!A1:C4`, a defined name
//!   such as `Totals` or a sheet-scoped `Sheet1!Local` — into a sheet and a
//!   block, or `None` for Excel's "Reference isn't valid."
//! - **Where does a `HYPERLINK()` cell point?** The function *displays* its
//!   friendly name, so the cell's value cannot say. [`Evaluator::hyperlink_target`]
//!   evaluates the link argument at the cell, which is what makes
//!   `=HYPERLINK("#"&A1, "Go")` follow the computed place.
//!
//! The editor used to split a location on its first `!` and read the rest as a
//! cell, in JavaScript. A name went nowhere, a missing sheet selected the cell on
//! the *current* sheet, a range selected its corner and `'My ! Sheet'!A1` split
//! inside the quotes. Each of those was a separate small mistake in a parser
//! that should not have been the host's to write.

use casual_calc_formula::{CellReference, Expr, parse_a1, parse_a1_axis};
use casual_calc_model::{CellRef, SheetVisibility, Workbook};

use crate::eval::{Evaluator, defined_name_in_scope};
use crate::value::{Value, number_to_text};

/// A block of cells on one sheet: what following an in-document link selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkPlace {
    /// The sheet's index in the workbook.
    pub sheet: usize,
    /// First row.
    pub r0: u32,
    /// First column.
    pub c0: u32,
    /// Last row.
    pub r1: u32,
    /// Last column.
    pub c1: u32,
}

/// Split `Sheet!rest` on the last `!` that is not inside a quoted sheet name,
/// unquoting the sheet: `'It''s'!A1` → `(Some("It's"), "A1")`.
///
/// `None` for the sheet when the text names none. An unterminated quote is not
/// a sheet name at all, so it answers `None` for the whole split.
#[must_use]
pub fn split_sheet(text: &str) -> Option<(Option<String>, &str)> {
    let mut quoted = false;
    let mut bang = None;
    for (i, ch) in text.char_indices() {
        match ch {
            // `''` inside quotes is an escaped apostrophe: it toggles twice,
            // which leaves `quoted` where it was.
            '\'' => quoted = !quoted,
            '!' if !quoted => bang = Some(i),
            _ => {}
        }
    }
    if quoted {
        return None;
    }
    let Some(i) = bang else {
        return Some((None, text));
    };
    let (sheet, rest) = (&text[..i], &text[i + 1..]);
    let sheet = match sheet.strip_prefix('\'') {
        Some(inner) => inner.strip_suffix('\'')?.replace("''", "'"),
        None => sheet.to_owned(),
    };
    if sheet.is_empty() {
        return None;
    }
    Some((Some(sheet), rest))
}

/// The sheet named `name` (case-insensitive, as Excel's are) if it can be shown.
///
/// A hidden sheet is not a place a link can take anyone: Excel will not select
/// a cell on a sheet it is not showing.
fn visible_sheet(workbook: &Workbook, name: &str) -> Option<usize> {
    workbook
        .sheets
        .iter()
        .position(|s| s.name.eq_ignore_ascii_case(name))
        .filter(|&i| workbook.sheets[i].visibility == SheetVisibility::Visible)
}

/// A1 text as a block: `B3`, `$B$3`, `B3:D5`, `A:A`, `3:5`.
fn parse_block(text: &str) -> Option<(u32, u32, u32, u32)> {
    let text = text.trim();
    let corner = |part: &str, at_end: bool| -> Option<CellReference> {
        parse_a1(part).or_else(|| parse_a1_axis(part, at_end))
    };
    let (a, b) = match text.split_once(':') {
        Some((a, b)) => (corner(a, false)?, corner(b, true)?),
        None => {
            let a = parse_a1(text)?;
            (a.clone(), a)
        }
    };
    Some((
        a.row.min(b.row),
        a.col.min(b.col),
        a.row.max(b.row),
        a.col.max(b.col),
    ))
}

/// The cells a defined name refers to, or `None` when it refers to anything
/// other than one block (a constant, a formula, a union).
fn name_place(workbook: &Workbook, scope_sheet: usize, name: &str) -> Option<LinkPlace> {
    let defined = defined_name_in_scope(workbook, scope_sheet, name)?;
    // Where an unqualified reference inside the name lands: the name's own
    // sheet when it is sheet-scoped, otherwise the sheet the link asked about.
    let home = defined
        .sheet
        .and_then(|id| workbook.sheets.iter().position(|s| s.id == id))
        .unwrap_or(scope_sheet);
    let origin = casual_calc_formula::stored::ABSOLUTE;
    let (a, b) = match &defined.formula {
        Expr::Reference(r) => {
            let r = r.resolve(origin)?;
            (r.clone(), r)
        }
        Expr::Range(a, b) => (a.resolve(origin)?, b.resolve(origin)?),
        _ => return None,
    };
    let sheet = match a.sheet.as_deref().or(b.sheet.as_deref()) {
        Some(name) => visible_sheet(workbook, name)?,
        None => home,
    };
    Some(LinkPlace {
        sheet,
        r0: a.row.min(b.row),
        c0: a.col.min(b.col),
        r1: a.row.max(b.row),
        c1: a.col.max(b.col),
    })
}

/// Resolve a link's in-document location, as Excel does when it is followed.
///
/// `current` is the sheet the link is on: an unqualified `B3` is a cell there.
/// Accepts a cell or range with or without a sheet (quoted or not), and a
/// defined name — workbook-scoped, or sheet-scoped written `Sheet!Name` —
/// compared case-insensitively. A single leading `#`, which is how
/// `HYPERLINK()` marks an in-document target, is accepted too.
///
/// `None` is Excel's "Reference isn't valid.": a sheet that does not exist or
/// is hidden, a name that is not defined or refers to no cells, or text that is
/// neither.
#[must_use]
pub fn resolve_location(workbook: &Workbook, current: usize, text: &str) -> Option<LinkPlace> {
    let text = text.trim();
    let text = text.strip_prefix('#').unwrap_or(text).trim();
    if text.is_empty() || current >= workbook.sheets.len() {
        return None;
    }
    let (sheet_name, rest) = split_sheet(text)?;
    let sheet = match &sheet_name {
        Some(name) => visible_sheet(workbook, name)?,
        None => current,
    };
    if let Some((r0, c0, r1, c1)) = parse_block(rest) {
        return Some(LinkPlace {
            sheet,
            r0,
            c0,
            r1,
            c1,
        });
    }
    name_place(workbook, sheet, rest.trim())
}

impl Evaluator<'_> {
    /// Where the `HYPERLINK()` formula in a cell points: its first argument,
    /// evaluated at the cell, as text.
    ///
    /// `None` when the cell holds no formula, when the formula is not a
    /// `HYPERLINK` call, or when the target evaluates to an error or to
    /// nothing. Only a formula that *is* a `HYPERLINK` call counts: one that
    /// merely contains a call somewhere has no single target to report.
    pub fn hyperlink_target(&mut self, sheet: usize, at: CellRef) -> Option<String> {
        let workbook = self.workbook();
        let handle = workbook.sheets.get(sheet)?.cells.get(at)?.formula?;
        let expr = workbook.formula(handle)?;
        let link = hyperlink_call(expr)?;
        match self.eval_expr_at(sheet, at, link) {
            Value::Text(t) if !t.trim().is_empty() => Some(t),
            Value::Number(n) => Some(number_to_text(n)),
            _ => None,
        }
    }
}

/// The link argument of a formula that is a `HYPERLINK` call.
///
/// Cheap and evaluation-free, so the grid can ask it of every visible cell to
/// decide which to draw as links.
#[must_use]
pub fn hyperlink_call(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::Function { name, args }
            if name.eq_ignore_ascii_case("HYPERLINK") && (1..=2).contains(&args.len()) =>
        {
            args.first()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casual_calc_formula::stored::Origin;
    use casual_calc_model::{Cell, CellValue, DefinedName, Id, Sheet, SheetId};

    fn book(names: &[&str]) -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        wb.sheets.clear();
        for (i, n) in names.iter().enumerate() {
            wb.sheets
                .push(Sheet::new(SheetId(Id::from_parts(2, i as u64 + 1)), *n));
        }
        wb
    }

    fn name(wb: &mut Workbook, n: &str, scope: Option<usize>, refers: &str) {
        wb.defined_names.push(DefinedName {
            name: n.to_owned(),
            sheet: scope.map(|i| wb.sheets[i].id),
            formula: casual_calc_formula::parse(refers).unwrap(),
            comment: None,
            attributes: Default::default(),
        });
    }

    const fn place(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> Option<LinkPlace> {
        Some(LinkPlace {
            sheet,
            r0,
            c0,
            r1,
            c1,
        })
    }

    #[test]
    fn splits_on_the_last_bang_outside_quotes() {
        assert_eq!(split_sheet("A1"), Some((None, "A1")));
        assert_eq!(
            split_sheet("Sheet2!B3"),
            Some((Some("Sheet2".to_owned()), "B3"))
        );
        assert_eq!(
            split_sheet("'My ! Sheet'!A1"),
            Some((Some("My ! Sheet".to_owned()), "A1"))
        );
        assert_eq!(
            split_sheet("'It''s'!B2"),
            Some((Some("It's".to_owned()), "B2"))
        );
        assert_eq!(split_sheet("'unterminated!A1"), None);
        assert_eq!(split_sheet("!A1"), None);
    }

    #[test]
    fn cells_and_ranges_on_named_sheets() {
        let wb = book(&["Sheet1", "Sheet2", "My ! Sheet", "It's"]);
        assert_eq!(resolve_location(&wb, 0, "C5"), place(0, 4, 2, 4, 2));
        assert_eq!(
            resolve_location(&wb, 0, "sheet2!$c$5"),
            place(1, 4, 2, 4, 2)
        );
        assert_eq!(
            resolve_location(&wb, 0, "Sheet2!E6:C3"),
            place(1, 2, 2, 5, 4),
            "a range is the whole block, however it was written"
        );
        assert_eq!(
            resolve_location(&wb, 0, "'My ! Sheet'!A1"),
            place(2, 0, 0, 0, 0)
        );
        assert_eq!(resolve_location(&wb, 0, "'It''s'!B2"), place(3, 1, 1, 1, 1));
        assert_eq!(resolve_location(&wb, 1, "#A1"), place(1, 0, 0, 0, 0));
    }

    #[test]
    fn a_missing_or_hidden_sheet_is_not_a_place() {
        let mut wb = book(&["Sheet1", "Hidden"]);
        wb.sheets[1].visibility = SheetVisibility::Hidden;
        assert_eq!(resolve_location(&wb, 0, "Nowhere!C7"), None);
        assert_eq!(resolve_location(&wb, 0, "Hidden!C7"), None);
        assert_eq!(resolve_location(&wb, 0, "not a reference"), None);
        assert_eq!(resolve_location(&wb, 0, ""), None);
    }

    #[test]
    fn defined_names_resolve_case_insensitively_and_in_scope() {
        let mut wb = book(&["Sheet1", "Sheet2"]);
        name(&mut wb, "Totals", None, "Sheet2!$B$3:$D$5");
        name(&mut wb, "Local", None, "Sheet1!$H$8");
        name(&mut wb, "Local", Some(1), "Sheet2!$E$2:$E$9");
        name(&mut wb, "Rate", None, "0.2");
        assert_eq!(resolve_location(&wb, 0, "totals"), place(1, 2, 1, 4, 3));
        assert_eq!(resolve_location(&wb, 0, "#TOTALS"), place(1, 2, 1, 4, 3));
        assert_eq!(
            resolve_location(&wb, 0, "Sheet2!Local"),
            place(1, 1, 4, 8, 4),
            "Sheet2!Local is the name scoped to Sheet2, not the workbook one"
        );
        assert_eq!(resolve_location(&wb, 0, "Local"), place(0, 7, 7, 7, 7));
        assert_eq!(
            resolve_location(&wb, 0, "Rate"),
            None,
            "a name that is a constant has no cells to select"
        );
        assert_eq!(resolve_location(&wb, 0, "Undefined"), None);
    }

    #[test]
    fn a_hyperlink_formula_reports_its_evaluated_target() {
        let mut wb = book(&["Sheet1"]);
        let mut set = |at: CellRef, text: &str| {
            let expr = casual_calc_formula::parse(text).unwrap();
            // Stored relative to its cell, as every formula is (`PERF-11`).
            let handle = wb.store_formula_at(expr, Origin::at(at.row, at.col));
            let mut cell = Cell::value(CellValue::Empty);
            cell.formula = Some(handle);
            wb.sheets[0].cells.set(at, cell);
        };
        set(CellRef::new(0, 1), r##"HYPERLINK("#"&A1,"Go")"##);
        set(CellRef::new(1, 1), r#"HYPERLINK("https://example.com")"#);
        set(CellRef::new(2, 1), "1+1");
        set(CellRef::new(3, 1), "HYPERLINK(1/0)");
        let id = wb.intern_string("Sheet1!C3");
        wb.sheets[0]
            .cells
            .set(CellRef::new(0, 0), Cell::value(CellValue::InlineString(id)));
        let mut ev = Evaluator::new(&wb);
        assert_eq!(
            ev.hyperlink_target(0, CellRef::new(0, 1)).as_deref(),
            Some("#Sheet1!C3"),
            "the target is the link argument, evaluated — not the friendly name"
        );
        assert_eq!(
            ev.hyperlink_target(0, CellRef::new(1, 1)).as_deref(),
            Some("https://example.com")
        );
        assert_eq!(ev.hyperlink_target(0, CellRef::new(2, 1)), None);
        assert_eq!(ev.hyperlink_target(0, CellRef::new(3, 1)), None);
        assert_eq!(ev.hyperlink_target(0, CellRef::new(9, 9)), None);
    }
}
