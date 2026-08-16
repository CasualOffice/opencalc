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
        // The associativity traps, which a fully-bracketed printer could not
        // get wrong and a minimal one can.
        "2^3^2",
        "(2^3)^2",
        "1-2-3",
        "1-(2-3)",
        "8/4/2",
        "8/(4/2)",
        "-2^2",
        "-(2^2)",
        "1+-2",
        "-A1%",
        "(-A1)%",
        "(1+2)%",
        "1<2=TRUE",
        "\"a\"&\"b\"&\"c\"",
        "1+2&\"x\"",
        "(1+2)*(3-4)",
        "SUM(A1:A3)/COUNT(A1:A3)",
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

/// A whole-column or whole-row reference must survive a round trip through the
/// AST unchanged. Excel writes `Print_Titles` as `Sheet1!$1:$2`, and a reader
/// that reformats it as `A1:XFD2` has written a different thing.
#[test]
fn axis_references_parse_and_print_back_verbatim() {
    for text in [
        "A:A",
        "$A:$C",
        "Sheet1!$1:$2",
        "SUM(A:A)",
        "SUM(Sheet1!$1:$1)",
    ] {
        let expr = parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(expr.to_string(), text, "round trip");
    }
}

/// Only a following colon makes an axis token a reference. Without this, every
/// one-letter defined name would become a column reference.
#[test]
fn a_bare_column_letter_is_still_a_name() {
    assert!(matches!(parse("A").unwrap(), Expr::Name(n) if n == "A"));
    assert!(matches!(parse("A + 1").unwrap(), Expr::Binary { .. }));
    // ...and the range form does resolve to a reference.
    assert!(matches!(parse("A:A").unwrap(), Expr::Range(..)));
}

/// The unnamed side carries the sheet's limit so ordinary consumers see a valid
/// span, and the flag is what tells the printer not to show it.
#[test]
fn an_unnamed_axis_spans_the_sheet_but_is_marked() {
    let Expr::Range(a, b) = parse("A:A").unwrap() else {
        panic!("expected a range");
    };
    assert!(a.row_implicit && b.row_implicit);
    assert_eq!((a.row, b.row), (0, crate::MAX_ROW));
    assert_eq!((a.col, b.col), (0, 0));
}

/// Copying `A:A` down must not turn it into `A2:A1048576`.
#[test]
fn shifting_leaves_an_unnamed_axis_alone() {
    let shifted = crate::shift_references(&parse("SUM(A:A)").unwrap(), 5, 0);
    assert_eq!(shifted.to_string(), "SUM(A:A)");
    // A named axis still shifts, so the guard is not simply disabling the shift.
    let normal = crate::shift_references(&parse("SUM(A1:A3)").unwrap(), 5, 0);
    assert_eq!(normal.to_string(), "SUM(A6:A8)");
}

/// A formula typed the way a person writes one comes back as they wrote it.
///
/// Stronger than the AST round-trip above, and only true since the printer
/// stopped bracketing every operator: `parse` then `print` is the identity on
/// text that is already minimally bracketed. This is the property a user
/// actually experiences, because the formula bar shows the *printed* tree —
/// there is no record of the original text to fall back on — so anything this
/// test allows to drift is a formula the editor rewrites under them, and
/// anything it saves is what Excel will show when the file is opened there.
#[test]
fn prints_what_was_typed() {
    for input in [
        // Precedence that needs no help.
        "1+2*3",
        "2*3+1",
        "1+2*3-4/5",
        "A1*B1+C1",
        "-A1+B1",
        "A1&B1&C1",
        "A1<B1",
        "SUM(A1:A9)/COUNT(A1:A9)",
        "IF(A1>0,B1*2,C1/2)",
        // Brackets the reader genuinely needs, which must survive.
        "(1+2)*3",
        "8/(4/2)",
        "1-(2-3)",
        "(2^3)^2",
        "-(2^2)",
        "(A1+B1)/(C1-D1)",
        "(1+2)%",
        // Right-associative `^` needs none.
        "2^3^2",
        // Nothing to bracket at all.
        "42",
        "A1",
        "$B$7",
        "Sheet2!C3",
        "SUM(A1:B2,3)",
        "10%",
        "TRUE",
    ] {
        let printed = parse(input)
            .unwrap_or_else(|e| panic!("parse {input:?}: {e}"))
            .to_string();
        assert_eq!(
            printed, input,
            "the editor would show {printed:?} for a cell typed as {input:?}"
        );
    }
}

/// Brackets that were never needed are dropped, which is the change itself.
///
/// Pinned as its own behaviour rather than left implied: printing is not
/// verbatim — it is the tree — so redundant brackets do not survive, and that
/// is correct. Only the ones the grammar requires come back.
#[test]
fn redundant_brackets_are_not_preserved() {
    for (typed, shown) in [
        ("(1+2*3)", "1+2*3"),
        ("1+(2*3)", "1+2*3"),
        ("((A1))", "A1"),
        ("(SUM(A1:A3))", "SUM(A1:A3)"),
        ("2^(3^2)", "2^3^2"),
        ("(1-2)-3", "1-2-3"),
    ] {
        assert_eq!(parse(typed).unwrap().to_string(), shown);
    }
}

/// The bounds that keep an untrusted formula from aborting the process.
///
/// PROD-02. A stack overflow is `SIGABRT` — not an `Err`, not a catchable
/// panic, and not something a `Result` signature can express — so it has to be
/// prevented rather than handled, and prevented *here*, because every caller
/// downstream is powerless. Reachable from an imported `.xlsx`, from the
/// formula bar and from the collaboration wire, where one document would take
/// down every other on the node.
mod depth_bounds {
    use crate::FormulaError;
    use crate::parse::{MAX_CHAIN, MAX_DEPTH};

    use super::*;

    fn nested(open: &str, close: &str, times: usize) -> String {
        format!("{}1{}", open.repeat(times), close.repeat(times))
    }

    #[test]
    fn nesting_is_refused_rather_than_followed_off_the_stack() {
        // Verified before the fix: twenty thousand of these was
        // "fatal runtime error: stack overflow, aborting".
        for src in [
            nested("(", ")", 20_000),
            nested("SUM(", ")", 20_000),
            format!("{}1", "-".repeat(20_000)),
        ] {
            assert!(
                matches!(parse(&src), Err(FormulaError::TooDeep { .. })),
                "should refuse a {}-deep expression",
                src.len()
            );
        }
    }

    #[test]
    fn a_long_operator_chain_is_refused_even_though_parsing_it_never_recurses() {
        // The second, stranger crash. A left-associative chain is parsed in a
        // *loop*, so the recursion counter never rises — and the tree it builds
        // is a left spine as long as the chain, which `Drop` and `Display` walk
        // recursively. It parsed cleanly and aborted on the way out of scope.
        let src = format!("1{}", "+1".repeat(200_000));
        assert!(matches!(parse(&src), Err(FormulaError::TooDeep { .. })));
    }

    #[test]
    fn what_is_accepted_can_also_be_printed_and_dropped() {
        // The property that actually matters: the parser must not hand back a
        // tree that the rest of the program cannot walk. Printing and dropping
        // are both recursive, and both were where the chain bug landed.
        for src in [
            nested("(", ")", (MAX_DEPTH - 1) as usize),
            format!("1{}", "+1".repeat((MAX_CHAIN - 1) as usize)),
        ] {
            let expr = parse(&src).expect("inside the bounds");
            let printed = expr.to_string();
            assert!(!printed.is_empty());
            assert!(parse(&printed).is_ok(), "and it round-trips");
            drop(expr);
        }
    }

    #[test]
    fn the_limits_leave_room_for_formulas_people_actually_write() {
        // Excel stops at 64 levels of function nesting and 8,192 characters of
        // formula, so both bounds are generous against the format rather than
        // tight against the stack.
        assert!(
            parse(&nested("SUM(", ")", 64)).is_ok(),
            "Excel's own ceiling"
        );
        assert!(parse("SUM(A1:A9)/COUNT(A1:A9)+MAX(B1:B9)").is_ok());
        assert!(
            parse(&format!("1{}", "+1".repeat((MAX_CHAIN - 1) as usize))).is_ok(),
            "and a chain up to the measured limit"
        );
    }

    #[test]
    fn the_chain_limit_is_measured_against_the_smallest_stack_not_the_largest() {
        // The first attempt at this bound reasoned from the file format — 8,192
        // characters, so about four thousand operators — passed on the main
        // thread's eight megabytes, and aborted the moment a *test* thread ran
        // it with two. This runs the accepted maximum on a megabyte, which is
        // what a WebAssembly thread gets, and does the three things that recurse
        // over the tree: print it, re-parse what was printed, and drop it.
        let survived = std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(|| {
                let src = format!("1{}", "+1".repeat((MAX_CHAIN - 1) as usize));
                let expr = parse(&src).expect("the accepted maximum");
                let printed = expr.to_string();
                assert!(parse(&printed).is_ok());
                drop(expr);
            })
            .expect("spawn")
            .join();
        assert!(
            survived.is_ok(),
            "the accepted maximum must fit the smallest stack"
        );
    }

    #[test]
    fn the_nesting_limit_is_excels_own() {
        // Measured against the stack and then found to coincide with Excel's
        // documented ceiling, which means it costs no compatibility. It is
        // deliberately *not* the evaluator's `MAX_DEPTH`: that one bounds a
        // chain of cell references, which no formula's own shape constrains.
        // Sixty-five expression levels: Excel's sixty-four levels of nesting,
        // plus the outermost expression they nest inside. Writing 64 here
        // rejects a formula Excel accepts, which is how this off-by-one showed
        // itself.
        assert_eq!(MAX_DEPTH, 65);
        assert!(
            parse(&nested("SUM(", ")", 64)).is_ok(),
            "exactly Excel's ceiling must parse"
        );
        assert!(matches!(
            parse(&nested("SUM(", ")", 65)),
            Err(FormulaError::TooDeep { .. })
        ));
    }
}

/// **A sheet name that is not an identifier has to be quoted, or the formula
/// does not survive being written down.**
///
/// The rule was "quote unless every character is alphanumeric or `_`", which a
/// sheet called `2024` passes — so it was written bare as `2024!A1`, text that
/// neither this parser nor Excel reads as a reference. On re-import the parse
/// failed, the stale cached value was kept and the formula dropped: the cell
/// became a constant that never recalculates.
///
/// Asserted as a round trip rather than against a literal string, because what
/// matters is that what we write, we can read — the exact quoting is an
/// implementation detail and the fixed point is the contract (docs/36).
#[test]
fn a_sheet_name_survives_being_printed_and_read_back() {
    for name in [
        "2024",       // starts with a digit
        "1",          // is a number
        "2025Q1",     // starts with a digit
        "A1",         // is itself a cell reference
        "XFD1048576", // the last cell, still a reference
        "R",          // reserved by R1C1
        "C",          //
        "R1C1",       // an R1C1 reference
        "My Sheet",   // a space
        "Q1'24",      // an apostrophe, which must be doubled
        "Sheet1",     // ordinary, and must stay unquoted
        "_hidden",    //
        "Data2024",   // digits, but not leading
    ] {
        let reference = crate::CellReference {
            sheet: Some(name.to_owned()),
            col: 0,
            row: 0,
            col_absolute: false,
            row_absolute: false,
            row_implicit: false,
            col_implicit: false,
        };
        let printed = format!("{reference}");
        let expression = format!("{printed}*2");
        let parsed = parse(&expression).unwrap_or_else(|e| {
            panic!("sheet {name:?} printed as {printed:?} and would not parse back: {e}")
        });
        // And it names the same sheet it started as.
        let found = format!("{parsed:?}");
        assert!(
            found.contains(name),
            "sheet {name:?} printed as {printed:?} and parsed back as something else: {found}"
        );
    }
}

/// The ordinary names stay bare, so this did not fix a round trip by quoting
/// everything and making every formula in every file noisier.
#[test]
fn an_ordinary_sheet_name_is_still_written_without_quotes() {
    let bare = |name: &str| {
        format!(
            "{}",
            crate::CellReference {
                sheet: Some(name.to_owned()),
                col: 0,
                row: 0,
                col_absolute: false,
                row_absolute: false,
                row_implicit: false,
                col_implicit: false,
            }
        )
    };
    assert_eq!(bare("Sheet1"), "Sheet1!A1");
    assert_eq!(bare("_x"), "_x!A1");
    assert_eq!(bare("Data2024"), "Data2024!A1");
    assert_eq!(bare("2024"), "'2024'!A1");
    assert_eq!(bare("A1"), "'A1'!A1");
}
