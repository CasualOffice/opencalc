//! AST rewrites over references — used by fill/copy to adjust relative
//! references by a row/column delta (absolute `$` anchors are preserved).

use crate::ast::Expr;
use crate::reference::CellReference;

/// Shift every **relative** reference in `expr` by `(dr, dc)` rows/columns,
/// leaving `$`-anchored coordinates fixed. Coordinates clamp at 0. This is the
/// copy/fill semantics: the same offset applies to bare cell references and to
/// each endpoint of a range.
pub fn shift_references(expr: &Expr, dr: i64, dc: i64) -> Expr {
    match expr {
        Expr::Reference(r) => Expr::Reference(shift_ref(r, dr, dc)),
        Expr::Range(a, b) => Expr::Range(shift_ref(a, dr, dc), shift_ref(b, dr, dc)),
        Expr::Unary { op, operand } => Expr::Unary {
            op: *op,
            operand: Box::new(shift_references(operand, dr, dc)),
        },
        Expr::Binary { op, left, right } => Expr::Binary {
            op: *op,
            left: Box::new(shift_references(left, dr, dc)),
            right: Box::new(shift_references(right, dr, dc)),
        },
        Expr::Function { name, args } => Expr::Function {
            name: name.clone(),
            args: args.iter().map(|a| shift_references(a, dr, dc)).collect(),
        },
        other => other.clone(),
    }
}

fn shift_ref(r: &CellReference, dr: i64, dc: i64) -> CellReference {
    let mut out = r.clone();
    // An axis the reference never named has no coordinate to move: `A:A` copied
    // one row down is still `A:A`. Shifting the placeholder bound would turn it
    // into `A2:A1048576`, which is a different — and wrong — range.
    if !out.row_absolute && !out.row_implicit {
        out.row = (out.row as i64 + dr).max(0) as u32;
    }
    if !out.col_absolute && !out.col_implicit {
        out.col = (out.col as i64 + dc).max(0) as u32;
    }
    out
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
        _ => false,
    }
}

fn rename_ref(r: &mut CellReference, old: &str, new: &str) -> bool {
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
    use super::shift_references;
    use crate::parse;

    fn shifted(src: &str, dr: i64, dc: i64) -> String {
        shift_references(&parse(src).unwrap(), dr, dc).to_string()
    }

    #[test]
    fn relative_refs_shift_absolute_hold() {
        assert_eq!(shifted("A1", 1, 0), "A2");
        assert_eq!(shifted("A1", 0, 1), "B1");
        assert_eq!(shifted("$A$1", 5, 5), "$A$1");
        assert_eq!(shifted("$A1", 2, 3), "$A3");
        assert_eq!(shifted("A$1", 2, 3), "D$1");
        assert_eq!(shifted("SUM(A1:A3)", 1, 0), "SUM(A2:A4)");
        assert_eq!(shifted("A1+$B$2*C1", 1, 1), "(B2+($B$2*D2))");
    }

    #[test]
    fn clamps_at_zero() {
        assert_eq!(shifted("B2", -5, -5), "A1");
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
            ("(New!A1+Other!B2)".into(), true)
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
}
