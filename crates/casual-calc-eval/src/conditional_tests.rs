//! Tests for formula conditional-format rules — and above all for the anchor.
//!
//! Every range here deliberately starts somewhere other than `A1`, because the
//! bug this feature invites is anchoring the formula to `A1` instead of to the
//! applied range's top-left. That bug does not crash and does not error: it
//! paints a highlight one or two rows out, which nothing but a test with an
//! off-origin range can tell from a correct one.

use casual_calc_layout::conditional::{CfExpressions, NoExpressions, effect_for, priority_order};
use casual_calc_model::{
    Cell, CellRange, CellRef, CellValue, CfRule, ConditionalFormat, Id, Sheet, SheetId, Workbook,
};

use crate::conditional::CfExpressionRules;

fn range(r0: u32, c0: u32, r1: u32, c1: u32) -> CellRange {
    CellRange::new(CellRef::new(r0, c0), CellRef::new(r1, c1))
}

/// A one-sheet workbook holding `numbers` and the conditional formats given.
fn workbook_with(numbers: &[((u32, u32), f64)], rules: Vec<ConditionalFormat>) -> Workbook {
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "Sheet1");
    for ((r, c), n) in numbers {
        sheet
            .cells
            .set(CellRef::new(*r, *c), Cell::value(CellValue::Number(*n)));
    }
    sheet.conditional_formats = rules;
    wb.sheets.push(sheet);
    wb
}

/// Whether the rules paint `(row, col)`, resolved the way a renderer does.
fn fill_at(wb: &Workbook, row: u32, col: u32, exprs: &dyn CfExpressions) -> Option<String> {
    let sheet = &wb.sheets[0];
    let stats: Vec<_> = sheet
        .conditional_formats
        .iter()
        .map(|cf| casual_calc_layout::conditional::range_stats(wb, sheet, cf))
        .collect();
    let order = priority_order(sheet);
    let value = sheet
        .cells
        .get(CellRef::new(row, col))
        .map_or(CellValue::Empty, |c| c.value.clone());
    let text = casual_calc_layout::display_text(
        wb,
        sheet
            .cells
            .get(CellRef::new(row, col))
            .unwrap_or(&Cell::value(CellValue::Empty)),
    );
    effect_for(sheet, &stats, &order, row, col, &value, &text, exprs).fill
}

/// **The whole-row highlight, over a range that does not start at `A1`.**
///
/// `=$D2>100` applied to `A2:H10` is the commonest conditional format in real
/// workbooks. The formula is anchored to `A2` — the range's top-left — and
/// shifted per cell from there, so row 2 asks about `D2`, row 3 about `D3`, and
/// the `$` keeps every one of them in column D no matter how far right the cell
/// is.
///
/// Anchoring it to `A1` instead is the mistake this asserts against: it would
/// shift every row down by one, so A2 would be decided by `D3` (50, no
/// highlight) and A3 by `D4`. The range therefore starts at row 2, not row 1 —
/// with an `A1:H10` range the two anchorings agree and the test proves nothing.
#[test]
fn an_expression_rule_shifts_per_cell_from_the_ranges_top_left_not_from_a1() {
    let mut rule = ConditionalFormat::new(
        range(1, 0, 9, 7), // A2:H10
        CfRule::Expression("$D2>100".to_owned()),
        "FFC7CE",
    );
    rule.priority = 1;
    let wb = workbook_with(
        &[
            ((1, 3), 150.0), // D2 — over
            ((2, 3), 50.0),  // D3 — under
            ((6, 3), 200.0), // D7 — over
        ],
        vec![rule],
    );
    let exprs = CfExpressionRules::new(&wb, 0);

    assert_eq!(
        fill_at(&wb, 1, 0, &exprs).as_deref(),
        Some("FFC7CE"),
        "A2 is painted because D2 is 150 — the top-left row reads its own row, \
         which is exactly what an A1 anchor would get wrong"
    );
    assert_eq!(
        fill_at(&wb, 2, 0, &exprs),
        None,
        "A3 is not, because D3 is 50"
    );
    assert_eq!(
        fill_at(&wb, 6, 0, &exprs).as_deref(),
        Some("FFC7CE"),
        "A7 is, because D7 is 200 — five rows down, five rows of shift"
    );
    assert_eq!(
        fill_at(&wb, 6, 7, &exprs).as_deref(),
        Some("FFC7CE"),
        "and so is H7: `$D` is pinned, so seven columns of shift move nothing"
    );
    assert_eq!(
        fill_at(&wb, 2, 7, &exprs),
        None,
        "while H3 stays unpainted, on D3's 50"
    );
}

/// A fully relative formula shifts in **both** axes, from a top-left that is
/// neither row nor column zero.
///
/// `C3>10` over `C3:E5` is "highlight the cells that are over ten" written the
/// long way. With an `A1` anchor, C3 would resolve to E5 — two rows and two
/// columns out in one step — so the diagonal of values below tells the two
/// apart cell by cell.
#[test]
fn a_relative_expression_shifts_in_both_axes() {
    let mut rule = ConditionalFormat::new(
        range(2, 2, 4, 4), // C3:E5
        CfRule::Expression("C3>10".to_owned()),
        "C6EFCE",
    );
    rule.priority = 1;
    let wb = workbook_with(
        &[
            ((2, 2), 50.0), // C3 over
            ((2, 4), 1.0),  // E3 under
            ((4, 2), 1.0),  // C5 under
            ((4, 4), 99.0), // E5 over
        ],
        vec![rule],
    );
    let exprs = CfExpressionRules::new(&wb, 0);

    assert_eq!(fill_at(&wb, 2, 2, &exprs).as_deref(), Some("C6EFCE"), "C3");
    assert_eq!(fill_at(&wb, 2, 4, &exprs), None, "E3");
    assert_eq!(fill_at(&wb, 4, 2, &exprs), None, "C5");
    assert_eq!(fill_at(&wb, 4, 4, &exprs).as_deref(), Some("C6EFCE"), "E5");
}

/// The formula may read a cell **outside** the rule's range, and shifts off the
/// range with it — which is what makes "highlight the row when its status
/// column says overdue" work when the status column is not in the range.
#[test]
fn an_expression_may_test_a_cell_outside_the_painted_range() {
    let mut rule = ConditionalFormat::new(
        range(1, 0, 3, 1), // A2:B4, painting columns A and B only
        CfRule::Expression("$F2=\"overdue\"".to_owned()),
        "FFD166",
    );
    rule.priority = 1;
    let mut wb = workbook_with(&[], vec![rule]);
    let overdue = wb.intern_string("overdue");
    let paid = wb.intern_string("paid");
    wb.sheets[0].cells.set(
        CellRef::new(1, 5), // F2
        Cell::value(CellValue::InlineString(overdue)),
    );
    wb.sheets[0].cells.set(
        CellRef::new(2, 5), // F3
        Cell::value(CellValue::InlineString(paid)),
    );
    let exprs = CfExpressionRules::new(&wb, 0);

    assert_eq!(
        fill_at(&wb, 1, 0, &exprs).as_deref(),
        Some("FFD166"),
        "row 2 is highlighted on F2"
    );
    assert_eq!(
        fill_at(&wb, 1, 1, &exprs).as_deref(),
        Some("FFD166"),
        "both painted columns of it, from the one status cell"
    );
    assert_eq!(fill_at(&wb, 2, 0, &exprs), None, "row 3 is not, on F3");
}

/// The formula follows the cells it reads, so a rule over cells that hold
/// formulas is decided by their **values**, not by their text.
#[test]
fn an_expression_reads_computed_values() {
    let mut rule = ConditionalFormat::new(
        range(1, 1, 3, 1), // B2:B4
        CfRule::Expression("B2>10".to_owned()),
        "D1F0D6",
    );
    rule.priority = 1;
    let mut wb = workbook_with(&[((1, 0), 4.0), ((2, 0), 2.0)], vec![]);
    // B2 = A2 * 3 = 12; B3 = A3 * 3 = 6. Stored relative to their own cells.
    for row in 1..=2u32 {
        let expr = casual_calc_formula::parse("A2*3").unwrap();
        let handle = wb.store_formula_at(
            casual_calc_formula::restore_at(
                &expr,
                casual_calc_formula::stored::ABSOLUTE,
                casual_calc_formula::stored::Origin::at(1, 1),
            ),
            casual_calc_formula::stored::Origin::at(row, 1),
        );
        let mut cell = Cell::value(CellValue::Empty);
        cell.formula = Some(handle);
        wb.sheets[0].cells.set(CellRef::new(row, 1), cell);
    }
    wb.sheets[0].conditional_formats = vec![rule];
    let exprs = CfExpressionRules::new(&wb, 0);

    assert_eq!(
        fill_at(&wb, 1, 1, &exprs).as_deref(),
        Some("D1F0D6"),
        "B2 computes 12, which is over 10"
    );
    assert_eq!(
        fill_at(&wb, 2, 1, &exprs),
        None,
        "B3 computes 6, which is not — and neither cell holds a cached value, \
         so a rule reading `cell.value` instead of evaluating would paint neither"
    );
}

/// A caller with no evaluator paints nothing for a formula rule, and paints the
/// other rules exactly as it did before.
///
/// This is the seam `NoExpressions` exists for: layout is below the calc
/// engine, so its own entry points cannot decide a formula. What matters is
/// that not deciding one is quiet — an unpainted cell — rather than a panic or
/// a wrong colour from `matches_number` testing the wrong operand.
#[test]
fn without_an_evaluator_a_formula_rule_is_simply_not_painted() {
    let mut formula_rule = ConditionalFormat::new(
        range(1, 0, 9, 7),
        CfRule::Expression("$D2>100".to_owned()),
        "FFC7CE",
    );
    formula_rule.priority = 1;
    let mut numeric_rule =
        ConditionalFormat::new(range(1, 0, 9, 7), CfRule::GreaterThan(5.0), "C6EFCE");
    numeric_rule.priority = 2;
    let wb = workbook_with(
        &[((1, 0), 7.0), ((1, 3), 150.0)],
        vec![formula_rule, numeric_rule],
    );

    assert_eq!(
        fill_at(&wb, 1, 0, &CfExpressionRules::new(&wb, 0)).as_deref(),
        Some("FFC7CE"),
        "with an evaluator the formula rule wins on priority"
    );
    assert_eq!(
        fill_at(&wb, 1, 0, &NoExpressions).as_deref(),
        Some("C6EFCE"),
        "without one it is skipped and the next rule decides — not a panic, \
         and not the formula rule's colour on a cell nobody tested"
    );
}

/// A formula that does not parse, or that evaluates to an error or to text,
/// paints nothing.
///
/// Excel paints on `TRUE` alone. Treating `#DIV/0!` as a hit would highlight
/// exactly the rows a user most needs to see are broken.
#[test]
fn a_rule_that_cannot_be_decided_paints_nothing() {
    let bad = |formula: &str| {
        let mut rule = ConditionalFormat::new(
            range(1, 0, 3, 3),
            CfRule::Expression(formula.to_owned()),
            "FFC7CE",
        );
        rule.priority = 1;
        let wb = workbook_with(&[((1, 0), 1.0)], vec![rule]);
        let exprs = CfExpressionRules::new(&wb, 0);
        fill_at(&wb, 1, 0, &exprs)
    };
    assert_eq!(bad("$D2>"), None, "a formula that does not parse");
    assert_eq!(bad("1/0"), None, "one that evaluates to #DIV/0!");
    assert_eq!(bad("\"maybe\""), None, "one that evaluates to text");
    assert_eq!(bad("0"), None, "and zero is false, as it is in Excel");
    assert_eq!(
        {
            let mut rule = ConditionalFormat::new(
                range(1, 0, 3, 3),
                CfRule::Expression("1".to_owned()),
                "FFC7CE",
            );
            rule.priority = 1;
            let wb = workbook_with(&[((1, 0), 1.0)], vec![rule]);
            let exprs = CfExpressionRules::new(&wb, 0);
            fill_at(&wb, 1, 0, &exprs)
        },
        Some("FFC7CE".to_owned()),
        "while a non-zero number is true"
    );
}
