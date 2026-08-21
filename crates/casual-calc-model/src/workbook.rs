//! The workbook envelope and deterministic snapshot I/O.
//! See `docs/22-NORMALIZED-SCHEMA.md`, `docs/25`-style snapshot discipline.

use std::collections::{BTreeMap, BTreeSet};

use casual_calc_formula::Expr;
use casual_calc_formula::stored::{ABSOLUTE, Origin};
use serde::{Deserialize, Serialize};

use crate::defined_name::DefinedName;
use crate::error::ModelError;
use crate::ids::{FormulaHandle, Id, StringId, StyleId};
use crate::sheet::Sheet;
use crate::store::CellRef;
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
    /// `TargetMode="External"`: the target is a URI to something outside the
    /// package — another workbook, a web address — not a path to a part.
    ///
    /// Two things depend on knowing the difference. The writer has to re-emit
    /// the attribute, because a target written without it is read back as a
    /// path inside the zip and the reference is destroyed. And nothing may
    /// resolve this target against the source part or look it up in the
    /// package: `file:///other.xlsx` under `xl/workbook.xml` "resolves" to
    /// `xl/file:/other.xlsx`, a part no package has ever contained.
    ///
    /// Additive by ADR-010: defaulted on the way in so a snapshot written
    /// before this field existed still reads, and skipped on the way out when
    /// false so a workbook with no external relationship serializes to the same
    /// bytes it always did. `SCHEMA_VERSION` therefore does not move.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub external: bool,
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

/// Who wrote a document, what it is called, and when it was written.
///
/// **Format-neutral on purpose.** Every spreadsheet format carries the same
/// handful of facts about the document itself under different names, and this
/// is the one place for them so that a converter does not have to invent a
/// second. ODF puts them in `meta.xml`; OOXML puts them in
/// `docProps/core.xml`, which the `.xlsx` importer currently keeps as a
/// [retained part](Self) rather than reading — so this starts out populated by
/// the OpenDocument path only, and the OOXML side can be moved onto it without
/// the model changing shape.
///
/// # The one mapping worth being careful about
///
/// `dc:creator` does **not** mean the same thing in the two formats. In ODF it
/// is who saved the document last, and the original author is
/// `meta:initial-creator`; in OOXML it is the original author, and the last
/// saver is `cp:lastModifiedBy`. Read one as the other and every file's author
/// silently becomes whoever last opened it. Hence two fields with the meanings
/// spelled out, rather than one field named after whichever format was
/// implemented first.
///
/// # Why the timestamps are strings
///
/// They are kept exactly as the file wrote them — ISO-8601 text — and are not
/// parsed. Parsing would need a date library this workspace does not have, and
/// would put a *reformatted* timestamp back into somebody's file over a detail
/// this engine has no opinion about. A host that wants a `struct` has the text
/// to parse; a host that wants a round trip gets its bytes back.
///
/// Empty strings mean the document said nothing, which is distinct from saying
/// something empty only in theory: no format distinguishes the two either.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentProperties {
    /// The document's title (`dc:title`), which is not its file name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Its subject (`dc:subject`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subject: String,
    /// A free-text description or comment (`dc:description`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Keywords, one per entry.
    ///
    /// A list rather than a joined string, which is the shape ODF uses — one
    /// `meta:keyword` element each. OOXML writes them as a single
    /// `cp:keywords`, so that side has to pick a separator; doing the joining
    /// *here* would mean a keyword containing the separator could not be told
    /// from two keywords, and a document whose keyword is `Q3, Q4` would come
    /// back with two of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// The **original author** — ODF `meta:initial-creator`, OOXML
    /// `dc:creator`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub creator: String,
    /// Who saved it last — ODF `dc:creator`, OOXML `cp:lastModifiedBy`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_modified_by: String,
    /// When it was created, as written (`meta:creation-date` /
    /// `dcterms:created`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created: String,
    /// When it was last saved, as written (`dc:date` / `dcterms:modified`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub modified: String,
    /// The document language (`dc:language`), e.g. `en-GB`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
    /// The application that wrote the file (`meta:generator` /
    /// OOXML `Application`).
    ///
    /// Carried rather than overwritten. It is the document's own account of
    /// where it came from, and a converter that stamps its own name over it
    /// has destroyed the one field that would have told a support engineer
    /// which program to blame — a silent loss, and this crate's whole reason
    /// for existing is not to do that.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub generator: String,
}

impl DocumentProperties {
    /// Whether the document said nothing about itself.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
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
    /// Fingerprint → candidate indices into [`Self::formulas`], so an identical
    /// formula is stored once.
    ///
    /// Derived state: **not serialised**, and rebuilt on deserialisation. A
    /// snapshot carries the arena, and carrying the index too would be storing
    /// the same fact twice — with the usual consequence when the two disagree.
    #[serde(skip)]
    formula_index: BTreeMap<u64, Vec<u32>>,
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
    /// What the document says about itself: author, title, timestamps.
    ///
    /// Not part of any sheet and not reachable from one, which is exactly how
    /// it went missing — a reader that walks the cells never meets it. See
    /// [`DocumentProperties`].
    ///
    /// Additive by ADR-010: defaulted on the way in so an older snapshot still
    /// reads, and skipped on the way out when empty so a workbook that carries
    /// no properties serializes to the bytes it always did. `SCHEMA_VERSION`
    /// therefore does not move.
    #[serde(default, skip_serializing_if = "DocumentProperties::is_empty")]
    pub properties: DocumentProperties,
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

/// What one snapshot may cost.
///
/// The last row of docs/21's scale table. Every other admission path had a
/// ceiling; this one accepted whatever it was handed, and it is reached from a
/// resumed collaborative session and from any host calling the model directly.
/// Over the wire the collaboration server's message cap bounded it in practice,
/// which is a bound in one deployment rather than a property of the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotLimits {
    /// The largest snapshot, in bytes, checked before parsing.
    pub max_bytes: u64,
    /// Populated cells across every sheet, checked after.
    pub max_populated_cells: usize,
}

impl Default for SnapshotLimits {
    fn default() -> Self {
        Self {
            // A snapshot is JSON, so it is several times the size of the model
            // it carries: this is comfortably more than a workbook at the
            // supported cell count needs, and far less than a host can be made
            // to allocate by accident.
            max_bytes: 512 << 20,
            // The same ceiling admission uses, so a workbook cannot enter by
            // one door at a size the other refuses.
            max_populated_cells: 8_000_000,
        }
    }
}

/// Intern `expr` into an arena and its fingerprint index.
///
/// Factored out of [`Workbook::store_formula`] because the snapshot conversions
/// need the same interning against a *fresh* arena, and a second copy of this
/// logic is a second place for the fingerprint-is-a-hint rule to be got wrong.
fn intern_into(
    arena: &mut Vec<Expr>,
    index: &mut BTreeMap<u64, Vec<u32>>,
    expr: Expr,
) -> FormulaHandle {
    let print = expr.fingerprint();
    if let Some(candidates) = index.get(&print) {
        for &i in candidates {
            if arena.get(i as usize) == Some(&expr) {
                return FormulaHandle(i);
            }
        }
    }
    let at = arena.len() as u32;
    arena.push(expr);
    index.entry(print).or_default().push(at);
    FormulaHandle(at)
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
            formula_index: BTreeMap::new(),
            defined_names: Vec::new(),
            sheets: Vec::new(),
            default_font_name: None,
            default_font_size_hp: None,
            theme_colors: Vec::new(),
            settings: WorkbookSettings::default(),
            properties: DocumentProperties::default(),
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
    /// Intern a formula for the cell at `origin`, storing its references
    /// **relative to that cell**.
    ///
    /// This is what collapses a filled-down column to one tree (`PERF-11`):
    /// `A1*2` in `B1` and `A2*2` in `B2` both come out as "one column left,
    /// same row", so the fingerprint matches and the arena keeps one of them.
    ///
    /// `expr` is expected in the **absolute form** — what
    /// [`parse`](fn@casual_calc_formula::parse) produces — because a tree already
    /// stored somewhere would be re-measured from the wrong place.
    pub fn store_formula_at(&mut self, expr: Expr, origin: Origin) -> FormulaHandle {
        self.store_formula(casual_calc_formula::restore_at(&expr, ABSOLUTE, origin))
    }

    pub fn store_formula(&mut self, expr: Expr) -> FormulaHandle {
        // **Interned, not appended** (`PERF-09`). Two things were wrong with
        // appending, and only the first was written down:
        //
        // - N cells carrying the identical expression cost N ASTs, against the
        //   arena docs/40 describes and the 1M-cell budget.
        // - Nothing reclaimed. Every edit that replaced a formula appended a
        //   new AST and orphaned the old one, so a person editing one cell back
        //   and forth grew the table once per edit for the life of the session.
        //   A thousand edits between two formulas left a thousand ASTs.
        //
        // The index is keyed by fingerprint rather than by the expression, so
        // the map does not hold a second copy of every AST — which would have
        // spent on unique formulas exactly what this saves on repeated ones.
        // A fingerprint is a hint; equality decides, so a collision costs one
        // comparison and never a wrong handle.
        intern_into(&mut self.formulas, &mut self.formula_index, expr)
    }

    /// Plant a wrong candidate under `print`, to exercise the collision path.
    ///
    /// Two expressions with the same fingerprint cannot be constructed on
    /// purpose — that is what makes the hash worth having — so the one property
    /// the design rests on, that **equality decides and the fingerprint only
    /// narrows**, has no natural test. This makes it reachable.
    #[cfg(test)]
    pub(crate) fn plant_collision(&mut self, print: u64, index: u32) {
        self.formula_index.entry(print).or_default().push(index);
    }

    /// Rebuild the intern index from the arena.
    ///
    /// The index is derived state and is not serialised — a snapshot carries
    /// the arena, and a workbook deserialised without this would intern nothing
    /// and start appending again, silently. Called wherever a `Workbook` is
    /// constructed other than by `new`.
    fn reindex_formulas(&mut self) {
        self.formula_index.clear();
        for (i, expr) in self.formulas.iter().enumerate() {
            self.formula_index
                .entry(expr.fingerprint())
                .or_default()
                .push(i as u32);
        }
    }

    /// Re-store every formula relative to the cell that holds it.
    ///
    /// The inverse of [`Self::absolute_view`], run once when a snapshot is
    /// read. This is where a filled-down column collapses: N absolute trees
    /// arrive, each becomes "one column left, same row", and interning keeps
    /// **one** of them (`PERF-11`).
    fn relativise_formulas(&mut self) {
        let (mut arena, mut index) = (Vec::new(), BTreeMap::new());
        let absolute = std::mem::take(&mut self.formulas);
        for sheet in &mut self.sheets {
            let addresses: Vec<CellRef> = sheet
                .cells
                .iter()
                .filter(|(_, c)| c.formula.is_some())
                .map(|(addr, _)| addr)
                .collect();
            for addr in addresses {
                let Some(cell) = sheet.cells.get(addr) else {
                    continue;
                };
                let Some(expr) = cell.formula.and_then(|h| absolute.get(h.0 as usize)) else {
                    continue;
                };
                let stored =
                    casual_calc_formula::restore_at(expr, ABSOLUTE, Origin::at(addr.row, addr.col));
                let handle = intern_into(&mut arena, &mut index, stored);
                let mut cell = cell.clone();
                cell.formula = Some(handle);
                sheet.cells.set(addr, cell);
            }
        }
        self.formulas = arena;
        self.formula_index = index;
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
        Ok(serde_json::to_vec(&self.absolute_view())?)
    }

    /// This workbook with every formula in the **absolute form**.
    ///
    /// The snapshot format does not change (`PERF-11`, and `ADR-010` forbids
    /// moving `SCHEMA_VERSION` anyway): relativity is an in-memory
    /// representation, and what goes on the wire is what always went on it.
    ///
    /// A shared tree has no single origin, so this cannot be a transformation
    /// of the arena alone — the cells are what say which origin belongs to
    /// which tree, and each gets its own resolved copy. That un-shares what
    /// sharing saved, which is the honest price of leaving the format alone:
    /// **resident memory improves, snapshot size does not.**
    fn absolute_view(&self) -> Self {
        let mut out = self.clone();
        out.formulas.clear();
        out.formula_index.clear();
        for sheet in &mut out.sheets {
            let addresses: Vec<CellRef> = sheet
                .cells
                .iter()
                .filter(|(_, c)| c.formula.is_some())
                .map(|(addr, _)| addr)
                .collect();
            for addr in addresses {
                let Some(cell) = sheet.cells.get(addr) else {
                    continue;
                };
                let Some(expr) = cell.formula.and_then(|h| self.formula(h)) else {
                    continue;
                };
                let absolute =
                    casual_calc_formula::restore_at(expr, Origin::at(addr.row, addr.col), ABSOLUTE);
                // Interned as it goes, so identical *absolute* formulas still
                // share — which is what `PERF-09` gave the snapshot and this
                // must not take away.
                let handle = intern_into(&mut out.formulas, &mut out.formula_index, absolute);
                let mut cell = cell.clone();
                cell.formula = Some(handle);
                sheet.cells.set(addr, cell);
            }
        }
        out
    }

    /// Parse a snapshot and validate it, under the default limits.
    ///
    /// # Errors
    ///
    /// [`ModelError::SnapshotTooLarge`] before anything is parsed, if the bytes
    /// are over the ceiling; see [`SnapshotLimits`].
    pub fn from_snapshot(bytes: &[u8]) -> Result<Self, ModelError> {
        Self::from_snapshot_with(bytes, SnapshotLimits::default())
    }

    /// The same, under given limits.
    ///
    /// A snapshot is untrusted in exactly the way an uploaded package is — it
    /// arrives from a host, a resumed session, or a cluster peer — and it was
    /// the one admission path with no ceiling at all (`SEC-013`). The byte
    /// check happens **before** `serde_json` sees the input, because a limit
    /// applied after parsing has already paid for the allocation it exists to
    /// prevent.
    ///
    /// The cell count is checked after, since it cannot be known before; it is
    /// there so a snapshot and a package cannot admit different amounts of the
    /// same workbook.
    ///
    /// # Errors
    ///
    /// [`ModelError::SnapshotTooLarge`] over either ceiling, [`ModelError::Snapshot`]
    /// if the bytes are not a snapshot, and whatever [`validate`](Self::validate)
    /// refuses.
    pub fn from_snapshot_with(bytes: &[u8], limits: SnapshotLimits) -> Result<Self, ModelError> {
        let asked = bytes.len() as u64;
        if asked > limits.max_bytes {
            return Err(ModelError::SnapshotTooLarge {
                what: "bytes",
                limit: limits.max_bytes,
                asked,
            });
        }
        let mut workbook: Workbook = serde_json::from_slice(bytes)?;
        // Derived state the snapshot does not carry. Without this the arena is
        // full and the index empty, so every later formula appends — the exact
        // behaviour PERF-09 removed, restored by a round trip.
        workbook.reindex_formulas();
        // The snapshot's formulas are absolute; the model's are relative to the
        // cell holding them (`PERF-11`). Re-storing here is also where the
        // sharing appears: a filled-down column arrives as N absolute trees and
        // becomes one.
        workbook.relativise_formulas();
        let cells: usize = workbook.sheets.iter().map(|s| s.cells.len()).sum();
        if cells > limits.max_populated_cells {
            return Err(ModelError::SnapshotTooLarge {
                what: "populated cells",
                limit: limits.max_populated_cells as u64,
                asked: cells as u64,
            });
        }
        workbook.validate()?;
        Ok(workbook)
    }
}
