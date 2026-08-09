//! Parser + pretty-printer tests, including the round-trip fixed point.

use crate::ast::{BinaryOp, Expr, UnaryOp};
use crate::parse::parse;

fn num(n: f64) -> Expr {
    Expr::Number(n)
}

#[test]
fn parses_literals() {
    assert_eq!(parse("42").unwrap(), num(42.0));
    assert_eq!(parse("3.5").unwrap(), num(3.5));
    assert_eq!(parse("TRUE").unwrap(), Expr::Bool(true));
    assert_eq!(parse("false").unwrap(), Expr::Bool(false));
    assert_eq!(parse("\"hi\"").unwrap(), Expr::Text("hi".to_owned()));
    assert_eq!(parse("#REF!").unwrap(), Expr::Error("#REF!".to_owned()));
}

#[test]
fn parses_references_and_ranges() {
    let expr = parse("A1").unwrap();
    assert!(matches!(expr, Expr::Reference(r) if r.col == 0 && r.row == 0));
    let expr = parse("Sheet2!$B$7").unwrap();
    assert!(
        matches!(expr, Expr::Reference(r) if r.sheet.as_deref() == Some("Sheet2") && r.col_absolute)
    );
    let expr = parse("A1:B2").unwrap();
    assert!(matches!(expr, Expr::Range(a, b) if a.col == 0 && b.col == 1 && b.row == 1));
    let expr = parse("'My Sheet'!C3").unwrap();
    assert!(matches!(expr, Expr::Reference(r) if r.sheet.as_deref() == Some("My Sheet")));
}

#[test]
fn respects_precedence() {
    // 1 + 2 * 3  ->  1 + (2 * 3)
    let expr = parse("1+2*3").unwrap();
    let Expr::Binary {
        op: BinaryOp::Add,
        right,
        ..
    } = expr
    else {
        panic!("expected add at the root");
    };
    assert!(matches!(
        *right,
        Expr::Binary {
            op: BinaryOp::Multiply,
            ..
        }
    ));

    // 2 ^ 3 ^ 2 is right-associative -> 2 ^ (3 ^ 2)
    let expr = parse("2^3^2").unwrap();
    let Expr::Binary {
        op: BinaryOp::Power,
        right,
        ..
    } = expr
    else {
        panic!("expected power at the root");
    };
    assert!(matches!(
        *right,
        Expr::Binary {
            op: BinaryOp::Power,
            ..
        }
    ));
}

#[test]
fn parses_unary_and_percent() {
    assert!(matches!(
        parse("-A1").unwrap(),
        Expr::Unary {
            op: UnaryOp::Negate,
            ..
        }
    ));
    assert!(matches!(
        parse("10%").unwrap(),
        Expr::Unary {
            op: UnaryOp::Percent,
            ..
        }
    ));
}

#[test]
fn parses_functions() {
    let expr = parse("SUM(A1:B2, 3)").unwrap();
    let Expr::Function { name, args } = expr else {
        panic!("expected function");
    };
    assert_eq!(name, "SUM");
    assert_eq!(args.len(), 2);
    assert!(matches!(args[0], Expr::Range(..)));

    // Case folded; nested and empty-arg forms.
    assert!(matches!(parse("if(A1>0,1,-1)").unwrap(), Expr::Function { name, .. } if name == "IF"));
    assert!(
        matches!(parse("NOW()").unwrap(), Expr::Function { name, args } if name == "NOW" && args.is_empty())
    );
}

#[test]
fn roundtrips_through_pretty_printer() {
    // parse -> print -> parse must be a fixed point on the AST.
    for input in [
        "42",
        "3.5",
        "TRUE",
        "\"quote \"\"inside\"\"\"",
        "A1",
        "$B$7",
        "Sheet2!C3",
        "A1:B2",
        "1+2*3",
        "2^3^2",
        "-A1",
        "10%",
        "(1+2)*3",
        "SUM(A1:B2,3)",
        "IF(A1>=0,\"pos\",\"neg\")",
        "A1&\"x\"",
        "MyName+1",
    ] {
        let first = parse(input).unwrap_or_else(|e| panic!("parse {input:?}: {e}"));
        let printed = first.to_string();
        let second =
            parse(&printed).unwrap_or_else(|e| panic!("reparse {printed:?} (from {input:?}): {e}"));
        assert_eq!(
            first, second,
            "round-trip changed AST for {input:?} -> {printed:?}"
        );
    }
}

#[test]
fn rejects_malformed() {
    assert!(parse("1+").is_err());
    assert!(parse("(1+2").is_err());
    assert!(parse("1 2").is_err());
    assert!(parse("\"unterminated").is_err());
}

#[test]
fn structured_references_parse_and_print() {
    // The specifier is kept as written: resolving it needs the table's
    // geometry, which the parser cannot see. Round-tripping the text means a
    // formula survives even when no table of that name is present.
    for text in [
        "SUM(Sales[Amount])",
        "SUM(Sales[#All])",
        "SUM([Amount])",
        "SUM(Sales[[#Headers],[Amount]])",
    ] {
        let expr = parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(expr.to_string(), text, "round trip of {text}");
    }
}

#[test]
fn an_unterminated_structured_reference_is_an_error() {
    // Not silently treated as a name: `Sales[Amount` would otherwise parse as
    // the defined name `Sales` and quietly drop the column.
    assert!(parse("SUM(Sales[Amount)").is_err());
}
