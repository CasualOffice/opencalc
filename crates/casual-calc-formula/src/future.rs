//! The `_xlfn.` prefix: how SpreadsheetML carries a function it predates.
//!
//! The file format's own function set was fixed in 2007. Everything added
//! since — `CONCAT`, `TEXTJOIN`, `SWITCH`, `XLOOKUP`, `LET`, `LAMBDA` and the
//! rest — is written into a `.xlsx` with an `_xlfn.` prefix, so that a reader
//! which does not know the function still knows that it is *a function it does
//! not know*, and shows `#NAME?` rather than mistaking it for a defined name.
//!
//! A writer that emits the bare name produces a file that is wrong in the worst
//! way available: it opens, it looks complete, and every cell using one of these
//! functions reads `#NAME?`. The formula is not recoverable by the user, because
//! nothing tells them what it used to say. This was OpenCalc's behaviour until
//! the oracle diff (P2-003) put a corpus of formulas through LibreOffice and
//! found nine of them coming back as `#NAME?` — none of which any test in this
//! repository could have caught, because both sides of every round-trip test
//! were this codebase.
//!
//! # Why the table lives here
//!
//! It is a fact about the file format, not about the language, and it belongs
//! with neither the reader nor the writer alone: the two must agree exactly, and
//! a list kept twice is a list that drifts. The walk belongs here for the plainer
//! reason that this crate owns [`Expr`].
//!
//! # What is still missing
//!
//! `LAMBDA`'s *parameters* carry their own `_xlpm.` prefix in the file, which
//! this does not yet write. A `LAMBDA` therefore still round-trips wrongly
//! through Excel; the function name is now right and the parameter names are
//! not.

use crate::ast::Expr;

/// The prefix SpreadsheetML puts on a function that postdates its function set.
const XLFN: &str = "_XLFN.";

/// A second prefix Excel writes on some worksheet-scoped future functions,
/// always after `_xlfn.`. Stripped on the way in; never written on the way out,
/// because it is optional and its absence is not an error.
const XLWS: &str = "_XLWS.";

/// The functions this engine implements that SpreadsheetML requires the
/// `_xlfn.` prefix for.
///
/// Sorted, and **only functions that are actually implemented** — a name here
/// that the evaluator does not know would make the writer decorate something it
/// should have refused instead. Add to it in the same change that adds the
/// function.
pub const FUTURE_FUNCTIONS: &[&str] = &[
    "BITAND",
    "BITLSHIFT",
    "BITOR",
    "BITRSHIFT",
    "BITXOR",
    "BYCOL",
    "BYROW",
    "COMBINA",
    "CONCAT",
    "DAYS",
    "FILTER",
    "IFNA",
    "IFS",
    "ISFORMULA",
    "ISOMITTED",
    "ISOWEEKNUM",
    "LAMBDA",
    "LET",
    "MAKEARRAY",
    "MAP",
    "MAXIFS",
    "MINIFS",
    "NUMBERVALUE",
    "PDURATION",
    "PERMUTATIONA",
    "REDUCE",
    "RRI",
    "SCAN",
    "SEC",
    "SECH",
    "SEQUENCE",
    "SHEET",
    "SHEETS",
    "SORT",
    "SORTBY",
    "SWITCH",
    "TEXTJOIN",
    "UNICHAR",
    "UNICODE",
    "UNIQUE",
    "XLOOKUP",
    "XMATCH",
];

/// Whether `name` must be written with the `_xlfn.` prefix.
#[must_use]
pub fn is_future_function(name: &str) -> bool {
    FUTURE_FUNCTIONS.binary_search(&name).is_ok()
}

/// Add the `_xlfn.` prefix to every future function in `expr`, for writing.
///
/// Idempotent: a name that already carries the prefix is left alone, so a tree
/// that has been through here twice is not `_xlfn._xlfn.CONCAT`.
pub fn qualify_future_functions(expr: &mut Expr) -> bool {
    map_function_names(expr, &mut |name| {
        if is_future_function(name) {
            Some(format!("{XLFN}{name}"))
        } else {
            None
        }
    })
}

/// Remove the `_xlfn.` (and `_xlfn._xlws.`) prefix from every function in
/// `expr`, for reading.
///
/// Unconditional about the name that follows: a file may legitimately carry a
/// prefixed function this engine does not implement, and stripping it gives the
/// evaluator the chance to report an unknown *function* rather than a mangled
/// one. The name is what the file said it was.
pub fn strip_future_prefixes(expr: &mut Expr) -> bool {
    map_function_names(expr, &mut |name| {
        let rest = name.strip_prefix(XLFN)?;
        Some(rest.strip_prefix(XLWS).unwrap_or(rest).to_owned())
    })
}

/// Apply `f` to every function name in the tree, replacing it where `f` returns
/// one. Returns whether anything changed.
fn map_function_names(expr: &mut Expr, f: &mut impl FnMut(&str) -> Option<String>) -> bool {
    let mut changed = false;
    match expr {
        Expr::Function { name, args } => {
            if let Some(next) = f(name) {
                *name = next;
                changed = true;
            }
            for arg in args {
                changed |= map_function_names(arg, f);
            }
        }
        // A first-class call — `LAMBDA(x,x+1)(3)`. The callee is an expression,
        // not a name, but it and the arguments can both contain named calls.
        Expr::Call { callee, args } => {
            changed |= map_function_names(callee, f);
            for arg in args {
                changed |= map_function_names(arg, f);
            }
        }
        Expr::Unary { operand, .. } => changed |= map_function_names(operand, f),
        Expr::Binary { left, right, .. } => {
            changed |= map_function_names(left, f);
            changed |= map_function_names(right, f);
        }
        _ => {}
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn qualified(src: &str) -> String {
        let mut expr = parse(src).unwrap();
        qualify_future_functions(&mut expr);
        expr.to_string()
    }

    fn stripped(src: &str) -> String {
        let mut expr = parse(src).unwrap();
        strip_future_prefixes(&mut expr);
        expr.to_string()
    }

    #[test]
    fn the_table_is_sorted_so_the_lookup_is_correct() {
        // `is_future_function` binary-searches it. An unsorted entry would not
        // fail loudly — it would simply never be found, and the function it
        // names would go on being written wrongly.
        let mut sorted = FUTURE_FUNCTIONS.to_vec();
        sorted.sort_unstable();
        assert_eq!(FUTURE_FUNCTIONS, sorted.as_slice());
        assert_eq!(
            sorted.len(),
            {
                sorted.dedup();
                sorted.len()
            },
            "and has no duplicates"
        );
    }

    #[test]
    fn a_future_function_is_written_with_the_prefix() {
        assert_eq!(
            qualified("CONCAT(\"a\",\"b\")"),
            "_XLFN.CONCAT(\"a\",\"b\")"
        );
        assert_eq!(
            qualified("XLOOKUP(1,A1:A3,B1:B3)"),
            "_XLFN.XLOOKUP(1,A1:A3,B1:B3)"
        );
    }

    #[test]
    fn a_function_the_format_already_knew_is_left_alone() {
        // The other half of the rule: prefixing `SUM` would break it just as
        // thoroughly in the other direction.
        assert_eq!(qualified("SUM(A1:A3)"), "SUM(A1:A3)");
        assert_eq!(qualified("IF(A1>0,1,2)"), "IF(A1>0,1,2)");
        assert_eq!(
            qualified("CONCATENATE(\"a\",\"b\")"),
            "CONCATENATE(\"a\",\"b\")"
        );
    }

    #[test]
    fn nesting_is_reached_on_both_sides() {
        assert_eq!(
            qualified("IF(ISFORMULA(A1),CONCAT(\"a\",TEXTJOIN(\",\",TRUE,B1:B3)),SUM(C1:C3))"),
            "IF(_XLFN.ISFORMULA(A1),_XLFN.CONCAT(\"a\",_XLFN.TEXTJOIN(\",\",TRUE,B1:B3)),SUM(C1:C3))"
        );
        // Inside an operator, too — a place a walker is easy to forget.
        assert_eq!(qualified("1+DAYS(A1,B1)*2"), "1+_XLFN.DAYS(A1,B1)*2");
    }

    #[test]
    fn a_first_class_call_is_reached_through_its_callee_and_its_arguments() {
        // `LAMBDA(x,x+1)(SEQUENCE(3))` — the callee is a future function and so
        // is the argument, and neither is an `Expr::Function` at the top.
        let out = qualified("LAMBDA(x,x+1)(SEQUENCE(3))");
        assert!(out.starts_with("_XLFN.LAMBDA("), "the callee: {out}");
        assert!(out.contains("_XLFN.SEQUENCE(3)"), "the argument: {out}");
    }

    #[test]
    fn reading_strips_what_writing_added() {
        for src in [
            "CONCAT(\"a\",\"b\")",
            "IF(ISFORMULA(A1),XLOOKUP(1,A1:A3,B1:B3),SUM(C1:C3))",
            "LET(x,1,x+1)",
        ] {
            let mut expr = parse(src).unwrap();
            qualify_future_functions(&mut expr);
            strip_future_prefixes(&mut expr);
            assert_eq!(expr, parse(src).unwrap(), "round trip of {src}");
        }
    }

    #[test]
    fn the_worksheet_prefix_is_stripped_too() {
        // Excel writes `_xlfn._xlws.FILTER`; both come off.
        assert_eq!(
            stripped("_xlfn._xlws.FILTER(A1:A3,B1:B3)"),
            "FILTER(A1:A3,B1:B3)"
        );
        assert_eq!(stripped("_xlfn.CONCAT(\"a\")"), "CONCAT(\"a\")");
    }

    #[test]
    fn a_prefixed_function_we_do_not_implement_still_loses_its_prefix() {
        // Reading is unconditional. Leaving the prefix on would report an
        // unknown function under a name that is not the one in the file.
        assert_eq!(stripped("_xlfn.NOTAREALFUNCTION(1)"), "NOTAREALFUNCTION(1)");
    }

    #[test]
    fn qualifying_twice_does_not_stack_prefixes() {
        let mut expr = parse("CONCAT(\"a\")").unwrap();
        qualify_future_functions(&mut expr);
        qualify_future_functions(&mut expr);
        assert_eq!(expr.to_string(), "_XLFN.CONCAT(\"a\")");
    }

    #[test]
    fn nothing_to_do_is_reported_as_nothing_to_do() {
        let mut expr = parse("SUM(A1:A3)").unwrap();
        assert!(!qualify_future_functions(&mut expr));
        assert!(!strip_future_prefixes(&mut expr));
    }
}
