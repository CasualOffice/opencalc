//! Formula conditional-format rules, evaluated per cell.
//!
//! [`CfRule::Expression`](casual_calc_model::CfRule::Expression) is the only
//! conditional-format kind whose answer is a
//! formula rather than a property of the cell's own value, and formulas are
//! this crate. Conditional formatting itself lives one layer down, in
//! `casual-calc-layout`, so that the browser canvas and the headless renderer
//! cannot disagree about it (`RND-05`). The two are joined by
//! [`casual_calc_layout::conditional::CfExpressions`]: layout asks, this
//! answers.
//!
//! # The anchoring, once, in the place that does it
//!
//! OOXML anchors a `<cfRule type="expression">` formula to the **top-left of
//! the applied range**, and shifts its relative references for every other cell
//! in that range — the same shift a fill-down does. So `$D2>100` over `A2:H10`
//! asks `$D2>100` of row 2 and `$D7>100` of row 7, and the `$` keeps the column
//! at D throughout.
//!
//! Since `PERF-11` that shift is free. A parsed tree holds offsets from its
//! holding cell, so re-storing it once against the range's top-left
//! ([`restore_at`]) and then evaluating it *at* each cell
//! ([`Evaluator::eval_expr_at`]) shifts the references by construction. There
//! is no per-cell rewrite, and no second place where the offset arithmetic
//! could be written down differently.

use std::cell::RefCell;
use std::collections::HashMap;

use casual_calc_formula::stored::{ABSOLUTE, Origin};
use casual_calc_formula::{Expr, restore_at};
use casual_calc_layout::conditional::CfExpressions;
use casual_calc_model::{CellRef, Sheet, Workbook};

use crate::Evaluator;

/// The formula rules of one sheet, parsed once and evaluated on demand.
///
/// Hand it to [`casual_calc_layout::layout_viewport_with`] (or to
/// [`casual_calc_layout::conditional::effect_for`] directly) to make
/// `expression` rules paint. Building one costs a parse per formula rule and
/// nothing at all for a sheet that has none.
#[derive(Debug)]
pub struct CfExpressionRules<'a> {
    sheet_index: usize,
    /// One entry per `Sheet::conditional_formats` entry, positionally — the
    /// index layout passes back. `None` for every rule that is not an
    /// expression, and for an expression whose formula does not parse.
    ///
    /// Each tree is stored against **its own rule's top-left**, so two rules
    /// over different ranges do not share an origin.
    programs: Vec<Option<Expr>>,
    /// Evaluated answers, keyed by rule and cell.
    ///
    /// A merged region asks for its anchor twice, and a caller may resolve the
    /// same cell for fill and for text; the underlying `Evaluator` memoizes
    /// cell *values*, but not the tree walk above them.
    answers: RefCell<HashMap<(usize, u32, u32), bool>>,
    evaluator: RefCell<Evaluator<'a>>,
}

impl<'a> CfExpressionRules<'a> {
    /// Parse the formula rules of `sheet_index`.
    ///
    /// A rule whose formula does not parse is left unmatched rather than
    /// treated as true: a highlight nobody asked for is worse than a highlight
    /// that is missing, and the rule is still in the model and still written
    /// back to the file.
    #[must_use]
    pub fn new(workbook: &'a Workbook, sheet_index: usize) -> Self {
        let programs = workbook
            .sheets
            .get(sheet_index)
            .map(Self::compile)
            .unwrap_or_default();
        Self {
            sheet_index,
            programs,
            answers: RefCell::new(HashMap::new()),
            evaluator: RefCell::new(Evaluator::new(workbook)),
        }
    }

    fn compile(sheet: &Sheet) -> Vec<Option<Expr>> {
        sheet
            .conditional_formats
            .iter()
            .map(|cf| {
                let formula = cf.rule.formula()?;
                let parsed = casual_calc_formula::parse(formula).ok()?;
                // `parse` yields absolute references. Re-store them against the
                // range's top-left, which is the anchor OOXML gave the formula,
                // so evaluating at a cell resolves them relative to *that*
                // cell — the per-cell shift, done by the origin rather than by
                // arithmetic here.
                Some(restore_at(
                    &parsed,
                    ABSOLUTE,
                    Origin::at(cf.range.start.row, cf.range.start.col),
                ))
            })
            .collect()
    }
}

impl CfExpressions for CfExpressionRules<'_> {
    fn matches(&self, rule: usize, row: u32, col: u32) -> bool {
        let Some(Some(program)) = self.programs.get(rule) else {
            return false;
        };
        if let Some(hit) = self.answers.borrow().get(&(rule, row, col)) {
            return *hit;
        }
        let value = self.evaluator.borrow_mut().eval_expr_at(
            self.sheet_index,
            CellRef::new(row, col),
            program,
        );
        // Excel paints on TRUE and on TRUE alone: `#DIV/0!` is not a highlight,
        // and neither is text that is not "TRUE". `as_bool` already draws those
        // lines, and an error there means "do not paint".
        let hit = value.as_bool().unwrap_or(false);
        self.answers.borrow_mut().insert((rule, row, col), hit);
        hit
    }
}
