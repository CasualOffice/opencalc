//! Evaluation tests: build formula cells directly, recalculate, check values.

use casual_calc_formula::parse;
use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

use crate::recalculate;

struct Builder {
    wb: Workbook,
    sheet: Sheet,
}

impl Builder {
    fn new() -> Self {
        Self {
            wb: Workbook::new(Id::from_parts(1, 1)),
            sheet: Sheet::new(SheetId(Id::from_parts(2, 1)), "Sheet1"),
        }
    }

    fn number(&mut self, at: (u32, u32), n: f64) -> &mut Self {
        self.sheet
            .cells
            .set(CellRef::new(at.0, at.1), Cell::value(CellValue::Number(n)));
        self
    }

    fn formula(&mut self, at: (u32, u32), text: &str) -> &mut Self {
        let expr = parse(text).unwrap_or_else(|e| panic!("parse {text:?}: {e}"));
        let handle = self.wb.store_formula(expr);
        let mut cell = Cell::value(CellValue::Empty);
        cell.formula = Some(handle);
        self.sheet.cells.set(CellRef::new(at.0, at.1), cell);
        self
    }

    fn build(mut self) -> Workbook {
        self.wb.sheets.push(self.sheet);
        self.wb
    }
}

fn value_at(wb: &Workbook, row: u32, col: u32) -> CellValue {
    wb.sheets[0]
        .cells
        .get(CellRef::new(row, col))
        .map(|c| c.value.clone())
        .unwrap_or(CellValue::Empty)
}

#[test]
fn arithmetic_and_references() {
    let mut b = Builder::new();
    b.number((0, 0), 10.0) // A1
        .number((1, 0), 5.0) // A2
        .formula((2, 0), "A1+A2") // A3
        .formula((3, 0), "A1*A2") // A4
        .formula((4, 0), "A1/A2") // A5
        .formula((5, 0), "A1-A2*2"); // A6 = 10 - 10 = 0
    let mut wb = b.build();
    recalculate(&mut wb);

    assert_eq!(value_at(&wb, 2, 0), CellValue::Number(15.0));
    assert_eq!(value_at(&wb, 3, 0), CellValue::Number(50.0));
    assert_eq!(value_at(&wb, 4, 0), CellValue::Number(2.0));
    assert_eq!(value_at(&wb, 5, 0), CellValue::Number(0.0));
}

#[test]
fn aggregate_functions_over_ranges() {
    let mut b = Builder::new();
    b.number((0, 1), 1.0) // B1
        .number((1, 1), 2.0) // B2
        .number((2, 1), 3.0) // B3
        .formula((3, 1), "SUM(B1:B3)")
        .formula((4, 1), "AVERAGE(B1:B3)")
        .formula((5, 1), "MAX(B1:B3)")
        .formula((6, 1), "MIN(B1:B3)")
        .formula((7, 1), "COUNT(B1:B3)");
    let mut wb = b.build();
    recalculate(&mut wb);

    assert_eq!(value_at(&wb, 3, 1), CellValue::Number(6.0));
    assert_eq!(value_at(&wb, 4, 1), CellValue::Number(2.0));
    assert_eq!(value_at(&wb, 5, 1), CellValue::Number(3.0));
    assert_eq!(value_at(&wb, 6, 1), CellValue::Number(1.0));
    assert_eq!(value_at(&wb, 7, 1), CellValue::Number(3.0));
}

#[test]
fn if_returns_text_branch() {
    let mut b = Builder::new();
    b.number((0, 0), 10.0)
        .formula((0, 1), "IF(A1>0,\"pos\",\"neg\")");
    let mut wb = b.build();
    recalculate(&mut wb);
    let CellValue::InlineString(id) = value_at(&wb, 0, 1) else {
        panic!("expected text result");
    };
    assert_eq!(wb.strings.get(id), Some("pos"));
}

#[test]
fn division_by_zero_is_an_error() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.number((0, 0), 10.0).formula((1, 0), "A1/0");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 1, 0), CellValue::Error(ErrorValue::Div0));
}

#[test]
fn transitive_chain_evaluates() {
    let mut b = Builder::new();
    b.number((0, 0), 1.0) // A1 = 1
        .formula((1, 0), "A1+1") // A2 = 2
        .formula((2, 0), "A2+1") // A3 = 3
        .formula((3, 0), "A3*10"); // A4 = 30
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 3, 0), CellValue::Number(30.0));
}

#[test]
fn circular_reference_is_an_error() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.formula((0, 0), "A2").formula((1, 0), "A1");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 0, 0), CellValue::Error(ErrorValue::Ref));
}

fn text_at(wb: &Workbook, row: u32, col: u32) -> String {
    match value_at(wb, row, col) {
        CellValue::InlineString(id) | CellValue::SharedString(id) => {
            wb.strings.get(id).unwrap_or_default().to_owned()
        }
        other => panic!("expected text at ({row},{col}), got {other:?}"),
    }
}

fn number_at(wb: &Workbook, row: u32, col: u32) -> f64 {
    match value_at(wb, row, col) {
        CellValue::Number(n) => n,
        other => panic!("expected number at ({row},{col}), got {other:?}"),
    }
}

fn bool_at(wb: &Workbook, row: u32, col: u32) -> bool {
    match value_at(wb, row, col) {
        CellValue::Bool(b) => b,
        other => panic!("expected bool at ({row},{col}), got {other:?}"),
    }
}

#[test]
fn iferror_wraps_div0() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.number((0, 0), 10.0)
        .formula((1, 0), "IFERROR(A1/0,-1)") // caught -> -1
        .formula((2, 0), "IFERROR(A1/2,-1)"); // no error -> 5
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 1, 0), CellValue::Number(-1.0));
    assert_eq!(value_at(&wb, 2, 0), CellValue::Number(5.0));
    // A bare error still propagates without IFERROR.
    let mut b2 = Builder::new();
    b2.formula((0, 0), "1/0");
    let mut wb2 = b2.build();
    recalculate(&mut wb2);
    assert_eq!(value_at(&wb2, 0, 0), CellValue::Error(ErrorValue::Div0));
}

#[test]
fn logical_and_or_not() {
    let mut b = Builder::new();
    b.number((0, 0), 1.0) // A1
        .number((1, 0), 0.0) // A2
        .formula((0, 1), "AND(A1>0,A2>0)") // false
        .formula((1, 1), "OR(A1>0,A2>0)") // true
        .formula((2, 1), "NOT(A2>0)") // true
        .formula((3, 1), "AND(A1>0,A1<10)"); // true
    let mut wb = b.build();
    recalculate(&mut wb);
    assert!(!bool_at(&wb, 0, 1));
    assert!(bool_at(&wb, 1, 1));
    assert!(bool_at(&wb, 2, 1));
    assert!(bool_at(&wb, 3, 1));
    // NOT on non-boolean text is a #VALUE! error.
    let mut b2 = Builder::new();
    b2.formula((0, 0), "NOT(\"x\")");
    let mut wb2 = b2.build();
    recalculate(&mut wb2);
    use casual_calc_model::ErrorValue;
    assert_eq!(value_at(&wb2, 0, 0), CellValue::Error(ErrorValue::Value));
}

#[test]
fn countif_with_comparison() {
    let mut b = Builder::new();
    b.number((0, 0), 1.0) // A1
        .number((1, 0), 5.0) // A2
        .number((2, 0), 8.0) // A3
        .number((3, 0), 5.0) // A4
        .formula((0, 1), "COUNTIF(A1:A4,\">4\")") // 3
        .formula((1, 1), "COUNTIF(A1:A4,5)") // 2
        .formula((2, 1), "COUNTIF(A1:A4,\"<>5\")"); // 2
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(number_at(&wb, 0, 1), 3.0);
    assert_eq!(number_at(&wb, 1, 1), 2.0);
    assert_eq!(number_at(&wb, 2, 1), 2.0);
}

#[test]
fn sumif_and_averageif() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.number((0, 0), 1.0) // A1
        .number((1, 0), 5.0) // A2
        .number((2, 0), 8.0) // A3
        .number((0, 1), 10.0) // B1
        .number((1, 1), 20.0) // B2
        .number((2, 1), 30.0) // B3
        .formula((0, 2), "SUMIF(A1:A3,\">4\")") // 5+8=13
        .formula((1, 2), "SUMIF(A1:A3,\">4\",B1:B3)") // 20+30=50
        .formula((2, 2), "AVERAGEIF(A1:A3,\">4\")") // (5+8)/2=6.5
        .formula((3, 2), "AVERAGEIF(A1:A3,\">100\")"); // no match -> #DIV/0!
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(number_at(&wb, 0, 2), 13.0);
    assert_eq!(number_at(&wb, 1, 2), 50.0);
    assert_eq!(number_at(&wb, 2, 2), 6.5);
    assert_eq!(value_at(&wb, 3, 2), CellValue::Error(ErrorValue::Div0));
}

#[test]
fn counta_counts_nonempty() {
    let mut b = Builder::new();
    b.number((0, 0), 1.0) // A1
        // A2 empty
        .number((2, 0), 3.0) // A3
        .formula((0, 1), "COUNTA(A1:A3)"); // 2
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(number_at(&wb, 0, 1), 2.0);
}

#[test]
fn concat_and_len() {
    let mut b = Builder::new();
    b.formula((0, 0), "CONCATENATE(\"foo\",\"bar\")")
        .formula((1, 0), "CONCAT(\"a\",1,\"b\")")
        .formula((2, 0), "LEN(\"hello\")");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(text_at(&wb, 0, 0), "foobar");
    assert_eq!(text_at(&wb, 1, 0), "a1b");
    assert_eq!(number_at(&wb, 2, 0), 5.0);
}

#[test]
fn left_right_mid_bounds() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.formula((0, 0), "LEFT(\"hello\",3)") // hel
        .formula((1, 0), "LEFT(\"hi\",10)") // hi (past end)
        .formula((2, 0), "RIGHT(\"hello\",2)") // lo
        .formula((3, 0), "MID(\"hello\",2,3)") // ell
        .formula((4, 0), "MID(\"hello\",10,3)") // "" (start past end)
        .formula((5, 0), "MID(\"hello\",0,3)"); // #VALUE! (start < 1)
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(text_at(&wb, 0, 0), "hel");
    assert_eq!(text_at(&wb, 1, 0), "hi");
    assert_eq!(text_at(&wb, 2, 0), "lo");
    assert_eq!(text_at(&wb, 3, 0), "ell");
    assert_eq!(text_at(&wb, 4, 0), "");
    assert_eq!(value_at(&wb, 5, 0), CellValue::Error(ErrorValue::Value));
}

#[test]
fn upper_lower_trim() {
    let mut b = Builder::new();
    b.formula((0, 0), "UPPER(\"aBc\")")
        .formula((1, 0), "LOWER(\"aBc\")")
        .formula((2, 0), "TRIM(\"  a   b  \")");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(text_at(&wb, 0, 0), "ABC");
    assert_eq!(text_at(&wb, 1, 0), "abc");
    assert_eq!(text_at(&wb, 2, 0), "a b");
}

#[test]
fn math_int_mod_power_sqrt() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.formula((0, 0), "INT(3.9)") // 3
        .formula((1, 0), "INT(-1.5)") // -2 (floor)
        .formula((2, 0), "MOD(7,3)") // 1
        .formula((3, 0), "MOD(-7,3)") // 2 (sign of divisor)
        .formula((4, 0), "POWER(2,10)") // 1024
        .formula((5, 0), "SQRT(16)") // 4
        .formula((6, 0), "SQRT(-1)") // #NUM!
        .formula((7, 0), "MOD(5,0)"); // #DIV/0!
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(number_at(&wb, 0, 0), 3.0);
    assert_eq!(number_at(&wb, 1, 0), -2.0);
    assert_eq!(number_at(&wb, 2, 0), 1.0);
    assert_eq!(number_at(&wb, 3, 0), 2.0);
    assert_eq!(number_at(&wb, 4, 0), 1024.0);
    assert_eq!(number_at(&wb, 5, 0), 4.0);
    assert_eq!(value_at(&wb, 6, 0), CellValue::Error(ErrorValue::Num));
    assert_eq!(value_at(&wb, 7, 0), CellValue::Error(ErrorValue::Div0));
}

#[test]
fn recalc_is_deterministic() {
    let mut b = Builder::new();
    b.number((0, 0), 3.0)
        .formula((1, 0), "SUM(A1:A1)*ROUND(2.5,0)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let first = value_at(&wb, 1, 0);
    recalculate(&mut wb);
    let second = value_at(&wb, 1, 0);
    assert_eq!(first, second);
}
