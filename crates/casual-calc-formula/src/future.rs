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
//! # The second prefix
//!
//! The names `LAMBDA` and `LET` *bind* carry their own prefix, `_xlpm.`, for
//! the same reason: without it a reader cannot tell a bound parameter from a
//! defined name, and would look `x` up in the workbook's names and find
//! nothing. Both halves are needed — a `LAMBDA` whose function name is
//! qualified but whose parameters are not is still a formula Excel will not
//! evaluate.

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
        let rest = strip_prefix_ignoring_case(name, XLFN)?;
        Some(
            strip_prefix_ignoring_case(rest, XLWS)
                .unwrap_or(rest)
                .to_owned(),
        )
    })
}

/// The prefix SpreadsheetML puts on a name bound by `LAMBDA` or `LET`.
const XLPM: &str = "_XLPM.";

/// Add the `_xlpm.` prefix to every name bound by a `LAMBDA` or a `LET`, for
/// writing.
///
/// Scope-aware, and it has to be: a bare [`Expr::Name`] is a *defined name*
/// unless something binds it, and prefixing one of those would point the
/// formula at a name no workbook contains. So the walk tracks what is in scope
/// and prefixes only a reference to a binding it can see.
///
/// Two details follow the language rather than convenience. `LET` binds
/// **sequentially** — `LET(x,1,y,x+1,y)` — so each value expression is
/// qualified against the bindings before it and not against its own name. And
/// scope is a stack, so an inner `LAMBDA` parameter shadowing an outer one is
/// handled by both being prefixed, which is what the file wants anyway.
pub fn qualify_bound_names(expr: &mut Expr) -> bool {
    let mut scope: Vec<String> = Vec::new();
    qualify_in_scope(expr, &mut scope)
}

/// Remove the `_xlpm.` prefix from every name in `expr`, for reading.
///
/// Needs no scope: the prefix is only ever written on a bound name, so its
/// presence is the whole answer.
pub fn strip_bound_name_prefixes(expr: &mut Expr) -> bool {
    map_names(expr, &mut |name| {
        strip_prefix_ignoring_case(name, XLPM).map(str::to_owned)
    })
}

/// `str::strip_prefix`, without caring about case.
///
/// Needed because Excel writes these prefixes in **lower case** — `_xlpm.x` —
/// while the constants here are upper. Function names survive a case-sensitive
/// compare only by accident: the parser upper-cases them, and a bound name is
/// not a function name, so it arrives exactly as the file spelled it.
fn strip_prefix_ignoring_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    if text.len() >= prefix.len() && text[..prefix.len()].eq_ignore_ascii_case(prefix) {
        return Some(&text[prefix.len()..]);
    }
    None
}

fn qualify_in_scope(expr: &mut Expr, scope: &mut Vec<String>) -> bool {
    match expr {
        Expr::Name(name) => {
            if scope.iter().any(|b| b.eq_ignore_ascii_case(name)) {
                *name = format!("{XLPM}{name}");
                return true;
            }
            false
        }
        Expr::Function { name, args } if is_binder(name, "LAMBDA") && !args.is_empty() => {
            let depth = scope.len();
            let last = args.len() - 1;
            let mut changed = false;
            for param in &mut args[..last] {
                if let Expr::Name(n) = param {
                    scope.push(n.clone());
                    *n = format!("{XLPM}{n}");
                    changed = true;
                }
            }
            changed |= qualify_in_scope(&mut args[last], scope);
            scope.truncate(depth);
            changed
        }
        Expr::Function { name, args } if is_binder(name, "LET") && args.len() >= 3 => {
            let depth = scope.len();
            let last = args.len() - 1;
            let mut changed = false;
            let mut i = 0;
            while i + 1 < last {
                // The value first: `LET` binds in order, so a value sees the
                // names before it and not the one it is defining.
                changed |= qualify_in_scope(&mut args[i + 1], scope);
                if let Some(Expr::Name(n)) = args.get_mut(i) {
                    scope.push(n.clone());
                    *n = format!("{XLPM}{n}");
                    changed = true;
                }
                i += 2;
            }
            changed |= qualify_in_scope(&mut args[last], scope);
            scope.truncate(depth);
            changed
        }
        Expr::Function { args, .. } => {
            let mut changed = false;
            for arg in args {
                changed |= qualify_in_scope(arg, scope);
            }
            changed
        }
        Expr::Call { callee, args } => {
            let mut changed = qualify_in_scope(callee, scope);
            for arg in args {
                changed |= qualify_in_scope(arg, scope);
            }
            changed
        }
        Expr::Unary { operand, .. } => qualify_in_scope(operand, scope),
        Expr::Binary { left, right, .. } => {
            let l = qualify_in_scope(left, scope);
            let r = qualify_in_scope(right, scope);
            l || r
        }
        _ => false,
    }
}

/// Whether `name` is the binder `want`, with or without its `_xlfn.` prefix —
/// so the two passes may run in either order.
fn is_binder(name: &str, want: &str) -> bool {
    name.eq_ignore_ascii_case(want)
        || strip_prefix_ignoring_case(name, XLFN)
            .is_some_and(|rest| rest.eq_ignore_ascii_case(want))
}

/// Apply `f` to every [`Expr::Name`] in the tree.
fn map_names(expr: &mut Expr, f: &mut impl FnMut(&str) -> Option<String>) -> bool {
    let mut changed = false;
    match expr {
        Expr::Name(name) => {
            if let Some(next) = f(name) {
                *name = next;
                changed = true;
            }
        }
        Expr::Function { args, .. } => {
            for arg in args {
                changed |= map_names(arg, f);
            }
        }
        Expr::Call { callee, args } => {
            changed |= map_names(callee, f);
            for arg in args {
                changed |= map_names(arg, f);
            }
        }
        Expr::Unary { operand, .. } => changed |= map_names(operand, f),
        Expr::Binary { left, right, .. } => {
            changed |= map_names(left, f);
            changed |= map_names(right, f);
        }
        _ => {}
    }
    changed
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
    fn the_prefixes_are_matched_however_the_file_spells_them() {
        // Excel writes them in lower case. Function names survive a
        // case-sensitive compare only because the parser upper-cases them; a
        // bound name arrives exactly as the file spelled it, and the first
        // version of this stripped nothing at all.
        let mut expr = parse("_xlfn.LAMBDA(_xlpm.x,_xlpm.x+1)").unwrap();
        strip_future_prefixes(&mut expr);
        strip_bound_name_prefixes(&mut expr);
        assert_eq!(expr.to_string(), "LAMBDA(x,x+1)");
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

    fn bound(src: &str) -> String {
        let mut expr = parse(src).unwrap();
        qualify_bound_names(&mut expr);
        expr.to_string()
    }

    #[test]
    fn a_lambda_parameter_is_prefixed_where_it_binds_and_where_it_is_used() {
        assert_eq!(
            bound("LAMBDA(x,x+1)"),
            "LAMBDA(_XLPM.x,_XLPM.x+1)",
            "the parameter and the body reference are the same name"
        );
        assert_eq!(
            bound("LAMBDA(a,b,a*b)"),
            "LAMBDA(_XLPM.a,_XLPM.b,_XLPM.a*_XLPM.b)"
        );
    }

    #[test]
    fn a_defined_name_is_left_alone() {
        // The distinction the whole pass exists for. A bare name is a defined
        // name unless something binds it, and prefixing one of those points the
        // formula at a name no workbook contains.
        assert_eq!(bound("TaxRate*2"), "TaxRate*2");
        assert_eq!(
            bound("LAMBDA(x,x*TaxRate)"),
            "LAMBDA(_XLPM.x,_XLPM.x*TaxRate)",
            "bound and free names side by side"
        );
    }

    #[test]
    fn a_binding_does_not_escape_the_expression_that_made_it() {
        // `x` inside the lambda is bound; `x` after it is a defined name again.
        assert_eq!(
            bound("LAMBDA(x,x+1)(1)+x"),
            "LAMBDA(_XLPM.x,_XLPM.x+1)(1)+x"
        );
    }

    #[test]
    fn let_binds_in_order_so_a_value_cannot_see_its_own_name() {
        // `LET(x,1,y,x+1,y)`: the value `x+1` sees `x`, and the name `y` is not
        // in scope in its own value expression.
        assert_eq!(
            bound("LET(x,1,y,x+1,y)"),
            "LET(_XLPM.x,1,_XLPM.y,_XLPM.x+1,_XLPM.y)"
        );
        // A value naming the variable being defined is a *different*, free name
        // — a defined name — and must not be prefixed.
        assert_eq!(bound("LET(x,x,x)"), "LET(_XLPM.x,x,_XLPM.x)");
    }

    #[test]
    fn an_inner_binding_shadows_an_outer_one() {
        assert_eq!(
            bound("LET(x,1,LET(x,x+1,x))"),
            "LET(_XLPM.x,1,LET(_XLPM.x,_XLPM.x+1,_XLPM.x))"
        );
    }

    #[test]
    fn the_bound_names_survive_a_round_trip() {
        for src in [
            "LAMBDA(x,x+1)",
            "LET(x,1,y,x+1,y)",
            "LAMBDA(x,x*TaxRate)",
            "MAP(A1:A3,LAMBDA(v,v*2))",
        ] {
            let mut expr = parse(src).unwrap();
            qualify_bound_names(&mut expr);
            qualify_future_functions(&mut expr);
            strip_future_prefixes(&mut expr);
            strip_bound_name_prefixes(&mut expr);
            assert_eq!(expr, parse(src).unwrap(), "round trip of {src}");
        }
    }

    #[test]
    fn the_two_passes_compose_into_what_the_file_carries() {
        let mut expr = parse("LAMBDA(x,x+1)").unwrap();
        qualify_bound_names(&mut expr);
        qualify_future_functions(&mut expr);
        assert_eq!(expr.to_string(), "_XLFN.LAMBDA(_XLPM.x,_XLPM.x+1)");
    }

    #[test]
    fn the_bound_name_pass_tolerates_an_already_qualified_binder() {
        // Order-independence is asserted rather than assumed, because relying
        // on it silently would make the writer's ordering a coincidence.
        let mut expr = parse("LAMBDA(x,x+1)").unwrap();
        qualify_future_functions(&mut expr);
        qualify_bound_names(&mut expr);
        assert_eq!(expr.to_string(), "_XLFN.LAMBDA(_XLPM.x,_XLPM.x+1)");
    }

    #[test]
    fn reading_strips_the_prefix_wherever_it_appears() {
        let mut expr = parse("_xlfn.LAMBDA(_xlpm.x,_xlpm.x+1)").unwrap();
        strip_future_prefixes(&mut expr);
        strip_bound_name_prefixes(&mut expr);
        assert_eq!(expr.to_string(), "LAMBDA(x,x+1)");
    }
}
