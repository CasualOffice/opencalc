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

#[allow(dead_code)] // used by the later groups
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

#[allow(dead_code)] // used by the later groups
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
