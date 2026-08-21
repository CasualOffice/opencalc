//! AST rewrites over references.
//!
//! Since `PERF-11` these are the rewrites relative storage does *not* do for
//! free: moving a formula while keeping its targets ([`restore_at`]) and
//! following a renamed sheet ([`rename_sheet_references`]). Copy and fill need
//! neither — resolving a shared tree at the destination is the shift.

use crate::ast::Expr;
use crate::stored::{Origin, StoredRef};

/// Re-store `expr` so its references point where they pointed, when the
/// formula **moves** from `from` to `to`.
///
/// The cut/move rewrite — and under relative storage the only one of the pair
/// that exists, which is the reverse of how it used to be.
///
/// - **Copy and fill need no rewrite at all.** A relative reference is an
///   offset from the holding cell, so resolving the *same* tree at the
///   destination already shifts it. The old `shift_references` did that by
///   hand; doing both would shift twice.
/// - **A move needs this one.** A moved formula must go on naming the cells it
///   named — `UX-CUT-03`, a cut travels verbatim — and so must a formula
///   carried by a row insertion, which is the same operation wearing a
///   different name.
///
/// A reference that resolves off the sheet from `from` is left alone: it is
/// already `#REF!` and there is no address to re-measure.
#[must_use]
pub fn restore_at(expr: &Expr, from: Origin, to: Origin) -> Expr {
    let moved = |r: &StoredRef| -> StoredRef {
        match r.resolve(from) {
            Some(target) => target.store(to),
            None => r.clone(),
        }
    };
    match expr {
        Expr::Reference(r) => Expr::Reference(moved(r)),
        Expr::Range(a, b) => Expr::Range(moved(a), moved(b)),
        Expr::Unary { op, operand } => Expr::Unary {
            op: *op,
            operand: Box::new(restore_at(operand, from, to)),
        },
        Expr::Binary { op, left, right } => Expr::Binary {
            op: *op,
            left: Box::new(restore_at(left, from, to)),
            right: Box::new(restore_at(right, from, to)),
        },
        Expr::Function { name, args } => Expr::Function {
            name: name.clone(),
            args: args.iter().map(|a| restore_at(a, from, to)).collect(),
        },
        Expr::Call { callee, args } => Expr::Call {
            callee: Box::new(restore_at(callee, from, to)),
            args: args.iter().map(|a| restore_at(a, from, to)).collect(),
        },
        other => other.clone(),
    }
}

/// Rewrite every reference whose sheet qualifier is `old` (matched
/// case-insensitively, as Excel resolves sheet names) to `new`, so a formula
/// like `=Old!A1` follows the sheet when it is renamed. Bare (unqualified)
/// references are untouched. Returns `true` if anything changed.
pub fn rename_sheet_references(expr: &mut Expr, old: &str, new: &str) -> bool {
    match expr {
        Expr::Reference(r) => rename_ref(r, old, new),
        Expr::Range(a, b) => {
            // Evaluate both so neither endpoint is skipped by short-circuiting.
            let left = rename_ref(a, old, new);
            let right = rename_ref(b, old, new);
            left || right
        }
        Expr::Unary { operand, .. } => rename_sheet_references(operand, old, new),
        Expr::Binary { left, right, .. } => {
            let l = rename_sheet_references(left, old, new);
            let r = rename_sheet_references(right, old, new);
            l || r
        }
        Expr::Function { args, .. } => {
            let mut changed = false;
            for arg in args {
                changed |= rename_sheet_references(arg, old, new);
            }
            changed
        }
        // As in `shift_references`: a first-class call's callee and arguments
        // are expressions and can name a sheet. Missing them left a stale sheet
        // name behind after a rename, which reads as `#REF!` on the next recalc.
        Expr::Call { callee, args } => {
            let mut changed = rename_sheet_references(callee, old, new);
            for arg in args {
                changed |= rename_sheet_references(arg, old, new);
            }
            changed
        }
        _ => false,
    }
}

fn rename_ref(r: &mut StoredRef, old: &str, new: &str) -> bool {
    if r.sheet
        .as_deref()
        .is_some_and(|s| s.eq_ignore_ascii_case(old))
    {
        r.sheet = Some(new.to_owned());
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::restore_at;
    use crate::parse;
    use crate::print::print_at;
    use crate::stored::{ABSOLUTE, Origin};

    /// How a *copied* formula reads, now that copying rewrites nothing: the
    /// same tree, read at the destination.
    fn copied(src: &str, dr: u32, dc: u32) -> String {
        print_at(&parse(src).unwrap(), Origin::at(dr, dc))
    }

    /// **Copy shifts by being read somewhere else.**
    #[test]
    fn a_copied_formula_shifts_its_relative_references_and_holds_its_anchors() {
        assert_eq!(copied("A1", 1, 0), "A2");
        assert_eq!(copied("A1", 0, 1), "B1");
        assert_eq!(copied("$A$1", 5, 5), "$A$1");
        assert_eq!(copied("$A1", 2, 3), "$A3");
        assert_eq!(copied("A$1", 2, 3), "D$1");
        assert_eq!(copied("SUM(A1:A3)", 1, 0), "SUM(A2:A4)");
        assert_eq!(copied("A1+$B$2*C1", 1, 1), "B2+$B$2*D2");
    }

    /// **Off the sheet is `#REF!`, not a clamp.**
    ///
    /// `shift_references` clamped at zero, so `=A1` in `A2` copied up became
    /// `=A1` — a formula quietly pointing at itself. Excel says `#REF!`.
    #[test]
    fn a_reference_carried_off_the_sheet_is_ref_rather_than_clamped() {
        let in_a2 = restore_at(&parse("A1").unwrap(), ABSOLUTE, Origin::at(1, 0));
        assert_eq!(
            print_at(&in_a2, Origin::at(1, 0)),
            "A1",
            "stored where it lives"
        );
        assert_eq!(print_at(&in_a2, ABSOLUTE), "#REF!");
    }

    /// **A moved formula keeps its targets** — the cut rewrite.
    #[test]
    fn a_moved_formula_names_the_same_cells() {
        let stored = restore_at(&parse("A1").unwrap(), ABSOLUTE, Origin::at(1, 0));
        let moved = restore_at(&stored, Origin::at(1, 0), Origin::at(4, 2));
        assert_eq!(print_at(&moved, Origin::at(4, 2)), "A1");
    }

    /// A move to nowhere changes nothing.
    #[test]
    fn a_move_to_the_same_place_is_the_identity() {
        let tree = parse("A1+$B$2*C1").unwrap();
        assert_eq!(restore_at(&tree, ABSOLUTE, ABSOLUTE), tree);
    }

    #[test]
    fn a_first_class_calls_arguments_shift_with_everything_else() {
        assert_eq!(copied("LAMBDA(x,x+1)(A1)", 2, 0), "LAMBDA(x,x+1)(A3)");
        assert_eq!(copied("LAMBDA(x,x+1)($A$1)", 2, 0), "LAMBDA(x,x+1)($A$1)");
        assert_eq!(
            copied("IF(TRUE,LAMBDA(x,x+A1),LAMBDA(x,x))(B1)", 1, 0),
            "IF(TRUE,LAMBDA(x,x+A2),LAMBDA(x,x))(B2)"
        );
        let moved = restore_at(
            &parse("LAMBDA(x,x+A1)(B1)").unwrap(),
            ABSOLUTE,
            Origin::at(3, 0),
        );
        assert_eq!(print_at(&moved, Origin::at(3, 0)), "LAMBDA(x,x+A1)(B1)");
    }

    use super::rename_sheet_references;

    fn renamed(src: &str, old: &str, new: &str) -> (String, bool) {
        let mut expr = parse(src).unwrap();
        let changed = rename_sheet_references(&mut expr, old, new);
        (expr.to_string(), changed)
    }

    #[test]
    fn renames_qualified_refs_leaving_bare_ones() {
        // A qualified ref follows the rename; a bare ref is untouched.
        assert_eq!(renamed("Old!A1", "Old", "New"), ("New!A1".into(), true));
        assert_eq!(renamed("A1", "Old", "New"), ("A1".into(), false));
        // A cross-sheet range (qualified at its first endpoint) is rewritten.
        assert_eq!(
            renamed("SUM(Old!A1:A3)", "Old", "New"),
            ("SUM(New!A1:A3)".into(), true)
        );
        // Only the matching sheet is touched inside a larger expression.
        assert_eq!(
            renamed("Old!A1+Other!B2", "Old", "New"),
            ("New!A1+Other!B2".into(), true)
        );
    }

    #[test]
    fn rename_is_case_insensitive_and_quotes_when_needed() {
        assert_eq!(renamed("OLD!A1", "Old", "New"), ("New!A1".into(), true));
        // A new name with a space must round-trip through quoting.
        assert_eq!(
            renamed("Old!A1", "Old", "My Data"),
            ("'My Data'!A1".into(), true)
        );
    }

    /// A first-class call is an expression like any other, and both rewrites
    /// used to walk straight past it.
    ///
    /// `LAMBDA(x,…)(A1)` parses as `Call { callee, args }`, which fell to the
    #[test]
    fn a_first_class_calls_arguments_follow_a_sheet_rename() {
        assert_eq!(
            renamed("LAMBDA(x,x+1)(Old!A1)", "Old", "New"),
            ("LAMBDA(x,x+1)(New!A1)".into(), true)
        );
        assert_eq!(
            renamed("LAMBDA(x,x+Old!B2)(1)", "Old", "New"),
            ("LAMBDA(x,x+New!B2)(1)".into(), true)
        );
    }
}
