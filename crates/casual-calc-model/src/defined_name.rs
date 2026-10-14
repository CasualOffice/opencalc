//! Defined names: named references or formulas, workbook- or sheet-scoped.
//! See `docs/22-NORMALIZED-SCHEMA.md`.

use casual_calc_formula::Expr;
use serde::{Deserialize, Serialize};

use crate::ids::SheetId;

/// A named reference/formula. Sheet-scoped when `sheet` is set, else
/// workbook-scoped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefinedName {
    /// The name.
    pub name: String,
    /// The sheet it is scoped to, if any (else workbook-scoped).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<SheetId>,
    /// The parsed formula the name refers to.
    pub formula: Expr,
    /// The author's note on the name — OOXML `<definedName comment=…>`, the
    /// Comment box of Excel's New Name dialog. Absent is written by omission,
    /// so a name without one serializes exactly as it did before the field
    /// existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}
