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
