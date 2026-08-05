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
    if !out.row_absolute {
        out.row = (out.row as i64 + dr).max(0) as u32;
    }
    if !out.col_absolute {
        out.col = (out.col as i64 + dc).max(0) as u32;
    }
    out
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
}
