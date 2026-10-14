//! Defined names: named references or formulas, workbook- or sheet-scoped.
//! See `docs/22-NORMALIZED-SCHEMA.md`.

use casual_calc_formula::Expr;
use serde::{Deserialize, Serialize};

use crate::ids::SheetId;

/// A named reference/formula. Sheet-scoped when `sheet` is set, else
/// workbook-scoped.
///
/// **`deny_unknown_fields` is load-bearing here, not hygiene.** Defined names
/// cross the collaboration wire as `SetDefinedNames`, a whole-list replace. A
/// peer that *skipped* a field it did not know would send the list back without
/// it on its next edit and erase it for everyone, silently. Refusing the field
/// makes the mismatch loud, and `PROTOCOL_VERSION` moving with every new field
/// makes it loud at the handshake rather than mid-session (`COL-65`).
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
    /// The rest of `<definedName>`'s attributes (ECMA-376 §18.2.5), kept so a
    /// save writes back what the file said (`XL-FEAT-124`). Skipped when every
    /// one is absent, so an ordinary name serializes exactly as before.
    #[serde(default, skip_serializing_if = "NameAttributes::is_empty")]
    pub attributes: NameAttributes,
}

impl DefinedName {
    /// Hidden from the Name Manager, the name box and Go To — add-in and macro
    /// plumbing, `_xlnm._FilterDatabase` and the like. Still defined: formulas
    /// that use it resolve exactly as before.
    #[must_use]
    pub fn hidden(&self) -> bool {
        self.attributes.hidden
    }
}

/// `<definedName>`'s attributes beyond `name`, `localSheetId` and `comment`
/// (ECMA-376 Part 1 §18.2.5), each absent unless the file stated it.
///
/// Carried rather than interpreted: only `hidden` changes what the editor
/// shows. The rest tell Excel a name is a macro (`function`, `vbProcedure`,
/// `xlm`, `functionGroupId`, `shortcutKey`) or how to publish it, and losing
/// them is a silent change to the user's file. Every field is skipped when
/// absent and defaulted when missing, so this is additive by ADR-010 and
/// `SCHEMA_VERSION` does not move.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NameAttributes {
    /// `hidden`: not shown in the user interface.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// `function`: the name is a function (a macro), not a value.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub function: bool,
    /// `vbProcedure`: the name is a VBA procedure.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vb_procedure: bool,
    /// `xlm`: the name is an XLM (Excel 4.0) macro.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub xlm: bool,
    /// `functionGroupId`: the Insert Function category of a function name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_group_id: Option<u32>,
    /// `shortcutKey`: the keyboard shortcut of a macro name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut_key: Option<String>,
    /// `publishToServer`: published with the workbook to a server.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub publish_to_server: bool,
    /// `workbookParameter`: a parameter when published to a server.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub workbook_parameter: bool,
    /// `customMenu`: the custom-menu text of a macro name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_menu: Option<String>,
    /// `description`: the description of a macro name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `help`: the help topic of a macro name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    /// `statusBar`: the status-bar text of a macro name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_bar: Option<String>,
}

impl NameAttributes {
    /// No attribute is set — what an ordinary name has.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}
