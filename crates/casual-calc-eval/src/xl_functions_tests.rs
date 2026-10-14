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
#[allow(dead_code)] // used by the later groups
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
