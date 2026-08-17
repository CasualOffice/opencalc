//! Evaluation tests: build formula cells directly, recalculate, check values.

use casual_calc_formula::parse;
use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

use crate::{Recalculator, recalculate};

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

    fn boolean(&mut self, at: (u32, u32), b: bool) -> &mut Self {
        self.sheet
            .cells
            .set(CellRef::new(at.0, at.1), Cell::value(CellValue::Bool(b)));
        self
    }

    fn text(&mut self, at: (u32, u32), s: &str) -> &mut Self {
        let id = self.wb.intern_string(s);
        self.sheet.cells.set(
            CellRef::new(at.0, at.1),
            Cell::value(CellValue::InlineString(id)),
        );
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
fn non_finite_arithmetic_is_num_error() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.formula((0, 0), "1E308*10") // overflow to +inf
        .formula((1, 0), "10^400") // overflow via power
        .formula((2, 0), "1E308+1E308"); // overflow via add
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 0, 0), CellValue::Error(ErrorValue::Num));
    assert_eq!(value_at(&wb, 1, 0), CellValue::Error(ErrorValue::Num));
    assert_eq!(value_at(&wb, 2, 0), CellValue::Error(ErrorValue::Num));
}

#[test]
fn countif_sumif_wildcards() {
    let mut b = Builder::new();
    b.text((0, 0), "Apple") // A1
        .text((1, 0), "Apricot") // A2
        .text((2, 0), "Banana") // A3
        .text((3, 0), "apple pie") // A4 (lowercase, longer)
        .text((4, 0), "*star") // A5 (literal asterisk)
        .number((0, 1), 1.0) // B1
        .number((1, 1), 2.0) // B2
        .number((2, 1), 4.0) // B3
        .number((3, 1), 8.0) // B4
        .number((4, 1), 16.0) // B5
        .formula((0, 2), "COUNTIF(A1:A5,\"A*\")") // starts A (ci): 3
        .formula((1, 2), "COUNTIF(A1:A5,\"*e\")") // ends e: Apple, apple pie = 2
        .formula((2, 2), "COUNTIF(A1:A5,\"?pple\")") // one char + pple: Apple = 1
        .formula((3, 2), "COUNTIF(A1:A5,\"<>A*\")") // not starting A: Banana, *star = 2
        .formula((4, 2), "COUNTIF(A1:A5,\"~*star\")") // literal *star = 1
        .formula((5, 2), "SUMIF(A1:A5,\"A*\",B1:B5)"); // 1+2+8 = 11
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(number_at(&wb, 0, 2), 3.0);
    assert_eq!(number_at(&wb, 1, 2), 2.0);
    assert_eq!(number_at(&wb, 2, 2), 1.0);
    assert_eq!(number_at(&wb, 3, 2), 2.0);
    assert_eq!(number_at(&wb, 4, 2), 1.0);
    assert_eq!(number_at(&wb, 5, 2), 11.0);
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

// --- Lookup / reference ---------------------------------------------------

/// Build a small vertical lookup table: A1:B3 = {1:"a", 2:"b", 3:"c"}.
fn lookup_table(b: &mut Builder) {
    b.number((0, 0), 1.0)
        .formula((0, 1), "\"a\"")
        .number((1, 0), 2.0)
        .formula((1, 1), "\"b\"")
        .number((2, 0), 3.0)
        .formula((2, 1), "\"c\"");
}

#[test]
fn vlookup_exact_and_approximate() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    lookup_table(&mut b);
    b.formula((0, 3), "VLOOKUP(2,A1:B3,2,FALSE)") // exact -> "b"
        .formula((1, 3), "VLOOKUP(2.5,A1:B3,2,TRUE)") // approx -> "b"
        .formula((2, 3), "VLOOKUP(0,A1:B3,2,FALSE)"); // not found -> #N/A
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(text_at(&wb, 0, 3), "b");
    assert_eq!(text_at(&wb, 1, 3), "b");
    assert_eq!(value_at(&wb, 2, 3), CellValue::Error(ErrorValue::Na));
}

#[test]
fn hlookup_exact() {
    let mut b = Builder::new();
    b.number((0, 0), 1.0) // A1
        .number((0, 1), 2.0) // B1
        .number((0, 2), 3.0) // C1
        .formula((1, 0), "\"x\"") // A2
        .formula((1, 1), "\"y\"") // B2
        .formula((1, 2), "\"z\"") // C2
        .formula((2, 0), "HLOOKUP(2,A1:C2,2,FALSE)");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(text_at(&wb, 2, 0), "y");
}

#[test]
fn index_row_col_and_out_of_bounds() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    lookup_table(&mut b);
    b.formula((0, 3), "INDEX(A1:B3,2,2)") // "b"
        .formula((1, 3), "INDEX(A1:A3,3)") // 3 (single column)
        .formula((2, 3), "INDEX(A1:B3,9,1)"); // out of range -> #REF!
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(text_at(&wb, 0, 3), "b");
    assert_eq!(number_at(&wb, 1, 3), 3.0);
    assert_eq!(value_at(&wb, 2, 3), CellValue::Error(ErrorValue::Ref));
}

#[test]
fn match_types() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    lookup_table(&mut b);
    b.formula((0, 3), "MATCH(3,A1:A3,0)") // exact -> 3
        .formula((1, 3), "MATCH(2.5,A1:A3,1)") // largest <= -> 2
        .formula((2, 3), "MATCH(9,A1:A3,0)"); // not found -> #N/A
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(number_at(&wb, 0, 3), 3.0);
    assert_eq!(number_at(&wb, 1, 3), 2.0);
    assert_eq!(value_at(&wb, 2, 3), CellValue::Error(ErrorValue::Na));
}

#[test]
fn choose_selects_argument() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.formula((0, 0), "CHOOSE(2,\"x\",\"y\",\"z\")") // "y"
        .formula((1, 0), "CHOOSE(5,\"x\",\"y\")"); // out of range -> #VALUE!
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(text_at(&wb, 0, 0), "y");
    assert_eq!(value_at(&wb, 1, 0), CellValue::Error(ErrorValue::Value));
}

// --- Extra math -----------------------------------------------------------

#[test]
fn product_and_rounding_directions() {
    let mut b = Builder::new();
    b.number((0, 0), 2.0)
        .number((1, 0), 3.0)
        .number((2, 0), 4.0)
        .formula((0, 1), "PRODUCT(A1:A3)") // 24
        .formula((1, 1), "ROUNDUP(3.14159,2)") // 3.15
        .formula((2, 1), "ROUNDDOWN(3.19,1)") // 3.1
        .formula((3, 1), "TRUNC(-3.99)"); // -3
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(number_at(&wb, 0, 1), 24.0);
    assert_eq!(number_at(&wb, 1, 1), 3.15);
    assert_eq!(number_at(&wb, 2, 1), 3.1);
    assert_eq!(number_at(&wb, 3, 1), -3.0);
}

#[test]
fn ceiling_floor_and_sign() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.formula((0, 0), "CEILING(2.1,0.5)") // 2.5
        .formula((1, 0), "FLOOR(2.6,0.5)") // 2.5
        .formula((2, 0), "SIGN(-5)") // -1
        .formula((3, 0), "SIGN(0)") // 0
        .formula((4, 0), "CEILING(-2.5,1)"); // sign mismatch -> #NUM!
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(number_at(&wb, 0, 0), 2.5);
    assert_eq!(number_at(&wb, 1, 0), 2.5);
    assert_eq!(number_at(&wb, 2, 0), -1.0);
    assert_eq!(number_at(&wb, 3, 0), 0.0);
    assert_eq!(value_at(&wb, 4, 0), CellValue::Error(ErrorValue::Num));
}

// --- Extra text -----------------------------------------------------------

#[test]
fn substitute_and_replace() {
    let mut b = Builder::new();
    b.formula((0, 0), "SUBSTITUTE(\"a-b-c\",\"-\",\"+\")") // a+b+c
        .formula((1, 0), "SUBSTITUTE(\"a-b-c\",\"-\",\"+\",2)") // a-b+c
        .formula((2, 0), "REPLACE(\"abcdef\",2,3,\"XY\")"); // aXYef
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(text_at(&wb, 0, 0), "a+b+c");
    assert_eq!(text_at(&wb, 1, 0), "a-b+c");
    assert_eq!(text_at(&wb, 2, 0), "aXYef");
}

#[test]
fn find_search_case_and_range() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.formula((0, 0), "SEARCH(\"B\",\"aBc\")") // case-insensitive -> 2
        .formula((1, 0), "FIND(\"b\",\"aBc\")") // case-sensitive miss -> #VALUE!
        .formula((2, 0), "FIND(\"x\",\"abc\",5)"); // start out of range -> #VALUE!
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(number_at(&wb, 0, 0), 2.0);
    assert_eq!(value_at(&wb, 1, 0), CellValue::Error(ErrorValue::Value));
    assert_eq!(value_at(&wb, 2, 0), CellValue::Error(ErrorValue::Value));
}

#[test]
fn value_proper_rept_exact() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.formula((0, 0), "VALUE(\"123.5\")") // 123.5
        .formula((1, 0), "VALUE(\"abc\")") // #VALUE!
        .formula((2, 0), "PROPER(\"hello WORLD\")") // Hello World
        .formula((3, 0), "REPT(\"ab\",3)") // ababab
        .formula((4, 0), "REPT(\"a\",-1)") // #VALUE!
        .formula((5, 0), "EXACT(\"abc\",\"abc\")") // true
        .formula((6, 0), "EXACT(\"abc\",\"Abc\")"); // false
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(number_at(&wb, 0, 0), 123.5);
    assert_eq!(value_at(&wb, 1, 0), CellValue::Error(ErrorValue::Value));
    assert_eq!(text_at(&wb, 2, 0), "Hello World");
    assert_eq!(text_at(&wb, 3, 0), "ababab");
    assert_eq!(value_at(&wb, 4, 0), CellValue::Error(ErrorValue::Value));
    assert!(bool_at(&wb, 5, 0));
    assert!(!bool_at(&wb, 6, 0));
}

// --- Dates ----------------------------------------------------------------

#[test]
fn date_year_month_day_round_trip() {
    let mut b = Builder::new();
    b.formula((0, 0), "YEAR(DATE(2024,3,15))") // 2024
        .formula((1, 0), "MONTH(DATE(2024,3,15))") // 3
        .formula((2, 0), "DAY(DATE(2024,3,15))") // 15
        .formula((3, 0), "YEAR(DATE(2008,14,2))") // month overflow -> 2009
        .formula((4, 0), "MONTH(DATE(2008,14,2))"); // -> 2
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(number_at(&wb, 0, 0), 2024.0);
    assert_eq!(number_at(&wb, 1, 0), 3.0);
    assert_eq!(number_at(&wb, 2, 0), 15.0);
    assert_eq!(number_at(&wb, 3, 0), 2009.0);
    assert_eq!(number_at(&wb, 4, 0), 2.0);
}

#[test]
fn weekday_edate_eomonth() {
    let mut b = Builder::new();
    b.formula((0, 0), "WEEKDAY(DATE(2024,3,15))") // Friday, type 1 -> 6
        .formula((1, 0), "WEEKDAY(DATE(2024,3,15),2)") // Mon=1 -> 5
        .formula((2, 0), "DAY(EDATE(DATE(2024,1,31),1))") // clamp to Feb 29
        .formula((3, 0), "MONTH(EDATE(DATE(2024,1,31),1))") // 2
        .formula((4, 0), "DAY(EOMONTH(DATE(2024,2,10),0))"); // 29 (leap Feb)
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(number_at(&wb, 0, 0), 6.0);
    assert_eq!(number_at(&wb, 1, 0), 5.0);
    assert_eq!(number_at(&wb, 2, 0), 29.0);
    assert_eq!(number_at(&wb, 3, 0), 2.0);
    assert_eq!(number_at(&wb, 4, 0), 29.0);
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

// --- Incremental recalc: differential against full recalc -----------------

use crate::recalculate_incremental;

/// Every cell's value in `a` equals the same cell in `b` (over the union of
/// populated cells). Text is compared by resolved contents, not string id.
fn assert_same_values(a: &Workbook, b: &Workbook) {
    for s in 0..a.sheets.len().max(b.sheets.len()) {
        let mut seen = std::collections::HashSet::new();
        for wb in [a, b] {
            if let Some(sheet) = wb.sheets.get(s) {
                for (at, _) in sheet.cells.iter() {
                    seen.insert((at.row, at.col));
                }
            }
        }
        for (row, col) in seen {
            let va = value_norm(a, s, row, col);
            let vb = value_norm(b, s, row, col);
            assert_eq!(
                va, vb,
                "cell ({s},{row},{col}) diverged: incr={va:?} full={vb:?}"
            );
        }
    }
}

/// A comparable snapshot of a cell value (text resolved to its contents).
fn value_norm(wb: &Workbook, sheet: usize, row: u32, col: u32) -> String {
    let v = wb
        .sheets
        .get(sheet)
        .and_then(|s| s.cells.get(CellRef::new(row, col)))
        .map(|c| c.value.clone())
        .unwrap_or(CellValue::Empty);
    match v {
        CellValue::InlineString(id) | CellValue::SharedString(id) => {
            format!("T:{}", wb.strings.get(id).unwrap_or_default())
        }
        other => format!("{other:?}"),
    }
}

/// Apply a literal-number edit to a cell (single sheet 0), returning the changed
/// key for `recalculate_incremental`.
fn set_number(wb: &mut Workbook, row: u32, col: u32, n: f64) -> (usize, CellRef) {
    let at = CellRef::new(row, col);
    let mut cell = wb.sheets[0]
        .cells
        .get(at)
        .cloned()
        .unwrap_or(Cell::value(CellValue::Empty));
    cell.value = CellValue::Number(n);
    cell.formula = None;
    wb.sheets[0].cells.set(at, cell);
    (0, at)
}

/// After an edit, an incremental pass must produce the same cached values as a
/// from-scratch full recalc — across chains, ranges, and unrelated cells.
#[test]
fn incremental_matches_full_recalc() {
    // A workbook mixing a dependency chain, a range aggregate, a cross-cell
    // reference, and cells that do not depend on the edit at all.
    let build = || {
        let mut b = Builder::new();
        b.number((0, 0), 1.0) // A1
            .number((1, 0), 2.0) // A2
            .number((2, 0), 3.0) // A3
            .formula((3, 0), "SUM(A1:A3)") // A4 = 6   (range dep)
            .formula((4, 0), "A4*2") // A5 = 12   (chain on A4)
            .formula((5, 0), "A5+A1") // A6 = 13   (two deps)
            .number((0, 2), 100.0) // C1  (independent)
            .formula((1, 2), "C1+1") // C2 = 101 (independent of A*)
            .formula((0, 3), "IF(A2>1,\"big\",\"small\")"); // D1 text branch
        let mut wb = b.build();
        recalculate(&mut wb);
        wb
    };

    // Edit A2 (feeds the range A1:A3, the chain, and the IF) to 10.
    let mut incr = build();
    let changed = set_number(&mut incr, 1, 0, 10.0);
    recalculate_incremental(&mut incr, &[changed]);

    let mut full = build();
    set_number(&mut full, 1, 0, 10.0);
    recalculate(&mut full);

    assert_same_values(&incr, &full);
    // Spot-check the propagation actually happened.
    assert_eq!(number_at(&incr, 3, 0), 14.0); // SUM(1,10,3)
    assert_eq!(number_at(&incr, 4, 0), 28.0); // *2
    assert_eq!(number_at(&incr, 5, 0), 29.0); // +A1
    assert_eq!(number_at(&incr, 1, 2), 101.0); // C2 untouched
}

/// A pseudo-random sequence of edits, each followed by an incremental pass,
/// must stay in lock-step with a full recalc of the same workbook. Uses a fixed
/// LCG so the case is deterministic (no wall-clock / RNG dependence).
#[test]
fn incremental_matches_full_under_random_edits() {
    let build = || {
        let mut b = Builder::new();
        // A grid where each cell sums two earlier ones — deep, wide dependents.
        b.number((0, 0), 1.0)
            .number((0, 1), 2.0)
            .number((0, 2), 3.0);
        for r in 1..8u32 {
            b.formula((r, 0), &format!("A{r}+B{r}"));
            b.formula((r, 1), &format!("B{r}+C{r}"));
            b.formula((r, 2), &format!("SUM(A{}:C{})", r, r));
        }
        let mut wb = b.build();
        recalculate(&mut wb);
        wb
    };

    let mut incr = build();
    let mut full = build();
    let mut state: u64 = 0x1234_5678;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as u32
    };
    for _ in 0..40 {
        // Edit one of the three base inputs (row 0, cols 0..3).
        let col = next() % 3;
        let n = (next() % 20) as f64 - 5.0;
        let changed = set_number(&mut incr, 0, col, n);
        recalculate_incremental(&mut incr, &[changed]);
        set_number(&mut full, 0, col, n);
        recalculate(&mut full);
        assert_same_values(&incr, &full);
    }
}

/// Replace a cell's formula, returning the key the recalculator is told about.
fn set_formula(wb: &mut Workbook, row: u32, col: u32, formula: &str) -> (usize, CellRef) {
    let at = CellRef::new(row, col);
    let handle = wb.store_formula(parse(formula).unwrap());
    let mut cell = wb.sheets[0]
        .cells
        .get(at)
        .cloned()
        .unwrap_or(Cell::value(CellValue::Empty));
    cell.formula = Some(handle);
    wb.sheets[0].cells.set(at, cell);
    (0, at)
}

/// **A graph kept across edits must still answer like a full recalculation.**
///
/// The existing differential tests drive `recalculate_incremental`, which
/// rebuilds the graph every pass and therefore cannot go stale — so none of them
/// exercise the thing step three of `docs/66` actually changed. This one holds a
/// single [`Recalculator`] across the whole sequence, so every edit after the
/// first is answered by a graph that was *patched* rather than built.
///
/// The edits deliberately include formula rewrites, not only value changes. A
/// value edit re-derives identical edges and would pass against a graph that
/// never patched anything at all; rewriting a formula is what moves edges, and a
/// failure to move them is invisible until some later edit reads the stale one.
#[test]
fn a_kept_graph_matches_full_recalc_under_random_edits() {
    let build = || {
        let mut b = Builder::new();
        b.number((0, 0), 1.0)
            .number((0, 1), 2.0)
            .number((0, 2), 3.0);
        for r in 1..8u32 {
            b.formula((r, 0), &format!("A{r}+B{r}"));
            b.formula((r, 1), &format!("B{r}+C{r}"));
            b.formula((r, 2), &format!("SUM(A{}:C{})", r, r));
        }
        let mut wb = b.build();
        recalculate(&mut wb);
        wb
    };

    let mut kept = build();
    let mut full = build();
    let mut recalc = Recalculator::new();
    let mut state: u64 = 0x9e37_79b9;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as u32
    };

    for _ in 0..80 {
        let row = 1 + next() % 7;
        let col = next() % 3;
        // Roughly a third of the edits rewrite a formula; the rest are values,
        // which is the ratio that matters — the kept graph has to survive being
        // mostly untouched and occasionally rearranged.
        let changed = match next() % 3 {
            0 => {
                let src = 1 + next() % 7;
                let f = match next() % 3 {
                    0 => format!("A{src}*2"),
                    1 => format!("SUM(A{src}:C{src})"),
                    _ => format!("B{src}+C{src}+1"),
                };
                let k = set_formula(&mut kept, row, col, &f);
                set_formula(&mut full, row, col, &f);
                k
            }
            _ => {
                let n = f64::from(next() % 20) - 5.0;
                let k = set_number(&mut kept, 0, col, n);
                set_number(&mut full, 0, col, n);
                k
            }
        };
        recalc.recalculate(&mut kept, &[changed]);
        recalculate(&mut full);
        assert_same_values(&kept, &full);
    }
}

/// A structural edit under a kept graph is only safe because the caller drops
/// it, so assert what happens when they do — and that the recalculator is
/// usable again afterwards rather than permanently degraded.
#[test]
fn a_recalculator_recovers_after_invalidation() {
    let mut b = Builder::new();
    b.number((0, 0), 2.0);
    b.formula((1, 0), "A1*10");
    let mut wb = b.build();
    recalculate(&mut wb);

    let mut recalc = Recalculator::new();
    let changed = set_number(&mut wb, 0, 0, 3.0);
    recalc.recalculate(&mut wb, &[changed]);
    assert_eq!(
        wb.sheets[0].cells.get(CellRef::new(1, 0)).unwrap().value,
        CellValue::Number(30.0)
    );

    // The document moves under the graph; the caller says so.
    recalc.invalidate();
    let changed = set_number(&mut wb, 0, 0, 4.0);
    recalc.recalculate(&mut wb, &[changed]);
    assert_eq!(
        wb.sheets[0].cells.get(CellRef::new(1, 0)).unwrap().value,
        CellValue::Number(40.0),
        "a rebuilt graph answers as well as the one it replaced"
    );
}

/// Cross-sheet reference with a differently-cased qualifier must be tracked by
/// the incremental dependency graph exactly as the evaluator resolves it, so an
/// edit on the referenced sheet updates the dependent (regression for the graph
/// resolving sheet names case-sensitively while eval is case-insensitive).
#[test]
fn incremental_tracks_case_insensitive_cross_sheet_ref() {
    use casual_calc_formula::parse;
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let mut s1 = Sheet::new(SheetId(Id::from_parts(2, 1)), "Sheet1");
    s1.cells
        .set(CellRef::new(0, 0), Cell::value(CellValue::Number(7.0))); // Sheet1!A1 = 7
    let mut s2 = Sheet::new(SheetId(Id::from_parts(2, 2)), "Sheet2");
    // Lowercase qualifier, as a user might type / an import might preserve.
    let handle = wb.store_formula(parse("sheet1!A1*2").unwrap());
    let mut c = Cell::value(CellValue::Empty);
    c.formula = Some(handle);
    s2.cells.set(CellRef::new(0, 0), c); // Sheet2!A1 = sheet1!A1 * 2
    wb.sheets.push(s1);
    wb.sheets.push(s2);
    recalculate(&mut wb);
    assert_eq!(
        wb.sheets[1].cells.get(CellRef::new(0, 0)).unwrap().value,
        CellValue::Number(14.0)
    );

    // Edit Sheet1!A1 -> 10, then incremental. Sheet2!A1 must recompute to 20.
    let at = CellRef::new(0, 0);
    let mut cell = wb.sheets[0].cells.get(at).cloned().unwrap();
    cell.value = CellValue::Number(10.0);
    wb.sheets[0].cells.set(at, cell);
    recalculate_incremental(&mut wb, &[(0, at)]);
    assert_eq!(
        wb.sheets[1].cells.get(CellRef::new(0, 0)).unwrap().value,
        CellValue::Number(20.0),
        "cross-sheet dependent stayed stale — graph missed the case-insensitive ref"
    );
}

#[test]
fn every_cataloged_function_dispatches() {
    use casual_calc_model::ErrorValue;
    // Each catalog entry must have a dispatch arm: evaluating `NAME()` must not
    // fall through to #NAME? (the unknown-function sentinel). This is what keeps
    // the catalog and the dispatch table from drifting apart.
    for (name, _) in crate::FUNCTIONS {
        let mut b = Builder::new();
        b.formula((0, 0), &format!("{name}()"));
        let mut wb = b.build();
        recalculate(&mut wb);
        assert_ne!(
            value_at(&wb, 0, 0),
            CellValue::Error(ErrorValue::Name),
            "catalog function {name} has no dispatch arm"
        );
    }
}

#[test]
fn catalog_is_sorted_and_unique() {
    let names: Vec<&str> = crate::FUNCTIONS.iter().map(|(n, _)| *n).collect();
    for w in names.windows(2) {
        assert!(w[0] < w[1], "catalog not sorted/unique at {:?}", w);
    }
}

#[test]
fn m6_2_math_logic_and_aggregates() {
    let mut b = Builder::new();
    for (i, v) in [10.0, 20.0, 30.0, 40.0, 50.0].iter().enumerate() {
        b.number((i as u32, 0), *v); // A1:A5
    }
    b.number((0, 1), 2.0)
        .number((1, 1), 3.0)
        .number((2, 1), 4.0); // B1:B3
    b.formula((0, 3), "SUMIFS(A1:A5,A1:A5,\">25\")"); // 120
    b.formula((1, 3), "COUNTIFS(A1:A5,\">25\")"); // 3
    b.formula((2, 3), "AVERAGEIFS(A1:A5,A1:A5,\">25\")"); // 40
    b.formula((3, 3), "MEDIAN(A1:A5)"); // 30
    b.formula((4, 3), "LARGE(A1:A5,2)"); // 40
    b.formula((5, 3), "SMALL(A1:A5,2)"); // 20
    b.formula((6, 3), "RANK(30,A1:A5)"); // 3 (descending)
    b.formula((7, 3), "STDEVP(A1:A5)"); // sqrt(200) ≈ 14.142
    b.formula((8, 3), "SUMPRODUCT(A1:A3,B1:B3)"); // 10*2+20*3+30*4 = 200
    b.formula((9, 3), "IFS(A1>100,1,A1>5,2)"); // 2
    b.formula((10, 3), "SWITCH(A1,10,100,999)"); // 100
    b.formula((11, 3), "IFNA(NA(),7)"); // 7
    b.formula((12, 3), "ROWS(A1:A5)"); // 5
    b.formula((13, 3), "COLUMNS(A1:B3)"); // 2
    let mut wb = b.build();
    recalculate(&mut wb);
    let num = |r| match value_at(&wb, r, 3) {
        CellValue::Number(n) => n,
        v => panic!("row {r}: {v:?}"),
    };
    assert_eq!(num(0), 120.0);
    assert_eq!(num(1), 3.0);
    assert_eq!(num(2), 40.0);
    assert_eq!(num(3), 30.0);
    assert_eq!(num(4), 40.0);
    assert_eq!(num(5), 20.0);
    assert_eq!(num(6), 3.0);
    assert!((num(7) - 200f64.sqrt()).abs() < 1e-9);
    assert_eq!(num(8), 200.0);
    assert_eq!(num(9), 2.0);
    assert_eq!(num(10), 100.0);
    assert_eq!(num(11), 7.0);
    assert_eq!(num(12), 5.0);
    assert_eq!(num(13), 2.0);
}

#[test]
fn m6_2_is_family_and_textjoin() {
    let mut b = Builder::new();
    b.number((0, 0), 5.0); // A1
    b.formula((0, 1), "ISNUMBER(A1)"); // true
    b.formula((1, 1), "ISBLANK(Z9)"); // true
    b.formula((2, 1), "ISEVEN(A1)"); // false
    b.formula((3, 1), "ISODD(A1)"); // true
    b.formula((4, 1), "ISNA(NA())"); // true
    b.formula((5, 1), "ISERROR(1/0)"); // true
    b.formula((6, 1), "ISTEXT(A1)"); // false
    b.formula((0, 2), "TEXTJOIN(\"-\",TRUE,\"a\",\"\",\"b\")"); // "a-b"
    let mut wb = b.build();
    recalculate(&mut wb);
    let boo = |r| match value_at(&wb, r, 1) {
        CellValue::Bool(x) => x,
        v => panic!("row {r}: {v:?}"),
    };
    assert!(boo(0) && boo(1) && boo(3) && boo(4) && boo(5));
    assert!(!boo(2) && !boo(6));
    assert_eq!(text_at(&wb, 0, 2), "a-b");
}

#[test]
fn row_and_column_functions() {
    let mut b = Builder::new();
    b.formula((4, 2), "ROW()"); // C5 → 5
    b.formula((4, 3), "COLUMN()"); // D5 → 4
    b.formula((0, 0), "ROW(B10)"); // 10
    b.formula((1, 0), "COLUMN(B10)"); // 2 (B)
    b.formula((2, 0), "ROW(D2:D9)"); // 2 (top-left)
    // A6 sums the value of C5's ROW() cell — C5 must still report its OWN row
    // (5) while evaluated as A6's precedent, proving current-cell save/restore.
    b.formula((5, 0), "C5+100"); // 5 + 100 = 105
    let mut wb = b.build();
    recalculate(&mut wb);
    let num = |r, c| match value_at(&wb, r, c) {
        CellValue::Number(n) => n,
        v => panic!("({r},{c}): {v:?}"),
    };
    assert_eq!(num(4, 2), 5.0);
    assert_eq!(num(4, 3), 4.0);
    assert_eq!(num(0, 0), 10.0);
    assert_eq!(num(1, 0), 2.0);
    assert_eq!(num(2, 0), 2.0);
    assert_eq!(num(5, 0), 105.0);
}

#[test]
fn text_function_formats_via_the_display_engine() {
    let mut b = Builder::new();
    b.number((0, 0), 1234.5); // A1
    b.number((1, 0), 0.25); // A2
    b.number((2, 0), -42.0); // A3
    b.formula((0, 1), "TEXT(A1,\"#,##0.00\")"); // "1,234.50"
    b.formula((1, 1), "TEXT(A2,\"0%\")"); // "25%"
    b.formula((2, 1), "TEXT(A3,\"$#,##0.00;[Red]($#,##0.00)\")"); // "($42.00)"
    b.formula((3, 1), "TEXT(5,\"0.000\")"); // "5.000"
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(text_at(&wb, 0, 1), "1,234.50");
    assert_eq!(text_at(&wb, 1, 1), "25%");
    assert_eq!(text_at(&wb, 2, 1), "($42.00)");
    assert_eq!(text_at(&wb, 3, 1), "5.000");
}

/// Evaluate a single formula and return its value.
fn eval1(text: &str) -> CellValue {
    let mut b = Builder::new();
    b.formula((0, 0), text);
    let mut wb = b.build();
    recalculate(&mut wb);
    value_at(&wb, 0, 0)
}

fn num(text: &str) -> f64 {
    match eval1(text) {
        CellValue::Number(n) => n,
        other => panic!("{text} gave {other:?}, expected a number"),
    }
}

fn err(text: &str) -> casual_calc_model::ErrorValue {
    match eval1(text) {
        CellValue::Error(e) => e,
        other => panic!("{text} gave {other:?}, expected an error"),
    }
}

#[test]
fn trigonometry_matches_the_spec() {
    use std::f64::consts::PI;
    assert!((num("SIN(PI()/2)") - 1.0).abs() < 1e-12);
    assert!((num("COS(0)") - 1.0).abs() < 1e-12);
    assert!((num("DEGREES(PI())") - 180.0).abs() < 1e-12);
    assert!((num("RADIANS(180)") - PI).abs() < 1e-12);
    assert!((num("ACOS(1)")).abs() < 1e-12);
    assert!((num("SINH(0)")).abs() < 1e-12);
    assert!((num("SEC(0)") - 1.0).abs() < 1e-12);
    assert!((num("CSC(PI()/2)") - 1.0).abs() < 1e-12);
    assert!((num("COT(PI()/4)") - 1.0).abs() < 1e-9);
}

#[test]
fn atan2_takes_x_before_y() {
    use casual_calc_model::ErrorValue;
    use std::f64::consts::PI;
    // OOXML orders the arguments x-then-y, the reverse of the atan2(y, x) that
    // every maths library uses. ATAN2(1, 0) is therefore 0 and ATAN2(0, 1) is
    // a quarter turn — passing them straight through would swap the two and
    // silently mirror every angle about the diagonal.
    assert!((num("ATAN2(1,0)")).abs() < 1e-12);
    assert!((num("ATAN2(0,1)") - PI / 2.0).abs() < 1e-12);
    assert!((num("ATAN2(1,1)") - PI / 4.0).abs() < 1e-12);
    assert_eq!(err("ATAN2(0,0)"), ErrorValue::Div0);
}

#[test]
fn domain_errors_are_num_not_nan() {
    use casual_calc_model::ErrorValue;
    // IEEE arithmetic yields NaN here; a spreadsheet must answer #NUM!, and a
    // NaN leaking into a cell would compare and format as nonsense.
    for f in [
        "ASIN(2)",
        "ACOS(-2)",
        "LN(-1)",
        "LOG10(0)",
        "ACOSH(0.5)",
        "ATANH(1)",
    ] {
        assert_eq!(err(f), ErrorValue::Num, "{f}");
    }
    // A zero denominator is a division error, not an infinity.
    assert_eq!(err("CSC(0)"), ErrorValue::Div0);
    assert_eq!(err("COT(0)"), ErrorValue::Div0);
}

#[test]
fn rounding_helpers_round_away_from_zero() {
    assert_eq!(num("EVEN(1.5)"), 2.0);
    assert_eq!(num("EVEN(3)"), 4.0);
    assert_eq!(num("EVEN(-1.5)"), -2.0);
    assert_eq!(num("EVEN(0)"), 0.0, "EVEN(0) is 0, not 2");
    assert_eq!(num("ODD(1.5)"), 3.0);
    assert_eq!(num("ODD(2)"), 3.0);
    assert_eq!(num("ODD(-2)"), -3.0);
    assert_eq!(num("ODD(0)"), 1.0, "ODD(0) is 1, not -1 or 0");
    assert_eq!(num("MROUND(10,3)"), 9.0);
    assert_eq!(num("MROUND(-10,-3)"), -9.0);
    assert_eq!(num("QUOTIENT(9,2)"), 4.0);
    assert_eq!(
        num("QUOTIENT(-9,2)"),
        -4.0,
        "QUOTIENT truncates toward zero"
    );
}

#[test]
fn mround_rejects_mismatched_signs() {
    use casual_calc_model::ErrorValue;
    // Excel refuses to round a positive number to a negative multiple.
    assert_eq!(err("MROUND(10,-3)"), ErrorValue::Num);
}

#[test]
fn combinatorics() {
    use casual_calc_model::ErrorValue;
    assert_eq!(num("FACT(5)"), 120.0);
    assert_eq!(num("FACT(0)"), 1.0);
    assert_eq!(err("FACT(-1)"), ErrorValue::Num);
    assert_eq!(num("FACTDOUBLE(7)"), 105.0); // 7·5·3·1
    assert_eq!(num("FACTDOUBLE(8)"), 384.0); // 8·6·4·2
    assert_eq!(num("COMBIN(8,2)"), 28.0);
    assert_eq!(num("COMBINA(4,3)"), 20.0);
    assert_eq!(num("PERMUT(4,2)"), 12.0);
    assert_eq!(num("PERMUTATIONA(4,2)"), 16.0);
    assert_eq!(err("COMBIN(2,8)"), ErrorValue::Num);
    // Large binomials must not overflow on the way to a small answer: 100!
    // exceeds f64 range but C(100,2) is 4950.
    assert_eq!(num("COMBIN(100,2)"), 4950.0);
}

#[test]
fn factorial_overflow_is_num() {
    use casual_calc_model::ErrorValue;
    // 170! is the largest representable; 171! is #NUM!, not an infinity.
    assert!(num("FACT(170)").is_finite());
    assert_eq!(err("FACT(171)"), ErrorValue::Num);
}

#[test]
fn gcd_lcm_multinomial_and_series() {
    assert_eq!(num("GCD(24,36)"), 12.0);
    assert_eq!(num("GCD(5,0)"), 5.0);
    assert_eq!(num("LCM(4,6)"), 12.0);
    assert_eq!(num("LCM(4,0)"), 0.0);
    assert_eq!(num("MULTINOMIAL(2,3,4)"), 1260.0);
    assert_eq!(num("SUMSQ(3,4)"), 25.0);
    // SERIESSUM(x=2, n=1, m=1, {1,1,1}) = 2 + 4 + 8.
    assert_eq!(num("SERIESSUM(2,1,1,1)"), 2.0);
    assert_eq!(num("LOG(8,2)"), 3.0);
    assert_eq!(num("LOG(100)"), 2.0);
    assert_eq!(num("SQRTPI(4)"), (4.0 * std::f64::consts::PI).sqrt());
}

#[test]
fn logical_and_information_functions() {
    use casual_calc_model::ErrorValue;
    assert_eq!(eval1("TRUE()"), CellValue::Bool(true));
    assert_eq!(eval1("FALSE()"), CellValue::Bool(false));
    // A stray argument is an error, not tolerated: it nearly always means the
    // author meant something else.
    assert_eq!(err("TRUE(1)"), ErrorValue::Value);
    assert_eq!(err("NA()"), ErrorValue::Na);

    // N reads text as 0 rather than erroring — that asymmetry is the whole
    // reason the function exists, so it must not route through as_number.
    assert_eq!(num("N(7)"), 7.0);
    assert_eq!(num("N(TRUE())"), 1.0);
    assert_eq!(num("N(\"abc\")"), 0.0);
    assert_eq!(err("N(NA())"), ErrorValue::Na);

    assert_eq!(num("TYPE(1)"), 1.0);
    assert_eq!(num("TYPE(\"x\")"), 2.0);
    assert_eq!(num("TYPE(TRUE())"), 4.0);
    assert_eq!(num("TYPE(NA())"), 16.0);

    assert_eq!(num("ERROR.TYPE(NA())"), 7.0);
    assert_eq!(num("ERROR.TYPE(1/0)"), 2.0);
    // Not an error: the answer is itself #N/A, not a number.
    assert_eq!(err("ERROR.TYPE(5)"), ErrorValue::Na);
}

#[test]
fn isref_reads_the_expression_not_the_value() {
    // By the time a function receives an argument the evaluator has resolved a
    // reference to its contents, so asking the value would answer FALSE for
    // every reference. ISREF has to inspect the expression instead.
    let mut b = Builder::new();
    b.number((0, 0), 5.0)
        .formula((1, 0), "ISREF(A1)")
        .formula((2, 0), "ISREF(5)");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 1, 0), CellValue::Bool(true));
    assert_eq!(value_at(&wb, 2, 0), CellValue::Bool(false));
}

#[test]
fn isformula_distinguishes_a_formula_from_a_value() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.number((0, 0), 5.0)
        .formula((1, 0), "A1*2")
        .formula((2, 0), "ISFORMULA(A2)")
        .formula((3, 0), "ISFORMULA(A1)")
        .formula((4, 0), "ISFORMULA(5)");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 2, 0), CellValue::Bool(true));
    assert_eq!(value_at(&wb, 3, 0), CellValue::Bool(false));
    // A non-reference is #VALUE!, not FALSE — FALSE would read as "that cell
    // has no formula", which is a different claim.
    assert_eq!(value_at(&wb, 4, 0), CellValue::Error(ErrorValue::Value));
}

#[test]
fn sheet_and_sheets_count_from_one() {
    let mut b = Builder::new();
    b.formula((0, 0), "SHEET()").formula((1, 0), "SHEETS()");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 0, 0), CellValue::Number(1.0));
    assert_eq!(value_at(&wb, 1, 0), CellValue::Number(1.0));
}

/// Build a sheet carrying a `Sales` table over A1:B4 with a totals row.
fn table_workbook() -> Workbook {
    use casual_calc_model::{CellRange, Table, TableColumn};
    let mut b = Builder::new();
    b.text((0, 0), "Region")
        .text((0, 1), "Amount")
        .text((1, 0), "North")
        .number((1, 1), 100.0)
        .text((2, 0), "South")
        .number((2, 1), 200.0)
        .text((3, 0), "Total")
        .formula((3, 1), "SUM(Sales[Amount])")
        .formula((5, 0), "SUM(Sales[Amount])")
        .formula((6, 0), "SUM(Sales[#All])")
        .formula((7, 0), "SUM(Missing[Amount])");
    let mut wb = b.build();
    wb.sheets[0].tables.push(Table {
        id: 1,
        name: "Sales".to_owned(),
        display_name: "Sales".to_owned(),
        range: CellRange {
            start: CellRef::new(0, 0),
            end: CellRef::new(3, 1),
        },
        header_row_count: 1,
        totals_row_count: 1,
        columns: vec![
            TableColumn {
                id: 1,
                name: "Region".to_owned(),
                totals_row_function: None,
                totals_row_label: None,
                calculated_column_formula: None,
                totals_row_formula: None,
            },
            TableColumn {
                id: 2,
                name: "Amount".to_owned(),
                totals_row_function: Some("sum".to_owned()),
                totals_row_label: None,
                calculated_column_formula: None,
                totals_row_formula: None,
            },
        ],
        auto_filter: None,
        style: Default::default(),
        attrs: Default::default(),
    });
    wb
}

#[test]
fn structured_references_resolve_to_the_data_body() {
    let mut wb = table_workbook();
    recalculate(&mut wb);
    // Sales[Amount] is the data rows only: 100 + 200. Including the header
    // would add nothing numeric, but including the totals row would double the
    // answer — and that mistake reads as plausible.
    assert_eq!(value_at(&wb, 5, 0), CellValue::Number(300.0));
    // #All spans header and totals too; the header is text so it contributes
    // nothing, but the totals cell (itself 300) does.
    assert_eq!(value_at(&wb, 6, 0), CellValue::Number(600.0));
}

#[test]
fn a_reference_to_a_missing_table_is_ref_not_zero() {
    use casual_calc_model::ErrorValue;
    let mut wb = table_workbook();
    recalculate(&mut wb);
    // Silently reading as an empty range would make a SUM over a deleted table
    // report 0, which looks like a real answer.
    assert_eq!(value_at(&wb, 7, 0), CellValue::Error(ErrorValue::Ref));
}

#[test]
fn a_totals_row_formula_does_not_include_itself() {
    let mut wb = table_workbook();
    recalculate(&mut wb);
    // B4 is the totals cell holding SUM(Sales[Amount]); if the data body
    // included the totals row this would be self-referential.
    assert_eq!(value_at(&wb, 3, 1), CellValue::Number(300.0));
}

#[test]
fn time_components_round_before_splitting() {
    // 13:45:30 as a day fraction is not exactly representable, so truncating
    // the raw product yields 29 seconds where the sheet plainly shows 30.
    let serial = (13.0 * 3600.0 + 45.0 * 60.0 + 30.0) / 86_400.0;
    let mut b = Builder::new();
    b.number((0, 0), serial)
        .formula((1, 0), "HOUR(A1)")
        .formula((2, 0), "MINUTE(A1)")
        .formula((3, 0), "SECOND(A1)");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 1, 0), CellValue::Number(13.0));
    assert_eq!(value_at(&wb, 2, 0), CellValue::Number(45.0));
    assert_eq!(value_at(&wb, 3, 0), CellValue::Number(30.0));
}

#[test]
fn time_components_roll_over() {
    // TIME(25,0,0) is 1:00, not an error — the rollover is what makes the
    // function usable for arithmetic.
    assert!((num("TIME(25,0,0)") - 1.0 / 24.0).abs() < 1e-12);
    assert!((num("TIME(0,90,0)") - 1.5 / 24.0).abs() < 1e-12);
}

#[test]
fn days360_follows_the_us_convention() {
    // 2024-01-31 to 2024-03-31. Under the US convention the start's 31st
    // becomes the 30th first, and only then does the end's 31st move — which
    // is why the two clamps cannot be written symmetrically.
    let start = num("DATE(2024,1,31)");
    let end = num("DATE(2024,3,31)");
    let mut b = Builder::new();
    b.number((0, 0), start)
        .number((1, 0), end)
        .formula((2, 0), "DAYS360(A1,A2)")
        .formula((3, 0), "DAYS360(A1,A2,TRUE())")
        .formula((4, 0), "DAYS(A2,A1)");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 2, 0), CellValue::Number(60.0));
    assert_eq!(value_at(&wb, 3, 0), CellValue::Number(60.0));
    // The real span, for contrast.
    assert_eq!(value_at(&wb, 4, 0), CellValue::Number(60.0));
}

#[test]
fn datedif_units_and_reversed_dates() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.formula((0, 0), "DATE(2020,3,15)")
        .formula((1, 0), "DATE(2024,7,20)")
        .formula((2, 0), "DATEDIF(A1,A2,\"Y\")")
        .formula((3, 0), "DATEDIF(A1,A2,\"M\")")
        .formula((4, 0), "DATEDIF(A1,A2,\"YM\")")
        .formula((5, 0), "DATEDIF(A2,A1,\"D\")");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 2, 0), CellValue::Number(4.0));
    assert_eq!(value_at(&wb, 3, 0), CellValue::Number(52.0));
    assert_eq!(value_at(&wb, 4, 0), CellValue::Number(4.0));
    // Excel reports #NUM! rather than a negative span.
    assert_eq!(value_at(&wb, 5, 0), CellValue::Error(ErrorValue::Num));
}

#[test]
fn iso_week_belongs_to_the_year_of_its_thursday() {
    // 2021-01-01 is a Friday, so its ISO week is week 53 of 2020 — the case
    // that separates ISOWEEKNUM from a naive day-count.
    let mut b = Builder::new();
    b.formula((0, 0), "DATE(2021,1,1)")
        .formula((1, 0), "ISOWEEKNUM(A1)")
        .formula((2, 0), "WEEKNUM(A1)")
        .formula((3, 0), "DATE(2024,1,1)")
        .formula((4, 0), "ISOWEEKNUM(A4)");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 1, 0), CellValue::Number(53.0));
    // WEEKNUM counts from the week containing 1 January, so it says 1.
    assert_eq!(value_at(&wb, 2, 0), CellValue::Number(1.0));
    // 2024-01-01 is a Monday: week 1 by both reckonings.
    assert_eq!(value_at(&wb, 4, 0), CellValue::Number(1.0));
}

#[test]
fn networkdays_and_workday_skip_weekends() {
    let mut b = Builder::new();
    // 2024-03-04 is a Monday; 2024-03-15 is the Friday of the next week.
    b.formula((0, 0), "DATE(2024,3,4)")
        .formula((1, 0), "DATE(2024,3,15)")
        .formula((2, 0), "NETWORKDAYS(A1,A2)")
        .formula((3, 0), "NETWORKDAYS(A2,A1)")
        .formula((4, 0), "WORKDAY(A1,5)-A1");
    let mut wb = b.build();
    recalculate(&mut wb);
    // Two full working weeks, counted inclusively at both ends.
    assert_eq!(value_at(&wb, 2, 0), CellValue::Number(10.0));
    // Reversed, the same magnitude negated.
    assert_eq!(value_at(&wb, 3, 0), CellValue::Number(-10.0));
    // Five working days from a Monday lands on the next Monday: 7 real days.
    assert_eq!(value_at(&wb, 4, 0), CellValue::Number(7.0));
}

#[test]
fn indirect_reads_the_cell_a_string_names() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.number((0, 0), 42.0)
        .text((1, 0), "A1")
        .formula((2, 0), "INDIRECT(\"A1\")")
        .formula((3, 0), "INDIRECT(A2)")
        .formula((4, 0), "INDIRECT(\"not a ref\")");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 2, 0), CellValue::Number(42.0));
    assert_eq!(value_at(&wb, 3, 0), CellValue::Number(42.0));
    // A string that is not a reference is #REF!, distinguishable from the cell
    // simply being empty.
    assert_eq!(value_at(&wb, 4, 0), CellValue::Error(ErrorValue::Ref));
}

#[test]
fn indirect_does_not_go_stale_when_its_target_changes() {
    // The dependency graph cannot see through INDIRECT: walking the arguments
    // finds the string, never the cell it names. Without treating it like a
    // defined name the formula would keep its first answer forever.
    let mut b = Builder::new();
    b.number((0, 0), 1.0).formula((1, 0), "INDIRECT(\"A1\")");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 1, 0), CellValue::Number(1.0));

    wb.sheets[0]
        .cells
        .set(CellRef::new(0, 0), Cell::value(CellValue::Number(99.0)));
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 1, 0), CellValue::Number(99.0));
}

#[test]
fn offset_shifts_and_reports_ref_off_grid() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.number((0, 0), 1.0)
        .number((2, 1), 7.0)
        .formula((5, 0), "OFFSET(A1,2,1)")
        .formula((6, 0), "OFFSET(A1,-1,0)")
        // A result larger than one cell is a range, and a range alone is
        // #VALUE! here exactly as A1:B2 is.
        .formula((7, 0), "OFFSET(A1,0,0,2,2)");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 5, 0), CellValue::Number(7.0));
    assert_eq!(value_at(&wb, 6, 0), CellValue::Error(ErrorValue::Ref));
    assert_eq!(value_at(&wb, 7, 0), CellValue::Error(ErrorValue::Value));
}

#[test]
fn address_builds_reference_text_not_a_reference() {
    // ADDRESS returns a *string*: it is INDIRECT that turns one back into
    // something to read, which is why the two are so often paired.
    let mut b = Builder::new();
    b.formula((0, 0), "ADDRESS(2,3)")
        .formula((1, 0), "ADDRESS(2,3,4)")
        .number((1, 2), 5.0)
        .formula((2, 0), "INDIRECT(ADDRESS(2,3))");
    let mut wb = b.build();
    recalculate(&mut wb);
    let text = |row: u32| match value_at(&wb, row, 0) {
        CellValue::SharedString(id) | CellValue::InlineString(id) => {
            wb.strings.get(id).unwrap().to_owned()
        }
        other => panic!("expected text, got {other:?}"),
    };
    assert_eq!(text(0), "$C$2");
    assert_eq!(text(1), "C2");
    assert_eq!(value_at(&wb, 2, 0), CellValue::Number(5.0));
}

#[test]
fn char_and_code_differ_from_their_unicode_twins_by_range() {
    use casual_calc_model::ErrorValue;
    let text = |t: &str| -> String {
        let mut b = Builder::new();
        b.formula((0, 0), t);
        let mut wb = b.build();
        recalculate(&mut wb);
        match value_at(&wb, 0, 0) {
            CellValue::SharedString(id) | CellValue::InlineString(id) => {
                wb.strings.get(id).unwrap().to_owned()
            }
            other => panic!("{t} gave {other:?}"),
        }
    };
    assert_eq!(text("CHAR(65)"), "A");
    assert_eq!(text("UNICHAR(955)"), "λ");
    // CHAR stops at 255; accepting 955 and returning λ is what Excel refuses,
    // and is the only thing separating the two functions.
    assert_eq!(err("CHAR(955)"), ErrorValue::Value);
    assert_eq!(num("CODE(\"A\")"), 65.0);
    assert_eq!(num("UNICODE(\"λ\")"), 955.0);
    assert_eq!(err("UNICODE(\"\")"), ErrorValue::Value);
    // CODE is byte-oriented, so a character it cannot express is #VALUE!.
    assert_eq!(err("CODE(\"λ\")"), ErrorValue::Value);
}

#[test]
fn fixed_and_dollar_group_and_round() {
    let text = |t: &str| -> String {
        let mut b = Builder::new();
        b.formula((0, 0), t);
        let mut wb = b.build();
        recalculate(&mut wb);
        match value_at(&wb, 0, 0) {
            CellValue::SharedString(id) | CellValue::InlineString(id) => {
                wb.strings.get(id).unwrap().to_owned()
            }
            other => panic!("{t} gave {other:?}"),
        }
    };
    assert_eq!(text("FIXED(1234.567)"), "1,234.57");
    assert_eq!(text("FIXED(1234.567,1)"), "1,234.6");
    assert_eq!(text("FIXED(1234.567,1,TRUE())"), "1234.6");
    // A negative `decimals` rounds to the *left* of the point. Clamping it to
    // zero instead would quietly give "1,235" for this.
    assert_eq!(text("FIXED(1234.5,-2)"), "1,200");
    assert_eq!(text("DOLLAR(1234.5)"), "$1,234.50");
    // Negatives use parentheses, as the accounting format does.
    assert_eq!(text("DOLLAR(-1234.5)"), "($1,234.50)");
}

#[test]
fn numbervalue_takes_its_separators_rather_than_guessing() {
    use casual_calc_model::ErrorValue;
    // The point of the function: the caller states the separators instead of
    // the engine inferring a locale, so the same text parses the same way
    // everywhere.
    assert_eq!(num("NUMBERVALUE(\"1.234,56\",\",\",\".\")"), 1234.56);
    assert_eq!(num("NUMBERVALUE(\"1,234.56\")"), 1234.56);
    assert_eq!(num("NUMBERVALUE(\"50%\")"), 0.5);
    assert_eq!(err("NUMBERVALUE(\"abc\")"), ErrorValue::Value);
}

#[test]
fn t_passes_text_through_and_does_not_convert() {
    let mut b = Builder::new();
    b.text((0, 0), "hello")
        .number((1, 0), 42.0)
        .formula((2, 0), "T(A1)")
        .formula((3, 0), "T(A2)");
    let mut wb = b.build();
    recalculate(&mut wb);
    match value_at(&wb, 2, 0) {
        CellValue::SharedString(id) | CellValue::InlineString(id) => {
            assert_eq!(wb.strings.get(id).unwrap(), "hello");
        }
        other => panic!("expected text, got {other:?}"),
    }
    // A number gives empty text, not "42" — T does not convert, which is the
    // difference from TEXT.
    match value_at(&wb, 3, 0) {
        CellValue::SharedString(id) | CellValue::InlineString(id) => {
            assert_eq!(wb.strings.get(id).unwrap(), "");
        }
        other => panic!("expected empty text, got {other:?}"),
    }
}

/// A sheet with a small sample in A1:A9 and a paired one in B1:B9.
fn sample_workbook() -> Builder {
    let xs = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0, 10.0];
    let ys = [1.0, 3.0, 2.0, 5.0, 4.0, 6.0, 6.0, 9.0, 8.0];
    let mut b = Builder::new();
    for (i, (x, y)) in xs.iter().zip(ys).enumerate() {
        b.number((i as u32, 0), *x).number((i as u32, 1), y);
    }
    b
}

#[test]
fn variance_divisor_separates_var_from_varp() {
    let mut b = sample_workbook();
    b.formula((0, 3), "VAR(A1:A9)")
        .formula((1, 3), "VARP(A1:A9)")
        .formula((2, 3), "DEVSQ(A1:A9)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 3) {
        CellValue::Number(v) => v,
        other => panic!("{other:?}"),
    };
    // Mean 5.5556 over nine values; sum of squared deviations 54.2222.
    assert!((n(2) - 54.222_222_222_222_23).abs() < 1e-9);
    // The divisor is the entire difference between the two, and the wrong one
    // is close enough to pass a glance on any large sample.
    assert!(
        (n(0) - 54.222_222_222_222_23 / 8.0).abs() < 1e-9,
        "VAR uses n-1"
    );
    assert!(
        (n(1) - 54.222_222_222_222_23 / 9.0).abs() < 1e-9,
        "VARP uses n"
    );
}

#[test]
fn descriptive_statistics() {
    let mut b = sample_workbook();
    b.formula((0, 3), "AVEDEV(A1:A9)")
        .formula((1, 3), "GEOMEAN(A1:A9)")
        .formula((2, 3), "HARMEAN(A1:A9)")
        .formula((3, 3), "MODE(A1:A9)")
        .formula((4, 3), "MEDIAN(A1:A9)")
        .formula((5, 3), "PERCENTILE(A1:A9,0.5)")
        .formula((6, 3), "QUARTILE(A1:A9,2)")
        .formula((7, 3), "TRIMMEAN(A1:A9,0.4)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 3) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    assert!((n(0) - 2.074_074_074_074_074).abs() < 1e-9, "AVEDEV");
    assert!((n(1) - 5.017_633_630_614_796).abs() < 1e-9, "GEOMEAN");
    assert!((n(2) - 4.491_089_108_910_891).abs() < 1e-9, "HARMEAN");
    assert_eq!(n(3), 4.0, "MODE is the most frequent value");
    // The median, the 50th percentile and the second quartile are one number
    // reached three ways; disagreement means the interpolation is wrong.
    assert_eq!(n(4), 5.0);
    assert_eq!(n(5), 5.0);
    assert_eq!(n(6), 5.0);
    assert!(n(7).is_finite());
}

#[test]
fn mode_of_all_distinct_values_is_an_error() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.number((0, 0), 1.0)
        .number((1, 0), 2.0)
        .number((2, 0), 3.0)
        .formula((3, 0), "MODE(A1:A3)");
    let mut wb = b.build();
    recalculate(&mut wb);
    // Excel reports an error rather than picking the first value, which would
    // look like a real mode.
    assert!(matches!(
        value_at(&wb, 3, 0),
        CellValue::Error(ErrorValue::Num | ErrorValue::Na)
    ));
}

#[test]
fn regression_takes_y_before_x() {
    let mut b = sample_workbook();
    // A perfect line through B: y = 2x + 1, so the slope and intercept are
    // exact and an argument-order mistake is unmissable.
    for i in 0..9u32 {
        b.number((i, 4), i as f64)
            .number((i, 5), 2.0 * i as f64 + 1.0);
    }
    b.formula((0, 3), "SLOPE(F1:F9,E1:E9)")
        .formula((1, 3), "INTERCEPT(F1:F9,E1:E9)")
        .formula((2, 3), "RSQ(F1:F9,E1:E9)")
        .formula((3, 3), "FORECAST(10,F1:F9,E1:E9)")
        .formula((4, 3), "CORREL(A1:A9,B1:B9)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 3) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    assert!((n(0) - 2.0).abs() < 1e-9, "slope");
    assert!((n(1) - 1.0).abs() < 1e-9, "intercept");
    assert!((n(2) - 1.0).abs() < 1e-9, "a perfect fit is r^2 = 1");
    assert!((n(3) - 21.0).abs() < 1e-9, "forecast at x = 10");
    assert!(n(4) > 0.9 && n(4) < 1.0);
}

#[test]
fn mismatched_paired_ranges_are_na_not_truncated() {
    use casual_calc_model::ErrorValue;
    let mut b = Builder::new();
    b.number((0, 0), 1.0)
        .number((1, 0), 2.0)
        .number((2, 0), 3.0)
        .number((0, 1), 1.0)
        .number((1, 1), 2.0)
        .formula((4, 0), "CORREL(A1:A3,B1:B2)");
    let mut wb = b.build();
    recalculate(&mut wb);
    // Zipping to the shorter range would silently answer over part of the data.
    assert_eq!(value_at(&wb, 4, 0), CellValue::Error(ErrorValue::Na));
}

#[test]
fn normal_distribution_and_its_inverse_agree() {
    // The inverse is a rational approximation refined by a Halley step; if the
    // refinement were dropped this round trip would drift around 1e-9.
    let mut b = Builder::new();
    b.formula((0, 0), "NORMSDIST(1.96)")
        .formula((1, 0), "NORMSINV(0.975)")
        .formula((2, 0), "NORMDIST(0,0,1,TRUE())")
        .formula((3, 0), "NORMINV(NORMSDIST(1.2345),0,1)")
        .formula((4, 0), "GAMMALN(5)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    // The true value is 0.97500210…, not 0.975 — the difference is what a
    // tolerance chosen from memory rather than computed would have hidden.
    assert!((n(0) - 0.975_002_104_851_78).abs() < 1e-6);
    assert!((n(1) - 1.96).abs() < 1e-4);
    assert!((n(2) - 0.5).abs() < 1e-12);
    assert!(
        (n(3) - 1.2345).abs() < 1e-6,
        "round trip through the inverse"
    );
    // ln(4!) = ln 24.
    assert!((n(4) - 24.0f64.ln()).abs() < 1e-9);
}

#[test]
fn discrete_distributions_sum_in_log_space() {
    // m^k / k! and C(n,k) both overflow f64 long before the probability itself
    // becomes unrepresentable, so the terms are computed via ln-gamma.
    let mut b = Builder::new();
    b.formula((0, 0), "POISSON(2,3,FALSE())")
        .formula((1, 0), "POISSON(200,300,TRUE())")
        .formula((2, 0), "BINOMDIST(3,10,0.5,FALSE())")
        .formula((3, 0), "BINOMDIST(500,1000,0.5,TRUE())")
        .formula((4, 0), "EXPONDIST(1,1,TRUE())");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    assert!((n(0) - (9.0 * (-3.0f64).exp() / 2.0)).abs() < 1e-12);
    // A naive implementation returns NaN here rather than a probability.
    assert!(n(1) > 0.0 && n(1) < 1.0, "large Poisson stays finite");
    assert!((n(2) - 120.0 * 0.5f64.powi(10)).abs() < 1e-12);
    assert!((n(3) - 0.5).abs() < 0.02, "large binomial stays finite");
    assert!((n(4) - (1.0 - (-1.0f64).exp())).abs() < 1e-12);
}

#[test]
fn the_a_variants_count_text_as_zero() {
    let mut b = Builder::new();
    b.number((0, 0), 10.0)
        .text((1, 0), "n/a")
        .number((2, 0), 20.0)
        .formula((0, 2), "AVERAGE(A1:A3)")
        .formula((1, 2), "AVERAGEA(A1:A3)")
        .formula((2, 2), "MAXA(A1:A3)")
        .formula((3, 2), "MINA(A1:A3)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 2) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    // AVERAGE skips the text entirely; AVERAGEA counts it as zero and so
    // reports a different, smaller number. That difference is the only reason
    // both functions exist.
    assert_eq!(n(0), 15.0);
    assert!((n(1) - 10.0).abs() < 1e-12);
    assert_eq!(n(2), 20.0);
    assert_eq!(n(3), 0.0, "MINA sees the text as zero");
}

#[test]
fn remaining_distributions() {
    let mut b = Builder::new();
    b.formula((0, 0), "WEIBULL(2,1,1,TRUE())")
        .formula((1, 0), "LOGNORMDIST(1,0,1)")
        .formula((2, 0), "LOGINV(0.5,0,1)")
        .formula((3, 0), "HYPGEOMDIST(1,4,8,20)")
        .formula((4, 0), "NEGBINOMDIST(10,5,0.25)")
        .formula((5, 0), "CRITBINOM(6,0.5,0.75)")
        .formula((6, 0), "CONFIDENCE(0.05,2.5,50)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    // Weibull with alpha = beta = 1 is the exponential.
    assert!((n(0) - (1.0 - (-2.0f64).exp())).abs() < 1e-12);
    // LOGNORMDIST(1, 0, 1) is the normal CDF at ln(1) = 0.
    assert!((n(1) - 0.5).abs() < 1e-12);
    // ...and its inverse at p = 0.5 is exp(0) = 1.
    assert!((n(2) - 1.0).abs() < 1e-9);
    assert!(n(3) > 0.0 && n(3) < 1.0);
    assert!(n(4) > 0.0 && n(4) < 1.0);
    assert!(n(5) >= 0.0 && n(5) <= 6.0);
    assert!((n(6) - 1.959_963_984_540_054 * 2.5 / 50.0f64.sqrt()).abs() < 1e-6);
}

#[test]
fn base_conversion_uses_twos_complement_for_negatives() {
    let text = |t: &str| -> String {
        let mut b = Builder::new();
        b.formula((0, 0), t);
        let mut wb = b.build();
        recalculate(&mut wb);
        match value_at(&wb, 0, 0) {
            CellValue::SharedString(id) | CellValue::InlineString(id) => {
                wb.strings.get(id).unwrap().to_owned()
            }
            other => panic!("{t} gave {other:?}"),
        }
    };
    assert_eq!(text("DEC2BIN(9)"), "1001");
    assert_eq!(text("DEC2BIN(9,8)"), "00001001");
    assert_eq!(text("DEC2HEX(255)"), "FF");
    assert_eq!(text("DEC2OCT(8)"), "10");
    // A ten-digit value with the top digit set is negative. Parsing it as an
    // unsigned integer gives 1023 — a plausible-looking wrong answer, and the
    // single most likely mistake in these functions.
    assert_eq!(num("BIN2DEC(\"1111111111\")"), -1.0);
    assert_eq!(text("DEC2BIN(-1)"), "1111111111");
    assert_eq!(num("HEX2DEC(\"FFFFFFFFFF\")"), -1.0);
    assert_eq!(num("OCT2DEC(\"7777777777\")"), -1.0);
    assert_eq!(text("BIN2HEX(\"1111111111\")"), "FFFFFFFFFF");
    assert_eq!(num("BIN2DEC(\"1001\")"), 9.0);
}

#[test]
fn places_cannot_truncate_a_value() {
    use casual_calc_model::ErrorValue;
    // Asking for fewer digits than the value needs is an error, not a silent
    // truncation that would change the number.
    assert_eq!(err("DEC2BIN(255,4)"), ErrorValue::Num);
    assert_eq!(err("DEC2BIN(1024)"), ErrorValue::Num);
}

#[test]
fn bit_operations_and_step_functions() {
    use casual_calc_model::ErrorValue;
    assert_eq!(num("BITAND(12,10)"), 8.0);
    assert_eq!(num("BITOR(12,10)"), 14.0);
    assert_eq!(num("BITXOR(12,10)"), 6.0);
    assert_eq!(num("BITLSHIFT(3,2)"), 12.0);
    assert_eq!(num("BITRSHIFT(12,2)"), 3.0);
    // A negative shift reverses direction, which is why the two share a body.
    assert_eq!(num("BITLSHIFT(12,-2)"), 3.0);
    // Defined only on non-negative integers.
    assert_eq!(err("BITAND(-1,1)"), ErrorValue::Num);
    assert_eq!(num("DELTA(5,5)"), 1.0);
    assert_eq!(num("DELTA(5,4)"), 0.0);
    assert_eq!(num("GESTEP(5,4)"), 1.0);
    assert_eq!(num("GESTEP(3,4)"), 0.0);
}

#[test]
fn erf_and_erfc_are_complementary() {
    // ERF(x) + ERFC(x) = 1 by definition, which holds regardless of the
    // approximation's accuracy — a stronger check than a recalled constant.
    for x in ["0.5", "1", "2"] {
        let sum = num(&format!("ERF({x})")) + num(&format!("ERFC({x})"));
        assert!((sum - 1.0).abs() < 1e-9, "ERF+ERFC at {x}");
    }
    // The two-argument form is the integral between bounds.
    assert!((num("ERF(0,1)") - num("ERF(1)")).abs() < 1e-12);
}

#[test]
fn the_annuity_family_is_one_equation_rearranged() {
    // A 30-year loan at 6% nominal: PMT, then PV and FV recovered from it. If
    // any of the three had a sign or factor wrong these would not agree.
    let mut b = Builder::new();
    b.formula((0, 0), "PMT(0.06/12,360,200000)")
        .formula((1, 0), "PV(0.06/12,360,A1)")
        .formula((2, 0), "FV(0.06/12,360,A1,200000)")
        .formula((3, 0), "NPER(0.06/12,A1,200000)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    // A borrower pays out, so the payment is negative.
    assert!(n(0) < 0.0, "PMT is a cash outflow");
    assert!((n(0) + 1_199.101_050_305_53).abs() < 1e-6);
    // PV of that payment stream is the loan back again.
    assert!((n(1) - 200_000.0).abs() < 1e-6);
    // ...and the loan is fully repaid, so the final balance is zero.
    assert!(n(2).abs() < 1e-6);
    assert!((n(3) - 360.0).abs() < 1e-6);
}

#[test]
fn a_zero_rate_annuity_is_a_limit_not_an_error() {
    // An interest-free loan is an ordinary thing to model, and the annuity
    // factor's (1+r)^n - 1 over r is 0/0 there. Rejecting it would make PMT
    // fail on the simplest case anyone tries.
    assert!((num("PMT(0,10,1000)") + 100.0).abs() < 1e-9);
    assert!((num("FV(0,10,-100)") - 1000.0).abs() < 1e-9);
    assert!((num("NPER(0,-100,1000)") - 10.0).abs() < 1e-9);
}

#[test]
fn ipmt_and_ppmt_split_the_payment() {
    let mut b = Builder::new();
    b.formula((0, 0), "PMT(0.05,10,10000)")
        .formula((1, 0), "IPMT(0.05,1,10,10000)")
        .formula((2, 0), "PPMT(0.05,1,10,10000)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    // The first period's interest is simply the rate on the whole balance.
    assert!((n(1) + 500.0).abs() < 1e-9);
    // Interest plus principal is the payment, by definition.
    assert!((n(1) + n(2) - n(0)).abs() < 1e-9);
}

#[test]
fn irr_and_npv_are_inverse() {
    let mut b = Builder::new();
    for (i, v) in [-1000.0, 300.0, 400.0, 500.0].iter().enumerate() {
        b.number((i as u32, 0), *v);
    }
    b.formula((0, 2), "IRR(A1:A4)")
        // NPV discounts the first flow by one period, so the initial outlay is
        // added outside it — the classic shape, and the reason NPV alone does
        // not equal zero at the IRR.
        .formula((1, 2), "A1+NPV(C1,A2:A4)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 2) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    assert!(n(0) > 0.0 && n(0) < 1.0);
    assert!(n(1).abs() < 1e-6, "NPV at the IRR is zero");
}

#[test]
fn depreciation_methods_all_exhaust_the_depreciable_base() {
    let mut b = Builder::new();
    b.formula((0, 0), "SLN(10000,1000,5)")
        .formula((1, 0), "SYD(10000,1000,5,1)")
        .formula((2, 0), "DDB(10000,1000,5,1)")
        .formula((3, 0), "SYD(10000,1000,5,1)+SYD(10000,1000,5,2)+SYD(10000,1000,5,3)+SYD(10000,1000,5,4)+SYD(10000,1000,5,5)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    assert!((n(0) - 1800.0).abs() < 1e-9);
    // First year of sum-of-years'-digits: 5/15 of the base.
    assert!((n(1) - 3000.0).abs() < 1e-9);
    assert!((n(2) - 4000.0).abs() < 1e-9);
    // Every method must depreciate exactly cost - salvage over the full life.
    assert!((n(3) - 9000.0).abs() < 1e-9);
}

#[test]
fn effect_and_nominal_are_inverses() {
    // Holds regardless of the constants, which is a stronger check than a
    // recalled figure.
    assert!((num("NOMINAL(EFFECT(0.08,12),12)") - 0.08).abs() < 1e-12);
    assert!((num("EFFECT(0.08,12)") - 0.083).abs() < 0.001);
    assert!((num("RRI(10,1000,2000)") - 2f64.powf(0.1) + 1.0).abs() < 1e-12);
}

#[test]
fn dollarde_reads_the_fraction_in_its_own_base() {
    // 1.02 at sixteenths is 1 + 2/16 = 1.125, not 1.02. The scale is set by the
    // digit count of the denominator, not by the denominator itself.
    assert!((num("DOLLARDE(1.02,16)") - 1.125).abs() < 1e-12);
    assert!((num("DOLLARFR(1.125,16)") - 1.02).abs() < 1e-12);
}

#[test]
fn complex_numbers_are_text_and_keep_their_suffix() {
    let text = |t: &str| -> String {
        let mut b = Builder::new();
        b.formula((0, 0), t);
        let mut wb = b.build();
        recalculate(&mut wb);
        match value_at(&wb, 0, 0) {
            CellValue::SharedString(id) | CellValue::InlineString(id) => {
                wb.strings.get(id).unwrap().to_owned()
            }
            other => panic!("{t} gave {other:?}"),
        }
    };
    assert_eq!(text("COMPLEX(3,4)"), "3+4i");
    assert_eq!(text("COMPLEX(3,-4)"), "3-4i");
    assert_eq!(text("COMPLEX(0,1)"), "i", "a unit coefficient is a bare i");
    assert_eq!(text("COMPLEX(5,0)"), "5", "no imaginary part, no suffix");
    // A workbook written in `j` must not come back in `i`: the suffix is part
    // of the value, and the first argument's wins for a fold.
    assert_eq!(text("COMPLEX(1,2,\"j\")"), "1+2j");
    assert_eq!(text("IMSUM(COMPLEX(1,2,\"j\"),COMPLEX(3,4,\"j\"))"), "4+6j");
    assert_eq!(text("IMCONJUGATE(\"3+4i\")"), "3-4i");
    assert_eq!(text("IMPRODUCT(\"3+4i\",\"1+2i\")"), "-5+10i");
    assert_eq!(text("IMSUB(\"5+6i\",\"2+2i\")"), "3+4i");
}

#[test]
fn complex_parsing_handles_bare_and_exponent_forms() {
    // "3+i" is 3 + 1i, not a parse failure — a bare sign is a coefficient of
    // one, and Excel writes it that way.
    assert_eq!(num("IMAGINARY(\"3+i\")"), 1.0);
    assert_eq!(num("IMAGINARY(\"3-i\")"), -1.0);
    assert_eq!(num("IMREAL(\"3+i\")"), 3.0);
    // The split must skip an exponent's sign, or `1e-3+2i` splits in the wrong
    // place and parses as nonsense.
    assert!((num("IMREAL(\"1e-3+2i\")") - 0.001).abs() < 1e-12);
    assert_eq!(num("IMAGINARY(\"1e-3+2i\")"), 2.0);
    assert_eq!(num("IMREAL(\"7\")"), 7.0);
    assert_eq!(num("IMAGINARY(\"i\")"), 1.0);
}

#[test]
fn complex_identities_hold() {
    // Checked against identities rather than recalled constants: |3+4i| = 5,
    // e^(i·pi) = -1, and sqrt(z)^2 = z.
    assert!((num("IMABS(\"3+4i\")") - 5.0).abs() < 1e-12);
    let mut b = Builder::new();
    b.formula((0, 0), "IMREAL(IMEXP(COMPLEX(0,PI())))")
        .formula((1, 0), "IMREAL(IMPOWER(IMSQRT(\"3+4i\"),2))")
        .formula((2, 0), "IMAGINARY(IMPOWER(IMSQRT(\"3+4i\"),2))")
        .formula(
            (3, 0),
            "IMREAL(IMDIV(IMPRODUCT(\"3+4i\",\"1+2i\"),\"1+2i\"))",
        );
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    assert!((n(0) + 1.0).abs() < 1e-12, "e^(i*pi) = -1");
    assert!((n(1) - 3.0).abs() < 1e-9, "sqrt(z)^2 real part");
    assert!((n(2) - 4.0).abs() < 1e-9, "sqrt(z)^2 imaginary part");
    assert!((n(3) - 3.0).abs() < 1e-9, "(z*w)/w = z");
}

#[test]
fn distributions_and_their_inverses_round_trip() {
    // Each pair is checked against the other rather than against recalled
    // table values: a shared error in the underlying incomplete gamma or beta
    // would cancel in a round trip, so these also assert one known point each.
    let mut b = Builder::new();
    b.formula((0, 0), "CHIINV(CHIDIST(3.5,4),4)")
        .formula((1, 0), "TINV(TDIST(2.1,10,2),10)")
        .formula((2, 0), "FINV(FDIST(2.5,3,8),3,8)")
        .formula((3, 0), "GAMMAINV(GAMMADIST(4,2,3,TRUE()),2,3)")
        .formula((4, 0), "BETAINV(BETADIST(0.3,2,5),2,5)")
        // Known points: chi-square with 2 df is exponential, so the upper tail
        // at x is exp(-x/2); the t distribution with huge df is normal.
        .formula((5, 0), "CHIDIST(2,2)")
        .formula((6, 0), "TDIST(1.96,100000,2)")
        .formula((7, 0), "BETADIST(0.5,1,1)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    assert!((n(0) - 3.5).abs() < 1e-6, "CHIINV(CHIDIST(x)) = x");
    assert!((n(1) - 2.1).abs() < 1e-6, "TINV(TDIST(x)) = x");
    assert!((n(2) - 2.5).abs() < 1e-6, "FINV(FDIST(x)) = x");
    assert!((n(3) - 4.0).abs() < 1e-6, "GAMMAINV(GAMMADIST(x)) = x");
    assert!((n(4) - 0.3).abs() < 1e-6, "BETAINV(BETADIST(x)) = x");
    // Chi-square with 2 degrees of freedom is the exponential: Q(x) = e^(-x/2).
    assert!((n(5) - (-1.0f64).exp()).abs() < 1e-9);
    // With enormous df the t distribution is the normal one, so the two-tailed
    // probability at 1.96 is about 0.05.
    assert!((n(6) - 0.05).abs() < 1e-3);
    // Beta(1,1) is uniform.
    assert!((n(7) - 0.5).abs() < 1e-12);
}

#[test]
fn chidist_and_fdist_are_upper_tail() {
    // Unlike almost every other *DIST, these report the upper tail. Returning
    // the CDF instead gives 1 - the right answer, which is a plausible-looking
    // probability and wrong every time.
    let mut b = Builder::new();
    b.formula((0, 0), "CHIDIST(0,4)")
        .formula((1, 0), "FDIST(0,3,8)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("{other:?}"),
    };
    assert!((n(0) - 1.0).abs() < 1e-12, "the whole mass is above zero");
    assert!((n(1) - 1.0).abs() < 1e-12);
}

#[test]
fn subtotal_100_series_skips_hidden_rows() {
    // The whole point of SUBTOTAL: a filtered list must not report a total that
    // includes what is hidden. 9 counts everything, 109 counts what is visible.
    let mut b = Builder::new();
    for (i, v) in [10.0, 20.0, 30.0, 40.0].iter().enumerate() {
        b.number((i as u32, 0), *v);
    }
    b.formula((5, 0), "SUBTOTAL(9,A1:A4)")
        .formula((6, 0), "SUBTOTAL(109,A1:A4)")
        .formula((7, 0), "SUBTOTAL(1,A1:A4)")
        .formula((8, 0), "SUBTOTAL(4,A1:A4)");
    let mut wb = b.build();
    wb.sheets[0].hidden_rows.insert(1); // hide the 20
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    assert_eq!(n(5), 100.0, "9 includes hidden rows");
    assert_eq!(n(6), 80.0, "109 excludes them");
    assert_eq!(n(7), 25.0, "average over all four");
    assert_eq!(n(8), 40.0, "max");
}

#[test]
fn statistical_tests() {
    let mut b = Builder::new();
    for (i, v) in [3.0, 4.0, 5.0, 6.0, 7.0].iter().enumerate() {
        b.number((i as u32, 0), *v);
    }
    // Deliberately not a constant offset from column A: a paired t-test on
    // differences that never vary has zero variance and is genuinely
    // undefined, so uniform data would test the #DIV/0! path rather than the
    // statistic.
    for (i, v) in [5.0, 7.0, 6.0, 9.0, 8.0].iter().enumerate() {
        b.number((i as u32, 1), *v);
    }
    b.formula((0, 3), "TTEST(A1:A5,B1:B5,2,2)")
        .formula((1, 3), "TTEST(A1:A5,B1:B5,2,1)")
        .formula((2, 3), "FTEST(A1:A5,B1:B5)")
        .formula((3, 3), "ZTEST(A1:A5,5)")
        .formula((4, 3), "CHITEST(A1:A5,B1:B5)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 3) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    for r in 0..5 {
        assert!((0.0..=1.0).contains(&n(r)), "row {r} is a probability");
    }
    // Both samples have the same spread here, so the F ratio is 1 and its
    // two-tailed probability is 1.
    assert!((n(2) - 1.0).abs() < 1e-9);
    // The sample mean is exactly the tested value, so ZTEST is one half.
    assert!((n(3) - 0.5).abs() < 1e-9);
}

#[test]
fn roman_and_ceiling_variants() {
    use casual_calc_model::ErrorValue;
    let text = |t: &str| -> String {
        let mut b = Builder::new();
        b.formula((0, 0), t);
        let mut wb = b.build();
        recalculate(&mut wb);
        match value_at(&wb, 0, 0) {
            CellValue::SharedString(id) | CellValue::InlineString(id) => {
                wb.strings.get(id).unwrap().to_owned()
            }
            other => panic!("{t} gave {other:?}"),
        }
    };
    assert_eq!(text("ROMAN(1994)"), "MCMXCIV");
    assert_eq!(text("ROMAN(4)"), "IV");
    assert_eq!(text("ROMAN(3999)"), "MMMCMXCIX");
    assert_eq!(err("ROMAN(4000)"), ErrorValue::Value);
    // The concise forms are not modelled, so a non-zero form is refused rather
    // than silently answered in the classic one.
    assert_eq!(err("ROMAN(1994,1)"), ErrorValue::Value);
    // The two ceilings agree on positives and differ on negatives: ISO rounds
    // toward positive infinity, ECMA away from zero.
    assert_eq!(num("ISO.CEILING(4.2,1)"), 5.0);
    assert_eq!(num("ECMA.CEILING(4.2,1)"), 5.0);
    assert_eq!(num("ISO.CEILING(-4.2,1)"), -4.0);
    assert_eq!(num("ECMA.CEILING(-4.2,1)"), -5.0);
}

#[test]
fn cumulative_payments_sum_to_the_loan() {
    // CUMIPMT and CUMPRINC over the whole term must account for every payment:
    // the principal repaid is the loan, and the two together are the payments.
    let mut b = Builder::new();
    b.formula((0, 0), "CUMPRINC(0.05,10,10000,1,10,0)")
        .formula((1, 0), "CUMIPMT(0.05,10,10000,1,10,0)")
        .formula((2, 0), "PMT(0.05,10,10000)*10");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    assert!(
        (n(0) + 10_000.0).abs() < 1e-6,
        "principal repaid is the loan"
    );
    assert!(
        (n(0) + n(1) - n(2)).abs() < 1e-6,
        "principal + interest = payments"
    );
}

#[test]
fn treasury_bill_functions_invert() {
    let mut b = Builder::new();
    b.formula((0, 0), "DATE(2024,1,1)")
        .formula((1, 0), "DATE(2024,7,1)")
        .formula((2, 0), "TBILLPRICE(A1,A2,0.05)")
        .formula((3, 0), "TBILLYIELD(A1,A2,A3)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    assert!(n(2) > 90.0 && n(2) < 100.0, "a bill trades below par");
    // The yield implied by that price is close to, and above, the discount —
    // which is the relationship between the two quoting conventions.
    assert!(n(3) > 0.05 && n(3) < 0.06);
}

/// `=SUM(A:A)` is one of the commonest formulas there is, and it used to be
/// `#NAME?` — the parser could not read a whole-column reference at all.
#[test]
fn whole_column_and_whole_row_references_evaluate() {
    let mut b = Builder::new();
    b.number((0, 0), 1.0) // A1
        .number((1, 0), 2.0) // A2
        .number((2, 0), 3.0) // A3
        .number((0, 1), 10.0) // B1
        .formula((0, 3), "SUM(A:A)") // D1
        .formula((1, 3), "COUNT(A:A)") // D2
        .formula((2, 3), "SUM($1:$1)"); // D3 — row 1 across
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 0, 3), CellValue::Number(6.0));
    assert_eq!(value_at(&wb, 1, 3), CellValue::Number(3.0));
    // Row 1 holds A1 (1) and B1 (10). D1 is a formula on the same row, so its
    // own value is excluded only by the self-reference guard, not by us — the
    // assertion is on the data cells.
    assert!(
        matches!(value_at(&wb, 2, 3), CellValue::Number(n) if n >= 11.0),
        "row range saw A1 and B1: {:?}",
        value_at(&wb, 2, 3)
    );
}

/// The bounds an open range walks come from the data, not from the sheet's
/// limits. Without the clamp this test would iterate 1,048,576 rows per
/// formula and take minutes rather than milliseconds.
#[test]
fn an_open_range_costs_the_data_not_the_sheet() {
    let mut b = Builder::new();
    b.number((0, 0), 1.0);
    for i in 0..200u32 {
        b.formula((i, 4), "SUM(A:A)");
    }
    let mut wb = b.build();
    let start = std::time::Instant::now();
    recalculate(&mut wb);
    let elapsed = start.elapsed();
    assert_eq!(value_at(&wb, 0, 4), CellValue::Number(1.0));
    // Generous by three orders of magnitude against the unclamped cost; this is
    // a cliff detector, not a benchmark.
    assert!(
        elapsed.as_secs() < 5,
        "200 open-range formulas took {elapsed:?} — the clamp is not being applied"
    );
}

/// A whole-column range grows with the sheet, so a dependency span frozen at
/// today's extent would go stale the moment a cell appears below it.
#[test]
fn an_open_range_recalculates_when_the_column_grows() {
    let mut b = Builder::new();
    b.number((0, 0), 1.0).formula((0, 3), "SUM(A:A)");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 0, 3), CellValue::Number(1.0));

    wb.sheets[0]
        .cells
        .set(CellRef::new(50, 0), Cell::value(CellValue::Number(41.0)));
    recalculate(&mut wb);
    assert_eq!(
        value_at(&wb, 0, 3),
        CellValue::Number(42.0),
        "a cell added below the old extent must still be counted"
    );
}

/// The `D` functions, and the criteria rules that make them what they are:
/// conditions across a row are AND, rows are OR, and an empty criteria cell is
/// not a condition at all.
#[test]
fn database_functions_aggregate_matching_rows() {
    let mut b = Builder::new();
    // A1:C5 — the table.
    b.text((0, 0), "Region")
        .text((0, 1), "Rep")
        .text((0, 2), "Sales");
    b.text((1, 0), "West")
        .text((1, 1), "Ann")
        .number((1, 2), 100.0);
    b.text((2, 0), "East")
        .text((2, 1), "Bob")
        .number((2, 2), 200.0);
    b.text((3, 0), "West")
        .text((3, 1), "Cid")
        .number((3, 2), 300.0);
    b.text((4, 0), "North")
        .text((4, 1), "Ann")
        .number((4, 2), 50.0);
    // E1:F3 — criteria: Region=West AND Sales>150, OR Rep=Ann.
    b.text((0, 4), "Region").text((0, 5), "Sales");
    b.text((1, 4), "West").text((1, 5), ">150");
    b.text((2, 4), "North").text((2, 5), "");

    b.formula((0, 7), "DSUM(A1:C5,\"Sales\",E1:F3)"); // 300 + 50
    b.formula((1, 7), "DCOUNT(A1:C5,\"Sales\",E1:F3)");
    b.formula((2, 7), "DMAX(A1:C5,3,E1:F3)"); // field by 1-based index
    b.formula((3, 7), "DMIN(A1:C5,\"Sales\",E1:F3)");
    b.formula((4, 7), "DAVERAGE(A1:C5,\"Sales\",E1:F3)");
    let mut wb = b.build();
    recalculate(&mut wb);

    assert_eq!(value_at(&wb, 0, 7), CellValue::Number(350.0));
    assert_eq!(value_at(&wb, 1, 7), CellValue::Number(2.0));
    assert_eq!(value_at(&wb, 2, 7), CellValue::Number(300.0));
    assert_eq!(value_at(&wb, 3, 7), CellValue::Number(50.0));
    assert_eq!(value_at(&wb, 4, 7), CellValue::Number(175.0));
}

/// `DGET` answers with the row it found — and refuses to guess when there is
/// not exactly one. Returning the first of several would be a plausible wrong
/// answer, which is the worst kind.
#[test]
fn dget_refuses_to_guess() {
    let mut b = Builder::new();
    b.text((0, 0), "Rep").text((0, 1), "Sales");
    b.text((1, 0), "Ann").number((1, 1), 100.0);
    b.text((2, 0), "Bob").number((2, 1), 200.0);
    b.text((3, 0), "Ann").number((3, 1), 300.0);
    b.text((0, 3), "Rep");
    b.text((1, 3), "Bob");
    b.text((0, 5), "Rep");
    b.text((1, 5), "Ann");
    b.text((0, 7), "Rep");
    b.text((1, 7), "Zoe");

    b.formula((0, 9), "DGET(A1:B4,\"Sales\",D1:D2)"); // exactly one
    b.formula((1, 9), "DGET(A1:B4,\"Sales\",F1:F2)"); // two → #NUM!
    b.formula((2, 9), "DGET(A1:B4,\"Sales\",H1:H2)"); // none → #VALUE!
    let mut wb = b.build();
    recalculate(&mut wb);

    assert_eq!(value_at(&wb, 0, 9), CellValue::Number(200.0));
    assert_eq!(
        value_at(&wb, 1, 9),
        CellValue::Error(casual_calc_model::ErrorValue::Num)
    );
    assert_eq!(
        value_at(&wb, 2, 9),
        CellValue::Error(casual_calc_model::ErrorValue::Value)
    );
}

/// `DCOUNT` counts numbers and `DCOUNTA` counts anything present — the same
/// distinction as COUNT and COUNTA. Swapping them silently changes a total.
#[test]
fn dcount_and_dcounta_differ_on_text() {
    let mut b = Builder::new();
    b.text((0, 0), "Rep").text((0, 1), "Sales");
    b.text((1, 0), "Ann").number((1, 1), 100.0);
    b.text((2, 0), "Ann").text((2, 1), "pending");
    b.text((0, 3), "Rep");
    b.text((1, 3), "Ann");
    b.formula((0, 5), "DCOUNT(A1:B3,\"Sales\",D1:D2)");
    b.formula((1, 5), "DCOUNTA(A1:B3,\"Sales\",D1:D2)");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 0, 5), CellValue::Number(1.0));
    assert_eq!(value_at(&wb, 1, 5), CellValue::Number(2.0));
}

/// The coupon schedule counts **back from maturity**, which is what keeps a
/// bond's payments on the day of the month it matures. Stepping forward from an
/// assumed start puts every date a few days out whenever month lengths differ.
#[test]
fn coupon_dates_count_back_from_maturity() {
    let mut b = Builder::new();
    // Settlement 2024-03-15, maturity 2026-11-30, semi-annual.
    b.formula((0, 0), "COUPPCD(DATE(2024,3,15),DATE(2026,11,30),2)");
    b.formula((1, 0), "COUPNCD(DATE(2024,3,15),DATE(2026,11,30),2)");
    b.formula((2, 0), "COUPNUM(DATE(2024,3,15),DATE(2026,11,30),2)");
    // A maturity on the 31st keeps the 31st where the month has one.
    b.formula((3, 0), "COUPPCD(DATE(2024,4,15),DATE(2025,1,31),2)");
    b.formula((4, 0), "COUPNCD(DATE(2024,4,15),DATE(2025,1,31),2)");
    let mut wb = b.build();
    recalculate(&mut wb);

    let ymd = |wb: &Workbook, r: u32| {
        let CellValue::Number(n) = value_at(wb, r, 0) else {
            panic!("expected a serial at row {r}: {:?}", value_at(wb, r, 0));
        };
        n as i64
    };
    let date = |y: i64, m: i64, d: i64| crate::functions::ymd_to_serial_for_test(y, m, d);
    // Coupons on 30 May and 30 November; settlement in March sits between them.
    assert_eq!(ymd(&wb, 0), date(2023, 11, 30));
    assert_eq!(ymd(&wb, 1), date(2024, 5, 30));
    // Nov-2023 → Nov-2026 inclusive of the period ending at the next coupon.
    assert_eq!(value_at(&wb, 2, 0), CellValue::Number(6.0));
    // 31 Jan maturity: the previous coupon is 31 July, the next 31 January.
    assert_eq!(ymd(&wb, 3), date(2024, 1, 31));
    assert_eq!(ymd(&wb, 4), date(2024, 7, 31));
}

/// On a 30/360 basis a coupon period is 360/frequency days *by definition*, so
/// the parts must add up to it. Measuring the period from the calendar instead
/// makes COUPDAYS disagree with COUPDAYBS + COUPDAYSNC.
#[test]
fn coupon_day_counts_sum_to_the_period() {
    let mut b = Builder::new();
    for (i, basis) in [0, 1, 2, 3, 4].iter().enumerate() {
        let r = i as u32;
        b.formula(
            (r, 0),
            &format!("COUPDAYBS(DATE(2024,3,15),DATE(2026,11,30),2,{basis})"),
        );
        b.formula(
            (r, 1),
            &format!("COUPDAYSNC(DATE(2024,3,15),DATE(2026,11,30),2,{basis})"),
        );
        b.formula(
            (r, 2),
            &format!("COUPDAYS(DATE(2024,3,15),DATE(2026,11,30),2,{basis})"),
        );
    }
    let mut wb = b.build();
    recalculate(&mut wb);
    for (i, basis) in [0, 1, 2, 3, 4].iter().enumerate() {
        let r = i as u32;
        let num = |c: u32| match value_at(&wb, r, c) {
            CellValue::Number(n) => n,
            other => panic!("basis {basis}: {other:?}"),
        };
        // Bases 2 and 3 use a fixed year over the frequency, so their nominal
        // period is not the actual one and the parts need not sum — every other
        // basis must balance.
        if matches!(basis, 0 | 1 | 4) {
            assert!(
                (num(0) + num(1) - num(2)).abs() < 1e-9,
                "basis {basis}: {} + {} != {}",
                num(0),
                num(1),
                num(2)
            );
        }
        assert!(num(0) > 0.0 && num(1) > 0.0, "basis {basis} has real parts");
    }
}

/// PRICE and YIELD must be exact inverses — YIELD is solved against the very
/// function PRICE uses, which is the only way to guarantee that rather than
/// approximate it.
#[test]
fn price_and_yield_invert_each_other() {
    let mut b = Builder::new();
    b.formula(
        (0, 0),
        "PRICE(DATE(2024,2,15),DATE(2034,11,15),0.0575,0.065,100,2,0)",
    );
    b.formula(
        (1, 0),
        "YIELD(DATE(2024,2,15),DATE(2034,11,15),0.0575,A1,100,2,0)",
    );
    // A bond yielding its coupon prices at par, whatever the dates.
    b.formula(
        (2, 0),
        "PRICE(DATE(2024,1,1),DATE(2030,1,1),0.05,0.05,100,2,0)",
    );
    let mut wb = b.build();
    recalculate(&mut wb);

    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    // Priced below par because the yield exceeds the coupon.
    assert!(n(0) > 80.0 && n(0) < 100.0, "price {}", n(0));
    assert!((n(1) - 0.065).abs() < 1e-6, "yield round trip: {}", n(1));
    assert!(
        (n(2) - 100.0).abs() < 1e-6,
        "coupon == yield prices at par: {}",
        n(2)
    );
}

/// Modified duration is Macaulay discounted by one period's yield — it answers
/// "how much does the price move", not "when is the money".
#[test]
fn modified_duration_is_macaulay_over_one_plus_periodic_yield() {
    let mut b = Builder::new();
    b.formula(
        (0, 0),
        "DURATION(DATE(2024,1,1),DATE(2030,1,1),0.06,0.08,2,0)",
    );
    b.formula(
        (1, 0),
        "MDURATION(DATE(2024,1,1),DATE(2030,1,1),0.06,0.08,2,0)",
    );
    let mut wb = b.build();
    recalculate(&mut wb);
    let (CellValue::Number(d), CellValue::Number(m)) = (value_at(&wb, 0, 0), value_at(&wb, 1, 0))
    else {
        panic!("expected numbers");
    };
    assert!(d > 4.0 && d < 6.0, "duration {d}");
    assert!((m - d / 1.04).abs() < 1e-9, "modified {m} vs {d}/1.04");
}

/// The at-maturity instruments accrue from *issue*, not from settlement: the
/// buyer pays the seller for the part of the term already elapsed.
#[test]
fn maturity_instruments_accrue_from_issue() {
    let mut b = Builder::new();
    b.formula(
        (0, 0),
        "ACCRINTM(DATE(2024,1,1),DATE(2024,7,1),0.06,1000,0)",
    );
    b.formula(
        (1, 0),
        "PRICEDISC(DATE(2024,1,1),DATE(2025,1,1),0.05,100,0)",
    );
    b.formula((2, 0), "YIELDDISC(DATE(2024,1,1),DATE(2025,1,1),95,100,0)");
    b.formula(
        (3, 0),
        "PRICEMAT(DATE(2024,4,1),DATE(2025,1,1),DATE(2024,1,1),0.06,0.05,0)",
    );
    b.formula(
        (4, 0),
        "YIELDMAT(DATE(2024,4,1),DATE(2025,1,1),DATE(2024,1,1),0.06,A4,0)",
    );
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    // Half a year at 6% on 1000, on a 30/360 basis.
    assert!((n(0) - 30.0).abs() < 1e-9, "accrued {}", n(0));
    // A 5% discount over exactly one year off 100.
    assert!((n(1) - 95.0).abs() < 1e-9, "price {}", n(1));
    assert!((n(2) - (100.0 / 95.0 - 1.0)).abs() < 1e-9, "yield {}", n(2));
    // PRICEMAT and YIELDMAT invert.
    assert!((n(4) - 0.05).abs() < 1e-9, "yieldmat round trip: {}", n(4));
}

/// The volatile functions read state the *host* supplies, not a clock in the
/// engine — which is the only reason this test can exist at all.
#[test]
fn volatile_functions_read_the_supplied_clock_and_seed() {
    let mut b = Builder::new();
    b.formula((0, 0), "TODAY()");
    b.formula((1, 0), "NOW()");
    b.formula((2, 0), "RAND()");
    b.formula((3, 0), "RAND()");
    b.formula((4, 0), "RANDBETWEEN(1,6)");
    let mut wb = b.build();
    // 2024-05-17 at 18:00.
    wb.volatile_now = 45429.75;
    wb.volatile_seed = 12345;
    recalculate(&mut wb);

    let n = |wb: &Workbook, r: u32| match value_at(wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    // TODAY drops the time of day; NOW keeps it.
    assert_eq!(n(&wb, 0), 45429.0);
    assert!((n(&wb, 1) - 45429.75).abs() < 1e-9);
    // Two draws in the same pass must differ — a seed alone cannot do that,
    // which is why the evaluator counts its draws.
    assert_ne!(n(&wb, 2), n(&wb, 3));
    for r in [2, 3] {
        assert!(
            (0.0..1.0).contains(&n(&wb, r)),
            "RAND in range: {}",
            n(&wb, r)
        );
    }
    let die = n(&wb, 4);
    assert!(
        (1.0..=6.0).contains(&die) && die.fract() == 0.0,
        "die {die}"
    );

    // Same seed, same values: a recalculation is reproducible, which is what
    // lets a test assert anything about it.
    let first = (n(&wb, 2), n(&wb, 3), die);
    recalculate(&mut wb);
    assert_eq!((n(&wb, 2), n(&wb, 3), n(&wb, 4)), first);

    // A new seed rerolls, which is what F9 asks for.
    wb.volatile_seed = 999;
    recalculate(&mut wb);
    assert_ne!(n(&wb, 2), first.0);
}

/// `DATEVALUE` takes the unambiguous forms and refuses the rest. `03/04/2024`
/// is 3 April in most of the world and 4 March in the United States; with no
/// locale to decide, guessing is wrong a third of the time.
#[test]
fn datevalue_refuses_ambiguous_text() {
    let mut b = Builder::new();
    b.formula((0, 0), "DATEVALUE(\"2024-05-17\")");
    b.formula((1, 0), "DATEVALUE(\"17-May-2024\")");
    b.formula((2, 0), "DATEVALUE(\"May 17, 2024\")");
    b.formula((3, 0), "DATEVALUE(\"03/04/2024\")");
    b.formula((4, 0), "DATEVALUE(\"2024-02-31\")");
    b.formula((5, 0), "TIMEVALUE(\"6:30 PM\")");
    b.formula((6, 0), "TIMEVALUE(\"12:00 AM\")");
    b.formula((7, 0), "TIMEVALUE(\"12:00 PM\")");
    let mut wb = b.build();
    recalculate(&mut wb);

    let same = value_at(&wb, 0, 0);
    assert_eq!(value_at(&wb, 1, 0), same, "ISO and D-MMM-YYYY agree");
    assert_eq!(value_at(&wb, 2, 0), same, "and MMM D, YYYY");
    assert_eq!(
        value_at(&wb, 3, 0),
        CellValue::Error(casual_calc_model::ErrorValue::Value),
        "ambiguous slash form is refused, not guessed"
    );
    assert_eq!(
        value_at(&wb, 4, 0),
        CellValue::Error(casual_calc_model::ErrorValue::Value),
        "31 February does not exist"
    );
    let t = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("{other:?}"),
    };
    assert!((t(5) - 18.5 / 24.0).abs() < 1e-9);
    // The one case where "add 12 for PM" is wrong in both directions.
    assert_eq!(t(6), 0.0, "12 AM is midnight");
    assert!((t(7) - 0.5).abs() < 1e-9, "12 PM is noon");
}

/// The `.INTL` weekend mask starts on **Monday** while `WEEKDAY` counts from
/// Sunday. Reading the mask with a Sunday origin shifts every weekend by a day.
#[test]
fn intl_weekend_mask_is_monday_origin() {
    let mut b = Builder::new();
    // 2024-05-13 is a Monday; 2024-05-19 the Sunday that ends the week.
    b.formula((0, 0), "NETWORKDAYS.INTL(DATE(2024,5,13),DATE(2024,5,19))");
    // Mask: Sunday only as the weekend → six working days.
    b.formula(
        (1, 0),
        "NETWORKDAYS.INTL(DATE(2024,5,13),DATE(2024,5,19),\"0000001\")",
    );
    // Preset 11 is Sunday only, so it must agree with that mask.
    b.formula(
        (2, 0),
        "NETWORKDAYS.INTL(DATE(2024,5,13),DATE(2024,5,19),11)",
    );
    // Friday+Saturday weekend (preset 7) → Sunday counts as a work day.
    b.formula(
        (3, 0),
        "NETWORKDAYS.INTL(DATE(2024,5,13),DATE(2024,5,19),7)",
    );
    b.formula((4, 0), "WORKDAY.INTL(DATE(2024,5,17),1,\"0000011\")");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(
        value_at(&wb, 0, 0),
        CellValue::Number(5.0),
        "default Sat+Sun"
    );
    assert_eq!(value_at(&wb, 1, 0), CellValue::Number(6.0));
    assert_eq!(
        value_at(&wb, 2, 0),
        CellValue::Number(6.0),
        "preset 11 == mask"
    );
    assert_eq!(value_at(&wb, 3, 0), CellValue::Number(5.0));
    // Friday + 1 working day, weekend Sat/Sun → the following Monday.
    let CellValue::Number(next) = value_at(&wb, 4, 0) else {
        panic!("expected a serial");
    };
    assert_eq!(
        next as i64,
        crate::functions::ymd_to_serial_for_test(2024, 5, 20)
    );
}

/// The `*B` functions count bytes under DBCS rules, where a full-width
/// character is two. Aliasing them to their character twins — which is what
/// they collapse to in a single-byte locale — would silently halve every count
/// on exactly the data they exist for.
#[test]
fn byte_text_functions_count_double_width_characters_as_two() {
    let mut b = Builder::new();
    b.text((0, 0), "日本語abc"); // 3 wide + 3 narrow = 9 bytes, 6 characters
    b.formula((0, 1), "LEN(A1)");
    b.formula((1, 1), "LENB(A1)");
    b.formula((2, 1), "LEFTB(A1,4)");
    b.formula((3, 1), "RIGHTB(A1,3)");
    b.formula((4, 1), "MIDB(A1,3,4)");
    b.formula((5, 1), "FINDB(\"abc\",A1)");
    b.formula((6, 1), "REPLACEB(A1,1,6,\"X\")");
    // A cut landing inside a wide character yields a space for the half it
    // cannot represent, so the width asked for is still the width returned.
    b.formula((7, 1), "LEFTB(A1,3)");
    let mut wb = b.build();
    recalculate(&mut wb);

    let t = |r: u32| match value_at(&wb, r, 1) {
        CellValue::InlineString(id) | CellValue::SharedString(id) => {
            wb.strings.get(id).unwrap_or_default().to_owned()
        }
        CellValue::Number(n) => n.to_string(),
        other => panic!("row {r}: {other:?}"),
    };
    assert_eq!(t(0), "6", "LEN counts characters");
    assert_eq!(t(1), "9", "LENB counts bytes");
    assert_eq!(t(2), "日本", "two wide characters is four bytes");
    assert_eq!(t(3), "abc");
    assert_eq!(t(4), "本語", "from byte 3, four bytes");
    assert_eq!(t(5), "7", "the ASCII run starts at byte 7");
    assert_eq!(t(6), "Xabc", "six bytes replaced is three wide characters");
    assert_eq!(t(7), "日 ", "the half character becomes a space");
}

/// On text with no double-byte characters the `*B` functions and their
/// character twins must agree exactly — that is what makes them safe to use
/// outside a DBCS locale.
#[test]
fn byte_and_character_text_functions_agree_on_ascii() {
    let mut b = Builder::new();
    b.text((0, 0), "Hello, world");
    for (i, (a, bb)) in [
        ("LEN(A1)", "LENB(A1)"),
        ("LEFT(A1,5)", "LEFTB(A1,5)"),
        ("RIGHT(A1,5)", "RIGHTB(A1,5)"),
        ("MID(A1,4,3)", "MIDB(A1,4,3)"),
        ("FIND(\"world\",A1)", "FINDB(\"world\",A1)"),
        ("SEARCH(\"WORLD\",A1)", "SEARCHB(\"WORLD\",A1)"),
        ("REPLACE(A1,1,5,\"Bye\")", "REPLACEB(A1,1,5,\"Bye\")"),
    ]
    .iter()
    .enumerate()
    {
        b.formula((i as u32, 1), a);
        b.formula((i as u32, 2), bb);
    }
    let mut wb = b.build();
    recalculate(&mut wb);
    for r in 0..7u32 {
        assert_eq!(
            value_at(&wb, r, 1),
            value_at(&wb, r, 2),
            "row {r}: the byte variant must match its character twin on ASCII"
        );
    }
}

/// `ASC` and `JIS` convert only the forms that have both widths; anything else
/// passes through rather than being mangled.
#[test]
fn width_conversion_is_reversible_and_leaves_the_rest_alone() {
    let mut b = Builder::new();
    b.text((0, 0), "AB1");
    b.text((1, 0), "ＡＢ１");
    b.text((2, 0), "日本語");
    b.formula((0, 1), "JIS(A1)");
    b.formula((1, 1), "ASC(A2)");
    b.formula((2, 1), "ASC(JIS(A1))");
    b.formula((3, 1), "ASC(A3)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let t = |r: u32| match value_at(&wb, r, 1) {
        CellValue::InlineString(id) | CellValue::SharedString(id) => {
            wb.strings.get(id).unwrap_or_default().to_owned()
        }
        other => panic!("row {r}: {other:?}"),
    };
    assert_eq!(t(0), "ＡＢ１");
    assert_eq!(t(1), "AB1");
    assert_eq!(t(2), "AB1", "the pair round-trips");
    assert_eq!(t(3), "日本語", "no half-width form, so untouched");
}

/// Bessel values checked against published tables and against a separately
/// written implementation of the same series — not against this one's own
/// output. A series that converges smoothly to the wrong number looks perfectly
/// healthy from the inside.
///
/// Two of the constants here were wrong when first written: J₂(2.5) and
/// K₁(1.5) were mistyped in the seventh and ninth decimal, and the failure was
/// the *test*, not the code. Both now agree with the tables to seven places,
/// which is as far as published values go.
#[test]
fn bessel_functions_match_reference_values() {
    let mut b = Builder::new();
    let cases = [
        ("BESSELJ(1.5,0)", 0.511_827_671_735_918),
        ("BESSELJ(1.5,1)", 0.557_936_507_910_100),
        ("BESSELJ(2.5,2)", 0.446_059_058_437_444),
        ("BESSELI(1.5,0)", 1.646_723_189_772_88),
        ("BESSELI(1.5,1)", 0.981_666_428_925_837),
        ("BESSELY(1.5,0)", 0.382_448_923_797_759),
        ("BESSELY(1.5,1)", -0.412_308_626_973_911),
        ("BESSELK(1.5,0)", 0.213_805_562_643_749),
        ("BESSELK(1.5,1)", 0.277_387_800_456_844),
    ];
    for (i, (formula, _)) in cases.iter().enumerate() {
        b.formula((i as u32, 0), formula);
    }
    // The domain edges: Y and K diverge at zero, and a negative argument is
    // undefined — both #NUM! rather than an infinity, because a spreadsheet
    // showing 1E+308 for an undefined value is worse than one that says so.
    b.formula((20, 0), "BESSELY(0,0)");
    b.formula((21, 0), "BESSELK(0,0)");
    b.formula((22, 0), "BESSELJ(-1,0)");
    let mut wb = b.build();
    recalculate(&mut wb);

    for (i, (formula, want)) in cases.iter().enumerate() {
        let got = match value_at(&wb, i as u32, 0) {
            CellValue::Number(n) => n,
            other => panic!("{formula}: {other:?}"),
        };
        assert!(
            (got - want).abs() < 1e-9,
            "{formula}: got {got}, want {want}"
        );
    }
    for r in [20, 21, 22] {
        assert_eq!(
            value_at(&wb, r, 0),
            CellValue::Error(casual_calc_model::ErrorValue::Num),
            "row {r} is outside the domain"
        );
    }
}

/// Thai number words have two irregularities that a digit-by-digit rendering
/// gets wrong: a tens digit of one is `สิบ`, and a units digit of one after any
/// tens is `เอ็ด`.
#[test]
fn bahttext_handles_the_thai_irregular_forms() {
    let mut b = Builder::new();
    b.formula((0, 0), "BAHTTEXT(1)");
    b.formula((1, 0), "BAHTTEXT(10)");
    b.formula((2, 0), "BAHTTEXT(11)");
    b.formula((3, 0), "BAHTTEXT(21)");
    b.formula((4, 0), "BAHTTEXT(0)");
    b.formula((5, 0), "BAHTTEXT(1.25)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let t = |r: u32| match value_at(&wb, r, 0) {
        CellValue::InlineString(id) | CellValue::SharedString(id) => {
            wb.strings.get(id).unwrap_or_default().to_owned()
        }
        other => panic!("row {r}: {other:?}"),
    };
    assert_eq!(t(0), "หนึ่งบาทถ้วน");
    assert_eq!(t(1), "สิบบาทถ้วน", "ten is สิบ, not หนึ่งสิบ");
    assert_eq!(t(2), "สิบเอ็ดบาทถ้วน", "eleven ends in เอ็ด");
    assert_eq!(t(3), "ยี่สิบเอ็ดบาทถ้วน", "twenty is ยี่สิบ");
    assert_eq!(t(4), "ศูนย์บาทถ้วน");
    assert_eq!(t(5), "หนึ่งบาทยี่สิบห้าสตางค์", "satang replace ถ้วน");
}

/// The converse of `every_cataloged_function_dispatches`, which was missing —
/// and six functions had slipped through the gap.
///
/// A function that dispatches but is not in the catalog works when typed and is
/// invisible everywhere else: no autocomplete, no signature help, and absent
/// from the coverage audit. The catalog is documented as the single source of
/// truth, so nothing may dispatch without being in it.
#[test]
fn every_dispatched_function_is_in_the_catalog() {
    let src = include_str!("functions.rs");
    let catalog: std::collections::HashSet<&str> =
        crate::FUNCTIONS.iter().map(|(n, _)| *n).collect();

    // The dispatch arms, including every alternative of a multi-name arm —
    // `"PRICE" | "YIELD" | "DURATION" =>` names three functions, not one.
    let mut missing: Vec<String> = Vec::new();
    for line in src.lines() {
        let Some(arrow) = line.find("=>") else {
            continue;
        };
        let head = &line[..arrow];
        if !head.trim_start().starts_with('"') {
            continue; // not a string-literal match arm
        }
        for part in head.split('|') {
            let part = part.trim();
            let Some(name) = part.strip_prefix('"').and_then(|p| p.strip_suffix('"')) else {
                continue;
            };
            // Function names only: the same match syntax is used for DATEDIF's
            // unit strings ("D", "YM"), which are arguments, not functions.
            if name.len() < 2
                || !name
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '.')
            {
                continue;
            }
            if !catalog.contains(name) && !missing.iter().any(|m| m == name) {
                missing.push(name.to_owned());
            }
        }
    }
    // DATEDIF's units are two letters and look exactly like function names, so
    // they are named rather than pattern-matched away.
    missing.retain(|n| !matches!(n.as_str(), "MD" | "YD" | "YM"));
    assert!(
        missing.is_empty(),
        "dispatched but not in the catalog: {missing:?}"
    );
}

/// `VDB`'s switch to straight line is the whole point of it: declining balance
/// never reaches the salvage value, so an asset depreciated purely that way is
/// still on the books at the end of its life.
#[test]
fn vdb_switches_to_straight_line_unless_told_not_to() {
    let mut b = Builder::new();
    // Whole life, switching allowed: everything above salvage is written off.
    b.formula((0, 0), "VDB(10000,1000,5,0,5)");
    // The switch shows in the *later* periods, not in the lifetime total:
    // declining balance is already clamped at salvage, so both reach 9000 over
    // a full life. It also needs a life long enough for straight line to
    // overtake — over five years at factor 2 it never does, so this uses ten.
    b.formula((1, 0), "VDB(10000,0,10,7,8)");
    b.formula((4, 0), "VDB(10000,0,10,7,8,2,TRUE)");
    // A partial span prorates its end periods.
    b.formula((2, 0), "VDB(10000,1000,5,0,1)");
    b.formula((3, 0), "VDB(10000,1000,5,0,0.5)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    assert!(
        (n(0) - 9000.0).abs() < 1e-6,
        "full life reaches salvage: {}",
        n(0)
    );
    assert!(
        n(1) > n(4),
        "switching writes off more in a late period: {} vs {}",
        n(1),
        n(4)
    );
    // First year at double declining on 10000 over 5 years.
    assert!((n(2) - 4000.0).abs() < 1e-6, "first period: {}", n(2));
    assert!((n(3) - 2000.0).abs() < 1e-6, "half of it: {}", n(3));
}

/// `ACCRINT`'s `calc_method` decides whether interest accrues from issue or
/// from the first interest date — a difference that only shows once settlement
/// is past the first coupon, which is when anyone passes the argument.
#[test]
fn accrint_calc_method_moves_the_accrual_start() {
    let mut b = Builder::new();
    b.formula(
        (0, 0),
        "ACCRINT(DATE(2024,1,1),DATE(2024,7,1),DATE(2024,10,1),0.06,1000,2,0,TRUE)",
    );
    b.formula(
        (1, 0),
        "ACCRINT(DATE(2024,1,1),DATE(2024,7,1),DATE(2024,10,1),0.06,1000,2,0,FALSE)",
    );
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    // Nine months at 6% on 1000, 30/360.
    assert!((n(0) - 45.0).abs() < 1e-9, "from issue: {}", n(0));
    // Three months, from the first interest date instead.
    assert!((n(1) - 15.0).abs() < 1e-9, "from first interest: {}", n(1));
}

/// The French systems prorate their first period from the purchase date, which
/// is why they take dates where the other depreciation functions take counts.
#[test]
fn french_depreciation_prorates_the_first_period() {
    let mut b = Builder::new();
    b.formula(
        (0, 0),
        "AMORLINC(2400,DATE(2024,8,19),DATE(2024,12,31),300,0,0.15,1)",
    );
    b.formula(
        (1, 0),
        "AMORLINC(2400,DATE(2024,8,19),DATE(2024,12,31),300,1,0.15,1)",
    );
    b.formula(
        (2, 0),
        "AMORDEGRC(2400,DATE(2024,8,19),DATE(2024,12,31),300,0,0.15,1)",
    );
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    // A part-year at 15% of 2400 — less than a full period's 360.
    assert!(
        n(0) > 0.0 && n(0) < 360.0,
        "first period is partial: {}",
        n(0)
    );
    assert!(
        (n(1) - 360.0).abs() < 1.0,
        "a full period follows: {}",
        n(1)
    );
    // The degressive coefficient makes the first period larger, never smaller.
    assert!(
        n(2) > n(0),
        "degressive exceeds linear: {} vs {}",
        n(2),
        n(0)
    );
}

/// `CELL` reports on the *reference*, not on the value at it — which is why
/// the argument has to stay an expression. Evaluating it first would leave a
/// number and lose the address entirely.
#[test]
fn cell_reports_on_the_reference_not_the_value() {
    let mut b = Builder::new();
    b.number((4, 2), 42.0); // C5
    b.text((5, 2), "hello"); // C6
    b.formula((0, 0), "CELL(\"address\",C5)");
    b.formula((1, 0), "CELL(\"row\",C5)");
    b.formula((2, 0), "CELL(\"col\",C5)");
    b.formula((3, 0), "CELL(\"type\",C5)");
    b.formula((4, 0), "CELL(\"type\",C6)");
    b.formula((5, 0), "CELL(\"type\",C9)");
    b.formula((6, 0), "CELL(\"contents\",C5)");
    // Locked is OOXML's default for a cell that says nothing — the same
    // default the protection guard relies on.
    b.formula((7, 0), "CELL(\"protect\",C5)");
    // No path exists here, and "" would read as an unsaved workbook.
    b.formula((8, 0), "CELL(\"filename\",C5)");
    let mut wb = b.build();
    recalculate(&mut wb);

    let t = |r: u32| match value_at(&wb, r, 0) {
        CellValue::InlineString(id) | CellValue::SharedString(id) => {
            wb.strings.get(id).unwrap_or_default().to_owned()
        }
        CellValue::Number(n) => n.to_string(),
        other => format!("{other:?}"),
    };
    assert_eq!(t(0), "$C$5");
    assert_eq!(t(1), "5");
    assert_eq!(t(2), "3");
    assert_eq!(t(3), "v", "a number is a value");
    assert_eq!(t(4), "l", "text is a label");
    assert_eq!(t(5), "b", "an empty cell is blank");
    assert_eq!(t(6), "42");
    assert_eq!(t(7), "1");
    assert_eq!(
        value_at(&wb, 8, 0),
        CellValue::Error(casual_calc_model::ErrorValue::Na)
    );
}

/// `INFO` answers only what it can answer truthfully. There is no working
/// directory in a browser, and inventing one would fail somewhere far away
/// from the formula that asked.
#[test]
fn info_declines_what_it_cannot_know() {
    let mut b = Builder::new();
    b.formula((0, 0), "INFO(\"recalc\")");
    b.formula((1, 0), "INFO(\"directory\")");
    b.formula((2, 0), "INFO(\"numfile\")");
    b.formula((3, 0), "INFO(\"nonsense\")");
    let mut wb = b.build();
    recalculate(&mut wb);
    let (CellValue::InlineString(id) | CellValue::SharedString(id)) = value_at(&wb, 0, 0) else {
        panic!("expected text");
    };
    assert_eq!(wb.strings.get(id).unwrap_or_default(), "Automatic");
    for r in [1, 2] {
        assert_eq!(
            value_at(&wb, r, 0),
            CellValue::Error(casual_calc_model::ErrorValue::Na),
            "row {r} is unknowable here, not guessable"
        );
    }
    assert_eq!(
        value_at(&wb, 3, 0),
        CellValue::Error(casual_calc_model::ErrorValue::Value),
        "an unrecognised type is a mistake, not an unknown"
    );
}

/// `CONVERT` goes through one base unit per category, so a conversion is a
/// division rather than a lookup of every pair — a pairwise table of eighty
/// units is six thousand entries and every one a chance to be wrong.
#[test]
fn convert_crosses_units_within_a_category_only() {
    let mut b = Builder::new();
    b.formula((0, 0), "CONVERT(1,\"lbm\",\"kg\")");
    b.formula((1, 0), "CONVERT(1,\"mi\",\"m\")");
    b.formula((2, 0), "CONVERT(1,\"day\",\"hr\")");
    b.formula((3, 0), "CONVERT(1,\"gal\",\"l\")");
    b.formula((4, 0), "CONVERT(1024,\"byte\",\"bit\")");
    // Metric prefixes apply, but an exact unit name wins over a prefixed
    // reading: `m` is metres, not milli-anything.
    b.formula((5, 0), "CONVERT(1,\"km\",\"m\")");
    b.formula((6, 0), "CONVERT(1,\"m\",\"cm\")");
    // Different categories have no answer at all — kilograms into metres is
    // not a small error, it is a question with none.
    b.formula((7, 0), "CONVERT(1,\"kg\",\"m\")");
    b.formula((8, 0), "CONVERT(1,\"kg\",\"nonsense\")");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    assert!((n(0) - 0.45359237).abs() < 1e-12);
    assert!((n(1) - 1609.344).abs() < 1e-9);
    assert!((n(2) - 24.0).abs() < 1e-12);
    assert!((n(3) - 3.785411784).abs() < 1e-12);
    assert!((n(4) - 8192.0).abs() < 1e-9);
    assert!((n(5) - 1000.0).abs() < 1e-9, "kilo prefix: {}", n(5));
    assert!(
        (n(6) - 100.0).abs() < 1e-9,
        "m is metres, not milli: {}",
        n(6)
    );
    for r in [7, 8] {
        assert_eq!(
            value_at(&wb, r, 0),
            CellValue::Error(casual_calc_model::ErrorValue::Na),
            "row {r}"
        );
    }
}

/// Temperature is the one family a factor cannot express, because the scales
/// have different zeros: a factor alone turns 0 °C into 0 °F.
#[test]
fn convert_handles_temperature_offsets() {
    let mut b = Builder::new();
    b.formula((0, 0), "CONVERT(0,\"C\",\"F\")");
    b.formula((1, 0), "CONVERT(100,\"C\",\"F\")");
    b.formula((2, 0), "CONVERT(-40,\"C\",\"F\")");
    b.formula((3, 0), "CONVERT(0,\"C\",\"K\")");
    b.formula((4, 0), "CONVERT(212,\"F\",\"C\")");
    // A temperature into a length has no answer.
    b.formula((5, 0), "CONVERT(0,\"C\",\"m\")");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    assert!((n(0) - 32.0).abs() < 1e-9, "freezing: {}", n(0));
    assert!((n(1) - 212.0).abs() < 1e-9, "boiling: {}", n(1));
    assert!(
        (n(2) + 40.0).abs() < 1e-9,
        "the scales cross at -40: {}",
        n(2)
    );
    assert!((n(3) - 273.15).abs() < 1e-9);
    assert!((n(4) - 100.0).abs() < 1e-9);
    assert_eq!(
        value_at(&wb, 5, 0),
        CellValue::Error(casual_calc_model::ErrorValue::Na)
    );
}

/// The property that pins the odd-period bonds down: a bond whose "odd" period
/// is actually regular is an ordinary bond, so `ODDLPRICE` must agree with
/// `PRICE`. Asserting that beats quoting numbers I cannot independently check.
#[test]
fn an_odd_period_that_is_regular_prices_like_an_ordinary_bond() {
    let mut b = Builder::new();
    // Settlement 2024-02-15, maturity 2024-11-15, last interest 2024-05-15 —
    // one regular semi-annual period from last interest to maturity.
    b.formula(
        (0, 0),
        "ODDLPRICE(DATE(2024,8,15),DATE(2025,2,15),DATE(2024,8,15),0.06,0.06,100,2,0)",
    );
    // A par bond: coupon equal to yield prices at 100 whatever the schedule.
    let mut wb = b.build();
    recalculate(&mut wb);
    let CellValue::Number(p) = value_at(&wb, 0, 0) else {
        panic!("expected a price: {:?}", value_at(&wb, 0, 0));
    };
    assert!(
        (p - 100.0).abs() < 1e-6,
        "coupon equal to yield prices at par: {p}"
    );
}

/// Each odd yield is solved against its own price function, so the pairs invert
/// exactly rather than approximately.
#[test]
fn odd_bond_yields_invert_their_prices() {
    let mut b = Builder::new();
    b.formula(
        (0, 0),
        "ODDLPRICE(DATE(2024,8,15),DATE(2025,2,15),DATE(2024,5,15),0.06,0.075,100,2,0)",
    );
    b.formula(
        (1, 0),
        "ODDLYIELD(DATE(2024,8,15),DATE(2025,2,15),DATE(2024,5,15),0.06,A1,100,2,0)",
    );
    b.formula(
        (2, 0),
        "ODDFPRICE(DATE(2024,3,1),DATE(2029,1,1),DATE(2024,1,1),DATE(2024,7,1),0.06,0.07,100,2,0)",
    );
    b.formula(
        (3, 0),
        "ODDFYIELD(DATE(2024,3,1),DATE(2029,1,1),DATE(2024,1,1),DATE(2024,7,1),0.06,A3,100,2,0)",
    );
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 0) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    assert!(n(0) > 50.0 && n(0) < 150.0, "a plausible price: {}", n(0));
    assert!((n(1) - 0.075).abs() < 1e-6, "ODDLYIELD inverts: {}", n(1));
    assert!(n(2) > 50.0 && n(2) < 150.0, "a plausible price: {}", n(2));
    assert!((n(3) - 0.07).abs() < 1e-6, "ODDFYIELD inverts: {}", n(3));
}

/// `MDETERM` by LU with partial pivoting — cofactor expansion is O(n!), and a
/// 10×10 array is small for a spreadsheet but millions of operations that way.
#[test]
fn mdeterm_computes_determinants_and_detects_singularity() {
    let mut b = Builder::new();
    // A 3×3 with a known determinant of 1.
    b.number((0, 0), 2.0)
        .number((0, 1), -1.0)
        .number((0, 2), 0.0);
    b.number((1, 0), -1.0)
        .number((1, 1), 2.0)
        .number((1, 2), -1.0);
    b.number((2, 0), 0.0)
        .number((2, 1), -1.0)
        .number((2, 2), 2.0);
    // A singular matrix: the second row is twice the first.
    b.number((4, 0), 1.0).number((4, 1), 2.0);
    b.number((5, 0), 2.0).number((5, 1), 4.0);
    // A leading zero forces a pivot swap, which must flip the sign.
    b.number((7, 0), 0.0).number((7, 1), 1.0);
    b.number((8, 0), 1.0).number((8, 1), 0.0);

    b.formula((0, 4), "MDETERM(A1:C3)");
    b.formula((1, 4), "MDETERM(A5:B6)");
    b.formula((2, 4), "MDETERM(A8:B9)");
    // Not square: a determinant is undefined.
    b.formula((3, 4), "MDETERM(A1:B3)");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32| match value_at(&wb, r, 4) {
        CellValue::Number(v) => v,
        other => panic!("row {r}: {other:?}"),
    };
    assert!((n(0) - 4.0).abs() < 1e-9, "3x3 determinant: {}", n(0));
    assert!(n(1).abs() < 1e-9, "a singular matrix is zero: {}", n(1));
    assert!(
        (n(2) + 1.0).abs() < 1e-9,
        "the row swap flips the sign: {}",
        n(2)
    );
    assert_eq!(
        value_at(&wb, 3, 4),
        CellValue::Error(casual_calc_model::ErrorValue::Value),
        "a non-square array has no determinant"
    );
}

/// An array result spills into the cells below and to the right, and the
/// spilled cells are flagged so the next pass can reclaim them.
#[test]
fn an_array_result_spills_into_its_neighbours() {
    use casual_calc_model::CellFlags;
    let mut b = Builder::new();
    b.number((0, 0), 1.0).number((0, 1), 2.0);
    b.number((1, 0), 3.0).number((1, 1), 4.0);
    b.formula((0, 4), "TRANSPOSE(A1:B2)");
    let mut wb = b.build();
    recalculate(&mut wb);

    // Transposed: E1:F2 holds 1,3 / 2,4.
    assert_eq!(value_at(&wb, 0, 4), CellValue::Number(1.0));
    assert_eq!(value_at(&wb, 0, 5), CellValue::Number(3.0));
    assert_eq!(value_at(&wb, 1, 4), CellValue::Number(2.0));
    assert_eq!(value_at(&wb, 1, 5), CellValue::Number(4.0));

    let flags = |r: u32, c: u32| {
        wb.sheets[0]
            .cells
            .get(CellRef::new(r, c))
            .map(|cell| cell.flags)
            .unwrap_or_default()
    };
    assert!(flags(0, 4).contains(CellFlags::SPILL_ANCHOR), "the anchor");
    assert!(flags(0, 5).contains(CellFlags::SPILL_CHILD), "a child");
    assert!(
        !flags(0, 5).contains(CellFlags::SPILL_ANCHOR),
        "a child is not an anchor"
    );
}

/// A spill refuses rather than overwrites. Silently replacing a value someone
/// typed is the one behaviour a spreadsheet must never have.
#[test]
fn a_blocked_spill_is_an_error_and_writes_nothing() {
    let mut b = Builder::new();
    b.number((0, 0), 1.0).number((0, 1), 2.0);
    b.number((1, 0), 3.0).number((1, 1), 4.0);
    b.formula((0, 4), "TRANSPOSE(A1:B2)");
    b.text((1, 5), "mine"); // sits inside the spill range
    let mut wb = b.build();
    recalculate(&mut wb);

    assert_eq!(
        value_at(&wb, 0, 4),
        CellValue::Error(casual_calc_model::ErrorValue::Spill)
    );
    // The obstruction is untouched — that is the point.
    let (CellValue::InlineString(id) | CellValue::SharedString(id)) = value_at(&wb, 1, 5) else {
        panic!("the blocking value must survive");
    };
    assert_eq!(wb.strings.get(id).unwrap_or_default(), "mine");
}

/// A formula that produces a smaller array than last time must give back the
/// cells it no longer covers, or the old values linger as ghosts.
#[test]
fn a_shrinking_spill_releases_the_cells_it_vacates() {
    let mut b = Builder::new();
    for r in 0..3u32 {
        b.number((r, 0), (r + 1) as f64);
    }
    b.formula((0, 4), "TRANSPOSE(A1:A3)"); // 1 row x 3 cols
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 0, 6), CellValue::Number(3.0));

    // Re-point the formula at two cells; the third column must clear.
    let expr = parse("TRANSPOSE(A1:A2)").unwrap();
    let handle = wb.store_formula(expr);
    let mut cell = wb.sheets[0].cells.get(CellRef::new(0, 4)).unwrap().clone();
    cell.formula = Some(handle);
    wb.sheets[0].cells.set(CellRef::new(0, 4), cell);
    recalculate(&mut wb);

    assert_eq!(value_at(&wb, 0, 5), CellValue::Number(2.0));
    assert_eq!(
        value_at(&wb, 0, 6),
        CellValue::Empty,
        "the vacated cell must not keep its old value"
    );
}

/// The matrix functions, checked by the identities that define them rather than
/// by quoted numbers: a matrix times its inverse is the identity, and
/// transposing twice is a no-op.
#[test]
fn matrix_functions_satisfy_their_identities() {
    let mut b = Builder::new();
    b.number((0, 0), 4.0).number((0, 1), 7.0);
    b.number((1, 0), 2.0).number((1, 1), 6.0);
    b.formula((0, 4), "MINVERSE(A1:B2)");
    b.formula((4, 4), "MMULT(A1:B2,E1:F2)");
    // FREQUENCY has one more bucket than bins, or the counts do not sum to the
    // data — everything above the last bin still has to land somewhere.
    b.number((0, 8), 1.0)
        .number((1, 8), 5.0)
        .number((2, 8), 9.0);
    b.number((0, 9), 3.0).number((1, 9), 6.0);
    b.formula((0, 10), "FREQUENCY(I1:I3,J1:J2)");
    let mut wb = b.build();
    recalculate(&mut wb);

    let n = |r: u32, c: u32| match value_at(&wb, r, c) {
        CellValue::Number(v) => v,
        other => panic!("({r},{c}): {other:?}"),
    };
    // A × A⁻¹ = I.
    assert!((n(4, 4) - 1.0).abs() < 1e-9, "{}", n(4, 4));
    assert!(n(4, 5).abs() < 1e-9);
    assert!(n(5, 4).abs() < 1e-9);
    assert!((n(5, 5) - 1.0).abs() < 1e-9);
    // Three bins-worth of buckets for two bins: 1 | 5 | 9.
    assert_eq!((n(0, 10), n(1, 10), n(2, 10)), (1.0, 1.0, 1.0));
}

/// A regression on points that lie exactly on a line must recover that line —
/// slope, intercept and an R² of one. Anything less means the fit is wrong in a
/// way no amount of plausible-looking output would reveal.
#[test]
fn linest_recovers_an_exact_line() {
    let mut b = Builder::new();
    // y = 3x + 2 at x = 1..5.
    for i in 0..5u32 {
        b.number((i, 0), (i + 1) as f64);
        b.number((i, 1), 3.0 * (i + 1) as f64 + 2.0);
    }
    b.formula((0, 3), "LINEST(B1:B5,A1:A5)");
    // TREND must agree with the line it fitted.
    b.formula((0, 6), "TREND(B1:B5,A1:A5,A1:A5)");
    let mut wb = b.build();
    recalculate(&mut wb);

    let n = |r: u32, c: u32| match value_at(&wb, r, c) {
        CellValue::Number(v) => v,
        other => panic!("({r},{c}): {other:?}"),
    };
    // Excel's order: slope first, intercept second.
    assert!((n(0, 3) - 3.0).abs() < 1e-9, "slope {}", n(0, 3));
    assert!((n(0, 4) - 2.0).abs() < 1e-9, "intercept {}", n(0, 4));
    for i in 0..5u32 {
        let want = 3.0 * (i + 1) as f64 + 2.0;
        assert!(
            (n(i, 6) - want).abs() < 1e-9,
            "TREND at {i}: {} want {want}",
            n(i, 6)
        );
    }
}

/// `LOGEST` and `GROWTH` fit `y = b·m^x`, which is the linear fit on `ln(y)`.
/// A non-positive y has no logarithm, and skipping it would fit a different
/// dataset than the one given — so it is `#NUM!`.
#[test]
fn logest_recovers_an_exact_exponential() {
    let mut b = Builder::new();
    // y = 2 · 3^x at x = 1..4.
    for i in 0..4u32 {
        let x = (i + 1) as f64;
        b.number((i, 0), x);
        b.number((i, 1), 2.0 * 3.0f64.powf(x));
    }
    b.formula((0, 3), "LOGEST(B1:B4,A1:A4)");
    b.formula((0, 6), "GROWTH(B1:B4,A1:A4,A1:A4)");
    // A zero in the data has no logarithm.
    b.number((0, 9), 1.0).number((1, 9), 0.0);
    b.number((0, 10), 1.0).number((1, 10), 2.0);
    b.formula((5, 3), "LOGEST(J1:J2,K1:K2)");
    let mut wb = b.build();
    recalculate(&mut wb);

    let n = |r: u32, c: u32| match value_at(&wb, r, c) {
        CellValue::Number(v) => v,
        other => panic!("({r},{c}): {other:?}"),
    };
    assert!((n(0, 3) - 3.0).abs() < 1e-9, "base {}", n(0, 3));
    assert!((n(0, 4) - 2.0).abs() < 1e-9, "coefficient {}", n(0, 4));
    for i in 0..4u32 {
        let want = 2.0 * 3.0f64.powf((i + 1) as f64);
        assert!((n(i, 6) - want).abs() < 1e-6, "GROWTH at {i}: {}", n(i, 6));
    }
    assert_eq!(
        value_at(&wb, 5, 3),
        CellValue::Error(casual_calc_model::ErrorValue::Num),
        "a non-positive y has no logarithm"
    );
}

/// `LINEST` with statistics returns a 5-row block. Forcing the intercept to
/// zero changes the degrees of freedom as well as the fit, which silently
/// corrupts R² if it is missed.
#[test]
fn linest_statistics_block_and_forced_intercept() {
    let mut b = Builder::new();
    for i in 0..5u32 {
        let x = (i + 1) as f64;
        b.number((i, 0), x);
        b.number((i, 1), 3.0 * x + 2.0);
    }
    b.formula((0, 3), "LINEST(B1:B5,A1:A5,TRUE,TRUE)");
    // Through the origin: the best fit is no longer y = 3x + 2.
    b.formula((10, 3), "LINEST(B1:B5,A1:A5,FALSE)");
    let mut wb = b.build();
    recalculate(&mut wb);

    let n = |r: u32, c: u32| match value_at(&wb, r, c) {
        CellValue::Number(v) => v,
        other => panic!("({r},{c}): {other:?}"),
    };
    // Row 0 coefficients, row 2 holds R² and the standard error of y.
    assert!((n(0, 3) - 3.0).abs() < 1e-9);
    assert!(
        (n(2, 3) - 1.0).abs() < 1e-9,
        "R² of an exact fit: {}",
        n(2, 3)
    );
    assert!(n(2, 4).abs() < 1e-9, "no residual error: {}", n(2, 4));
    // Degrees of freedom: five points, two coefficients.
    assert!((n(3, 4) - 3.0).abs() < 1e-9, "df {}", n(3, 4));
    // Forced through the origin the intercept is reported as zero.
    assert!(n(10, 4).abs() < 1e-12, "forced intercept {}", n(10, 4));
    assert!(n(10, 3) > 3.0, "and the slope absorbs it: {}", n(10, 3));
}

/// XLOOKUP replaces INDEX/MATCH, not just VLOOKUP: it can look left, and a
/// wider return array gives back a whole row.
#[test]
fn xlookup_searches_both_directions_and_returns_rows() {
    let mut b = Builder::new();
    for (i, (name, qty, price)) in [("Ann", 3.0, 1.5), ("Bob", 5.0, 2.5), ("Cid", 7.0, 3.5)]
        .iter()
        .enumerate()
    {
        let r = i as u32;
        b.text((r, 1), name)
            .number((r, 2), *qty)
            .number((r, 3), *price);
    }
    // Look up by name, return the qty — the column to the *left* of nothing,
    // which VLOOKUP could not do without rearranging the data.
    b.formula((0, 6), "XLOOKUP(\"Bob\",B1:B3,C1:C3)");
    // A wider return array gives the whole row.
    b.formula((2, 6), "XLOOKUP(\"Cid\",B1:B3,C1:D3)");
    // Not found uses the fourth argument rather than #N/A.
    b.formula((5, 6), "XLOOKUP(\"Zoe\",B1:B3,C1:C3,\"none\")");
    b.formula((6, 6), "XLOOKUP(\"Zoe\",B1:B3,C1:C3)");
    // XMATCH gives the position, one-based.
    b.formula((7, 6), "XMATCH(\"Cid\",B1:B3)");
    let mut wb = b.build();
    recalculate(&mut wb);

    assert_eq!(value_at(&wb, 0, 6), CellValue::Number(5.0));
    assert_eq!(value_at(&wb, 2, 6), CellValue::Number(7.0));
    assert_eq!(
        value_at(&wb, 2, 7),
        CellValue::Number(3.5),
        "spilled second column"
    );
    let (CellValue::InlineString(id) | CellValue::SharedString(id)) = value_at(&wb, 5, 6) else {
        panic!("expected the if_not_found text");
    };
    assert_eq!(wb.strings.get(id).unwrap_or_default(), "none");
    assert_eq!(
        value_at(&wb, 6, 6),
        CellValue::Error(casual_calc_model::ErrorValue::Na),
        "no if_not_found means #N/A"
    );
    assert_eq!(value_at(&wb, 7, 6), CellValue::Number(3.0));
}

/// The ordered match modes need the *best* candidate, not the first acceptable
/// one — taking the first returns whichever end the scan started from, which is
/// a different answer to the same question.
#[test]
fn xlookup_approximate_modes_take_the_nearest() {
    let mut b = Builder::new();
    for (i, v) in [10.0, 20.0, 30.0, 40.0].iter().enumerate() {
        b.number((i as u32, 0), *v);
    }
    b.formula((0, 2), "XLOOKUP(25,A1:A4,A1:A4,,-1)"); // next smaller → 20
    b.formula((1, 2), "XLOOKUP(25,A1:A4,A1:A4,,1)"); // next larger → 30
    b.formula((2, 2), "XLOOKUP(30,A1:A4,A1:A4,,-1)"); // exact wins
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 0, 2), CellValue::Number(20.0));
    assert_eq!(value_at(&wb, 1, 2), CellValue::Number(30.0));
    assert_eq!(value_at(&wb, 2, 2), CellValue::Number(30.0));
}

/// FILTER, UNIQUE, SORT, SORTBY and SEQUENCE — the dynamic-array core, which
/// only became possible once results could spill.
#[test]
fn the_dynamic_array_core_filters_sorts_and_generates() {
    let mut b = Builder::new();
    for (i, (name, qty)) in [("Ann", 3.0), ("Bob", 9.0), ("Cid", 5.0), ("Ann", 3.0)]
        .iter()
        .enumerate()
    {
        b.text((i as u32, 0), name).number((i as u32, 1), *qty);
    }
    b.formula((0, 3), "FILTER(A1:B4,B1:B4>4)"); // Bob and Cid
    b.formula((0, 6), "UNIQUE(A1:A4)"); // Ann, Bob, Cid
    b.formula((0, 8), "SORT(B1:B4,1,-1)"); // 9,5,3,3
    b.formula((0, 10), "SEQUENCE(3,2,10,5)"); // 10,15 / 20,25 / 30,35
    // Nothing matches: #CALC!, which is what Excel returns and is why the
    // error had to exist.
    b.formula((10, 3), "FILTER(A1:B4,B1:B4>100)");
    let mut wb = b.build();
    recalculate(&mut wb);

    assert_eq!(value_at(&wb, 0, 4), CellValue::Number(9.0), "Bob's qty");
    assert_eq!(value_at(&wb, 1, 4), CellValue::Number(5.0), "Cid's qty");
    let text = |r: u32, c: u32| match value_at(&wb, r, c) {
        CellValue::InlineString(id) | CellValue::SharedString(id) => {
            wb.strings.get(id).unwrap_or_default().to_owned()
        }
        other => panic!("({r},{c}): {other:?}"),
    };
    assert_eq!(
        (text(0, 6), text(1, 6), text(2, 6)),
        ("Ann".into(), "Bob".into(), "Cid".into())
    );
    assert_eq!(value_at(&wb, 0, 8), CellValue::Number(9.0));
    assert_eq!(value_at(&wb, 3, 8), CellValue::Number(3.0));
    assert_eq!(value_at(&wb, 0, 10), CellValue::Number(10.0));
    assert_eq!(value_at(&wb, 0, 11), CellValue::Number(15.0));
    assert_eq!(value_at(&wb, 2, 11), CellValue::Number(35.0));
    assert_eq!(
        value_at(&wb, 10, 3),
        CellValue::Error(casual_calc_model::ErrorValue::Calc)
    );
}

/// MAXIFS and MINIFS of nothing are 0, not an error — Excel's choice, and
/// different from AVERAGEIFS because a maximum of no numbers has a defensible
/// answer where a mean does not.
#[test]
fn maxifs_and_minifs_of_no_matches_are_zero() {
    let mut b = Builder::new();
    for (i, (region, v)) in [("W", 10.0), ("E", 20.0), ("W", 30.0)].iter().enumerate() {
        b.text((i as u32, 0), region).number((i as u32, 1), *v);
    }
    b.formula((0, 3), "MAXIFS(B1:B3,A1:A3,\"W\")");
    b.formula((1, 3), "MINIFS(B1:B3,A1:A3,\"W\")");
    b.formula((2, 3), "MAXIFS(B1:B3,A1:A3,\"Z\")");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 0, 3), CellValue::Number(30.0));
    assert_eq!(value_at(&wb, 1, 3), CellValue::Number(10.0));
    assert_eq!(value_at(&wb, 2, 3), CellValue::Number(0.0));
}

/// `LET` binds in order, so a later value may use an earlier name — that is
/// what makes it worth having over repeating a subexpression.
#[test]
fn let_binds_in_order_and_shadows() {
    let mut b = Builder::new();
    b.number((0, 0), 10.0);
    b.formula((0, 2), "LET(x,A1,y,x*2,x+y)"); // 10 + 20
    b.formula((1, 2), "LET(x,1,LET(x,x+1,x))"); // the inner x wins
    // A binding shadows a defined name of the same name.
    b.formula((2, 2), "LET(Rng,7,Rng)");
    // An even argument count has no calculation to return.
    b.formula((3, 2), "LET(x,1)");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 0, 2), CellValue::Number(30.0));
    assert_eq!(value_at(&wb, 1, 2), CellValue::Number(2.0));
    assert_eq!(value_at(&wb, 2, 2), CellValue::Number(7.0));
    assert_eq!(
        value_at(&wb, 3, 2),
        CellValue::Error(casual_calc_model::ErrorValue::Value)
    );
}

/// A `LAMBDA` is called either immediately or through the name it is bound to,
/// and a named one may call itself.
#[test]
fn lambda_applies_inline_and_by_name() {
    use casual_calc_formula::parse;
    let mut b = Builder::new();
    b.formula((0, 0), "LAMBDA(x,x*2)(21)");
    b.formula((1, 0), "DOUBLE(5)");
    b.formula((2, 0), "FACT2(5)"); // recursive: 120
    // Wrong arity is a mistake, not a default.
    b.formula((3, 0), "DOUBLE(1,2)");
    // A LAMBDA never called has no value of its own.
    b.formula((4, 0), "LAMBDA(x,x)");
    // Currying: a LAMBDA returning a LAMBDA, invoked twice.
    b.formula((5, 0), "LAMBDA(x,LAMBDA(y,x+y))(3)(4)");
    let mut wb = b.build();
    for (name, text) in [
        ("DOUBLE", "LAMBDA(n,n*2)"),
        ("FACT2", "LAMBDA(n,IF(n<=1,1,n*FACT2(n-1)))"),
    ] {
        wb.defined_names.push(casual_calc_model::DefinedName {
            name: name.to_owned(),
            sheet: None,
            formula: parse(text).unwrap(),
        });
    }
    recalculate(&mut wb);

    assert_eq!(value_at(&wb, 0, 0), CellValue::Number(42.0));
    assert_eq!(value_at(&wb, 1, 0), CellValue::Number(10.0));
    assert_eq!(value_at(&wb, 2, 0), CellValue::Number(120.0));
    assert_eq!(
        value_at(&wb, 3, 0),
        CellValue::Error(casual_calc_model::ErrorValue::Value)
    );
    assert_eq!(
        value_at(&wb, 4, 0),
        CellValue::Error(casual_calc_model::ErrorValue::Calc)
    );
    assert_eq!(value_at(&wb, 5, 0), CellValue::Number(7.0));
}

/// A recursive LAMBDA with no base case must stop, not take the process down.
#[test]
fn runaway_recursion_is_an_error_not_a_crash() {
    use casual_calc_formula::parse;
    let mut b = Builder::new();
    b.formula((0, 0), "FOREVER(1)");
    let mut wb = b.build();
    wb.defined_names.push(casual_calc_model::DefinedName {
        name: "FOREVER".to_owned(),
        sheet: None,
        formula: parse("LAMBDA(n,FOREVER(n+1))").unwrap(),
    });
    recalculate(&mut wb);
    assert_eq!(
        value_at(&wb, 0, 0),
        CellValue::Error(casual_calc_model::ErrorValue::Num)
    );
}

/// A defined name must not shadow a builtin: silently preferring the user's
/// `SUM` would change every existing formula in the file.
#[test]
fn a_lambda_cannot_take_over_a_builtin_name() {
    use casual_calc_formula::parse;
    let mut b = Builder::new();
    b.number((0, 0), 2.0).number((1, 0), 3.0);
    b.formula((0, 2), "SUM(A1:A2)");
    let mut wb = b.build();
    wb.defined_names.push(casual_calc_model::DefinedName {
        name: "SUM".to_owned(),
        sheet: None,
        formula: parse("LAMBDA(x,999)").unwrap(),
    });
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 0, 2), CellValue::Number(5.0));
}

/// The LAMBDA helpers are why first-class functions matter: a user-defined
/// function you cannot hand to anything can only ever be called by name.
#[test]
fn lambda_helpers_map_reduce_and_scan() {
    let mut b = Builder::new();
    for i in 0..4u32 {
        b.number((i, 0), (i + 1) as f64); // 1..4
    }
    b.formula((0, 2), "MAP(A1:A4,LAMBDA(v,v*10))"); // 10,20,30,40
    b.formula((0, 4), "REDUCE(0,A1:A4,LAMBDA(acc,v,acc+v))"); // 10
    b.formula((0, 6), "SCAN(0,A1:A4,LAMBDA(acc,v,acc+v))"); // running total
    b.formula((0, 8), "MAKEARRAY(2,2,LAMBDA(r,c,r*10+c))");
    let mut wb = b.build();
    recalculate(&mut wb);
    let n = |r: u32, c: u32| match value_at(&wb, r, c) {
        CellValue::Number(v) => v,
        other => panic!("({r},{c}): {other:?}"),
    };
    assert_eq!((n(0, 2), n(3, 2)), (10.0, 40.0));
    assert_eq!(n(0, 4), 10.0, "REDUCE gives the final accumulator");
    assert_eq!(
        (n(0, 6), n(1, 6), n(3, 6)),
        (1.0, 3.0, 10.0),
        "SCAN gives each"
    );
    // MAKEARRAY's lambda takes one-based row and column, as ROW() and COLUMN()
    // report them.
    assert_eq!(
        (n(0, 8), n(0, 9), n(1, 8), n(1, 9)),
        (11.0, 12.0, 21.0, 22.0)
    );
}

/// BYROW and BYCOL hand a whole slice to the lambda, so an aggregate can be
/// applied to it — and a row-wise result is a column of answers.
#[test]
fn byrow_and_bycol_pass_slices() {
    let mut b = Builder::new();
    // 1 2 / 3 4
    b.number((0, 0), 1.0).number((0, 1), 2.0);
    b.number((1, 0), 3.0).number((1, 1), 4.0);
    b.formula((0, 3), "BYROW(A1:B2,LAMBDA(r,SUM(r)))"); // 3, 7 down
    b.formula((4, 3), "BYCOL(A1:B2,LAMBDA(c,SUM(c)))"); // 4, 6 across
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 0, 3), CellValue::Number(3.0));
    assert_eq!(value_at(&wb, 1, 3), CellValue::Number(7.0));
    assert_eq!(value_at(&wb, 4, 3), CellValue::Number(4.0));
    assert_eq!(value_at(&wb, 4, 4), CellValue::Number(6.0));
}

/// A spilling formula must survive an unrelated edit elsewhere on the sheet.
///
/// It did not: its own spilled cells were counted as obstructions on the next
/// pass, so the anchor turned itself into `#SPILL!` the moment anything else
/// was typed. A genuine obstruction still blocks.
#[test]
fn a_spill_is_not_blocked_by_its_own_previous_output() {
    let mut b = Builder::new();
    for i in 0..4u32 {
        b.number((i, 0), (i + 1) as f64);
    }
    b.formula((0, 2), "TRANSPOSE(A1:A4)");
    let mut wb = b.build();
    recalculate(&mut wb);
    assert_eq!(value_at(&wb, 0, 5), CellValue::Number(4.0));

    // Something unrelated, far away, forcing another pass.
    wb.sheets[0]
        .cells
        .set(CellRef::new(20, 20), Cell::value(CellValue::Number(1.0)));
    recalculate_incremental(&mut wb, &[(0, CellRef::new(20, 20))]);
    assert_eq!(
        value_at(&wb, 0, 2),
        CellValue::Number(1.0),
        "the anchor must not have turned itself into #SPILL!"
    );
    assert_eq!(value_at(&wb, 0, 5), CellValue::Number(4.0));

    // Real data in the way still blocks — that is the rule this must not break.
    wb.sheets[0]
        .cells
        .set(CellRef::new(0, 4), Cell::value(CellValue::Number(99.0)));
    recalculate(&mut wb);
    assert_eq!(
        value_at(&wb, 0, 2),
        CellValue::Error(casual_calc_model::ErrorValue::Spill)
    );
    assert_eq!(value_at(&wb, 0, 4), CellValue::Number(99.0), "untouched");
}

/// Iterative calculation: a formula that depends on itself, on purpose.
///
/// P2-004. Circular references were detected and reported, never resolved — so
/// a workbook whose author enabled iteration, which is the only way some
/// financial models can be written at all, opened here as a sheet of `#REF!`.
/// `docs/29`'s note on `<calcPr>` said exactly this would happen: the settings
/// have been carried verbatim since before there was an engine to read them.
mod iterative {
    use casual_calc_formula::parse;
    use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

    use crate::recalculate;

    /// `A1 = A1 + 1`, the smallest loop there is, with iteration configurable.
    fn self_incrementing(settings: &[(&str, &str)]) -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        for (k, v) in settings {
            wb.settings.calc.insert((*k).to_owned(), (*v).to_owned());
        }
        let handle = wb.store_formula(parse("A1+1").unwrap());
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        sheet.cells.set(
            CellRef::new(0, 0),
            Cell {
                formula: Some(handle),
                ..Cell::default()
            },
        );
        wb.sheets.push(sheet);
        wb
    }

    fn value(wb: &Workbook) -> CellValue {
        wb.sheets[0]
            .cells
            .get(CellRef::new(0, 0))
            .unwrap()
            .value
            .clone()
    }

    #[test]
    fn without_iteration_a_loop_is_still_an_error() {
        // Unchanged behaviour, asserted so the new path cannot quietly become
        // the old one: a circular reference nobody asked for is a mistake, and
        // silently returning a number would hide it.
        let mut wb = self_incrementing(&[]);
        recalculate(&mut wb);
        assert_eq!(
            value(&wb),
            CellValue::Error(casual_calc_model::ErrorValue::Ref)
        );
    }

    #[test]
    fn with_iteration_the_loop_runs_the_number_of_passes_asked_for() {
        // `A1 = A1 + 1` never converges, so the count is what stops it — and
        // makes the result exactly countable: five passes from empty is 5.
        let mut wb = self_incrementing(&[("iterate", "1"), ("iterateCount", "5")]);
        recalculate(&mut wb);
        assert_eq!(value(&wb), CellValue::Number(5.0));
    }

    #[test]
    fn a_converging_loop_stops_early_rather_than_running_the_full_count() {
        // `A1 = (A1 + 10) / 2` converges on 10. With a hundred passes allowed
        // and a loose tolerance it must stop well before the hundredth, which
        // is the difference between a convergence test and a fixed loop.
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        wb.settings.calc.insert("iterate".into(), "1".into());
        wb.settings.calc.insert("iterateCount".into(), "100".into());
        wb.settings.calc.insert("iterateDelta".into(), "0.5".into());
        let handle = wb.store_formula(parse("(A1+10)/2").unwrap());
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        sheet.cells.set(
            CellRef::new(0, 0),
            Cell {
                formula: Some(handle),
                ..Cell::default()
            },
        );
        wb.sheets.push(sheet);
        recalculate(&mut wb);

        let CellValue::Number(n) = value(&wb) else {
            panic!("expected a number, got {:?}", value(&wb));
        };
        // Halving the gap each pass from 0: 5, 7.5, 8.75… The tolerance of 0.5
        // is first met between the fourth and fifth pass, so it stops there —
        // near 10 but not at it, which is what "converged to a tolerance" means.
        assert!(n > 9.0 && n < 10.0, "stopped short of the cap at {n}");
    }

    #[test]
    fn a_zero_iteration_count_does_not_hang() {
        // A degenerate setting a file may legitimately carry.
        let mut wb = self_incrementing(&[("iterate", "1"), ("iterateCount", "0")]);
        recalculate(&mut wb);
        assert!(matches!(value(&wb), CellValue::Number(_)));
    }

    #[test]
    fn the_settings_are_read_from_the_file_and_default_to_excels() {
        use casual_calc_model::WorkbookSettings;

        let mut settings = WorkbookSettings::default();
        assert!(!settings.iteration().enabled, "off unless the file says so");

        settings.calc.insert("iterate".into(), "1".into());
        let it = settings.iteration();
        assert!(it.enabled);
        assert_eq!(it.max_count, 100, "Excel's default");
        assert!((it.max_change - 0.001).abs() < f64::EPSILON);

        // A malformed limit falls back rather than disabling the loop: the
        // author asked for iteration, and refusing over a detail they cannot
        // see would turn a working model into a sheet of errors.
        settings.calc.insert("iterateCount".into(), "lots".into());
        assert_eq!(settings.iteration().max_count, 100);
    }

    #[test]
    fn iteration_does_not_disturb_a_workbook_that_has_no_loop() {
        // The cost of the feature for the workbooks that do not use it.
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        wb.settings.calc.insert("iterate".into(), "1".into());
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        sheet
            .cells
            .set(CellRef::new(0, 0), Cell::value(CellValue::Number(4.0)));
        let handle = wb.store_formula(parse("A1*2").unwrap());
        sheet.cells.set(
            CellRef::new(1, 0),
            Cell {
                formula: Some(handle),
                ..Cell::default()
            },
        );
        wb.sheets.push(sheet);
        recalculate(&mut wb);
        assert_eq!(
            wb.sheets[0].cells.get(CellRef::new(1, 0)).unwrap().value,
            CellValue::Number(8.0)
        );
    }
}

/// **Which cell gets which `RAND()` draw must not depend on hash order.**
///
/// `apply_dirty` walked the dirty set straight out of a `HashSet`, whose
/// iteration order is seeded per process. `Evaluator::next_random` draws from a
/// counter incremented per draw, so the walk order decided the assignment:
/// identical input produced a different workbook — and different saved bytes —
/// on every run. Priority 2 in AGENTS.md, and the one property a spreadsheet
/// engine cannot negotiate.
///
/// A single-process test cannot observe the seed varying, so this asserts the
/// stronger property instead: the incremental path assigns draws exactly as the
/// full path does. The full path walks sheets and cells in order and has always
/// been deterministic, so pinning the two together pins the incremental one to a
/// fixed order without asserting *which* order in a way that a later change to
/// `next_random` would have to come and edit.
///
/// Thirty volatile cells, so an unsorted walk that happened to match sorted
/// order is not a thing that can occur.
#[test]
fn incremental_assigns_volatile_draws_in_the_same_order_as_a_full_recalc() {
    let build = || {
        let mut b = Builder::new();
        b.number((0, 0), 1.0);
        for row in 0..30u32 {
            b.formula((row, 1), "RAND()");
        }
        let mut wb = b.build();
        wb.volatile_seed = 12345;
        recalculate(&mut wb);
        wb
    };

    let mut incr = build();
    let changed = set_number(&mut incr, 0, 0, 2.0);
    recalculate_incremental(&mut incr, &[changed]);

    let mut full = build();
    set_number(&mut full, 0, 0, 2.0);
    recalculate(&mut full);

    let draws = |wb: &Workbook| -> Vec<CellValue> { (0..30).map(|r| value_at(wb, r, 1)).collect() };
    assert_eq!(
        draws(&incr),
        draws(&full),
        "the incremental path assigned the volatile draws to different cells \
         than a full recalculation did"
    );
}

/// **A logical held in a cell is not a number, and Excel does not count it.**
///
/// `flatten_numbers` coerced `Value::Bool` to 1/0 in its two *reference*
/// branches, so a column with a `TRUE` in it corrupted every aggregate over it —
/// and `AVERAGE` twice over, because the boolean inflated the sum and the
/// divisor together. Text was already skipped by the very next arm of the same
/// match, so the two were treated inconsistently in one expression.
///
/// Excel's rule is about how the value arrived, not what it is: written as an
/// argument a logical counts, read out of a reference it does not. Both halves
/// are asserted here, because a fix that skipped logicals everywhere would pass
/// the first half of this test and break `=SUM(TRUE,1)`.
#[test]
fn aggregates_ignore_logicals_held_in_a_range_but_not_ones_written_as_arguments() {
    let mut b = Builder::new();
    b.boolean((0, 0), true) // A1 = TRUE
        .number((1, 0), 10.0) // A2 = 10
        .formula((0, 2), "SUM(A1:A2)")
        .formula((1, 2), "AVERAGE(A1:A2)")
        .formula((2, 2), "COUNT(A1:A2)")
        .formula((3, 2), "MIN(A1:A2)")
        .formula((4, 2), "MAX(A1:A2)")
        // Written as arguments, logicals still count — the other half of the rule.
        .formula((5, 2), "SUM(TRUE,1)")
        .formula((6, 2), "COUNT(TRUE,1)")
        // The A-variants are the ones that do count a logical in a reference.
        .formula((7, 2), "AVERAGEA(A1:A2)");
    let mut wb = b.build();
    recalculate(&mut wb);

    assert_eq!(
        value_at(&wb, 0, 2),
        CellValue::Number(10.0),
        "SUM ignores TRUE in a range"
    );
    assert_eq!(
        value_at(&wb, 1, 2),
        CellValue::Number(10.0),
        "AVERAGE must not count TRUE in either the total or the divisor"
    );
    assert_eq!(
        value_at(&wb, 2, 2),
        CellValue::Number(1.0),
        "COUNT counts numbers only"
    );
    assert_eq!(
        value_at(&wb, 3, 2),
        CellValue::Number(10.0),
        "MIN skips the TRUE"
    );
    assert_eq!(
        value_at(&wb, 4, 2),
        CellValue::Number(10.0),
        "MAX skips the TRUE"
    );
    assert_eq!(
        value_at(&wb, 5, 2),
        CellValue::Number(2.0),
        "SUM(TRUE,1) is still 2"
    );
    assert_eq!(
        value_at(&wb, 6, 2),
        CellValue::Number(2.0),
        "COUNT(TRUE,1) is still 2"
    );
    assert_eq!(
        value_at(&wb, 7, 2),
        CellValue::Number(5.5),
        "AVERAGEA is the function that does count a logical in a reference"
    );
}

/// **Excel's comparison rules, as a matrix.**
///
/// `comparison()` tried `as_number()` on both operands first, and `as_number`
/// parses text — so `="1"=1` was TRUE and `=TRUE=1` was TRUE, neither of which
/// Excel agrees with. The text fallback compared raw UTF-8 bytes, so comparison
/// was case-sensitive and code-point-ordered.
///
/// Asserted as the whole matrix in one place because the rules are a system:
/// fixing the coercion without fixing the ordering, or either without the
/// contextual empty, produces a comparator that is differently wrong. See
/// docs/70-COMPARISON-SEMANTICS.md.
#[test]
fn comparison_follows_excel_type_ordering() {
    let mut b = Builder::new();
    b.number((0, 0), 1.0) // A1 = 1
        .text((1, 0), "Yes") // A2 = "Yes"
        .boolean((2, 0), true) // A3 = TRUE
        // (A4 deliberately left empty)
        // No coercion across types.
        .formula((0, 2), "IF(\"1\"=1,1,0)") // Excel: 0
        .formula((1, 2), "IF(TRUE=1,1,0)") // Excel: 0
        // number < text < logical
        .formula((2, 2), "IF(1<\"a\",1,0)") // Excel: 1
        .formula((3, 2), "IF(\"a\"<TRUE,1,0)") // Excel: 1
        // Text compares case-insensitively.
        .formula((4, 2), "IF(A2=\"YES\",1,0)") // Excel: 1
        .formula((5, 2), "IF(\"apple\"<\"Banana\",1,0)") // Excel: 1
        // An empty cell takes the shape of the other operand.
        .formula((6, 2), "IF(A4=0,1,0)") // Excel: 1
        .formula((7, 2), "IF(A4=\"\",1,0)") // Excel: 1
        // FALSE < TRUE.
        .formula((8, 2), "IF(FALSE()<TRUE(),1,0)") // Excel: 1
        // Errors propagate rather than comparing as text.
        .formula((9, 2), "ISERROR(1=(1/0))"); // Excel: TRUE
    let mut wb = b.build();
    recalculate(&mut wb);

    let cases = [
        (0, "\"1\"=1 must be FALSE — text is not a number", 0.0),
        (1, "TRUE=1 must be FALSE — a logical is not a number", 0.0),
        (2, "every number sorts before every text", 1.0),
        (3, "every text sorts before every logical", 1.0),
        (4, "text comparison is case-insensitive", 1.0),
        (5, "and ordered case-insensitively, not by code point", 1.0),
        (6, "an empty cell equals 0 beside a number", 1.0),
        (7, "and equals \"\" beside text", 1.0),
        (8, "FALSE sorts before TRUE", 1.0),
    ];
    for (row, why, expect) in cases {
        assert_eq!(value_at(&wb, row, 2), CellValue::Number(expect), "{why}");
    }
    assert_eq!(
        value_at(&wb, 9, 2),
        CellValue::Bool(true),
        "an error on either side of a comparison is the result"
    );
}

/// **The two halves of the engine must agree about what matches.**
///
/// This is the defect that made the comparison rules worth fixing rather than
/// merely wrong. `COUNTIF` went through `criterion_matches`, which upper-cases;
/// `SUM(IF(range="yes",…))` went through `comparison`, which did not. The same
/// data gave two different answers depending on which function the user reached
/// for — reported as "the numbers don't add up" and reproducible by nobody.
#[test]
fn countif_and_a_comparison_agree() {
    let mut b = Builder::new();
    b.text((0, 0), "yes")
        .text((1, 0), "YES")
        .text((2, 0), "Yes")
        .text((3, 0), "no")
        .formula((0, 2), "COUNTIF(A1:A4,\"yes\")")
        // Written cell by cell rather than as `A1:A4="yes"`, which would also
        // be asking whether a range broadcasts against a scalar — a separate
        // question, and one this test would then answer confusingly.
        .formula(
            (1, 2),
            "IF(A1=\"yes\",1,0)+IF(A2=\"yes\",1,0)+IF(A3=\"yes\",1,0)+IF(A4=\"yes\",1,0)",
        );
    let mut wb = b.build();
    recalculate(&mut wb);

    assert_eq!(
        value_at(&wb, 0, 2),
        value_at(&wb, 1, 2),
        "COUNTIF and an `=` comparison disagree about the same four cells"
    );
    assert_eq!(
        value_at(&wb, 0, 2),
        CellValue::Number(3.0),
        "and both find three"
    );
}

/// **Serial 60 is 1900-02-29, a day that never happened.**
///
/// Lotus 1-2-3 treated 1900 as a leap year; Excel reproduced the bug on purpose
/// so those files' arithmetic kept working, and every spreadsheet since has
/// reproduced Excel. This engine computed a straight proleptic Gregorian offset
/// and skipped all of it, so every serial from 1 to 60 was one too high on the
/// way in and one too low on the way out.
///
/// Both directions and both sides of the boundary are asserted, because a fix
/// applied to only one conversion is worse than the bug: `DATE()` and `DAY()`
/// would stop being inverses of each other.
#[test]
fn the_1900_leap_year_bug_is_reproduced_as_excel_has_it() {
    let mut b = Builder::new();
    b.formula((0, 0), "DATE(1900,1,1)") // 1
        .formula((1, 0), "DATE(1900,2,28)") // 59
        .formula((2, 0), "DATE(1900,2,29)") // 60 — the phantom day
        .formula((3, 0), "DATE(1900,3,1)") // 61
        .formula((4, 0), "DAY(59)") // 28
        .formula((5, 0), "DAY(60)") // 29
        .formula((6, 0), "DAY(61)") // 1
        .formula((7, 0), "MONTH(60)") // 2
        // Unaffected either side of the boundary — the correction must not
        // leak into ordinary dates.
        .formula((8, 0), "DATE(2020,1,1)") // 43831
        .formula((9, 0), "DATE(2024,2,29)") // 45351, a real leap day
        // Round-trip: the two conversions stay inverses across the boundary.
        .formula((10, 0), "DAY(DATE(1900,1,15))") // 15
        .formula((11, 0), "YEAR(DATE(1900,2,28))"); // 1900
    let mut wb = b.build();
    recalculate(&mut wb);

    for (row, expect, why) in [
        (0u32, 1.0, "1900-01-01 is serial 1, not 2"),
        (1, 59.0, "1900-02-28 is serial 59"),
        (2, 60.0, "the phantom 1900-02-29 is serial 60"),
        (3, 61.0, "1900-03-01 is serial 61, where both systems agree"),
        (4, 28.0, "serial 59 is the 28th"),
        (5, 29.0, "serial 60 is the phantom 29th"),
        (6, 1.0, "serial 61 is the 1st"),
        (7, 2.0, "and the phantom day is in February"),
        (8, 43831.0, "an ordinary modern date is untouched"),
        (9, 45351.0, "and so is a real leap day"),
        (10, 15.0, "DATE and DAY are inverses below the boundary"),
        (11, 1900.0, "and DATE and YEAR are too"),
    ] {
        assert_eq!(value_at(&wb, row, 0), CellValue::Number(expect), "{why}");
    }
}

// --- Cancellation (SEC-012) --------------------------------------------------

mod cancellation {
    use super::*;
    use crate::{Recalculated, recalculate_cancellable};
    use casual_calc_model::{CancelFlag, Never};
    use std::cell::Cell as StdCell;

    /// Enough formula cells for the periodic check to come round twice — once
    /// cannot distinguish a check inside the loop from one at the top of it.
    const FORMULAS: u32 = 9000;

    fn many_formulas() -> Workbook {
        let mut builder = Builder::new();
        builder.number((0, 0), 2.0);
        for r in 1..=FORMULAS {
            builder.formula((r, 0), "A1*2");
        }
        builder.build()
    }

    fn value_at(wb: &Workbook, row: u32) -> CellValue {
        wb.sheets[0]
            .cells
            .get(CellRef::new(row, 0))
            .map(|c| c.value.clone())
            .unwrap_or(CellValue::Empty)
    }

    /// **A full recalculation stops when it is asked to.**
    #[test]
    fn a_recalculation_stops_when_asked() {
        let mut wb = many_formulas();
        let stop = CancelFlag::new();
        stop.cancel();

        assert_eq!(
            recalculate_cancellable(&mut wb, &stop),
            Recalculated::Cancelled
        );
    }

    /// **It stops part-way, not only before it starts.**
    #[test]
    fn a_recalculation_stops_after_it_has_begun() {
        let mut wb = many_formulas();
        let asks = StdCell::new(0);
        let stop_on_the_second_ask = || {
            asks.set(asks.get() + 1);
            asks.get() >= 2
        };

        assert_eq!(
            recalculate_cancellable(&mut wb, &stop_on_the_second_ask),
            Recalculated::Cancelled
        );
        assert!(
            asks.get() >= 2,
            "asked {} time(s); a check at the top of the loop cannot stop a running job",
            asks.get()
        );
    }

    /// **The caller is told, rather than handed a half-fresh document as
    /// though it were finished.**
    ///
    /// Unlike an import there is nothing to fail closed about — the workbook is
    /// the user's own and was already holding stale cached values. Throwing the
    /// fresh ones away would leave it strictly worse. What must not happen is
    /// the *caller* believing the result is final, and then saving it.
    #[test]
    fn a_cancelled_recalculation_says_so_rather_than_looking_complete() {
        let mut wb = many_formulas();
        let stop = CancelFlag::new();
        stop.cancel();
        assert_eq!(
            recalculate_cancellable(&mut wb, &stop),
            Recalculated::Cancelled,
            "a cancelled recalc that reported success is one a host would save"
        );

        // And running it properly afterwards completes the job.
        assert_eq!(
            recalculate_cancellable(&mut wb, &Never),
            Recalculated::Fully
        );
        assert_eq!(value_at(&wb, 1), CellValue::Number(4.0));
        assert_eq!(value_at(&wb, FORMULAS), CellValue::Number(4.0));
    }

    /// **A token that never fires computes exactly what no token computes.**
    #[test]
    fn a_token_that_never_fires_is_invisible() {
        let mut with_token = many_formulas();
        let mut without = many_formulas();

        assert_eq!(
            recalculate_cancellable(&mut with_token, &Never),
            Recalculated::Fully
        );
        recalculate(&mut without);

        for row in [1, FORMULAS / 2, FORMULAS] {
            assert_eq!(
                value_at(&with_token, row),
                value_at(&without, row),
                "row {row}"
            );
        }
        assert_eq!(value_at(&without, FORMULAS), CellValue::Number(4.0));
    }
}
