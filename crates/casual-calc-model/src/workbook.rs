//! The workbook envelope and deterministic snapshot I/O.
//! See `docs/22-NORMALIZED-SCHEMA.md`, `docs/25`-style snapshot discipline.

use std::collections::BTreeSet;

use casual_calc_formula::Expr;
use serde::{Deserialize, Serialize};

use crate::defined_name::DefinedName;
use crate::error::ModelError;
use crate::ids::{FormulaHandle, Id, StringId, StyleId};
use crate::sheet::Sheet;
use crate::strings::StringTable;
use crate::style::{Style, StyleTable};
use crate::value::CellValue;

/// The current workbook schema version. Snapshots record this so migrations can
/// upgrade older ones deterministically.
pub const SCHEMA_VERSION: u32 = 0;

/// The normalized workbook: an identity, a schema version, and its sheets in tab
/// order. Additive fields use `skip_serializing_if` so older snapshots stay
/// byte-identical as the schema grows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Workbook {
    /// The schema version this workbook conforms to.
    pub schema_version: u32,
    /// Stable workbook identity.
    pub workbook_id: Id,
    /// The interned string table cells reference.
    #[serde(default, skip_serializing_if = "StringTable::is_empty")]
    pub strings: StringTable,
    /// The interned style table cells reference.
    #[serde(default, skip_serializing_if = "StyleTable::is_empty")]
    pub styles: StyleTable,
    /// The formula AST arena; `Cell::formula` indexes into it (the reserved calc
    /// seam). ASTs are parsed at import; the calc engine evaluates them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub formulas: Vec<Expr>,
    /// Defined names (workbook- or sheet-scoped).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defined_names: Vec<DefinedName>,
    /// Sheets in tab order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<Sheet>,
    /// The workbook default font name (`<fonts>` entry 0 in `styles.xml`), shown
    /// for cells that carry no explicit font. `None` for a blank workbook.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_font_name: Option<String>,
    /// The workbook default font size in half-points, paired with
    /// [`Self::default_font_name`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_font_size_hp: Option<u32>,
    /// The workbook's theme colour slots as `RRGGBB`, in OOXML slot order
    /// (`lt1, dk1, lt2, dk2, accent1..6, hlink, folHlink`).
    ///
    /// Kept so a host can offer *this file's* theme colours rather than the
    /// stock Office ones. Empty means the package had no theme part, and the
    /// stock scheme applies — which is also what keeps an untouched workbook's
    /// snapshot byte-identical.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub theme_colors: Vec<String>,
}

impl Workbook {
    /// A new, empty workbook at the current schema version.
    pub fn new(workbook_id: Id) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            workbook_id,
            strings: StringTable::new(),
            styles: StyleTable::new(),
            formulas: Vec::new(),
            defined_names: Vec::new(),
            sheets: Vec::new(),
            default_font_name: None,
            default_font_size_hp: None,
            theme_colors: Vec::new(),
        }
    }

    /// Intern a string into the workbook's table, returning its id.
    pub fn intern_string(&mut self, value: &str) -> StringId {
        self.strings.intern(value)
    }

    /// Intern a style into the workbook's table, returning its id.
    pub fn intern_style(&mut self, style: Style) -> StyleId {
        self.styles.intern(style)
    }

    /// Store a formula AST in the arena, returning a handle to it.
    pub fn store_formula(&mut self, expr: Expr) -> FormulaHandle {
        let index = self.formulas.len() as u32;
        self.formulas.push(expr);
        FormulaHandle(index)
    }

    /// Resolve a formula handle to its AST.
    pub fn formula(&self, handle: FormulaHandle) -> Option<&Expr> {
        self.formulas.get(handle.0 as usize)
    }

    /// Validate model invariants: known schema version and unique sheet ids.
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ModelError::Invariant("unsupported schema version"));
        }
        let mut seen = BTreeSet::new();
        for sheet in &self.sheets {
            if !seen.insert(sheet.id) {
                return Err(ModelError::Invariant("duplicate sheet id"));
            }
            for (_, cell) in sheet.cells.iter() {
                if let CellValue::SharedString(id) | CellValue::InlineString(id) = cell.value
                    && !self.strings.contains(id)
                {
                    return Err(ModelError::Invariant("dangling string reference"));
                }
                if let Some(handle) = cell.formula
                    && self.formula(handle).is_none()
                {
                    return Err(ModelError::Invariant("dangling formula handle"));
                }
                if let Some(style) = cell.style
                    && !self.styles.contains(style)
                {
                    return Err(ModelError::Invariant("dangling style reference"));
                }
            }
        }
        Ok(())
    }

    /// Serialize to a deterministic, byte-stable JSON snapshot.
    ///
    /// Field order is fixed by declaration and cell order by the ordered store,
    /// so the same model always produces the same bytes.
    pub fn to_snapshot(&self) -> Result<Vec<u8>, ModelError> {
        Ok(serde_json::to_vec(self)?)
    }

    /// Parse a snapshot and validate it.
    pub fn from_snapshot(bytes: &[u8]) -> Result<Self, ModelError> {
        let workbook: Workbook = serde_json::from_slice(bytes)?;
        workbook.validate()?;
        Ok(workbook)
    }
}
