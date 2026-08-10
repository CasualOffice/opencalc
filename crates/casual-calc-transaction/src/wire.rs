//! Making an operation mean the same thing on another replica.
//!
//! [`Operation`] is fast because a cell refers to its formula and its style by
//! **handle** — an index into the workbook's own arena and style table. That is
//! right for local work and wrong on a wire: `FormulaHandle(7)` is the seventh
//! formula *this* workbook happens to have interned, and the seventh of another
//! workbook is a different formula, or none at all.
//!
//! The failure is silent and worse than losing the formula. A chunk carrying a
//! foreign handle commits without error; the writer then finds a handle
//! indexing nothing and drops **the whole cell**, not merely its formula.
//!
//! # Why a side table rather than a portable mirror of the op set
//!
//! The obvious fix is a parallel `WireOperation` tree carrying expressions and
//! styles by value. It is also a second definition of every operation, kept in
//! step by hand, for a problem that lives in exactly one type — `Cell` is the
//! only thing in the model holding a handle, since even `DefinedName` carries
//! its expression by value.
//!
//! So the operation travels unchanged and takes its meanings with it:
//! [`WireOperation`] is an operation plus the formulas and styles its handles
//! refer to. The receiver interns those into its own tables and rewrites the
//! handles to match. One type, one transform matrix, one `apply`.

use std::collections::BTreeMap;

use casual_calc_formula::Expr;
use casual_calc_model::{FormulaHandle, Sheet, Style, StyleId, Workbook};

use crate::Operation;

/// An operation together with what its handles mean.
///
/// Produced with [`WireOperation::of`] against the workbook the operation was
/// written on, and turned back into an [`Operation`] with
/// [`WireOperation::localise`] against the workbook it is arriving at.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireOperation {
    /// The operation, with the sender's handles still in it.
    pub op: Operation,
    /// Every formula the operation's handles refer to, by the sender's index.
    pub formulas: BTreeMap<FormulaHandle, Expr>,
    /// Every style the operation's ids refer to, by the sender's index.
    pub styles: BTreeMap<StyleId, Style>,
}

impl WireOperation {
    /// Package `op` with the meanings its handles have in `workbook`.
    ///
    /// A handle that resolves to nothing is left out rather than guessed at, so
    /// [`Self::localise`] sees the same absence the sender had rather than a
    /// silently different formula.
    #[must_use]
    pub fn of(op: Operation, workbook: &Workbook) -> Self {
        let mut formulas = BTreeMap::new();
        let mut styles = BTreeMap::new();
        visit(&op, &mut |formula, style| {
            if let Some(handle) = formula
                && let Some(expr) = workbook.formula(handle)
            {
                formulas.insert(handle, expr.clone());
            }
            if let Some(id) = style
                && let Some(found) = workbook.styles.get(id)
            {
                styles.insert(id, found.clone());
            }
        });
        Self {
            op,
            formulas,
            styles,
        }
    }

    /// Rewrite the handles so they mean the same thing in `workbook`.
    ///
    /// Interning is by value, so two replicas that arrive at the same formula
    /// or the same style converge on whatever their own tables already hold
    /// rather than growing a duplicate each time an operation crosses.
    #[must_use]
    pub fn localise(self, workbook: &mut Workbook) -> Operation {
        let Self {
            mut op,
            formulas,
            styles,
        } = self;

        let mut formula_map = BTreeMap::new();
        for (theirs, expr) in formulas {
            formula_map.insert(theirs, workbook.store_formula(expr));
        }
        let mut style_map = BTreeMap::new();
        for (theirs, style) in styles {
            style_map.insert(theirs, workbook.intern_style(style));
        }

        visit_mut(&mut op, &mut |formula, style| {
            // A handle with no accompanying meaning is dropped, not kept: kept,
            // it would index this workbook's arena and silently name some other
            // replica's formula.
            *formula = formula.and_then(|handle| formula_map.get(&handle).copied());
            *style = style.and_then(|id| style_map.get(&id).copied());
        });
        op
    }
}

/// Visit every handle-bearing slot an operation carries.
///
/// Two slots, not one: a cell holds both a formula handle and a style id,
/// and [`Operation::SetStyle`] holds a bare style id with no cell around it.
/// Missing the second is how a style crosses replicas meaning something else —
/// which it did, until this walk covered it.
fn visit(op: &Operation, f: &mut impl FnMut(Option<FormulaHandle>, Option<StyleId>)) {
    match op {
        Operation::SetCell {
            cell: Some(cell), ..
        } => f(cell.formula, cell.style),
        Operation::SetStyle { style, .. } => f(None, *style),
        Operation::InsertSheet { sheet, .. } => {
            for (_, cell) in sheet.cells.iter() {
                f(cell.formula, cell.style);
            }
        }
        Operation::Batch(ops) => {
            for member in ops {
                visit(member, f);
            }
        }
        _ => {}
    }
}

/// The same walk, rewriting each slot through `f`.
fn visit_mut(
    op: &mut Operation,
    f: &mut impl FnMut(&mut Option<FormulaHandle>, &mut Option<StyleId>),
) {
    match op {
        Operation::SetCell {
            cell: Some(cell), ..
        } => f(&mut cell.formula, &mut cell.style),
        Operation::SetStyle { style, .. } => f(&mut None, style),
        Operation::InsertSheet { sheet, .. } => rewrite_sheet(sheet, f),
        Operation::Batch(ops) => {
            for member in ops {
                visit_mut(member, f);
            }
        }
        _ => {}
    }
}

/// Rebuild a sheet's cells through `f`.
///
/// The store has no mutable iterator, and giving it one to serve this would
/// widen a type used everywhere for the sake of a path used once.
fn rewrite_sheet(
    sheet: &mut Sheet,
    f: &mut impl FnMut(&mut Option<FormulaHandle>, &mut Option<StyleId>),
) {
    let existing: Vec<_> = sheet
        .cells
        .iter()
        .map(|(at, cell)| (at, cell.clone()))
        .collect();
    for (at, mut cell) in existing {
        f(&mut cell.formula, &mut cell.style);
        sheet.cells.set(at, cell);
    }
}

/// Whether an operation refers to a formula or a style by handle.
///
/// What to check before sending an operation that has not been through
/// [`WireOperation::of`]: such an operation is not wrong here, only meaningless
/// anywhere else.
#[must_use]
pub fn carries_handles(op: &Operation) -> bool {
    let mut found = false;
    visit(op, &mut |formula, style| {
        found |= formula.is_some() || style.is_some();
    });
    found
}

/// A formula handle that resolves to nothing in `workbook`.
///
/// A server can use this to refuse an operation rather than commit one whose
/// cell the writer will silently drop.
#[must_use]
pub fn dangling_handle(op: &Operation, workbook: &Workbook) -> Option<FormulaHandle> {
    let mut dangling = None;
    visit(op, &mut |formula, _| {
        if let Some(handle) = formula
            && workbook.formula(handle).is_none()
        {
            dangling = Some(handle);
        }
    });
    dangling
}

#[cfg(test)]
mod tests {
    use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Style, Workbook};

    use super::*;

    fn workbook(namespace: u64) -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(namespace, 1));
        wb.sheets
            .push(Sheet::new(SheetId(Id::from_parts(namespace, 2)), "S"));
        wb
    }

    fn bold() -> Style {
        Style {
            bold: true,
            ..Style::default()
        }
    }

    #[test]
    fn a_formula_crosses_to_a_workbook_that_has_never_seen_it() {
        // The sender has interned other formulas first, so its handle is not
        // one the receiver would allocate — which is the whole failure.
        let mut sender = workbook(1);
        sender.store_formula(casual_calc_formula::parse("1").unwrap());
        sender.store_formula(casual_calc_formula::parse("2").unwrap());
        let handle = sender.store_formula(casual_calc_formula::parse("3+4").unwrap());
        assert_eq!(handle, casual_calc_model::FormulaHandle(2));

        let mut cell = Cell::value(CellValue::Number(7.0));
        cell.formula = Some(handle);
        let op = Operation::SetCell {
            sheet: 0,
            at: CellRef::new(0, 0),
            cell: Some(cell),
        };

        let wire = WireOperation::of(op, &sender);
        let mut receiver = workbook(2);
        let localised = wire.localise(&mut receiver);

        let Operation::SetCell {
            cell: Some(cell), ..
        } = localised
        else {
            panic!("still a cell edit")
        };
        let landed = cell.formula.and_then(|h| receiver.formula(h)).cloned();
        assert_eq!(
            landed,
            Some(casual_calc_formula::parse("3+4").unwrap()),
            "the expression arrived, whatever index it took here"
        );
        assert_ne!(
            cell.formula,
            Some(handle),
            "and it is the receiver's handle"
        );
    }

    #[test]
    fn a_style_crosses_too_including_the_bare_one_set_by_set_style() {
        let mut sender = workbook(1);
        sender.intern_style(Style {
            italic: true,
            ..Style::default()
        });
        let id = sender.intern_style(bold());

        let mut receiver = workbook(2);
        let localised = WireOperation::of(
            Operation::SetStyle {
                sheet: 0,
                at: CellRef::new(1, 1),
                style: Some(id),
            },
            &sender,
        )
        .localise(&mut receiver);

        let Operation::SetStyle {
            style: Some(landed),
            ..
        } = localised
        else {
            panic!("still a style edit")
        };
        assert_eq!(
            receiver.styles.get(landed).cloned(),
            Some(bold()),
            "SetStyle carries a bare id with no cell around it, and it travels"
        );
    }

    #[test]
    fn interning_by_value_does_not_duplicate_what_the_receiver_already_has() {
        let mut sender = workbook(1);
        let theirs = sender.intern_style(bold());
        let mut receiver = workbook(2);
        let mine = receiver.intern_style(bold());

        let localised = WireOperation::of(
            Operation::SetStyle {
                sheet: 0,
                at: CellRef::new(0, 0),
                style: Some(theirs),
            },
            &sender,
        )
        .localise(&mut receiver);

        let Operation::SetStyle {
            style: Some(landed),
            ..
        } = localised
        else {
            panic!("still a style edit")
        };
        assert_eq!(
            landed, mine,
            "the same style is the same id, not a second one"
        );
    }

    #[test]
    fn a_sheet_full_of_cells_travels_with_all_of_them() {
        let mut sender = workbook(1);
        let handle = sender.store_formula(casual_calc_formula::parse("9*9").unwrap());
        let style = sender.intern_style(bold());
        let mut sheet = Sheet::new(SheetId(Id::from_parts(1, 9)), "added");
        for row in 0..3u32 {
            let mut cell = Cell::value(CellValue::Number(f64::from(row)));
            cell.formula = Some(handle);
            cell.style = Some(style);
            sheet.cells.set(CellRef::new(row, 0), cell);
        }

        let mut receiver = workbook(2);
        let localised = WireOperation::of(
            Operation::InsertSheet {
                index: 0,
                sheet: Box::new(sheet),
            },
            &sender,
        )
        .localise(&mut receiver);

        let Operation::InsertSheet { sheet, .. } = localised else {
            panic!("still a sheet insert")
        };
        for (_, cell) in sheet.cells.iter() {
            assert_eq!(
                cell.formula.and_then(|h| receiver.formula(h)).cloned(),
                Some(casual_calc_formula::parse("9*9").unwrap()),
                "every cell in the sheet, not just the first"
            );
            assert_eq!(
                cell.style.and_then(|s| receiver.styles.get(s)).cloned(),
                Some(bold())
            );
        }
    }

    #[test]
    fn a_handle_the_sender_could_not_resolve_is_dropped_rather_than_carried() {
        // Carrying it would index the receiver's arena and silently name some
        // other formula — a wrong answer where the sender had none.
        let sender = workbook(1);
        let mut cell = Cell::value(CellValue::Number(1.0));
        cell.formula = Some(casual_calc_model::FormulaHandle(99));
        let wire = WireOperation::of(
            Operation::SetCell {
                sheet: 0,
                at: CellRef::new(0, 0),
                cell: Some(cell),
            },
            &sender,
        );
        assert!(wire.formulas.is_empty());

        let mut receiver = workbook(2);
        receiver.store_formula(casual_calc_formula::parse("1+1").unwrap());
        let Operation::SetCell {
            cell: Some(cell), ..
        } = wire.localise(&mut receiver)
        else {
            panic!("still a cell edit")
        };
        assert_eq!(cell.formula, None, "dropped, not silently rebound");
    }

    #[test]
    fn carrying_no_handles_is_detectable() {
        let plain = Operation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(1.0),
        };
        assert!(!carries_handles(&plain));

        let mut wb = workbook(1);
        let handle = wb.store_formula(casual_calc_formula::parse("1").unwrap());
        let mut cell = Cell::value(CellValue::Number(1.0));
        cell.formula = Some(handle);
        assert!(carries_handles(&Operation::SetCell {
            sheet: 0,
            at: CellRef::new(0, 0),
            cell: Some(cell),
        }));
    }

    #[test]
    fn a_dangling_handle_is_reportable_so_a_server_can_refuse_it() {
        let wb = workbook(1);
        let mut cell = Cell::value(CellValue::Number(1.0));
        cell.formula = Some(casual_calc_model::FormulaHandle(3));
        let op = Operation::SetCell {
            sheet: 0,
            at: CellRef::new(0, 0),
            cell: Some(cell),
        };
        assert_eq!(
            dangling_handle(&op, &wb),
            Some(casual_calc_model::FormulaHandle(3))
        );
    }
}
