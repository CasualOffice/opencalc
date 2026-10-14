//! The functions `XL-FEAT-012` / `XL-FEAT-013` found missing: `XOR`,
//! `AGGREGATE`, the `.MATH` / `.PRECISE` roundings, `FORMULATEXT`, the 2010
//! statistics family, the 2013 maths and engineering additions and the 2022
//! dynamic-array shapers.
//!
//! **Where the expected values come from.** Not from this implementation. Each
//! is one of:
//!
//! * Microsoft's own documented example for the function (cited `MS:`), or
//! * LibreOffice Calc 26.2's answer to the same formula over the same fixture
//!   (cited `LO:`), read off the oracle harness (`tools/casual-calc-fidelity`),
//!   which also runs every one of these in CI from its corpus, or
//! * where LibreOffice and Microsoft's documentation disagree, the documented
//!   Excel behaviour, with the disagreement named at the assertion.
//!
//! The fixture is the oracle corpus's, cell for cell, so an `LO:` value here
//! is the value the corpus line of the same formula is compared against.

use casual_calc_formula::parse;
use casual_calc_formula::stored::Origin;
use casual_calc_model::{Cell, CellRef, CellValue, ErrorValue, Id, Sheet, SheetId, Workbook};

use crate::recalculate;

/// Where the formula under test is written: well clear of the fixture, with
/// room below and to the right for a spill.
const AT: (u32, u32) = (19, 10);

/// The oracle corpus's fixture block (`tools/casual-calc-fidelity/corpus`).
///
/// `C1:C10` 1..10, `D1:D10` 10..100, `E1:E5` five words, `F1:F5` alternating
/// logicals, `G1:G5` mixed signs, `H1:H5` numeric-looking text with a blank.
fn fixture() -> (Workbook, Sheet) {
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    for row in 0..10u32 {
        let n = f64::from(row + 1);
        sheet
            .cells
            .set(CellRef::new(row, 2), Cell::value(CellValue::Number(n)));
        sheet.cells.set(
            CellRef::new(row, 3),
            Cell::value(CellValue::Number(n * 10.0)),
        );
    }
    for (row, word) in ["alpha", "beta", "gamma", "delta", "epsilon"]
        .iter()
        .enumerate()
    {
        let id = wb.intern_string(word);
        sheet.cells.set(
            CellRef::new(row as u32, 4),
            Cell::value(CellValue::SharedString(id)),
        );
    }
    for (row, flag) in [true, false, true, false, true].iter().enumerate() {
        sheet.cells.set(
            CellRef::new(row as u32, 5),
            Cell::value(CellValue::Bool(*flag)),
        );
    }
    for (row, n) in [2.5f64, -3.5, 0.0, 1e10, -0.25].iter().enumerate() {
        sheet.cells.set(
            CellRef::new(row as u32, 6),
            Cell::value(CellValue::Number(*n)),
        );
    }
    for (row, text) in ["10", "-2.5", "abc", "", "3"].iter().enumerate() {
        if text.is_empty() {
            continue;
        }
        let id = wb.intern_string(text);
        sheet.cells.set(
            CellRef::new(row as u32, 7),
            Cell::value(CellValue::SharedString(id)),
        );
    }
    (wb, sheet)
}

fn put_formula(wb: &mut Workbook, sheet: &mut Sheet, at: (u32, u32), text: &str) {
    let expr = parse(text).unwrap_or_else(|e| panic!("parse {text:?}: {e}"));
    let handle = wb.store_formula_at(expr, Origin::at(at.0, at.1));
    let mut cell = Cell::value(CellValue::Empty);
    cell.formula = Some(handle);
    sheet.cells.set(CellRef::new(at.0, at.1), cell);
}

fn put_number(sheet: &mut Sheet, at: (u32, u32), n: f64) {
    sheet
        .cells
        .set(CellRef::new(at.0, at.1), Cell::value(CellValue::Number(n)));
}

fn put_text(wb: &mut Workbook, sheet: &mut Sheet, at: (u32, u32), s: &str) {
    let id = wb.intern_string(s);
    sheet.cells.set(
        CellRef::new(at.0, at.1),
        Cell::value(CellValue::SharedString(id)),
    );
}

fn cell(wb: &Workbook, row: u32, col: u32) -> CellValue {
    wb.sheets[0]
        .cells
        .get(CellRef::new(row, col))
        .map(|c| c.value.clone())
        .unwrap_or(CellValue::Empty)
}

/// A readable rendering, so a failure says what came back.
fn show(wb: &Workbook, v: &CellValue) -> String {
    match v {
        CellValue::Empty => "<empty>".to_owned(),
        CellValue::Number(n) => format!("{n}"),
        CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        CellValue::Error(e) => e.to_string(),
        CellValue::SharedString(id) | CellValue::InlineString(id) => {
            format!("{:?}", wb.strings.get(*id).unwrap_or_default())
        }
    }
}

/// Evaluate `formula` over the fixture, with `setup` run on the sheet first.
fn eval_in(formula: &str, setup: impl FnOnce(&mut Workbook, &mut Sheet)) -> (Workbook, CellValue) {
    let (mut wb, mut sheet) = fixture();
    setup(&mut wb, &mut sheet);
    put_formula(&mut wb, &mut sheet, AT, formula);
    wb.sheets.push(sheet);
    recalculate(&mut wb);
    let v = cell(&wb, AT.0, AT.1);
    (wb, v)
}

fn eval(formula: &str) -> (Workbook, CellValue) {
    eval_in(formula, |_, _| {})
}

/// Agreement to twelve significant digits — the oracle harness's own
/// tolerance, and far tighter than any real defect.
fn close(a: f64, b: f64) -> bool {
    a == b || (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1e-300)
}

#[track_caller]
fn num(formula: &str, expected: f64) {
    let (wb, v) = eval(formula);
    match v {
        CellValue::Number(n) if close(n, expected) => {}
        other => panic!("{formula}: expected {expected}, got {}", show(&wb, &other)),
    }
}

/// As [`num`], to a looser tolerance — for a documented example Microsoft
/// prints to only a few digits.
#[track_caller]
fn approx(formula: &str, expected: f64, tolerance: f64) {
    let (wb, v) = eval(formula);
    match v {
        CellValue::Number(n) if (n - expected).abs() <= tolerance => {}
        other => panic!(
            "{formula}: expected {expected} ± {tolerance}, got {}",
            show(&wb, &other)
        ),
    }
}

#[track_caller]
fn error(formula: &str, expected: ErrorValue) {
    let (wb, v) = eval(formula);
    assert_eq!(
        v,
        CellValue::Error(expected),
        "{formula}: got {}",
        show(&wb, &v)
    );
}

#[track_caller]
fn boolean(formula: &str, expected: bool) {
    let (wb, v) = eval(formula);
    assert_eq!(
        v,
        CellValue::Bool(expected),
        "{formula}: got {}",
        show(&wb, &v)
    );
}

#[track_caller]
fn text(formula: &str, expected: &str) {
    let (wb, v) = eval(formula);
    match &v {
        CellValue::SharedString(id) | CellValue::InlineString(id)
            if wb.strings.get(*id).unwrap_or_default() == expected => {}
        other => panic!("{formula}: expected {expected:?}, got {}", show(&wb, other)),
    }
}

// --- Group 1: XOR, AGGREGATE, the .MATH/.PRECISE roundings, FORMULATEXT ------

#[test]
fn xor_is_true_for_an_odd_count_of_trues() {
    // LO: each of these.
    boolean("XOR(TRUE,FALSE)", true);
    boolean("XOR(TRUE,TRUE)", false);
    boolean("XOR(F1:F5)", true); // three TRUEs
    boolean("XOR(1,0,1)", false);
    boolean("XOR(C1:C3)", true); // three non-zero numbers
    boolean("XOR(F1:F5,TRUE)", false);
    // MS: =XOR(3>0,2<9) is FALSE, =XOR(3>12,4>6) is FALSE.
    boolean("XOR(3>0,2<9)", false);
    boolean("XOR(3>12,4>6)", false);
}

#[test]
fn xor_rejects_text_and_skips_it_in_references() {
    // LO: a text argument is #VALUE!, and a reference holding only text has
    // no logical to work with — also #VALUE!.
    error("XOR(\"a\")", ErrorValue::Value);
    error("XOR(E1:E5)", ErrorValue::Value);
    // MS: "If an array or reference argument contains text or empty cells,
    // those values are ignored." The text in E1 must not poison the TRUE.
    boolean("XOR(E1:F1)", true);
    // Errors propagate.
    error("XOR(TRUE,1/0)", ErrorValue::Div0);
    error("XOR()", ErrorValue::Value);
}

#[test]
fn aggregate_reference_form_computes_all_thirteen_functions() {
    // LO, every value.
    num("AGGREGATE(9,0,C1:C10)", 55.0);
    num("AGGREGATE(1,4,C1:C10)", 5.5);
    num("AGGREGATE(2,0,C1:C10,D1:D10)", 20.0);
    num("AGGREGATE(3,0,E1:E5)", 5.0);
    num("AGGREGATE(4,0,G1:G5)", 1e10);
    num("AGGREGATE(5,0,G1:G5)", -3.5);
    num("AGGREGATE(6,0,C1:C5)", 120.0);
    num("AGGREGATE(7,0,C1:C10)", 3.02765035409749);
    num("AGGREGATE(8,0,C1:C10)", 2.87228132326901);
    num("AGGREGATE(10,0,C1:C10)", 9.16666666666667);
    num("AGGREGATE(11,0,C1:C10)", 8.25);
    num("AGGREGATE(12,0,C1:C10)", 5.5);
    num("AGGREGATE(13,0,C1:C3,C2)", 2.0);
    num("AGGREGATE(9,0,C1:C10,D1:D10)", 605.0);
    num("AGGREGATE(9.7,0,C1:C10)", 55.0); // function_num truncates
    // Text is not a number to these, as for the functions they stand for.
    num("AGGREGATE(9,0,H1:H5)", 0.0);
    num("AGGREGATE(4,0,E1:E5)", 0.0);
    error("AGGREGATE(1,0,E1:E5)", ErrorValue::Div0);
}

#[test]
fn aggregate_array_form_computes_the_six_k_functions() {
    // LO, every value.
    num("AGGREGATE(14,0,C1:C10,2)", 9.0);
    num("AGGREGATE(15,0,C1:C10,2)", 2.0);
    num("AGGREGATE(16,0,C1:C10,0.3)", 3.7);
    num("AGGREGATE(17,0,C1:C10,1)", 3.25);
    num("AGGREGATE(18,0,C1:C10,0.3)", 3.3);
    num("AGGREGATE(19,0,C1:C10,1)", 2.75);
    // The reason the array form exists: a computed array whose unwanted
    // elements are errors, skipped by option 6.
    num("AGGREGATE(14,6,C1:C10/(C1:C10>5),1)", 10.0);
    num("AGGREGATE(15,6,C1:C10/(C1:C10>5),1)", 6.0);
}

#[test]
fn aggregate_refuses_what_excel_refuses() {
    // LO raises an error for each; the token is Excel's.
    error("AGGREGATE(20,0,C1:C10)", ErrorValue::Value);
    error("AGGREGATE(0,0,C1:C10)", ErrorValue::Value);
    error("AGGREGATE(9,8,C1:C10)", ErrorValue::Value);
    // MS: "If a second ref argument is required, but not provided, AGGREGATE
    // returns a #VALUE! error."
    error("AGGREGATE(14,0,C1:C10)", ErrorValue::Value);
    // k out of range is LARGE's own #NUM!.
    error("AGGREGATE(14,0,C1:C10,11)", ErrorValue::Num);
    // The reference form takes references: a computed array is #VALUE! in
    // Excel. LibreOffice accepts it (6) — recorded @differs in the corpus.
    error("AGGREGATE(9,0,SEQUENCE(3))", ErrorValue::Value);
    error("AGGREGATE(9,0,C1:C3*2)", ErrorValue::Value);
}

/// `A1:A6` = 1, #DIV/0!, 3, 4 (hidden row), =SUBTOTAL(9,A1), =AGGREGATE(9,4,A3).
fn aggregate_options_sheet(wb: &mut Workbook, sheet: &mut Sheet) {
    put_number(sheet, (0, 0), 1.0);
    put_formula(wb, sheet, (1, 0), "1/0");
    put_number(sheet, (2, 0), 3.0);
    put_number(sheet, (3, 0), 4.0);
    sheet.hidden_rows.insert(3);
    put_formula(wb, sheet, (4, 0), "SUBTOTAL(9,A1)");
    put_formula(wb, sheet, (5, 0), "AGGREGATE(9,4,A3)");
}

#[track_caller]
fn aggregate_option(formula: &str, expected: Result<f64, ErrorValue>) {
    let (wb, v) = eval_in(formula, aggregate_options_sheet);
    let want = match expected {
        Ok(n) => CellValue::Number(n),
        Err(e) => CellValue::Error(e),
    };
    assert_eq!(v, want, "{formula}: got {}", show(&wb, &v));
}

#[test]
fn aggregate_options_choose_what_to_ignore() {
    // MS's option table, applied to: 1, #DIV/0!, 3, 4 (hidden), a nested
    // SUBTOTAL worth 1, and a nested AGGREGATE worth 3.
    // 0: nested only — the error still propagates.
    aggregate_option("AGGREGATE(9,0,A1:A6)", Err(ErrorValue::Div0));
    aggregate_option("AGGREGATE(9,,A1:A6)", Err(ErrorValue::Div0));
    // 1: hidden + nested — error still there.
    aggregate_option("AGGREGATE(9,1,A1:A6)", Err(ErrorValue::Div0));
    // 2: errors + nested: 1 + 3 + 4.
    aggregate_option("AGGREGATE(9,2,A1:A6)", Ok(8.0));
    // 3: hidden, errors, nested: 1 + 3.
    aggregate_option("AGGREGATE(9,3,A1:A6)", Ok(4.0));
    // 4: nothing ignored.
    aggregate_option("AGGREGATE(9,4,A1:A6)", Err(ErrorValue::Div0));
    // 5: hidden only.
    aggregate_option("AGGREGATE(9,5,A1:A6)", Err(ErrorValue::Div0));
    // 6: errors only: 1 + 3 + 4 + 1 + 3.
    aggregate_option("AGGREGATE(9,6,A1:A6)", Ok(12.0));
    // 7: hidden and errors: 1 + 3 + 1 + 3.
    aggregate_option("AGGREGATE(9,7,A1:A6)", Ok(8.0));
    // The array form ignores the same things when handed a plain range…
    aggregate_option("AGGREGATE(14,7,A1:A6,1)", Ok(3.0));
    aggregate_option("AGGREGATE(14,6,A1:A6,1)", Ok(4.0));
    // …but MS: "will not ignore hidden rows, nested subtotals or nested
    // aggregates if the array argument includes a calculation".
    aggregate_option("AGGREGATE(14,7,A1:A6*1,1)", Ok(4.0));
    // COUNTA over the same, errors skipped: five values remain.
    aggregate_option("AGGREGATE(3,6,A1:A6)", Ok(5.0));
}

#[test]
fn ceiling_math_rounds_up_and_its_mode_flips_negatives() {
    // MS examples.
    num("CEILING.MATH(24.3,5)", 25.0);
    num("CEILING.MATH(6.7)", 7.0);
    num("CEILING.MATH(-8.1,2)", -8.0);
    num("CEILING.MATH(-5.5,2,-1)", -6.0);
    // LO.
    num("CEILING.MATH(-5.5,2,1)", -6.0);
    num("CEILING.MATH(-5.5,2,0)", -4.0);
    num("CEILING.MATH(5.5,-2)", 6.0); // significance's sign is ignored
    num("CEILING.MATH(0,2)", 0.0);
    num("CEILING.MATH(4.2,0)", 0.0);
    error("CEILING.MATH(\"x\")", ErrorValue::Value);
}

#[test]
fn floor_math_rounds_down_and_its_mode_flips_negatives() {
    // MS examples.
    num("FLOOR.MATH(24.3,5)", 20.0);
    num("FLOOR.MATH(6.7)", 6.0);
    num("FLOOR.MATH(-8.1,2)", -10.0);
    num("FLOOR.MATH(-5.5,2,-1)", -4.0);
    // LO.
    num("FLOOR.MATH(-5.5,2,1)", -4.0);
    num("FLOOR.MATH(5.5,-2)", 4.0);
    num("FLOOR.MATH(4.2,0)", 0.0);
}

#[test]
fn precise_roundings_ignore_the_significance_sign() {
    // MS examples for CEILING.PRECISE.
    num("CEILING.PRECISE(4.3)", 5.0);
    num("CEILING.PRECISE(-4.3)", -4.0);
    num("CEILING.PRECISE(4.3,2)", 6.0);
    num("CEILING.PRECISE(4.3,-2)", 6.0);
    num("CEILING.PRECISE(-4.3,2)", -4.0);
    num("CEILING.PRECISE(-4.3,-2)", -4.0);
    num("CEILING.PRECISE(4.3,0)", 0.0); // LO
    // MS examples for FLOOR.PRECISE.
    num("FLOOR.PRECISE(-3.2,-1)", -4.0);
    num("FLOOR.PRECISE(3.2)", 3.0);
    num("FLOOR.PRECISE(-3.2,1)", -4.0);
    num("FLOOR.PRECISE(3.2,-1)", 3.0);
    num("FLOOR.PRECISE(3.2,0)", 0.0);
    num("FLOOR.PRECISE(3.2,1.5)", 3.0); // LO
}

#[test]
fn formulatext_shows_the_formula_as_excel_displays_it() {
    let setup = |wb: &mut Workbook, sheet: &mut Sheet| {
        put_formula(wb, sheet, (0, 0), "SUM(C1:C3)*2");
        put_formula(wb, sheet, (1, 0), "XOR(TRUE,FALSE)");
        put_formula(wb, sheet, (2, 0), "IF(C1>0,\"pos\",\"neg\")");
    };
    let read = |f: &str| {
        let (wb, v) = eval_in(f, setup);
        show(&wb, &v)
    };
    assert_eq!(read("FORMULATEXT(A1)"), "\"=SUM(C1:C3)*2\"");
    // The display form: logicals as written, not LibreOffice's `TRUE()`.
    assert_eq!(read("FORMULATEXT(A2)"), "\"=XOR(TRUE,FALSE)\"");
    assert_eq!(
        read("FORMULATEXT(A3)"),
        "\"=IF(C1>0,\\\"pos\\\",\\\"neg\\\")\""
    );
    // MS: a range answers for its upper-left cell.
    assert_eq!(read("FORMULATEXT(A2:A3)"), "\"=XOR(TRUE,FALSE)\"");
    // MS: "If the Reference argument is to a cell that does not contain a
    // formula, FORMULATEXT returns the #N/A error value." LO agrees.
    assert_eq!(read("FORMULATEXT(C1)"), "#N/A");
    assert_eq!(read("FORMULATEXT(Z99)"), "#N/A");
    // Not a reference at all.
    assert_eq!(read("FORMULATEXT(\"A1\")"), "#VALUE!");
    assert_eq!(read("FORMULATEXT(1+1)"), "#VALUE!");
}

#[test]
fn formulatext_reads_another_sheet() {
    let (mut wb, sheet) = fixture();
    let mut other = Sheet::new(SheetId(Id::from_parts(2, 2)), "Other");
    put_formula(&mut wb, &mut other, (4, 1), "C1+1");
    let mut first = sheet;
    put_formula(&mut wb, &mut first, AT, "FORMULATEXT(Other!B5)");
    wb.sheets.push(first);
    wb.sheets.push(other);
    recalculate(&mut wb);
    let v = cell(&wb, AT.0, AT.1);
    assert_eq!(show(&wb, &v), "\"=C1+1\"");
}

// --- Group 2: statistics -----------------------------------------------------

/// A setup that writes `values` down column A from `A1`, for the documented
/// examples whose data is not the fixture's.
fn column_a(values: &'static [f64]) -> impl FnOnce(&mut Workbook, &mut Sheet) {
    move |_, sheet| {
        for (i, v) in values.iter().enumerate() {
            put_number(sheet, (i as u32, 0), *v);
        }
    }
}

/// Two columns, A and B, for the paired examples.
fn columns_ab(a: &'static [f64], b: &'static [f64]) -> impl FnOnce(&mut Workbook, &mut Sheet) {
    move |_, sheet| {
        for (i, v) in a.iter().enumerate() {
            put_number(sheet, (i as u32, 0), *v);
        }
        for (i, v) in b.iter().enumerate() {
            put_number(sheet, (i as u32, 1), *v);
        }
    }
}

#[track_caller]
fn num_with(
    formula: &str,
    setup: impl FnOnce(&mut Workbook, &mut Sheet),
    expected: f64,
    tolerance: f64,
) {
    let (wb, v) = eval_in(formula, setup);
    match v {
        CellValue::Number(n) if (n - expected).abs() <= tolerance => {}
        other => panic!(
            "{formula}: expected {expected} ± {tolerance}, got {}",
            show(&wb, &other)
        ),
    }
}

#[test]
fn student_t_family() {
    // LO, and MS (T.DIST: 0.99469533, 0.00073691; T.DIST.2T 0.054644930;
    // T.DIST.RT 0.027322465; T.INV 0.8164966).
    num("T.DIST(60,1,TRUE)", 0.994695326367377);
    num("T.DIST(8,3,FALSE)", 0.000736906520946927);
    num("T.DIST(-1.5,4,TRUE)", 0.104);
    num("T.DIST(0,4,TRUE)", 0.5);
    num("T.DIST(1,1.5,TRUE)", 0.75); // degrees of freedom truncate
    error("T.DIST(1,0,TRUE)", ErrorValue::Num);
    error("T.DIST(1,4)", ErrorValue::Value); // cumulative is required
    num("T.DIST.2T(1.959999998,60)", 0.0546449299759211);
    num("T.DIST.2T(0,10)", 1.0);
    error("T.DIST.2T(-1,10)", ErrorValue::Num); // MS: x < 0 is #NUM!
    num("T.DIST.RT(1.959999998,60)", 0.0273224649879605);
    num("T.DIST.RT(-1,10)", 0.82955343384897);
    num("T.INV(0.75,2)", 0.816496580927726);
    num("T.INV(0.1,10)", -1.37218364111034);
    error("T.INV(0,2)", ErrorValue::Num);
    error("T.INV(1,2)", ErrorValue::Num);
    error("T.INV(0.5,0.5)", ErrorValue::Num);
}

#[test]
fn f_and_chi_square_left_tails() {
    // LO; MS gives 0.99, 0.0012238, 0.10930991.
    num("F.DIST(15.2069,6,4,TRUE)", 0.990000043002763);
    num("F.DIST(15.2069,6,4,FALSE)", 0.00122379170878317);
    num("F.DIST(0,6,4,FALSE)", 0.0);
    num("F.DIST(0,2,4,FALSE)", 1.0);
    error("F.DIST(-1,6,4,TRUE)", ErrorValue::Num);
    error("F.DIST(1,0.5,4,TRUE)", ErrorValue::Num);
    num("F.INV(0.01,6,4)", 0.109309914124579);
    num("F.INV(0.99,6,4)", 15.2068648611575);
    error("F.INV(-0.1,6,4)", ErrorValue::Num);
    // LO; MS gives 0.52049988, 0.20755375, 3.283020287, 1.832581464.
    num("CHISQ.DIST(0.5,1,TRUE)", 0.520499877813047);
    num("CHISQ.DIST(2,3,FALSE)", 0.207553748710297);
    error("CHISQ.DIST(-1,3,TRUE)", ErrorValue::Num);
    error("CHISQ.DIST(1,0.5,TRUE)", ErrorValue::Num);
    num("CHISQ.INV(0.93,1)", 3.28302028675954);
    num("CHISQ.INV(0.6,2)", 1.83258146374831);
    num("CHISQ.INV(0,2)", 0.0);
    error("CHISQ.INV(1,2)", ErrorValue::Num);
}

#[test]
fn beta_dist_with_and_without_bounds() {
    // LO; MS gives 0.6854706 and 1.4837646.
    num("BETA.DIST(2,8,10,TRUE,1,3)", 0.685470581054688);
    num("BETA.DIST(2,8,10,FALSE,1,3)", 1.4837646484375);
    num("BETA.DIST(0.5,2,3,TRUE)", 0.6875);
    num("BETA.DIST(0.5,2,3,FALSE)", 1.5);
    error("BETA.DIST(4,8,10,TRUE,1,3)", ErrorValue::Num);
    error("BETA.DIST(0.5,0,3,TRUE)", ErrorValue::Num);
    error("BETA.DIST(0.5,2,3)", ErrorValue::Value);
}

#[test]
fn discrete_distributions() {
    // LO; MS: BINOM.INV(6,0.5,0.75) is 4.
    num("BINOM.INV(6,0.5,0.75)", 4.0);
    num("BINOM.INV(10,0.3,0)", 0.0);
    num("BINOM.INV(10,0.3,1)", 10.0);
    num("BINOM.INV(10,0.3,0.999999)", 10.0);
    error("BINOM.INV(10,1.3,0.5)", ErrorValue::Num);
    // MS: 0.084 and 0.524 (LibreOffice cannot read this function at all).
    approx("BINOM.DIST.RANGE(60,0.75,48)", 0.084, 5e-4);
    approx("BINOM.DIST.RANGE(60,0.75,45,50)", 0.524, 5e-4);
    // The range form is the sum of the point form, which LO can check.
    let (wb, a) = eval("BINOM.DIST.RANGE(60,0.75,45,50)");
    let (_, b) = eval("BINOM.DIST(50,60,0.75,TRUE)-BINOM.DIST(44,60,0.75,TRUE)");
    match (&a, &b) {
        (CellValue::Number(x), CellValue::Number(y)) => assert!(close(*x, *y), "{x} vs {y}"),
        _ => panic!("{} / {}", show(&wb, &a), show(&wb, &b)),
    }
    error("BINOM.DIST.RANGE(60,0.75,50,45)", ErrorValue::Num);
    error("BINOM.DIST.RANGE(60,0.75,61)", ErrorValue::Num);
    error("BINOM.DIST.RANGE(60,1.75,45)", ErrorValue::Num);
    // LO; MS: 0.4654 and 0.3633.
    num("HYPGEOM.DIST(1,4,8,20,TRUE)", 0.465428276573787);
    num("HYPGEOM.DIST(1,4,8,20,FALSE)", 0.363261093911249);
    error("HYPGEOM.DIST(5,4,8,20,FALSE)", ErrorValue::Num);
    error("HYPGEOM.DIST(-1,4,8,20,FALSE)", ErrorValue::Num);
    error("HYPGEOM.DIST(1,4,25,20,FALSE)", ErrorValue::Num);
    // LO; MS: 0.3135 and 0.0550.
    num("NEGBINOM.DIST(10,5,0.25,TRUE)", 0.313514058478177);
    num("NEGBINOM.DIST(10,5,0.25,FALSE)", 0.0550486603751779);
    error("NEGBINOM.DIST(-1,5,0.25,FALSE)", ErrorValue::Num);
    error("NEGBINOM.DIST(10,0,0.25,FALSE)", ErrorValue::Num);
    error("NEGBINOM.DIST(10,5,1.25,FALSE)", ErrorValue::Num);
}

/// MS's data for the `.EXC` examples: 1, 2, 3, 6, 6, 6, 7, 8, 9.
const EXC_DATA: &[f64] = &[1.0, 2.0, 3.0, 6.0, 6.0, 6.0, 7.0, 8.0, 9.0];

#[test]
fn exclusive_percentiles_and_quartiles() {
    // LO over the fixture.
    num("PERCENTILE.EXC(C1:C10,0.25)", 2.75);
    num("PERCENTILE.EXC(C1:C10,0.5)", 5.5);
    num("PERCENTILE.EXC(C1:C10,0.0909091)", 1.0000001);
    error("PERCENTILE.EXC(C1:C10,0)", ErrorValue::Num);
    error("PERCENTILE.EXC(C1:C10,0.05)", ErrorValue::Num);
    error("PERCENTILE.EXC(C1:C10,0.95)", ErrorValue::Num);
    error("PERCENTILE.EXC(C1:C10,1)", ErrorValue::Num);
    num("QUARTILE.EXC(C1:C10,1)", 2.75);
    num("QUARTILE.EXC(C1:C10,3)", 8.25);
    num("QUARTILE.EXC(C1:C10,2.7)", 5.5);
    error("QUARTILE.EXC(C1:C10,0)", ErrorValue::Num);
    error("QUARTILE.EXC(C1:C10,4)", ErrorValue::Num);
    // MS: PERCENTILE.EXC 2.5, then #NUM! for 0, 0.01 and 2; QUARTILE.EXC
    // 2.5 and 7.5 for the first and third.
    num_with("PERCENTILE.EXC(A1:A9,0.25)", column_a(EXC_DATA), 2.5, 1e-12);
    for k in ["0", "0.01", "2"] {
        let (wb, v) = eval_in(&format!("PERCENTILE.EXC(A1:A9,{k})"), column_a(EXC_DATA));
        assert_eq!(
            v,
            CellValue::Error(ErrorValue::Num),
            "{k}: {}",
            show(&wb, &v)
        );
    }
    num_with("QUARTILE.EXC(A1:A9,1)", column_a(EXC_DATA), 2.5, 1e-12);
    num_with("QUARTILE.EXC(A1:A9,3)", column_a(EXC_DATA), 7.5, 1e-12);
}

#[test]
fn percentrank_exc_truncates_its_digits() {
    // MS: 0.7, 0.381, and 0.3 with one digit — **truncated**, not rounded,
    // and to decimal places ("0.xxx"), not significant digits.
    num_with("PERCENTRANK.EXC(A1:A9,7)", column_a(EXC_DATA), 0.7, 1e-12);
    num_with(
        "PERCENTRANK.EXC(A1:A9,5.43)",
        column_a(EXC_DATA),
        0.381,
        1e-12,
    );
    num_with(
        "PERCENTRANK.EXC(A1:A9,5.43,1)",
        column_a(EXC_DATA),
        0.3,
        1e-12,
    );
    // A tie takes the rank of the first of the equal values: 4 of 10.
    num_with("PERCENTRANK.EXC(A1:A9,6)", column_a(EXC_DATA), 0.4, 1e-12);
    // LO over the fixture, where it agrees.
    num("PERCENTRANK.EXC(C1:C10,7)", 0.636);
    num("PERCENTRANK.EXC(C1:C10,7,5)", 0.63636);
    num("PERCENTRANK.EXC(C1:C10,5.43)", 0.493);
    num("PERCENTRANK.EXC(C1:C10,5.43,1)", 0.4);
    num("PERCENTRANK.EXC(C1:C10,1)", 0.09);
    error("PERCENTRANK.EXC(C1:C10,11)", ErrorValue::Na);
    error("PERCENTRANK.EXC(C1:C10,7,0)", ErrorValue::Num);
}

#[test]
fn rank_avg_averages_ties() {
    // MS: 94 in {89, 88, 92, 101, 94, 97, 95} ranks 4th.
    num_with(
        "RANK.AVG(94,A1:A7)",
        column_a(&[89.0, 88.0, 92.0, 101.0, 94.0, 97.0, 95.0]),
        4.0,
        0.0,
    );
    // Two 3s share 2nd and 3rd place: 2.5 either way round.
    num_with(
        "RANK.AVG(3,A1:A4)",
        column_a(&[1.0, 3.0, 3.0, 5.0]),
        2.5,
        0.0,
    );
    num_with(
        "RANK.AVG(3,A1:A4,1)",
        column_a(&[1.0, 3.0, 3.0, 5.0]),
        2.5,
        0.0,
    );
    // LO.
    num("RANK.AVG(5,C1:C10)", 6.0);
    num("RANK.AVG(3,C1:C10,1)", 3.0);
    num("RANK.AVG(-0.25,G1:G5)", 4.0);
    error("RANK.AVG(5.5,C1:C10)", ErrorValue::Na);
}

#[test]
fn covariance_skew_confidence_and_forecast() {
    // MS: 9.666666667.
    num_with(
        "COVARIANCE.S(A1:A3,B1:B3)",
        columns_ab(&[2.0, 4.0, 8.0], &[5.0, 11.0, 12.0]),
        29.0 / 3.0,
        1e-12,
    );
    // LO.
    num("COVARIANCE.S(C1:C3,G1:G3)", -1.25);
    num("COVARIANCE.S(C1:C10,D1:D10)", 91.6666666666667);
    // MS: one data point is #DIV/0!; mismatched sizes are #N/A.
    error("COVARIANCE.S(1,2)", ErrorValue::Div0);
    error("COVARIANCE.S(C1:C2,C1:C3)", ErrorValue::Na);
    // MS: 0.303193.
    num_with(
        "SKEW.P(A1:A10)",
        column_a(&[3.0, 4.0, 5.0, 2.0, 3.0, 4.0, 5.0, 6.0, 4.0, 7.0]),
        0.303193,
        5e-7,
    );
    approx("SKEW.P(C1:C10)", 0.0, 1e-12);
    num("SKEW.P(G1:G5)", 1.5); // LO
    // MS: fewer than three points, or no spread, is #DIV/0!.
    error("SKEW.P(1,2)", ErrorValue::Div0);
    error("SKEW.P(1,1,1)", ErrorValue::Div0);
    // LO; MS gives 0.284196855.
    num("CONFIDENCE.T(0.05,1,50)", 0.28419685549573);
    error("CONFIDENCE.T(0.05,1,1)", ErrorValue::Div0);
    error("CONFIDENCE.T(0,1,50)", ErrorValue::Num);
    error("CONFIDENCE.T(0.05,0,50)", ErrorValue::Num);
    // MS: 10.607253.
    num_with(
        "FORECAST.LINEAR(30,A1:A5,B1:B5)",
        columns_ab(
            &[6.0, 7.0, 9.0, 15.0, 21.0],
            &[20.0, 28.0, 31.0, 38.0, 40.0],
        ),
        10.607253,
        5e-7,
    );
    num("FORECAST.LINEAR(30,G1:G5,C1:C5)", 28999999994.35);
    num("FORECAST.LINEAR(5,D1:D10,C1:C10)", 50.0);
    error("FORECAST.LINEAR(5,C1:C3,C1:C3*0+2)", ErrorValue::Div0);
}

#[test]
fn gamma_and_the_normal_helpers() {
    // LO; MS gives 1.329 and 0.268 and #NUM! for 0 and -2.
    num("GAMMA(2.5)", 1.32934038817914);
    num("GAMMA(-3.75)", 0.267866128861416);
    num("GAMMA(0.5)", 1.77245385090552);
    num("GAMMA(171)", 7.257415615308e306);
    error("GAMMA(0)", ErrorValue::Num);
    error("GAMMA(-2)", ErrorValue::Num);
    error("GAMMA(172)", ErrorValue::Num);
    num("GAMMALN.PRECISE(4)", 1.79175946922806);
    num("GAMMALN.PRECISE(0.5)", 0.5723649429247);
    error("GAMMALN.PRECISE(0)", ErrorValue::Num);
    // LO; MS gives 0.47725, 0.301137432.
    num("GAUSS(2)", 0.477249868051821);
    num("GAUSS(-1)", -0.341344746068543);
    num("GAUSS(0)", 0.0);
    num("PHI(0.75)", 0.301137432154804);
    num("PHI(0)", 0.398942280401433);
    // LO; MS gives 0.70792892, 0.84270079 and 0.15729921.
    num("ERF.PRECISE(0.745)", 0.707928920095738);
    num("ERF.PRECISE(1)", 0.842700792949715);
    num("ERF.PRECISE(-1)", -0.842700792949715);
    num("ERFC.PRECISE(1)", 0.157299207050285);
    num("ERFC.PRECISE(-1)", 1.84270079294971);
}

/// Read a block `rows × cols` at `AT` after recalculating, plus whether the
/// cell beyond each edge is empty — a spill must be exactly this big.
fn spilled(
    formula: &str,
    setup: impl FnOnce(&mut Workbook, &mut Sheet),
    rows: u32,
    cols: u32,
) -> (Vec<Vec<String>>, bool) {
    let (wb, _) = eval_in(formula, setup);
    let grid = (0..rows)
        .map(|r| {
            (0..cols)
                .map(|c| show(&wb, &cell(&wb, AT.0 + r, AT.1 + c)))
                .collect()
        })
        .collect();
    let below = (0..cols).all(|c| cell(&wb, AT.0 + rows, AT.1 + c) == CellValue::Empty);
    let right = (0..rows).all(|r| cell(&wb, AT.0 + r, AT.1 + cols) == CellValue::Empty);
    (grid, below && right)
}

#[track_caller]
fn spills(formula: &str, setup: impl FnOnce(&mut Workbook, &mut Sheet), expected: &[&[&str]]) {
    let rows = expected.len() as u32;
    let cols = expected.first().map_or(0, |r| r.len()) as u32;
    let (grid, exact) = spilled(formula, setup, rows, cols);
    let want: Vec<Vec<String>> = expected
        .iter()
        .map(|r| r.iter().map(|s| (*s).to_owned()).collect())
        .collect();
    assert_eq!(grid, want, "{formula}");
    assert!(exact, "{formula} spilled further than {rows}×{cols}");
}

#[test]
fn mode_mult_spills_every_mode_down_a_column() {
    // MS: 1, 2, 3, 4, 3, 2, 1, 2, 3, 5, 6, 1 has three modes, 1, 2 and 3.
    spills(
        "MODE.MULT(A1:A12)",
        column_a(&[1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0, 2.0, 3.0, 5.0, 6.0, 1.0]),
        &[&["1"], &["2"], &["3"]],
    );
    // A computed array: (C1:C10>5)*1 is five 0s then five 1s — two modes,
    // in order of appearance; (C1:C10>3)*1 has one.
    spills("MODE.MULT((C1:C10>5)*1)", |_, _| {}, &[&["0"], &["1"]]);
    num("MODE.MULT((C1:C10>3)*1)", 1.0);
    // No value repeats: #N/A, as MODE.SNGL.
    error("MODE.MULT(C1:C10)", ErrorValue::Na);
}

/// `ROWS`/`COLUMNS` of a computed array. They read only a literal range, so
/// `ROWS(MODE.MULT(…))` — the way a formula asks how far a spill reaches, and
/// the way the oracle corpus checks one — was `#VALUE!`.
#[test]
fn rows_and_columns_measure_a_computed_array() {
    // LO: ROWS(MODE.MULT((C1:C10>5)*1)) is 2, COLUMNS 1.
    num("ROWS(MODE.MULT((C1:C10>5)*1))", 2.0);
    num("COLUMNS(MODE.MULT((C1:C10>5)*1))", 1.0);
    num("ROWS(SEQUENCE(4,3))", 4.0);
    num("COLUMNS(SEQUENCE(4,3))", 3.0);
    num("ROWS(5)", 1.0);
    error("ROWS(1/0)", ErrorValue::Div0);
    // The literal forms are unchanged.
    num("ROWS(C1:D10)", 10.0);
    num("COLUMNS(C1:D10)", 2.0);
}

/// The error function every normal-distribution function stands on was the
/// Abramowitz & Stegun rational approximation, good to 1.5e-7 — the seventh
/// digit of `NORM.S.DIST`, and of `ERF.PRECISE`, which exists to be precise.
/// It is now the incomplete gamma, to full precision; these are tabulated
/// values (and the oracle corpus checks the same against LibreOffice).
#[test]
fn the_error_function_is_precise_to_the_last_digit() {
    num("ERF(0.5)", 0.520499877813047);
    num("ERFC(2)", 0.004677734981047266);
    // The far tail keeps its digits: Φ(−6), not 0.5·(1 + erf) ≈ 0.
    num("NORMSDIST(-6)", 9.865_876_450_376_98e-10);
    num("NORM.S.DIST(1,TRUE)", 0.841344746068543);
}

// --- Security: every array result is bounded ---------------------------------

/// Evaluate, and fail if it took long enough to mean something was
/// materialised: an over-sized array must be refused from its dimensions, not
/// discovered by allocating it.
#[track_caller]
fn refused_quickly(formula: &str, expected: ErrorValue) {
    let started = std::time::Instant::now();
    error(formula, expected);
    let took = started.elapsed();
    assert!(
        took < std::time::Duration::from_secs(2),
        "{formula} took {took:?} — it built the array before refusing it"
    );
}

/// `SEQUENCE(1E10,1E10)` overflowed `rows * cols` and panicked; a size inside
/// `i64` but past memory — the whole grid, 17 billion cells — aborted the
/// process on allocation. Either is reachable from a file someone opens, in
/// the browser, the desktop shell or the collaboration server.
#[test]
fn an_array_larger_than_the_engine_can_hold_is_an_error_not_a_crash() {
    refused_quickly("SEQUENCE(1E10,1E10)", ErrorValue::Num);
    refused_quickly("SEQUENCE(1048576,16384)", ErrorValue::Num);
    // Past the grid on one axis, whatever the total.
    refused_quickly("SEQUENCE(1048577)", ErrorValue::Num);
    refused_quickly("SEQUENCE(1,16385)", ErrorValue::Num);
    // MAKEARRAY multiplied in i64 and could overflow the same way.
    refused_quickly("MAKEARRAY(1E10,1E10,LAMBDA(r,c,1))", ErrorValue::Num);
    // MMULT's result is rows(a) × cols(b): two modest inputs, an enormous
    // product.
    refused_quickly("MMULT(SEQUENCE(4000,1),SEQUENCE(1,4000))", ErrorValue::Num);
    // Within the budget is unchanged.
    num("SUM(SEQUENCE(1000,1000))", 500_000_500_000.0);
    num("ROWS(SEQUENCE(1048576))", 1_048_576.0);
}

/// REPT's text is capped where Excel caps every string, 32,767 characters —
/// `REPT("x",1E9)` was a gigabyte allocated on request.
#[test]
fn rept_cannot_build_a_string_past_excels_limit() {
    refused_quickly("REPT(\"ab\",1E15)", ErrorValue::Value);
    error("REPT(\"ab\",16384)", ErrorValue::Value);
    num("LEN(REPT(\"a\",32767))", 32767.0);
}

/// FIXED and DOLLAR passed `decimals` straight into a format width.
#[test]
fn fixed_and_dollar_refuse_more_than_127_decimals() {
    refused_quickly("FIXED(1,1E9)", ErrorValue::Value);
    refused_quickly("DOLLAR(1,1E9)", ErrorValue::Value);
    error("FIXED(1,128)", ErrorValue::Value);
    num("LEN(FIXED(1,127))", 129.0);
}

// --- Group 3: maths and engineering -----------------------------------------

#[test]
fn inverse_cotangents() {
    // LO; MS: ACOT(2) is 0.4636, ACOTH(6) is 0.168.
    num("ACOT(2)", 0.463647609000806);
    num("ACOT(0)", std::f64::consts::FRAC_PI_2); // LO prints 1.5707963267949
    num("ACOT(-2)", 2.67794504458899); // the range is (0, π), not (−π/2, π/2)
    num("ACOTH(6)", 0.168236118310606);
    num("ACOTH(-6)", -0.168236118310606);
    error("ACOTH(1)", ErrorValue::Num);
    error("ACOTH(0.5)", ErrorValue::Num);
    error("ACOT(\"x\")", ErrorValue::Value);
}

#[test]
fn arabic_reads_roman_numerals() {
    // MS: LVII is 57, mcmxii 1912, "mxmvii" 1997, "" is 0.
    num("ARABIC(\"LVII\")", 57.0);
    num("ARABIC(\"mcmxii\")", 1912.0);
    num("ARABIC(\"mxmvii\")", 1997.0);
    num("ARABIC(\"\")", 0.0);
    // LO: non-standard forms are read by the subtractive rule.
    num("ARABIC(\"IIII\")", 4.0);
    num("ARABIC(\"IC\")", 99.0);
    // MS: "evaluation of a negative Roman numeral is supported" and "leading
    // and trailing spaces are ignored" (LibreOffice agrees).
    num("ARABIC(\"-MMXI\")", -2011.0);
    num("ARABIC(\"  XV  \")", 15.0);
    // MS: numbers and text that is not a numeral are #VALUE!.
    error("ARABIC(\"ABC\")", ErrorValue::Value);
    error("ARABIC(5)", ErrorValue::Value);
    error("ARABIC(\"X V\")", ErrorValue::Value);
    // MS: at most 255 characters, so 255,000 is the largest.
    let max = format!("ARABIC(\"{}\")", "M".repeat(255));
    num(&max, 255_000.0);
    let over = format!("ARABIC(\"{}\")", "M".repeat(256));
    error(&over, ErrorValue::Value);
}

#[test]
fn base_and_decimal_convert_between_radixes() {
    // LO; MS: BASE(7,2) "111", BASE(100,16) "64", BASE(15,2,10) "0000001111".
    text("BASE(7,2)", "111");
    text("BASE(100,16)", "64");
    text("BASE(15,2,10)", "0000001111");
    text("BASE(10,36)", "A");
    text("BASE(10.9,2)", "1010"); // truncates
    text("BASE(0,2)", "0");
    text("BASE(255,16,1)", "FF"); // min_length pads, never truncates
    error("BASE(-1,2)", ErrorValue::Num);
    error("BASE(10,1)", ErrorValue::Num);
    error("BASE(10,37)", ErrorValue::Num);
    error("BASE(2^53,2)", ErrorValue::Num);
    error("BASE(1,2,256)", ErrorValue::Num);
    // LO; MS: DECIMAL("FF",16) 255, DECIMAL(111,2) 7, DECIMAL("zap",36) 45745.
    num("DECIMAL(\"FF\",16)", 255.0);
    num("DECIMAL(111,2)", 7.0);
    num("DECIMAL(\"zap\",36)", 45745.0);
    num("DECIMAL(\"ff\",16)", 255.0);
    num("DECIMAL(\"\",2)", 0.0);
    error("DECIMAL(\"12\",2)", ErrorValue::Num);
    error("DECIMAL(\"FF\",37)", ErrorValue::Num);
}

#[test]
fn munit_spills_an_identity_matrix() {
    spills(
        "MUNIT(3)",
        |_, _| {},
        &[&["1", "0", "0"], &["0", "1", "0"], &["0", "0", "1"]],
    );
    num("MUNIT(1)", 1.0);
    // MS: zero or less is #VALUE!.
    error("MUNIT(0)", ErrorValue::Value);
    error("MUNIT(-1)", ErrorValue::Value);
    // LO.
    num("INDEX(MUNIT(3),3,3)", 1.0);
    num("ROWS(MUNIT(3))", 3.0);
    // Bounded: a matrix of more cells than any range may hold is refused
    // rather than allocated.
    error("MUNIT(100000)", ErrorValue::Num);
}

/// The real and imaginary parts of a complex-valued formula.
#[track_caller]
fn complex(formula: &str, re: f64, im: f64) {
    num(&format!("IMREAL({formula})"), re);
    num(&format!("IMAGINARY({formula})"), im);
}

#[test]
fn reciprocal_complex_trigonometry() {
    // MS and LO agree on all five, to the digit.
    complex("IMCOT(\"4+3i\")", 0.00490118239430447, -0.999266927805902);
    complex("IMCSC(\"4+3i\")", -0.0754898329158637, 0.0648774713706355);
    complex("IMCSCH(\"4+3i\")", -0.036275889628626, -0.0051744731840194);
    complex("IMSEC(\"4+3i\")", -0.065294027857947, -0.0752249603027732);
    complex(
        "IMSECH(\"4+3i\")",
        -0.0362534969158689,
        -0.00516434460775318,
    );
    // LO: a real argument and a pure imaginary one.
    num("IMREAL(IMSEC(\"1\"))", 1.85081571768093);
    num("IMREAL(IMSECH(\"2j\"))", -2.40299796172238);
    // The suffix survives.
    let (wb, v) = eval("IMSECH(\"2j\")");
    let shown = show(&wb, &v);
    assert!(!shown.contains('i'), "{shown}");
    // A pole is #NUM!, as Excel's IMDIV by zero; text that is not a complex
    // number is #NUM! too.
    error("IMCSC(\"0\")", ErrorValue::Num);
    error("IMCOT(\"0\")", ErrorValue::Num);
    error("IMCSCH(\"0\")", ErrorValue::Num);
    error("IMCOT(\"x\")", ErrorValue::Num);
}

#[test]
fn encodeurl_percent_encodes_utf8() {
    // MS's example, exactly: the dots survive.
    text(
        "ENCODEURL(\"http://contoso.sharepoint.com/Finance/Profit and Loss Statement.xlsx\")",
        "http%3A%2F%2Fcontoso.sharepoint.com%2FFinance%2FProfit%20and%20Loss%20Statement.xlsx",
    );
    // Reserved characters and non-ASCII are encoded as UTF-8 bytes; the
    // unreserved set of RFC 3986 is not.
    text("ENCODEURL(\"a b&c=d/é\")", "a%20b%26c%3Dd%2F%C3%A9");
    text("ENCODEURL(\"A-z_0.9~\")", "A-z_0.9~");
    text("ENCODEURL(12)", "12");
    error("ENCODEURL(1/0)", ErrorValue::Div0);
}

// --- Group 4: the 2022 dynamic-array functions ------------------------------

#[test]
fn vstack_and_hstack_pad_the_narrower_with_na() {
    // LO: 5 rows; the fourth is D1 (10); a 2-wide over a 1-wide pads #N/A.
    num("ROWS(VSTACK(C1:C3,D1:D2))", 5.0);
    num("INDEX(VSTACK(C1:C3,D1:D2),4,1)", 10.0);
    error("INDEX(VSTACK(C1:D2,E1:E2),3,2)", ErrorValue::Na);
    num("COLUMNS(VSTACK(C1:D2,E1:E2))", 2.0);
    error("INDEX(VSTACK(1,C1:D1),1,2)", ErrorValue::Na);
    num("COLUMNS(HSTACK(C1:C3,D1:D2))", 2.0);
    num("INDEX(HSTACK(C1:C3,D1:D2),2,2)", 20.0);
    error("INDEX(HSTACK(C1:C3,D1:D2),3,2)", ErrorValue::Na);
    spills(
        "VSTACK(C1:D1,E1)",
        |_, _| {},
        &[&["1", "10"], &["\"alpha\"", "#N/A"]],
    );
    spills("HSTACK(C1:C2,7)", |_, _| {}, &[&["1", "7"], &["2", "#N/A"]]);
}

#[test]
fn take_and_drop_count_from_either_end() {
    // LO.
    num("INDEX(TAKE(C1:D10,2),2,2)", 20.0);
    num("ROWS(TAKE(C1:D10,2))", 2.0);
    num("INDEX(TAKE(C1:D10,-2),1,1)", 9.0);
    num("INDEX(TAKE(C1:D10,,-1),3,1)", 30.0);
    num("COLUMNS(TAKE(C1:D10,,-1))", 1.0);
    num("ROWS(TAKE(C1:D10,20))", 10.0);
    num("INDEX(TAKE(C1:D10,-3,-1),1,1)", 80.0);
    num("INDEX(DROP(C1:D10,2),1,1)", 3.0);
    num("ROWS(DROP(C1:D10,2))", 8.0);
    num("INDEX(DROP(C1:D10,-2,1),8,1)", 80.0);
    num("INDEX(DROP(C1:D10,0,0),1,1)", 1.0);
    // Nothing left is an empty array: #CALC!. (LibreOffice answers #N/A
    // for the first and, wrongly, a value for the second.)
    error("TAKE(C1:D10,0)", ErrorValue::Calc);
    error("DROP(C1:D10,10)", ErrorValue::Calc);
}

#[test]
fn choosecols_and_chooserows_pick_lines_by_index() {
    // LO.
    text("INDEX(CHOOSECOLS(C1:E3,3,1),1,1)", "alpha");
    text("INDEX(CHOOSECOLS(C1:E3,-1),2,1)", "beta");
    num("INDEX(CHOOSEROWS(C1:D10,10,-1),2,2)", 100.0);
    num("INDEX(CHOOSEROWS(C1:D10,SEQUENCE(2,1,2)),2,1)", 3.0);
    spills(
        "CHOOSEROWS(C1:D10,2,2)",
        |_, _| {},
        &[&["2", "20"], &["2", "20"]],
    );
    // MS: an index of zero or past the array is #VALUE!.
    error("CHOOSECOLS(C1:E3,4)", ErrorValue::Value);
    error("CHOOSECOLS(C1:E3,0)", ErrorValue::Value);
    error("CHOOSEROWS(C1:D10,11)", ErrorValue::Value);
}

#[test]
fn tocol_and_torow_flatten_and_filter() {
    // LO.
    num("ROWS(TOCOL(C1:D3))", 6.0);
    num("INDEX(TOCOL(C1:D3),2,1)", 10.0); // by row: 1, 10, 2, …
    num("INDEX(TOCOL(C1:D3,0,TRUE),2,1)", 2.0); // by column: 1, 2, 3, …
    num("ROWS(TOCOL(H1:H5,1))", 4.0); // the blank H4 left out
    num("ROWS(TOCOL(H1:H5))", 5.0);
    num("COLUMNS(TOROW(C1:D3))", 6.0);
    num("INDEX(TOROW(C1:D3),1,2)", 10.0);
    num("INDEX(TOROW(C1:D3,,TRUE),1,2)", 2.0);
    // Errors skipped with 2.
    num("ROWS(TOCOL(C1:C3/(C1:C3>1),2))", 2.0);
    error("TOCOL(C1:C3,4)", ErrorValue::Value);
}

#[test]
fn wraprows_and_wrapcols_fold_a_vector() {
    // LO.
    num("INDEX(WRAPROWS(C1:C10,4),3,2)", 10.0);
    error("INDEX(WRAPROWS(C1:C10,4),3,3)", ErrorValue::Na);
    num("INDEX(WRAPROWS(C1:C10,4,0),3,3)", 0.0);
    num("ROWS(WRAPROWS(C1:C10,4))", 3.0);
    num("INDEX(WRAPCOLS(C1:C10,4),2,3)", 10.0);
    error("INDEX(WRAPCOLS(C1:C10,4),4,3)", ErrorValue::Na);
    spills(
        "WRAPCOLS(C1:C5,2)",
        |_, _| {},
        &[&["1", "3", "5"], &["2", "4", "#N/A"]],
    );
    // MS: a block is #VALUE!; a wrap count below 1 is #NUM!.
    error("WRAPROWS(C1:D2,2)", ErrorValue::Value);
    error("WRAPROWS(C1:C10,0)", ErrorValue::Num);
}

#[test]
fn expand_pads_to_a_size() {
    // LO.
    num("ROWS(EXPAND(C1:D2,3,3))", 3.0);
    error("INDEX(EXPAND(C1:D2,3,3),3,3)", ErrorValue::Na);
    text("INDEX(EXPAND(C1:D2,3,3,\"-\"),3,3)", "-");
    error("INDEX(EXPAND(C1:D2,3),3,1)", ErrorValue::Na);
    num("INDEX(EXPAND(C1:D2,,4,0),1,4)", 0.0);
    // MS: smaller than the array is #VALUE!.
    error("EXPAND(C1:D2,1)", ErrorValue::Value);
    // Bounded like every array result.
    refused_quickly("EXPAND(C1,1E7,1E7)", ErrorValue::Num);
}

#[test]
fn randarray_draws_within_its_bounds() {
    num("ROWS(RANDARRAY(3,2))", 3.0);
    num("COLUMNS(RANDARRAY(3,2))", 2.0);
    num("RANDARRAY(1,1,3,3,TRUE)", 3.0); // LO
    boolean(
        "AND(RANDARRAY(5,5,1,2,TRUE)>=1,RANDARRAY(5,5,1,2,TRUE)<=2)",
        true,
    );
    boolean("SUM(RANDARRAY(4,4,10,20))>=160", true);
    let (_, v) = eval("RANDARRAY()");
    match v {
        CellValue::Number(n) => assert!((0.0..1.0).contains(&n), "{n}"),
        other => panic!("RANDARRAY(): {other:?}"),
    }
    // MS: min above max is #VALUE!; an empty array is #CALC!.
    error("RANDARRAY(2,2,5,1)", ErrorValue::Value);
    error("RANDARRAY(0)", ErrorValue::Calc);
    refused_quickly("RANDARRAY(1E6,1E6)", ErrorValue::Num);
}

/// RANDARRAY is volatile: a recalculation must redraw it even when nothing it
/// reads has changed, exactly as RAND.
#[test]
fn randarray_is_volatile() {
    let (mut wb, mut sheet) = fixture();
    put_formula(&mut wb, &mut sheet, (0, 0), "RANDARRAY()");
    wb.sheets.push(sheet);
    wb.volatile_seed = 1;
    recalculate(&mut wb);
    let first = cell(&wb, 0, 0);
    wb.volatile_seed = 2;
    crate::recalculate_incremental(&mut wb, &[(0, CellRef::new(0, 7))]);
    assert_ne!(
        cell(&wb, 0, 0),
        first,
        "an unrelated edit did not redraw it"
    );
}

#[test]
fn textsplit_cuts_rows_and_columns() {
    // LO.
    text("INDEX(TEXTSPLIT(\"a,b,c\",\",\"),1,2)", "b");
    num("COLUMNS(TEXTSPLIT(\"a,b,c\",\",\"))", 3.0);
    text("INDEX(TEXTSPLIT(\"a,b;c,d\",\",\",\";\"),2,2)", "d");
    error(
        "INDEX(TEXTSPLIT(\"a,b;c\",\",\",\";\"),2,2)",
        ErrorValue::Na,
    );
    text("INDEX(TEXTSPLIT(\"a,b;c\",\",\",\";\",,,\"x\"),2,2)", "x");
    num("COLUMNS(TEXTSPLIT(\"a,,b\",\",\"))", 3.0);
    num("COLUMNS(TEXTSPLIT(\"a,,b\",\",\",,TRUE))", 2.0);
    text("INDEX(TEXTSPLIT(\"aXbxc\",\"x\",,,1),1,2)", "b");
    num("COLUMNS(TEXTSPLIT(\"aXbxc\",\"x\"))", 2.0);
    text("TEXTSPLIT(\"abc\",\",\")", "abc");
    // MS: "Dakota Lennon Sanchez" split on a space.
    spills(
        "TEXTSPLIT(\"Dakota Lennon Sanchez\",\" \")",
        |_, _| {},
        &[&["\"Dakota\"", "\"Lennon\"", "\"Sanchez\""]],
    );
    // Several delimiters at once, from a range.
    let two = |wb: &mut Workbook, sheet: &mut Sheet| {
        put_text(wb, sheet, (0, 0), "-");
        put_text(wb, sheet, (1, 0), " ");
    };
    let (wb, _) = eval_in("TEXTSPLIT(\"a-b c\",A1:A2)", two);
    let row: Vec<String> = (0..3)
        .map(|c| show(&wb, &cell(&wb, AT.0, AT.1 + c)))
        .collect();
    assert_eq!(row, ["\"a\"", "\"b\"", "\"c\""]);
}

#[test]
fn textbefore_and_textafter() {
    let red = "\"Red riding hood's, red hood\"";
    // MS and LO.
    text(&format!("TEXTBEFORE({red},\" hood\")"), "Red riding");
    text(&format!("TEXTAFTER({red},\"hood\")"), "'s, red hood");
    // LO.
    text(
        &format!("TEXTBEFORE({red},\"red\",-1)"),
        "Red riding hood's, ",
    );
    text(&format!("TEXTBEFORE({red},\"RED\",1,1)"), "");
    text(&format!("TEXTAFTER({red},\"hood\",-1)"), "");
    error(&format!("TEXTBEFORE({red},\"red\",2)"), ErrorValue::Na);
    error("TEXTBEFORE(\"abc\",\"x\")", ErrorValue::Na);
    text("TEXTBEFORE(\"abc\",\"x\",,,,\"none\")", "none");
    text("TEXTBEFORE(\"abc\",\"x\",,,1)", "abc");
    text("TEXTAFTER(\"abc\",\"x\",,,1)", "abc");
    error("TEXTBEFORE(\"a-b-c\",\"-\",3)", ErrorValue::Na);
    text("TEXTBEFORE(\"a-b-c\",\"-\",3,,1)", "a-b-c");
    text("TEXTBEFORE(\"a-b-c\",\"-\",-3,,1)", "a");
    text("TEXTAFTER(\"a-b-c\",\"-\",3,,1)", "c");
    text("TEXTAFTER(\"a-b-c\",\"-\",-3,,1)", "a-b-c");
    text("TEXTAFTER(\"alpha-beta\",\"a\",2)", "-beta");
    // MS: an empty delimiter matches at once — TEXTBEFORE gives "" forwards
    // and the whole text backwards, TEXTAFTER the reverse. (LibreOffice: #N/A.)
    text(&format!("TEXTBEFORE({red},\"\")"), "");
    text(
        &format!("TEXTBEFORE({red},\"\",-1)"),
        "Red riding hood's, red hood",
    );
    text("TEXTAFTER(\"abc\",\"\")", "abc");
    text("TEXTAFTER(\"abc\",\"\",-1)", "");
    // MS: instance 0, or past the text's length, is #VALUE!.
    error("TEXTBEFORE(\"abc\",\"b\",0)", ErrorValue::Value);
    error("TEXTAFTER(\"abc\",\"b\",4)", ErrorValue::Value);
}

#[test]
fn arraytotext_and_valuetotext() {
    // MS's example block: TRUE, #VALUE!, 1234.01234, Seattle, Hello, "1,123".
    let data = |wb: &mut Workbook, sheet: &mut Sheet| {
        sheet
            .cells
            .set(CellRef::new(0, 0), Cell::value(CellValue::Bool(true)));
        sheet.cells.set(
            CellRef::new(0, 1),
            Cell::value(CellValue::Error(ErrorValue::Value)),
        );
        put_number(sheet, (1, 0), 1234.01234);
        put_text(wb, sheet, (1, 1), "Seattle");
        put_text(wb, sheet, (2, 0), "Hello");
        put_text(wb, sheet, (2, 1), "1,123");
    };
    let read = |f: &str| {
        let (wb, v) = eval_in(f, data);
        show(&wb, &v)
    };
    assert_eq!(
        read("ARRAYTOTEXT(A1:B3)"),
        "\"TRUE, #VALUE!, 1234.01234, Seattle, Hello, 1,123\""
    );
    assert_eq!(
        read("ARRAYTOTEXT(A1:B3,1)"),
        "\"{TRUE,#VALUE!;1234.01234,\\\"Seattle\\\";\\\"Hello\\\",\\\"1,123\\\"}\""
    );
    assert_eq!(read("ARRAYTOTEXT(A1:B3,2)"), "#VALUE!");
    // MS: VALUETOTEXT quotes text only when strict, and writes an error as
    // its text.
    assert_eq!(read("VALUETOTEXT(A3)"), "\"Hello\"");
    assert_eq!(read("VALUETOTEXT(A3,1)"), "\"\\\"Hello\\\"\"");
    assert_eq!(read("VALUETOTEXT(A2,1)"), "\"1234.01234\"");
    assert_eq!(read("VALUETOTEXT(A1,1)"), "\"TRUE\"");
    assert_eq!(read("VALUETOTEXT(B1)"), "\"#VALUE!\"");
    assert_eq!(read("VALUETOTEXT(1,2)"), "#VALUE!");
}

#[test]
fn trimrange_drops_blank_edges() {
    // C1:D10 holds data, rows 11–20 of it are blank.
    num("ROWS(TRIMRANGE(C1:D20))", 10.0);
    num("COLUMNS(TRIMRANGE(B1:E20))", 3.0); // C, D and E hold data; B none
    num("COLUMNS(TRIMRANGE(A1:E20))", 3.0);
    num("COLUMNS(TRIMRANGE(A1:E20,3,2))", 5.0); // leading columns kept
    num("ROWS(TRIMRANGE(C1:D20,0))", 20.0);
    num("SUM(TRIMRANGE(C1:C1000))", 55.0);
    error("TRIMRANGE(Z1:Z9)", ErrorValue::Calc);
    error("TRIMRANGE(C1:D2,4)", ErrorValue::Value);
}

// --- XL-FEAT-131: scalar functions lift over arrays --------------------------

/// Every function the engine lifts element-wise, each with a call shape and
/// the fixture column it is lifted over (`#` is the lifted argument).
///
/// Microsoft: when a function that expects a single value is handed an array
/// or a range, it is evaluated for each value and returns an array (dynamic
/// arrays and spilled array behaviour). Every entry is a function whose
/// documented parameter is a single value, for which Excel does that.
pub(crate) const LIFTED: &[(&str, char)] = &[
    // Maths, over G (2.5, -3.5, 0, 1e10 — signs, zero and a large value,
    // which is where the domain errors are).
    ("ABS(#)", 'G'),
    ("SIGN(#)", 'G'),
    ("INT(#)", 'G'),
    ("TRUNC(#,1)", 'G'),
    ("ROUND(#,1)", 'G'),
    ("ROUNDUP(#,0)", 'G'),
    ("ROUNDDOWN(#,0)", 'G'),
    ("MOD(#,4)", 'G'),
    ("POWER(#,2)", 'G'),
    ("SQRT(#)", 'G'),
    ("EXP(#)", 'G'),
    ("SIN(#)", 'G'),
    ("COS(#)", 'G'),
    ("TAN(#)", 'G'),
    ("ASIN(#)", 'G'),
    ("ACOS(#)", 'G'),
    ("ATAN(#)", 'G'),
    ("SINH(#)", 'G'),
    ("COSH(#)", 'G'),
    ("TANH(#)", 'G'),
    ("ASINH(#)", 'G'),
    ("ACOSH(#)", 'G'),
    ("ATANH(#)", 'G'),
    ("ATAN2(#,1)", 'G'),
    ("ACOT(#)", 'G'),
    ("ACOTH(#)", 'G'),
    ("COT(#)", 'G'),
    ("COTH(#)", 'G'),
    ("CSC(#)", 'G'),
    ("CSCH(#)", 'G'),
    ("SEC(#)", 'G'),
    ("SECH(#)", 'G'),
    ("DEGREES(#)", 'G'),
    ("RADIANS(#)", 'G'),
    ("LN(#)", 'G'),
    ("LOG(#,10)", 'G'),
    ("LOG10(#)", 'G'),
    ("SQRTPI(#)", 'G'),
    ("EVEN(#)", 'G'),
    ("ODD(#)", 'G'),
    ("CEILING(#,1)", 'G'),
    ("FLOOR(#,1)", 'G'),
    ("MROUND(#,2)", 'G'),
    ("QUOTIENT(#,2)", 'G'),
    ("FACT(#)", 'C'),
    ("FACTDOUBLE(#)", 'C'),
    ("COMBIN(10,#)", 'C'),
    ("COMBINA(#,2)", 'C'),
    ("PERMUT(10,#)", 'C'),
    ("CEILING.MATH(#)", 'G'),
    ("FLOOR.MATH(#)", 'G'),
    ("CEILING.PRECISE(#)", 'G'),
    ("FLOOR.PRECISE(#)", 'G'),
    ("ISO.CEILING(#)", 'G'),
    ("BASE(#,2)", 'D'),
    ("DECIMAL(#,10)", 'H'),
    ("ROMAN(#)", 'D'),
    // Text, over E (words) or H (numeric-looking text and a blank).
    ("LEN(#)", 'E'),
    ("UPPER(#)", 'E'),
    ("LOWER(#)", 'E'),
    ("PROPER(#)", 'E'),
    ("TRIM(#)", 'E'),
    ("CLEAN(#)", 'E'),
    ("LEFT(#,2)", 'E'),
    ("RIGHT(#,2)", 'E'),
    ("MID(#,2,2)", 'E'),
    ("REPT(#,2)", 'E'),
    ("SUBSTITUTE(#,\"a\",\"x\")", 'E'),
    ("REPLACE(#,1,1,\"Z\")", 'E'),
    ("FIND(\"a\",#)", 'E'),
    ("SEARCH(\"A\",#)", 'E'),
    ("EXACT(#,\"beta\")", 'E'),
    ("CODE(#)", 'E'),
    ("UNICODE(#)", 'E'),
    ("CHAR(#+64)", 'C'),
    ("UNICHAR(#+64)", 'C'),
    ("CONCATENATE(#,\"!\")", 'E'),
    ("VALUE(#)", 'H'),
    ("NUMBERVALUE(#)", 'H'),
    ("TEXT(#,\"0.0\")", 'G'),
    ("FIXED(#,1)", 'G'),
    // Dates and times, over C (1..4) — and over computed arrays, not only
    // ranges: `#*1000` is an array before the function sees it.
    ("DATE(2024,#,1)", 'C'),
    ("TIME(#,30,0)", 'C'),
    ("YEAR(#*1000)", 'C'),
    ("MONTH(#*100)", 'C'),
    ("DAY(#*10)", 'C'),
    ("HOUR(#/7)", 'C'),
    ("MINUTE(#/7)", 'C'),
    ("SECOND(#/7)", 'C'),
    ("WEEKDAY(#)", 'C'),
    ("WEEKNUM(#*30)", 'C'),
    ("ISOWEEKNUM(#*30)", 'C'),
    ("EDATE(45000,#)", 'C'),
    ("EOMONTH(45000,#)", 'C'),
    ("DAYS(45000,#)", 'C'),
    // Information and logic, over H (text, numbers-as-text, a blank).
    ("ISNUMBER(#)", 'G'),
    ("ISTEXT(#)", 'H'),
    ("ISBLANK(#)", 'H'),
    ("ISERROR(VALUE(#))", 'H'),
    ("ISERR(VALUE(#))", 'H'),
    ("ISNA(#)", 'H'),
    ("ISLOGICAL(#)", 'F'),
    ("ISNONTEXT(#)", 'H'),
    ("ISEVEN(#)", 'C'),
    ("ISODD(#)", 'C'),
    ("NOT(#)", 'F'),
    // The criteria lifts; the range does not. `COUNTIF(r, r)` is how a
    // spreadsheet counts each value's occurrences.
    ("COUNTIF(C1:C10,\">\"&#)", 'C'),
    ("SUMIF(C1:C10,\"<\"&#)", 'C'),
    ("AVERAGEIF(C1:C10,\"<=\"&#)", 'C'),
];

/// Functions that must never lift, whichever way the table grows: each reads
/// a range whole, inspects the reference rather than its value, is lazy in an
/// argument, or shapes an array.
const NEVER_LIFTED: &[&str] = &[
    "SUM",
    "SUMPRODUCT",
    "PRODUCT",
    "AVERAGE",
    "MIN",
    "MAX",
    "COUNT",
    "COUNTA",
    "INDEX",
    "MATCH",
    "XMATCH",
    "VLOOKUP",
    "HLOOKUP",
    "XLOOKUP",
    "LOOKUP",
    "ROWS",
    "COLUMNS",
    "ROW",
    "COLUMN",
    "ISREF",
    "ISFORMULA",
    "FORMULATEXT",
    "CELL",
    "OFFSET",
    "INDIRECT",
    "IF",
    "IFS",
    "IFERROR",
    "IFNA",
    "CHOOSE",
    "SWITCH",
    "AND",
    "OR",
    "XOR",
    "CONCAT",
    "TEXTJOIN",
    "N",
    "T",
    "SUBTOTAL",
    "AGGREGATE",
    "COUNTIFS",
    "SUMIFS",
    "TRANSPOSE",
    "SEQUENCE",
    "FILTER",
    "SORT",
    "UNIQUE",
    "RAND",
    "RANDBETWEEN",
];

#[test]
fn the_lift_table_is_what_the_tests_say() {
    let name = |template: &str| template[..template.find('(').unwrap()].to_owned();
    let tested: std::collections::BTreeSet<String> = LIFTED.iter().map(|(t, _)| name(t)).collect();
    let table: Vec<&str> = crate::functions::lifted_functions().collect();
    // Sorted, because it is binary-searched.
    let mut sorted = table.clone();
    sorted.sort_unstable();
    assert_eq!(table, sorted, "the lift table must stay alphabetical");
    let table: std::collections::BTreeSet<String> = table.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(
        table, tested,
        "every lifted function is exercised by the property test, and only those"
    );
    for f in &table {
        assert!(crate::functions::is_builtin(f), "{f} is not a builtin");
    }
    for f in NEVER_LIFTED {
        assert!(!table.contains(*f), "{f} must not lift");
    }
}

#[test]
fn lifting_composes() {
    // Nested lifts, a LET-bound array, a single-cell range.
    spills(
        "ROUND(SQRT(C1:C3),2)",
        |_, _| {},
        &[&["1"], &["1.41"], &["1.73"]],
    );
    num("LET(x,C1:C3,SUMPRODUCT(MOD(x,2)))", 2.0);
    num("ABS(G2:G2)", 3.5);
    // Each value's count of occurrences, the unique-count idiom.
    num("SUMPRODUCT(1/COUNTIF(C1:C10,C1:C10))", 10.0);
    // A scalar argument that is not a literal still evaluates once and is
    // passed through: no array anywhere means no array out.
    num("ROUND(SUM(C1:C3)/7,2)", 0.86);
    // An error argument broadcasts like any value.
    spills("MOD(C1:C2,1/0)", |_, _| {}, &[&["#DIV/0!"], &["#DIV/0!"]]);
}

/// Functions that take a range or an array **as such** — reducing it,
/// searching it or measuring it — and therefore must not be lifted: `SUM`
/// over a column is one total, not ten. Each is checked to still answer its
/// one value.
const NOT_LIFTED: &[(&str, f64)] = &[
    ("SUM(C1:C4)", 10.0),
    ("SUMPRODUCT(C1:C4,D1:D4)", 300.0),
    ("INDEX(C1:C10,3)", 3.0),
    ("MATCH(3,C1:C10,0)", 3.0),
    ("COUNTIF(C1:C10,\">5\")", 5.0),
    ("SUMIF(C1:C10,\">5\")", 40.0),
    ("ROWS(C1:C4)", 4.0),
    ("COLUMNS(C1:D4)", 2.0),
    ("AVERAGE(C1:C4)", 2.5),
    ("MAX(C1:C4)", 4.0),
    ("COUNTA(E1:E5)", 5.0),
    ("LEN(CONCAT(E1:E2))", 9.0),
    ("LEN(TEXTJOIN(\",\",TRUE,E1:E2))", 10.0),
    ("VLOOKUP(3,C1:D10,2,FALSE)", 30.0),
];

#[test]
fn scalar_functions_lift_element_wise_over_a_range() {
    // The property, for every lifted function: element k of F(range) is
    // F(the k-th cell). Expected values are the function's own answer on a
    // single cell — the scalar path, which the rest of this file pins to
    // Microsoft and LibreOffice — so this is a test of lifting, not of F.
    let mut failures = Vec::new();
    for (template, col) in LIFTED {
        let lifted = template.replace('#', &format!("{col}1:{col}4"));
        let (wb, rows) = eval(&format!("ROWS({lifted})"));
        if rows != CellValue::Number(4.0) {
            failures.push(format!("ROWS({lifted}) = {}", show(&wb, &rows)));
            continue;
        }
        for k in 1..=4u32 {
            let (wb_l, got) = eval(&format!("INDEX({lifted},{k})"));
            let single = template.replace('#', &format!("{col}{k}"));
            let (wb_s, want) = eval(&single);
            let (got, want) = (show(&wb_l, &got), show(&wb_s, &want));
            if got != want {
                failures.push(format!("INDEX({lifted},{k}) = {got}, {single} = {want}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} lifting failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn range_taking_functions_are_not_lifted() {
    for (formula, expected) in NOT_LIFTED {
        num(formula, *expected);
    }
}

#[test]
fn a_lifted_function_spills_from_a_cell() {
    // MS: `=MOD(C1:C10,4)` in one cell spills ten results.
    spills(
        "MOD(C1:C10,4)",
        |_, _| {},
        &[
            &["1"],
            &["2"],
            &["3"],
            &["0"],
            &["1"],
            &["2"],
            &["3"],
            &["0"],
            &["1"],
            &["2"],
        ],
    );
    spills(
        "UPPER(E1:E3)",
        |_, _| {},
        &[&["\"ALPHA\""], &["\"BETA\""], &["\"GAMMA\""]],
    );
    spills("ROUND(G1:G2,0)", |_, _| {}, &[&["3"], &["-4"]]);
    // CONCATENATE is the scalar one, so it lifts; CONCAT takes ranges and
    // joins them.
    spills(
        "CONCATENATE(E1:E2,\"!\")",
        |_, _| {},
        &[&["\"alpha!\""], &["\"beta!\""]],
    );
}

#[test]
fn lifted_arguments_broadcast_as_excel_does() {
    // A column against a row is the outer product: 3×1 with 1×2 is 3×2.
    spills(
        "MOD(C3:C5,TRANSPOSE(C2:C3))",
        |_, _| {},
        &[&["1", "0"], &["0", "1"], &["1", "2"]],
    );
    // A single value repeats against every element.
    spills(
        "DATE(2024,C1:C3,1)",
        |_, _| {},
        &[&["45292"], &["45323"], &["45352"]],
    );
    // Two columns of different lengths pair positionally, and where the
    // shorter has run out the element is #N/A.
    spills(
        "POWER(C1:C3,C1:C2)",
        |_, _| {},
        &[&["1"], &["4"], &["#N/A"]],
    );
    // 2×2 against 3×1: the 3×1 repeats across, and the row the 2×2 lacks is
    // #N/A.
    spills(
        "POWER(C1:D2,C1:C3)",
        |_, _| {},
        &[&["1", "10"], &["4", "400"], &["#N/A", "#N/A"]],
    );
    // Errors stay in their element.
    spills(
        "SQRT(G1:G3)",
        |_, _| {},
        &[&["1.5811388300841898"], &["#NUM!"], &["0"]],
    );
    // And lifted results feed array-consuming functions.
    num("SUMPRODUCT(MOD(C1:C10,4))", 15.0);
    num("SUM(LEN(E1:E5))", 26.0);
    num("SUMPRODUCT(--ISNUMBER(SEARCH(\"a\",E1:E5)))", 4.0);
}

#[test]
fn binary_operators_broadcast_by_the_same_rule() {
    // One broadcast rule for operators, IF and lifted functions: the outer
    // product, and #N/A where a shorter array runs out.
    spills(
        "C1:C2+TRANSPOSE(D1:D3)",
        |_, _| {},
        &[&["11", "21", "31"], &["12", "22", "32"]],
    );
    spills("C1:C3*C1:C2", |_, _| {}, &[&["1"], &["4"], &["#N/A"]]);
    spills("IF(F1:F3,C1:C2,0)", |_, _| {}, &[&["1"], &["0"], &["#N/A"]]);
}

#[test]
fn a_lifted_result_is_bounded_like_every_array() {
    // 2,000 × 2,000 is four million elements, past the engine's two-million
    // budget: #NUM!, not an allocation the size of the request.
    error(
        "ROWS(MOD(SEQUENCE(2000),SEQUENCE(1,2000)))",
        ErrorValue::Num,
    );
    error("ROWS(SEQUENCE(2000)+SEQUENCE(1,2000))", ErrorValue::Num);
    // ...while one inside the budget is fine.
    num("ROWS(MOD(SEQUENCE(1000),SEQUENCE(1,1000)))", 1000.0);
}

#[test]
fn and_or_skip_text_and_blanks_inside_a_reference() {
    // MS (AND/OR): "If an array or reference argument contains text or empty
    // cells, those values are ignored." E1 is text, F1 TRUE.
    boolean("AND(E1:F1)", true);
    boolean("OR(E1:E5,F2)", false);
    boolean("AND(C1:C3,E1:E5)", true);
    boolean("OR(E1:E5,F1:F5)", true);
    boolean("AND(H1:H5,TRUE)", true); // H is all text and a blank
    // A range holding no logical values at all is #VALUE!.
    error("AND(E1:E5)", ErrorValue::Value);
    error("OR(H1:H5)", ErrorValue::Value);
    // Text typed as an argument is not skipped: it must read as a logical.
    error("AND(\"abc\",TRUE)", ErrorValue::Value);
    error("OR(\"abc\",TRUE)", ErrorValue::Value);
    boolean("AND(\"TRUE\",TRUE)", true);
    // A computed array is read whole — OR(C1:C10>5) is not just C1>5.
    boolean("OR(C1:C10>5)", true);
    boolean("AND(C1:C10>5)", false);
    boolean("AND(C1:C10>0)", true);
    // Errors propagate.
    error("AND(TRUE,1/0)", ErrorValue::Div0);
}
