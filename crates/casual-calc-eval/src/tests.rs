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
        auto_filter_ref: None,
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
