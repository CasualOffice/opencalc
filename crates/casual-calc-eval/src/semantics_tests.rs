//! How numbers compare and how text becomes a number — the two places where a
//! formula that is right in Excel quietly gives a different answer here.
//!
//! `XL-FEAT-011`: binary floating point makes `0.1+0.2` one unit in the last
//! place away from `0.3`. Excel answers `=0.1+0.2=0.3` with TRUE, and a money
//! model's `IF(total=budget, …)` and `COUNTIF(…, 0.3)` depend on it.
//!
//! `XL-FEAT-019`: typed entry reads `1,234`, `$5.00`, `50%` and `12/31/2026`
//! (`XL-FEAT-001`); `VALUE`, `DATEVALUE`, `TIMEVALUE` and the implicit coercion
//! in `="1,234"*1` have to read them the same way, through the same parser.

use casual_calc_formula::parse;
use casual_calc_formula::stored::Origin;
use casual_calc_model::{Cell, CellRef, CellValue, ErrorValue, Id, Sheet, SheetId, Workbook};

use crate::recalculate;

/// A one-sheet workbook: `cells` are formulas (written without `=`) at
/// `(row, 0)`, evaluated together so one formula can read another.
fn sheet_of(formulas: &[&str]) -> Workbook {
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "Sheet1");
    for (row, text) in formulas.iter().enumerate() {
        let row = row as u32;
        let expr = parse(text).unwrap_or_else(|e| panic!("parse {text:?}: {e}"));
        let handle = wb.store_formula_at(expr, Origin::at(row, 0));
        let mut cell = Cell::value(CellValue::Empty);
        cell.formula = Some(handle);
        sheet.cells.set(CellRef::new(row, 0), cell);
    }
    wb.sheets.push(sheet);
    recalculate(&mut wb);
    wb
}

fn at(wb: &Workbook, row: u32) -> CellValue {
    wb.sheets[0]
        .cells
        .get(CellRef::new(row, 0))
        .map(|c| c.value.clone())
        .unwrap_or(CellValue::Empty)
}

/// The value of one formula on its own.
fn eval(formula: &str) -> CellValue {
    at(&sheet_of(&[formula]), 0)
}

fn text_at(wb: &Workbook, row: u32) -> String {
    match at(wb, row) {
        CellValue::InlineString(id) | CellValue::SharedString(id) => {
            wb.strings.get(id).unwrap_or_default().to_owned()
        }
        other => panic!("row {row} gave {other:?}, expected text"),
    }
}

fn num(formula: &str) -> f64 {
    match eval(formula) {
        CellValue::Number(n) => n,
        other => panic!("={formula} gave {other:?}, expected a number"),
    }
}

fn truth(formula: &str) -> bool {
    match eval(formula) {
        CellValue::Bool(b) => b,
        other => panic!("={formula} gave {other:?}, expected TRUE/FALSE"),
    }
}

// ------------------------------------------------------------ XL-FEAT-011

/// The headline case, every comparison operator, and both orders.
#[test]
fn a_sum_one_ulp_from_its_literal_compares_equal_to_it() {
    assert!(truth("0.1+0.2=0.3"), "=0.1+0.2=0.3");
    assert!(truth("0.3=0.1+0.2"), "=0.3=0.1+0.2");
    assert!(!truth("0.1+0.2<>0.3"), "=0.1+0.2<>0.3");
    assert!(!truth("0.1+0.2>0.3"), "equal is not greater");
    assert!(truth("0.1+0.2>=0.3"));
    assert!(truth("0.1+0.2<=0.3"));
    assert!(!truth("0.1+0.2<0.3"));
    // Through a cell, which is how a model actually meets it.
    let wb = sheet_of(&["0.1+0.2", "A1=0.3", "IF(A1=0.3,\"on budget\",\"off\")"]);
    assert_eq!(at(&wb, 1), CellValue::Bool(true));
    assert_eq!(text_at(&wb, 2), "on budget");
}

/// The tolerance is the last few bits, not a loose epsilon: numbers a user
/// could tell apart stay apart.
#[test]
fn genuinely_different_numbers_still_compare_unequal() {
    assert!(
        !truth("1=1+1E-10"),
        "1 and 1+1e-10 differ in the 11th digit"
    );
    assert!(truth("1<1+1E-10"));
    assert!(
        !truth("0.123456789012345=0.123456789012346"),
        "15th digit differs"
    );
    assert!(
        !truth("1E-20=2E-20"),
        "tiny numbers compare by ratio, not by distance"
    );
    assert!(!truth("0=1E-300"), "zero is only equal to zero");
}

/// Whole numbers compare exactly, at every magnitude a double holds one: the
/// relative tolerance would otherwise call two adjacent 15-digit account
/// numbers equal.
#[test]
fn large_integers_compare_exactly() {
    assert!(!truth("999999999999999=999999999999998"));
    assert!(truth("999999999999999>999999999999998"));
    assert!(!truth("2^52+1=2^52"));
    assert!(truth("123456789012345=123456789012345"));
}

/// Excel 97 and later: an addition or subtraction whose result is at or very
/// near zero is compensated to exactly zero ("Floating-point arithmetic may
/// give inaccurate results in Excel", Microsoft). Only when it is the
/// formula's final operation: `=1*(0.5-0.4-0.1)` keeps the residue.
#[test]
fn a_final_sum_that_cancels_to_near_zero_is_zero() {
    assert_eq!(
        num("1.333+1.225-1.333-1.225"),
        0.0,
        "Microsoft's own example"
    );
    assert_eq!(num("0.5-0.4-0.1"), 0.0);
    assert_eq!(num("0.1+0.2-0.3"), 0.0);
    // Not the final operation: the residue survives, as it does in Excel.
    assert!(
        num("1*(0.5-0.4-0.1)") != 0.0,
        "=1*(0.5-0.4-0.1) is -2.78E-17 in Excel"
    );
    // Microsoft's other example: not near zero, so not touched.
    let n = num("(43.1-43.2)+1");
    assert!(n != 0.9 && (n - 0.9).abs() < 1e-14, "=(43.1-43.2)+1 is {n}");
    // A real difference is kept.
    assert_eq!(num("1-0.999"), 1.0 - 0.999);
}

/// The criteria and lookup functions compare the way `=` does — a COUNTIF of
/// `0.3` over a column of `0.1+0.2` that counted nothing is the money-model
/// bug the row exists for.
#[test]
fn criteria_and_lookups_use_the_same_equality() {
    let wb = sheet_of(&[
        "0.1+0.2",                         // A1
        "COUNTIF(A1,0.3)",                 // A2
        "COUNTIF(A1,\"=0.3\")",            // A3
        "COUNTIF(A1,\">0.3\")",            // A4
        "SUMIF(A1,\"<>0.3\")",             // A5
        "MATCH(0.3,A1:A1,0)",              // A6
        "VLOOKUP(0.3,A1:A1,1,FALSE)=A1",   // A7
        "XMATCH(0.3,A1:A1)",               // A8
        "SWITCH(A1,0.3,\"hit\",\"miss\")", // A9
        "COUNTIF(A1,\">=0.3\")",           // A10
    ]);
    assert_eq!(at(&wb, 1), CellValue::Number(1.0), "COUNTIF(A1,0.3)");
    assert_eq!(at(&wb, 2), CellValue::Number(1.0), "COUNTIF(A1,\"=0.3\")");
    assert_eq!(at(&wb, 3), CellValue::Number(0.0), "COUNTIF(A1,\">0.3\")");
    assert_eq!(at(&wb, 4), CellValue::Number(0.0), "SUMIF(A1,\"<>0.3\")");
    assert_eq!(at(&wb, 5), CellValue::Number(1.0), "MATCH exact");
    assert_eq!(at(&wb, 6), CellValue::Bool(true), "VLOOKUP exact");
    assert_eq!(at(&wb, 7), CellValue::Number(1.0), "XMATCH exact");
    assert_eq!(text_at(&wb, 8), "hit", "SWITCH");
    assert_eq!(at(&wb, 9), CellValue::Number(1.0), "COUNTIF(A1,\">=0.3\")");
}

// ------------------------------------------------------------ XL-FEAT-019

/// 2026-12-31 as an Excel serial (2026-01-01 is 46023).
const DEC_31_2026: f64 = 46387.0;

#[test]
fn value_reads_what_typed_entry_reads() {
    assert_eq!(num("VALUE(\"1,234\")"), 1234.0);
    assert_eq!(num("VALUE(\"1,234,567.5\")"), 1_234_567.5);
    assert_eq!(num("VALUE(\"$1,234.50\")"), 1234.5);
    assert_eq!(num("VALUE(\"-$5\")"), -5.0);
    assert_eq!(num("VALUE(\"50%\")"), 0.5);
    assert_eq!(num("VALUE(\"(5)\")"), -5.0);
    assert_eq!(num("VALUE(\"1e3\")"), 1000.0);
    assert_eq!(num("VALUE(\" 42 \")"), 42.0);
    assert_eq!(num("VALUE(\"007\")"), 7.0);
    assert_eq!(num("VALUE(\"12/31/2026\")"), DEC_31_2026);
    assert_eq!(num("VALUE(\"2026-12-31\")"), DEC_31_2026);
    assert_eq!(num("VALUE(\"1:30 PM\")"), 13.5 / 24.0);
    assert_eq!(
        num("VALUE(\"12/31/2026 1:30 PM\")"),
        DEC_31_2026 + 13.5 / 24.0
    );
    // What entry refuses, VALUE refuses.
    for text in ["1,23", "abc", "TRUE", "inf", "NaN", "1e400", "$5%", ""] {
        assert_eq!(
            eval(&format!("VALUE(\"{text}\")")),
            CellValue::Error(ErrorValue::Value),
            "VALUE({text:?})"
        );
    }
}

#[test]
fn datevalue_and_timevalue_read_what_typed_entry_reads() {
    assert_eq!(num("DATEVALUE(\"12/31/2026\")"), DEC_31_2026);
    assert_eq!(num("DATEVALUE(\"Dec 31, 2026\")"), DEC_31_2026);
    assert_eq!(num("DATEVALUE(\"31-Dec-2026\")"), DEC_31_2026);
    assert_eq!(num("DATEVALUE(\"2026/12/31\")"), DEC_31_2026);
    // "Any time information in date_text is ignored" (Microsoft, DATEVALUE).
    assert_eq!(num("DATEVALUE(\"12/31/2026 1:30 PM\")"), DEC_31_2026);
    assert_eq!(num("TIMEVALUE(\"1:30 PM\")"), 13.5 / 24.0);
    assert_eq!(num("TIMEVALUE(\"13:30:00\")"), 13.5 / 24.0);
    // "Any date information in time_text is ignored" (Microsoft, TIMEVALUE).
    assert!((num("TIMEVALUE(\"12/31/2026 1:30 PM\")") - 13.5 / 24.0).abs() < 1e-9);
    for bad in [
        "DATEVALUE(\"2/30/2026\")",
        "DATEVALUE(\"5\")",
        "DATEVALUE(\"1,234\")",
        "TIMEVALUE(\"noon\")",
    ] {
        assert_eq!(eval(bad), CellValue::Error(ErrorValue::Value), "{bad}");
    }
}

/// An ISO date need not be zero-padded: Excel en-US reads `2024-5-7` as
/// 7 May 2024. The private DATEVALUE parser this replaced read it, so moving to
/// the shared reader must not lose it.
#[test]
fn datevalue_reads_an_unpadded_iso_date() {
    const MAY_7_2024: f64 = 45419.0;
    assert_eq!(num("DATEVALUE(\"2024-5-7\")"), MAY_7_2024);
    assert_eq!(num("DATEVALUE(\"2024-5-07\")"), MAY_7_2024);
    assert_eq!(num("DATEVALUE(\"2024-05-7\")"), MAY_7_2024);
    assert_eq!(num("VALUE(\"2024-5-7\")"), MAY_7_2024);
    assert_eq!(num("DATEVALUE(\"2024-5-7 13:30\")"), MAY_7_2024);
    // A padded ISO date with a time the ISO reader does not take — a 12-hour
    // clock, fractional seconds — is still a date and a time.
    assert_eq!(num("DATEVALUE(\"2024-05-07 1:30 PM\")"), MAY_7_2024);
    assert_eq!(num("DATEVALUE(\"2024-05-07 13:30:15.5\")"), MAY_7_2024);
    assert_eq!(num("TIMEVALUE(\"2024-05-07 1:30 PM\")"), 13.5 / 24.0);
    assert_eq!(
        eval("DATEVALUE(\"2024-2-30\")"),
        CellValue::Error(ErrorValue::Value)
    );
}

/// `h:mm:ss.0` — a decimal fraction of a second — is a time to Excel, on
/// entry and in TIMEVALUE.
#[test]
fn timevalue_reads_fractional_seconds() {
    let expected = (13.0 * 3600.0 + 30.0 * 60.0 + 15.5) / 86400.0;
    assert_eq!(num("TIMEVALUE(\"13:30:15.5\")"), expected);
    assert_eq!(num("TIMEVALUE(\"1:30:15.5 PM\")"), expected);
    assert_eq!(num("TIMEVALUE(\"12/31/2026 13:30:15.5\")"), expected);
    for bad in [
        "TIMEVALUE(\"13:30:60.5\")",
        "TIMEVALUE(\"13:30:15.\")",
        "TIMEVALUE(\"13:30.5\")",
    ] {
        assert_eq!(eval(bad), CellValue::Error(ErrorValue::Value), "{bad}");
    }
}

/// TIMEVALUE of date-and-time text is the time read on its own, not the
/// fraction of the summed serial: at a serial near 45 000 the sum has already
/// lost about five digits of the time, so `fract()` of it is wrong from the
/// eleventh digit and even `=` (to 15 digits) tells the two apart. 6:01 is not
/// exact in binary, which is what makes the difference visible.
#[test]
fn timevalue_of_a_date_and_time_is_the_time_alone() {
    let six_01 = num("TIMEVALUE(\"6:01\")");
    assert_eq!(num("TIMEVALUE(\"5/17/2024 6:01\")"), six_01);
    assert_eq!(num("TIMEVALUE(\"2024-05-17 06:01\")"), six_01);
    assert_eq!(num("TIMEVALUE(\"2024-05-17T06:01\")"), six_01);
    assert!(truth("TIMEVALUE(\"5/17/2024 6:01\")=TIMEVALUE(\"6:01\")"));
    assert_eq!(
        num("TIMEVALUE(\"12/31/9999 23:59:59\")"),
        num("TIMEVALUE(\"23:59:59\")")
    );
}

/// The coercion an operator applies to text is the same reading.
#[test]
fn implicit_coercion_reads_what_typed_entry_reads() {
    assert_eq!(num("\"1,234\"*1"), 1234.0);
    assert_eq!(num("\"$5.00\"+0"), 5.0);
    assert_eq!(num("\"50%\"*2"), 1.0);
    assert_eq!(num("\"12/31/2026\"+0"), DEC_31_2026);
    assert_eq!(num("-\"(5)\""), 5.0);
    assert_eq!(num("ABS(\"-1,000\")"), 1000.0);
    assert_eq!(eval("\"TRUE\"*1"), CellValue::Error(ErrorValue::Value));
    assert_eq!(eval("\"inf\"*1"), CellValue::Error(ErrorValue::Value));
    // A criterion is text, and a date in it means the date.
    let wb = sheet_of(&[
        "DATE(2026,12,31)",
        "COUNTIF(A1,\">=12/31/2026\")",
        "COUNTIF(A1,\"1,234\")",
    ]);
    assert_eq!(at(&wb, 1), CellValue::Number(1.0));
    assert_eq!(at(&wb, 2), CellValue::Number(0.0));
}
