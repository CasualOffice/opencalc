//! The workbook envelope and deterministic snapshot I/O.
//! See `docs/22-NORMALIZED-SCHEMA.md`, `docs/25`-style snapshot discipline.

use std::collections::{BTreeMap, BTreeSet};

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
pub const SCHEMA_VERSION: u32 = 1;

/// A part carried through the round trip without being modelled.
///
/// This is the **retention** path: external-link caches, drawings, charts,
/// images, pivot caches and anything else we do not understand yet. Keeping the
/// bytes is what separates "we do not support charts" from "we delete charts",
/// and only one of those is acceptable in a file people already have work in.
///
/// A retained part is inert. It is never parsed, never edited, and is written
/// back byte for byte along with the relationship that reaches it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetainedPart {
    /// The part's path inside the package, e.g. `xl/externalLinks/externalLink1.xml`.
    pub path: String,
    /// Its bytes, exactly as read.
    pub bytes: Vec<u8>,
    /// The `[Content_Types].xml` override that declares it, when it had one.
    /// Without this the package is invalid, and Excel refuses to open it rather
    /// than ignoring the part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

/// An element that names a retained part, as `(element name, attributes)`.
pub type RetainedRef = (String, BTreeMap<String, String>);

/// A relationship pointing at a retained part, to be re-emitted verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetainedRel {
    /// The part that declares the relationship (e.g. `xl/workbook.xml`).
    pub source: String,
    /// The original relationship id. Kept because the referencing element —
    /// `<externalReference r:id>`, `<drawing r:id>` — names it, and a re-minted
    /// id would point at nothing.
    pub id: String,
    /// The relationship type URI.
    pub rel_type: String,
    /// The target, relative to the source part.
    pub target: String,
}

/// Workbook-level settings carried through verbatim.
///
/// Same reasoning as [`crate::SheetProtection`] and [`crate::PrintSetup`]: these
/// elements hold roughly sixty attributes between them, most of which nothing in
/// the editor reads. `workbookProtection` and `fileSharing` additionally carry
/// password hashes and salts, where writing a regenerated value back locks the
/// author out of their own workbook — a failure far worse than not supporting
/// the feature.
///
/// `calcPr` is the one to watch: it is inert while the calc engine is held back,
/// and becomes load-bearing the moment it lands, because a workbook that needs
/// iterative calculation must not be recalculated without it.
/// What `<calcPr>` says about resolving a circular reference.
///
/// A workbook that needs iteration is one whose author *meant* the loop — a
/// balance that depends on the interest it accrues, a rate that depends on the
/// balance. Recalculating it without iteration does not merely lose a feature;
/// it turns a working model into a sheet of `#REF!`, which is why the settings
/// have been carried verbatim since before there was an engine to read them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Iteration {
    /// Whether a circular reference should be resolved by iterating rather than
    /// reported as an error.
    pub enabled: bool,
    /// How many passes to make before giving up on convergence. Excel's
    /// default is 100.
    pub max_count: u32,
    /// The largest change across a pass that still counts as converged. Excel's
    /// default is 0.001.
    pub max_change: f64,
}

impl Default for Iteration {
    fn default() -> Self {
        // Off, with Excel's own defaults for the other two so that a file which
        // enables iteration without saying how much gets what its author saw.
        Self {
            enabled: false,
            max_count: 100,
            max_change: 0.001,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkbookSettings {
    /// `<calcPr>` attributes as read. Interpreted by
    /// [`iteration`](Self::iteration); everything else here travels verbatim.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub calc: BTreeMap<String, String>,
    /// `<fileVersion>` attributes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub file_version: BTreeMap<String, String>,
    /// `<workbookPr>` attributes other than `date1904`, which is interpreted.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub workbook_pr: BTreeMap<String, String>,
    /// `<workbookProtection>` attributes, hashes included.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub protection: BTreeMap<String, String>,
    /// `<fileSharing>` attributes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub file_sharing: BTreeMap<String, String>,
    /// Each `<workbookView>` inside `<bookViews>`, in document order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<BTreeMap<String, String>>,
}

impl WorkbookSettings {
    /// What `<calcPr>` asks for when a formula depends on itself.
    ///
    /// Read from the carried attributes rather than stored separately, so the
    /// verbatim round-trip stays the single source of truth and there is no
    /// second copy to fall out of step with it.
    ///
    /// An unparseable count or delta falls back to Excel's default rather than
    /// disabling iteration: the author asked for a loop to be resolved, and
    /// refusing on the strength of a malformed *limit* would turn their working
    /// model into a sheet of errors over a detail they cannot see.
    #[must_use]
    pub fn iteration(&self) -> Iteration {
        let flag = |key: &str| {
            self.calc
                .get(key)
                .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        };
        let default = Iteration::default();
        Iteration {
            enabled: flag("iterate"),
            max_count: self
                .calc
                .get("iterateCount")
                .and_then(|v| v.parse().ok())
                .unwrap_or(default.max_count),
            max_change: self
                .calc
                .get("iterateDelta")
                .and_then(|v| v.parse().ok())
                .unwrap_or(default.max_change),
        }
    }

    /// Whether nothing at all was carried.
    pub fn is_empty(&self) -> bool {
        self.calc.is_empty()
            && self.file_version.is_empty()
            && self.workbook_pr.is_empty()
            && self.protection.is_empty()
            && self.file_sharing.is_empty()
            && self.views.is_empty()
    }
}

/// The stock Office theme, in `theme="N"` slot order, used for a workbook that
/// never came from a package or whose theme part was missing.
pub const STOCK_THEME_SLOTS: [&str; 12] = [
    "FFFFFF", // 0  background 1 (lt1)
    "000000", // 1  text 1       (dk1)
    "E7E6E6", // 2  background 2 (lt2)
    "44546A", // 3  text 2       (dk2)
    "4472C4", // 4  accent 1
    "ED7D31", // 5  accent 2
    "A5A5A5", // 6  accent 3
    "FFC000", // 7  accent 4
    "5B9BD5", // 8  accent 5
    "70AD47", // 9  accent 6
    "0563C1", // 10 hyperlink
    "954F72", // 11 followed hyperlink
];

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
    /// Workbook-level settings, carried through verbatim.
    #[serde(default, skip_serializing_if = "WorkbookSettings::is_empty")]
    pub settings: WorkbookSettings,
    /// The moment `TODAY()` and `NOW()` report, as a date serial.
    ///
    /// Supplied by the host rather than read from a clock here, and **not**
    /// serialised: it is environment, not document state, and a calc engine
    /// that reaches for the wall clock cannot be tested or replayed. A test
    /// sets it and gets the same answer every run.
    #[serde(skip)]
    pub volatile_now: f64,
    /// The seed the random functions draw from, likewise supplied and not
    /// serialised. Excel rerolls `RAND` on every recalculation; the host
    /// changes this to ask for that, and leaving it alone reproduces the
    /// previous values exactly.
    #[serde(skip)]
    pub volatile_seed: u64,
    /// Parts kept byte for byte because nothing here models them yet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retained_parts: Vec<RetainedPart>,
    /// The relationships that reach those parts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retained_rels: Vec<RetainedRel>,
    /// Elements inside `workbook.xml` that reference a retained part, kept so
    /// the reference survives alongside the part it names — a retained chart
    /// nothing points at is invisible, which is indistinguishable from having
    /// dropped it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retained_refs: Vec<RetainedRef>,
    /// Named cell styles (`Normal`, `Good`, `Heading 1`, …) in `cellStyleXfs`
    /// order, which is the order `Style::style_ref` indexes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cell_styles: Vec<NamedCellStyle>,
    /// Whether the workbook counts dates from 1904 rather than 1900 — OOXML
    /// `<workbookPr date1904="1">`, the legacy Mac Excel epoch.
    ///
    /// Serials are stored in the file's own system, so this has to be known to
    /// display a date at all: read a 1904 workbook as 1900 and every date is
    /// wrong by 1462 days. Dropping the flag on save is worse still — the
    /// serials stay put while their meaning shifts, corrupting every date in the
    /// file permanently.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub date1904: bool,
}

/// A named cell style: OOXML's `<cellStyle>` entry paired with the
/// `<cellStyleXf>` it points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NamedCellStyle {
    /// The name shown in a style gallery, e.g. `Heading 1`.
    pub name: String,
    /// Excel's `builtinId`, when this is one of its stock styles. Preserved
    /// because Excel keys its own gallery off the id, not off the name — a
    /// localized file names them differently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin_id: Option<u32>,
    /// The formatting the name stands for.
    #[serde(default, skip_serializing_if = "Style::is_default")]
    pub style: Style,
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
            settings: WorkbookSettings::default(),
            volatile_now: 0.0,
            volatile_seed: 0,
            retained_parts: Vec::new(),
            retained_rels: Vec::new(),
            retained_refs: Vec::new(),
            cell_styles: Vec::new(),
            date1904: false,
        }
    }

    /// Intern a string into the workbook's table, returning its id.
    /// The `RRGGBB` for a `theme="N"` slot: this workbook's own if it carries a
    /// theme, else the stock Office one.
    ///
    /// The index is the order a `theme` attribute uses, which is **not** the
    /// order `<a:clrScheme>` lists: slots 0/1 and 2/3 are swapped relative to
    /// the scheme's `dk1`/`lt1` and `dk2`/`lt2`. Getting that backwards turns
    /// black text white.
    #[must_use]
    pub fn theme_slot(&self, index: usize) -> &str {
        self.theme_colors
            .get(index)
            .filter(|c| !c.is_empty())
            .map_or_else(
                || STOCK_THEME_SLOTS.get(index).copied().unwrap_or("000000"),
                String::as_str,
            )
    }

    /// Intern rich text — the runs and their formatting — returning its id.
    /// Collapses to a plain string when no run carries formatting.
    pub fn intern_rich_text(&mut self, runs: Vec<crate::style::TextRun>) -> StringId {
        self.strings.intern_rich(runs)
    }

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
