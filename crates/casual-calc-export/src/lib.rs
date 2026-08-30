//! `casual-calc-export` — the semantic SpreadsheetML writer.
//!
//! Phase 1B: serializes a normalized [`Workbook`] back to a valid, deterministic
//! `.xlsx` package — cell values, formulas (from the AST), number formats,
//! merged ranges, frozen panes, and defined names. The output is a *semantic*
//! reconstruction (canonical OOXML), not a byte-identical copy of an original
//! (that is the retention-mode repackager, a later increment). The guarantee is
//! the **semantic fixed point**: `import → write → import` yields an equal model.
//!
//! See `docs/36-EXPORT-AND-ROUNDTRIP-DESIGN.md`.

mod chart;
mod error;
pub mod pivot;
mod xml;

pub use error::ExportError;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};

use casual_calc_formula::stored::{ABSOLUTE, Origin};
use casual_calc_formula::{Expr, column_to_letters, qualify_bound_names, qualify_future_functions};
use casual_calc_model::{
    AutoFilter, BorderEdge, Borders, Cell, CellRange, CellValue, CfRule, ConditionalFormat, DvKind,
    DvOperator, ErrorValue, FilterRule, GradientFill, HAlign, RetainedRel, RunFont, Sheet, SheetId,
    StringId, Style, Table, ThemeTint, Underline, VAlign, VertAlign, Workbook, from_micro,
};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use xml::{escape_attr, escape_text};

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const NS_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
/// The DrawingML namespace, used by the theme part.
const NS_DRAWING: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
/// The 2018 threaded-comments namespace, shared by the persons and
/// threadedComments parts.
const NS_TC: &str = "http://schemas.microsoft.com/office/spreadsheetml/2018/threadedcomments";
/// The relationship types for those parts live outside the standard `NS_R`
/// family, under Microsoft's 2017 extension namespace.
const NS_R_TC: &str = "http://schemas.microsoft.com/office/2017/10/relationships/threadedComment";
const NS_R_PERSON: &str = "http://schemas.microsoft.com/office/2017/10/relationships/person";
/// The author recorded when a comment carries none. Deliberately empty rather
/// than a stand-in name: both schemas require an author slot, but inventing one
/// would attribute an anonymous note to somebody, and it would come back on the
/// next import as a real name the file never held.
const DEFAULT_AUTHOR: &str = "";
/// The timestamp written for a thread that has none. Threaded comments require
/// `dT`, and inventing "now" here would make two saves of one workbook differ.
const EPOCH_STAMP: &str = "1970-01-01T00:00:00.00";
const FIRST_CUSTOM_NUM_FMT: u32 = 164;
const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";

/// The content type of `xl/workbook.xml` in a plain workbook package.
const CT_WORKBOOK: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
/// The content type of `xl/workbook.xml` in a macro-enabled package.
///
/// The **only** difference between an `.xlsx` and an `.xlsm` at the package
/// level, besides the VBA part itself: same schema, same parts, same reader.
const CT_WORKBOOK_MACRO: &str = "application/vnd.ms-excel.sheet.macroEnabled.main+xml";

/// Which OOXML spreadsheet flavour a written package declares itself to be.
///
/// Not a guess about the file name. The workbook part's content type is what a
/// reader keys off, and getting it wrong is not cosmetic in either direction: a
/// macro-enabled declaration on a package with no macros makes Excel warn about
/// content it will not find, and a plain declaration on a package that *does*
/// carry a VBA project makes Excel report the file as damaged and repair it by
/// deleting the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PackageKind {
    /// `.xlsx` — a workbook that carries no macros.
    #[default]
    Workbook,
    /// `.xlsm` — a macro-enabled workbook.
    MacroEnabled,
}

impl PackageKind {
    /// The flavour a workbook **must** be written as, given what it carries.
    ///
    /// A package holding a VBA project is macro-enabled whether or not its
    /// caller thought so, which is why this is consulted even when the caller
    /// named a flavour: the alternative is a file that denies what is inside it.
    #[must_use]
    pub fn for_workbook(workbook: &Workbook) -> Self {
        if workbook.macro_project().is_some() {
            Self::MacroEnabled
        } else {
            Self::Workbook
        }
    }

    fn workbook_content_type(self) -> &'static str {
        match self {
            Self::Workbook => CT_WORKBOOK,
            Self::MacroEnabled => CT_WORKBOOK_MACRO,
        }
    }
}

/// Which shared strings reach the package, and at what index (`FID-36`).
///
/// The model's table is append-only and a [`StringId`](casual_calc_model::StringId)
/// *is* an index into it, so it cannot be compacted in place — the undo stack,
/// the clipboard and a collaboration session all hold ids that would then
/// resolve to somebody else's text. The pruning therefore happens **here**, at
/// the moment of writing, and the model is left alone.
///
/// Two kinds of entry are emitted, and telling them apart is the whole of the
/// decision:
///
/// - Everything **below the table's preserved watermark** arrived with the
///   document. It is written whether or not a cell refers to it: an
///   unreferenced `<si>` in the file somebody opened is theirs, and dropping it
///   would be exactly the silent data loss AGENTS.md forbids. Because the whole
///   prefix is emitted in order, these entries keep the indices they had, so
///   anything retained verbatim that names a shared string by index still
///   resolves.
/// - Everything **at or above it** this session interned. Those are emitted
///   only while a cell still refers to them, which is what stops the text of an
///   edit that was undone — or of a formula result that was recalculated away —
///   from reaching a file the user hands to somebody else.
struct SharedStrings {
    /// Model index → index in the written `<sst>`, or `None` when the entry is
    /// not written at all.
    emitted_at: Vec<Option<u32>>,
    /// Model indices in written order.
    order: Vec<u32>,
}

impl SharedStrings {
    /// Decide the table for `workbook`. One pass over the populated cells,
    /// skipped entirely when the workbook has interned nothing of its own.
    fn plan(workbook: &Workbook) -> Self {
        let len = workbook.strings.len();
        let preserved = workbook.strings.preserved_len().min(len);
        // Nothing this session interned, so nothing is reclaimable and the
        // cells need not be walked: a freshly opened workbook pays nothing.
        if preserved == len {
            return Self {
                emitted_at: (0..len as u32).map(Some).collect(),
                order: (0..len as u32).collect(),
            };
        }
        // Only the tail can be dropped, so only the tail is tracked — one bool
        // per string this session interned, not one per string in the workbook.
        let mut used = vec![false; len - preserved];
        for sheet in &workbook.sheets {
            for (_, cell) in sheet.cells.iter() {
                let id = match &cell.value {
                    CellValue::SharedString(id) | CellValue::InlineString(id) => *id,
                    _ => continue,
                };
                if let Some(index) = workbook.strings.index_of(id)
                    && (index as usize) >= preserved
                {
                    used[index as usize - preserved] = true;
                }
            }
        }
        let mut emitted_at = Vec::with_capacity(len);
        let mut order = Vec::with_capacity(len);
        for index in 0..len as u32 {
            let keep = (index as usize) < preserved || used[index as usize - preserved];
            if keep {
                emitted_at.push(Some(order.len() as u32));
                order.push(index);
            } else {
                emitted_at.push(None);
            }
        }
        Self { emitted_at, order }
    }

    /// Whether any string reaches the package — and so whether the part, its
    /// relationship and its content-type override are written at all.
    fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The index to write in a cell's `<v>`, for a string that is emitted.
    fn index_of(&self, workbook: &Workbook, id: StringId) -> Option<u32> {
        let model = workbook.strings.index_of(id)? as usize;
        *self.emitted_at.get(model)?
    }
}

/// Serialize a workbook to a deterministic `.xlsx` package.
///
/// A workbook still carrying a [VBA
/// project](casual_calc_model::Workbook::macro_project) is written as the
/// macro-enabled package it is — see [`write_workbook_as`] for the caller that
/// wants to name the flavour, and
/// [`remove_macro_project`](casual_calc_model::Workbook::remove_macro_project)
/// for the one that wants a plain `.xlsx` out of a macro workbook.
///
/// # Errors
///
/// As [`write_workbook_as`].
pub fn write_workbook(workbook: &Workbook) -> Result<Vec<u8>, ExportError> {
    write_workbook_as(workbook, PackageKind::for_workbook(workbook))
}

/// The same, declaring a named flavour.
///
/// `kind` is **raised** to [`PackageKind::MacroEnabled`] when the workbook
/// carries a VBA project: a package cannot both hold macros and deny holding
/// them, and the writer is the last place that can be true. A caller that wants
/// a genuinely macro-free `.xlsx` out of a macro workbook removes the project
/// first and says so in its compatibility report.
///
/// # Errors
///
/// As [`write_workbook`]: an unwritable workbook or a zip failure.
pub fn write_workbook_as(workbook: &Workbook, kind: PackageKind) -> Result<Vec<u8>, ExportError> {
    let kind = match PackageKind::for_workbook(workbook) {
        PackageKind::MacroEnabled => PackageKind::MacroEnabled,
        PackageKind::Workbook => kind,
    };
    // Which strings reach the file, and at what index. Decided before anything
    // is written, because the part, its relationship, its content-type override
    // and every cell that names an index all have to agree about it.
    let strings = SharedStrings::plan(workbook);
    let has_strings = !strings.is_empty();
    // Conditional-format fills become `<dxfs>` in styles.xml, shared by dxfId
    // with the worksheet `<cfRule>`s — so styles.xml is written when there are
    // dxfs even if no cell carries a style.
    let dxfs = collect_dxfs(workbook);
    let has_styles = !workbook.styles.is_empty() || !dxfs.is_empty();

    // Charts made here, before anything that has to reference them: a worksheet
    // names its drawing, the content types declare both parts, and neither can
    // be written without knowing which sheets ended up with one. Numbered
    // across the workbook like tables, so two sheets never claim `chart1.xml`.
    let mut chart_builds: Vec<chart::SheetCharts> = Vec::new();
    let mut next_chart = 1;
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        if let Some(built) = chart::build(workbook, sheet, i, next_chart) {
            next_chart += built.chart_parts.len();
            chart_builds.push(built);
        } else {
            // A placeholder keeps the vector index equal to the sheet index,
            // which is what the worksheet writer looks up by.
            chart_builds.push(chart::SheetCharts::none());
        }
    }

    // Once, because `workbook.xml` and `workbook.xml.rels` have to name the
    // same id for the same sheet.
    // Authored pivots, numbered across the workbook. An *imported* pivot keeps
    // its retained part and is written back byte for byte, exactly as a chart
    // is — this is only the ones created here, which until now reached the file
    // as the cells their last refresh wrote and nothing else (`PIV-02`).
    let mut pivot_builds: Vec<pivot::SheetPivots> = Vec::new();
    let mut next_pivot = 1usize;
    for sheet in &workbook.sheets {
        let built = pivot::build(workbook, sheet, next_pivot, next_pivot);
        next_pivot += pivot::authored(sheet).len();
        pivot_builds.push(built);
    }
    let workbook_rel_ids = WorkbookRelIds::mint(workbook);
    // Likewise per sheet, for `<legacyDrawing r:id>` and the worksheet `.rels`.
    let sheet_rel_ids: Vec<SheetRelIds> = (0..workbook.sheets.len())
        .map(|i| {
            SheetRelIds::mint(
                &workbook.retained_rels,
                &format!("xl/worksheets/sheet{}.xml", i + 1),
            )
        })
        .collect();

    let mut parts: Vec<(String, String)> = vec![
        (
            "[Content_Types].xml".to_owned(),
            content_types(
                workbook,
                kind,
                has_styles,
                has_strings,
                any_theme_link(workbook),
                &chart_builds,
                &pivot_builds,
            ),
        ),
        ("_rels/.rels".to_owned(), root_rels(workbook)),
        (
            "xl/workbook.xml".to_owned(),
            workbook_xml(workbook, &workbook_rel_ids, &pivot_builds),
        ),
        (
            "xl/_rels/workbook.xml.rels".to_owned(),
            workbook_rels(
                workbook,
                &workbook_rel_ids,
                has_styles,
                has_strings,
                any_theme_link(workbook),
                &pivot_builds,
            ),
        ),
    ];
    if has_strings {
        parts.push((
            "xl/sharedStrings.xml".to_owned(),
            shared_strings_xml(workbook, &strings),
        ));
    }
    if has_styles {
        parts.push(("xl/styles.xml".to_owned(), styles_xml(workbook, &dxfs)));
    }
    // The theme part goes in whenever a style names a theme slot: without it a
    // `theme="4"` resolves against the reader's default palette rather than
    // this workbook's, so the colours would change on reopen.
    let has_theme = any_theme_link(workbook);
    if has_theme {
        parts.push(("xl/theme/theme1.xml".to_owned(), theme_xml(workbook)));
    }
    for (i, built) in chart_builds.iter().enumerate() {
        parts.push((
            format!("xl/worksheets/sheet{}.xml", i + 1),
            worksheet_xml(workbook, i, &dxfs, built, &sheet_rel_ids[i], &strings),
        ));
    }
    // Comment parts: a comments part, a legacy VML drawing (so Excel renders the
    // note markers), and the per-sheet rels that tie them to the worksheet.
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        let sheet_part = format!("xl/worksheets/sheet{}.xml", i + 1);
        let has_retained = workbook
            .retained_rels
            .iter()
            .any(|r| r.source == sheet_part);
        if sheet.comments.is_empty()
            && sheet.hyperlinks.is_empty()
            && sheet.tables.is_empty()
            && !has_retained
            && chart_builds[i].sheet_rel.is_none()
            // A pivot is reached through this part and nothing else, so a sheet
            // whose only attachment is a pivot still needs one written.
            && pivot_builds[i].sheet_rels.is_empty()
        {
            continue;
        }
        let n = i + 1;
        let threaded = sheet.comments.iter().any(|c| c.is_threaded());
        if !sheet.comments.is_empty() {
            parts.push((format!("xl/comments{n}.xml"), comments_xml(sheet)));
            parts.push((format!("xl/drawings/vmlDrawing{n}.vml"), vml_drawing(sheet)));
            if threaded {
                parts.push((
                    format!("xl/threadedComments/threadedComment{n}.xml"),
                    threaded_comments_xml(workbook, i),
                ));
            }
        }
        // Written for hyperlinks too: their targets live only in this part, so
        // a sheet with links and no notes still needs it.
        parts.push((
            format!("xl/worksheets/_rels/sheet{n}.xml.rels"),
            sheet_rels(
                sheet,
                n,
                threaded,
                &sheet_rel_ids[i],
                &workbook.retained_rels,
                &(0..sheet.tables.len())
                    .map(|j| table_index(workbook, i, j))
                    .collect::<Vec<_>>(),
                (&chart_builds[i], &pivot_builds[i]),
            ),
        ));
    }
    // Table parts are numbered across the whole workbook, not per sheet, which
    // is how Excel numbers them and what `table_index` below reproduces.
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        for (j, table) in sheet.tables.iter().enumerate() {
            let n = table_index(workbook, i, j);
            parts.push((format!("xl/tables/table{n}.xml"), table_xml(table)));
        }
    }
    if any_threaded(workbook) {
        parts.push(("xl/persons/person1.xml".to_owned(), persons_xml(workbook)));
    }

    // Written from the model, and *generated* so it wins over a retained copy at
    // the same path — the mechanism `FID-27` used for an edited chart's part.
    if has_core_properties(workbook) {
        parts.push((
            "docProps/core.xml".to_owned(),
            core_properties_part(workbook),
        ));
    }

    for built in &pivot_builds {
        parts.extend(built.parts.iter().cloned());
        parts.extend(built.rels.iter().cloned());
    }

    for built in &chart_builds {
        // An empty path is the placeholder for a sheet that contributed
        // nothing. A sheet can still contribute a drawing with no chart parts:
        // that is the rebuild that clears an anchor left dangling by a deleted
        // or edited imported chart.
        if built.drawing_part.is_empty() {
            continue;
        }
        parts.push((built.drawing_part.clone(), built.drawing_xml.clone()));
        parts.push((
            rels_path_for(&built.drawing_part),
            built.drawing_rels.clone(),
        ));
        parts.extend(built.chart_parts.iter().cloned());
    }

    // A retained part can declare relationships of its own — a drawing names
    // its charts and images — so every source that is not workbook.xml or a
    // worksheet needs its `.rels` regenerated too. Without this the chart is in
    // the package but the drawing no longer reaches it, which Excel reports as
    // a file needing repair.
    let mut by_source: BTreeMap<&str, Vec<&RetainedRel>> = BTreeMap::new();
    for rel in &workbook.retained_rels {
        by_source.entry(rel.source.as_str()).or_default().push(rel);
    }
    for (source, rels) in by_source {
        if source.is_empty() || is_workbook(source) || source.contains("/worksheets/") {
            continue; // written by root_rels / workbook_rels / sheet_rels
        }
        // A drawing this run rebuilt already had its rels written, with the
        // retained entries folded in, so writing them again puts two `.rels` at
        // one path — which the zip writer refuses, failing the whole save.
        //
        // The condition used to also require the build to have produced *chart
        // parts*, and that was the bug: a sheet can contribute a rebuilt
        // drawing with none, which is exactly what deleting an imported chart
        // leaves behind once its dangling anchor is stripped. Saving such a
        // workbook failed outright with `Duplicate filename`. Having a chart
        // part was never what made the rels get written — a non-empty
        // `drawing_part` is (`FID-30`).
        if chart_builds.iter().any(|b| b.drawing_part == source) {
            continue;
        }
        let mut xml = format!("{DECL}<Relationships xmlns=\"{NS_REL}\">");
        for rel in rels {
            xml.push_str(&retained_rel_xml(rel));
        }
        xml.push_str("</Relationships>");
        parts.push((rels_path_for(source), xml));
    }

    package_with_retained(&parts, workbook)
}

/// Write the semantic parts, then the retained ones byte for byte.
///
/// Retained parts are appended rather than merged into `parts` because they are
/// raw bytes, not XML we generated: an image or an OLE stream is not a string.
/// A formula **as the file must carry it**, which is not quite the language.
///
/// SpreadsheetML prefixes any function it postdates with `_xlfn.`; a writer that
/// emits the bare name produces a file that opens, looks complete, and shows
/// `#NAME?` in every cell that used one — with no way for a reader to recover
/// what the formula said. Applied here rather than in the printer because it is
/// a fact about the file, not about the expression: the same tree printed into a
/// formula bar must not carry it.
/// A formula as `<f>` should carry it: the text a person wrote, at `origin`.
///
/// The origin matters since `PERF-11`: a stored tree's references are offsets
/// from the cell holding it, so printing one absolutely writes a formula that
/// names whatever those offsets happen to point at from `A1`. A defined name
/// has no holding cell and passes [`ABSOLUTE`].
fn formula_text(expr: &Expr, origin: Origin) -> String {
    let mut owned = expr.clone();
    // Bound names first: the pass that finds them matches `LAMBDA` and `LET`,
    // which the next pass is about to rename. (It tolerates either order, but
    // depending on that would be a coincidence rather than a decision.)
    qualify_bound_names(&mut owned);
    qualify_future_functions(&mut owned);
    casual_calc_formula::print_at(&owned, origin)
}

fn package_with_retained(
    parts: &[(String, String)],
    workbook: &Workbook,
) -> Result<Vec<u8>, ExportError> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    for (path, content) in parts {
        writer.start_file(path, options)?;
        writer.write_all(content.as_bytes())?;
    }
    for retained in &workbook.retained_parts {
        // A part we also generated wins: retaining a stale copy of something
        // now modelled would write it twice and let the older one win on read.
        if parts.iter().any(|(p, _)| p == &retained.path) {
            continue;
        }
        writer.start_file(&retained.path, options)?;
        writer.write_all(&retained.bytes)?;
    }
    Ok(writer.finish()?.into_inner())
}

fn content_types(
    workbook: &Workbook,
    kind: PackageKind,
    has_styles: bool,
    has_strings: bool,
    has_theme: bool,
    charts: &[chart::SheetCharts],
    pivots: &[pivot::SheetPivots],
) -> String {
    let any_comments = workbook.sheets.iter().any(|s| !s.comments.is_empty());
    let mut s = format!("{DECL}<Types xmlns=\"{NS_CT}\">");
    s.push_str("<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>");
    s.push_str("<Default Extension=\"xml\" ContentType=\"application/xml\"/>");
    if any_comments {
        s.push_str("<Default Extension=\"vml\" ContentType=\"application/vnd.openxmlformats-officedocument.vmlDrawing\"/>");
    }
    // The one part whose type says which flavour of package this is. Excel
    // reads it, not the file name, and a `vbaProject.bin` under the plain
    // workbook type is a package it opens as damaged.
    s.push_str(&format!(
        "<Override PartName=\"/xl/workbook.xml\" ContentType=\"{}\"/>",
        kind.workbook_content_type()
    ));
    for i in 0..workbook.sheets.len() {
        s.push_str(&format!(
            "<Override PartName=\"/xl/worksheets/sheet{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>",
            i + 1
        ));
    }
    if has_styles {
        s.push_str("<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>");
    }
    if has_strings {
        s.push_str("<Override PartName=\"/xl/sharedStrings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml\"/>");
    }
    if has_theme {
        s.push_str("<Override PartName=\"/xl/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>");
    }
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        if !sheet.comments.is_empty() {
            s.push_str(&format!(
                "<Override PartName=\"/xl/comments{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml\"/>",
                i + 1
            ));
        }
        if sheet.comments.iter().any(|c| c.is_threaded()) {
            s.push_str(&format!(
                "<Override PartName=\"/xl/threadedComments/threadedComment{}.xml\" ContentType=\"application/vnd.ms-excel.threadedcomments+xml\"/>",
                i + 1
            ));
        }
    }
    if any_threaded(workbook) {
        s.push_str("<Override PartName=\"/xl/persons/person1.xml\" ContentType=\"application/vnd.ms-excel.person+xml\"/>");
    }
    // A retained part must be declared here or the package is invalid, and
    // Excel refuses to open it rather than ignoring the undeclared part.
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        for j in 0..sheet.tables.len() {
            s.push_str(&format!(
                "<Override PartName=\"/xl/tables/table{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml\"/>",
                table_index(workbook, i, j)
            ));
        }
    }
    // Charts made here, and the drawing part holding them when the sheet did
    // not already have one. An undeclared part is not ignored: Excel refuses
    // the package.
    // Both new part kinds need an override: a package with an undeclared part
    // opens as damaged rather than as a workbook without a pivot.
    if has_core_properties(workbook) {
        s.push_str(
            "<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>",
        );
    }
    for built in pivots {
        for (path, _) in &built.parts {
            let ct = if path.contains("pivotCacheDefinition") {
                pivot::CT_CACHE
            } else {
                pivot::CT_TABLE
            };
            s.push_str(&format!(
                "<Override PartName=\"/{}\" ContentType=\"{ct}\"/>",
                escape_attr(path)
            ));
        }
    }
    for (path, content_type) in chart::content_types(charts) {
        s.push_str(&format!(
            "<Override PartName=\"/{}\" ContentType=\"{content_type}\"/>",
            escape_attr(&path)
        ));
    }
    // Retained parts, each with its own `<Override>` — deliberately, and not
    // because it is the shorter code.
    //
    // A real producer writes `<Default Extension="bin" …/>` for a workbook full
    // of `printerSettings*.bin`, and re-emitting these as Defaults would look
    // more like the file we read. It is also ambiguous in a way an Override
    // never is: one extension carries one type per package, and `.bin` is
    // printer settings here, an OLE object there, and a pivot cache record
    // stream in the file after that. Grouping by extension would make the
    // writer pick a winner among types that disagree, and the part that lost
    // would be declared as something it is not — worse than the undeclared part
    // this fixes, because the package would open and misread it. Per part costs
    // a line of XML each and cannot collide.
    //
    // An Override also outranks whatever `<Default>` this writer emitted above,
    // so a retained `.vml` or `.xml` part with a specific type keeps it rather
    // than being flattened to `application/xml`.
    for retained in &workbook.retained_parts {
        // `None` only when the source package declared no type for the part
        // either — it arrived undeclared and there is nothing to carry. It is
        // not the writer's place to invent one from the extension: the file
        // that comes out would claim something no file ever said.
        if let Some(ct) = &retained.content_type {
            s.push_str(&format!(
                "<Override PartName=\"/{}\" ContentType=\"{}\"/>",
                escape_attr(&retained.path),
                escape_attr(ct)
            ));
        }
    }
    s.push_str("</Types>");
    s
}

/// The `xl/comments{n}.xml` part: authors + one `<comment>` per note.
fn comments_xml(sheet: &Sheet) -> String {
    let mut authors: Vec<String> = Vec::new();
    for c in &sheet.comments {
        let a = c
            .author
            .clone()
            .unwrap_or_else(|| DEFAULT_AUTHOR.to_owned());
        if !authors.contains(&a) {
            authors.push(a);
        }
    }
    if authors.is_empty() {
        authors.push(DEFAULT_AUTHOR.to_owned());
    }
    let mut s = format!("{DECL}<comments xmlns=\"{NS_MAIN}\"><authors>");
    for a in &authors {
        s.push_str(&format!("<author>{}</author>", escape_text(a)));
    }
    s.push_str("</authors><commentList>");
    for c in &sheet.comments {
        let a = c
            .author
            .clone()
            .unwrap_or_else(|| DEFAULT_AUTHOR.to_owned());
        let aid = authors.iter().position(|x| *x == a).unwrap_or(0);
        s.push_str(&format!(
            "<comment ref=\"{}\" authorId=\"{aid}\"><text><r><t xml:space=\"preserve\">{}</t></r></text></comment>",
            cell_a1(c.at.row, c.at.col),
            escape_text(&c.text)
        ));
    }
    s.push_str("</commentList></comments>");
    s
}

/// Every author named anywhere in the workbook's threads, in first-seen order.
fn thread_authors(workbook: &Workbook) -> Vec<String> {
    let mut authors: Vec<String> = Vec::new();
    let mut note = |a: &Option<String>| {
        let a = a.clone().unwrap_or_else(|| DEFAULT_AUTHOR.to_owned());
        if !authors.contains(&a) {
            authors.push(a);
        }
    };
    for sheet in &workbook.sheets {
        for c in &sheet.comments {
            note(&c.author);
            for r in &c.replies {
                note(&r.author);
            }
        }
    }
    authors
}

/// A stable GUID in Excel's `{8-4-4-4-12}` form, derived from `seed`.
///
/// Excel expects a GUID here and does not care which one, so it is derived from
/// the thread's own coordinates rather than drawn from a random source: writing
/// the same workbook twice has to produce the same bytes, and a random GUID
/// would make every save differ from the last for no reason.
fn stable_guid(seed: &str) -> String {
    // FNV-1a, run over the seed with four different offsets for 128 bits.
    let mut out = String::with_capacity(38);
    let mut hex = String::with_capacity(32);
    for salt in 0u8..4 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325 ^ u64::from(salt);
        for b in seed.bytes() {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hex.push_str(&format!("{hash:016x}"));
    }
    let hex = &hex[..32];
    out.push('{');
    for (i, chunk) in [0..8, 8..12, 12..16, 16..20, 20..32]
        .into_iter()
        .enumerate()
    {
        if i > 0 {
            out.push('-');
        }
        out.push_str(&hex[chunk]);
    }
    out.push('}');
    out
}

/// The `xl/persons/person1.xml` part: the people who can be referenced by a
/// threaded comment's `personId`.
fn persons_xml(workbook: &Workbook) -> String {
    let mut s = format!("{DECL}<personList xmlns=\"{NS_TC}\" xmlns:x=\"{NS_MAIN}\">");
    for a in thread_authors(workbook) {
        s.push_str(&format!(
            "<person displayName=\"{}\" id=\"{}\" userId=\"{}\" providerId=\"None\"/>",
            escape_text(&a),
            stable_guid(&format!("person:{a}")),
            escape_text(&a),
        ));
    }
    s.push_str("</personList>");
    s
}

/// The `xl/threadedComments/threadedComment{n}.xml` part: the opening remark and
/// each reply as a sibling `<threadedComment>`, replies pointing at the root
/// through `parentId` (the schema is flat, not nested).
fn threaded_comments_xml(workbook: &Workbook, sheet_index: usize) -> String {
    let sheet = &workbook.sheets[sheet_index];
    let person = |a: &Option<String>| {
        stable_guid(&format!(
            "person:{}",
            a.as_deref().unwrap_or(DEFAULT_AUTHOR)
        ))
    };
    let mut s = format!("{DECL}<ThreadedComments xmlns=\"{NS_TC}\" xmlns:x=\"{NS_MAIN}\">");
    for c in sheet.comments.iter().filter(|c| c.is_threaded()) {
        let reference = cell_a1(c.at.row, c.at.col);
        let root_id = stable_guid(&format!("tc:{sheet_index}:{reference}"));
        s.push_str(&format!(
            "<threadedComment ref=\"{reference}\" dT=\"{}\" personId=\"{}\" id=\"{root_id}\"{}><text>{}</text></threadedComment>",
            escape_text(c.created.as_deref().unwrap_or(EPOCH_STAMP)),
            person(&c.author),
            if c.resolved { " done=\"1\"" } else { "" },
            escape_text(&c.text),
        ));
        for (i, r) in c.replies.iter().enumerate() {
            s.push_str(&format!(
                "<threadedComment ref=\"{reference}\" dT=\"{}\" personId=\"{}\" id=\"{}\" parentId=\"{root_id}\"><text>{}</text></threadedComment>",
                escape_text(r.created.as_deref().unwrap_or(EPOCH_STAMP)),
                person(&r.author),
                stable_guid(&format!("tc:{sheet_index}:{reference}:{i}")),
                escape_text(&r.text),
            ));
        }
    }
    s.push_str("</ThreadedComments>");
    s
}

/// A minimal legacy VML drawing anchoring a note marker at each commented cell,
/// so Excel shows the red indicator. The comment text lives in comments{n}.xml.
fn vml_drawing(sheet: &Sheet) -> String {
    let mut s = String::from(
        "<xml xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:x=\"urn:schemas-microsoft-com:office:excel\">\
<o:shapelayout v:ext=\"edit\"><o:idmap v:ext=\"edit\" data=\"1\"/></o:shapelayout>\
<v:shapetype id=\"_x0000_t202\" coordsize=\"21600,21600\" o:spt=\"202\" path=\"m,l,21600r21600,l21600,xe\"><v:stroke joinstyle=\"miter\"/><v:path gradientshapeok=\"t\" o:connecttype=\"rect\"/></v:shapetype>",
    );
    for (idx, c) in sheet.comments.iter().enumerate() {
        s.push_str(&format!(
            "<v:shape id=\"_x0000_s{}\" type=\"#_x0000_t202\" style=\"position:absolute;visibility:hidden\" fillcolor=\"#ffffe1\" o:insetmode=\"auto\">\
<v:fill color2=\"#ffffe1\"/><v:shadow on=\"t\" color=\"black\" obscured=\"t\"/><v:path o:connecttype=\"none\"/>\
<x:ClientData ObjectType=\"Note\"><x:MoveWithCells/><x:SizeWithCells/><x:AutoFill>False</x:AutoFill><x:Row>{}</x:Row><x:Column>{}</x:Column></x:ClientData>\
</v:shape>",
            1025 + idx,
            c.at.row,
            c.at.col
        ));
    }
    s.push_str("</xml>");
    s
}

/// The `xl/worksheets/_rels/sheet{n}.xml.rels`: links the VML drawing and the
/// comments part to the worksheet.
fn sheet_rels(
    sheet: &Sheet,
    n: usize,
    threaded: bool,
    ids: &SheetRelIds,
    retained: &[RetainedRel],
    table_part_numbers: &[usize],
    attached: (&chart::SheetCharts, &pivot::SheetPivots),
) -> String {
    let (charts, pivots) = attached;
    let mut s = format!("{DECL}<Relationships xmlns=\"{NS_REL}\">");
    // A pivot table is reached through the sheet's relationships and nothing
    // else: the worksheet XML never names one.
    for (id, target) in &pivots.sheet_rels {
        s.push_str(&format!(
            "<Relationship Id=\"{id}\" Type=\"{NS_R}/pivotTable\" Target=\"{target}\"/>"
        ));
    }
    if !sheet.comments.is_empty() {
        s.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"{NS_R}/vmlDrawing\" Target=\"../drawings/vmlDrawing{n}.vml\"/>\
<Relationship Id=\"{}\" Type=\"{NS_R}/comments\" Target=\"../comments{n}.xml\"/>",
            ids.vml, ids.comments
        ));
        if threaded {
            s.push_str(&format!(
                "<Relationship Id=\"{}\" Type=\"{NS_R_TC}\" Target=\"../threadedComments/threadedComment{n}.xml\"/>",
                ids.threaded
            ));
        }
    }
    for (j, _) in sheet.tables.iter().enumerate() {
        s.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"{NS_R}/table\" Target=\"../tables/table{}.xml\"/>",
            table_rel_id(j),
            table_part_numbers[j]
        ));
    }
    // Relationships to retained parts (drawings, and through them charts and
    // images) keep their original ids, because the `<drawing r:id>` element
    // above names them.
    let sheet_part = format!("xl/worksheets/sheet{n}.xml");
    for rel in workbook_rels_for(retained, &sheet_part) {
        s.push_str(&retained_rel_xml(rel));
    }
    // The drawing holding this sheet's authored charts, when the sheet did not
    // already point at one. A sheet that did keeps its original relationship
    // above, because the `<drawing r:id>` already written names it.
    if let Some((id, target)) = &charts.sheet_rel {
        s.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"{NS_R}/drawing\" Target=\"../{}\"/>",
            escape_attr(id),
            escape_attr(target.trim_start_matches("xl/"))
        ));
    }
    // A hyperlink target is `TargetMode="External"`: without that the URI is
    // read back as a path inside the package and the link is destroyed.
    for (i, target) in external_targets(sheet).iter().enumerate() {
        s.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"{NS_R}/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",
            hyperlink_rel_id(i),
            escape_attr(target)
        ));
    }
    s.push_str("</Relationships>");
    s
}

/// The distinct external targets a sheet links to, in first-use order.
///
/// Deduplicated because a workbook linking to the same address from fifty cells
/// should carry one relationship, not fifty; Excel writes it that way and the
/// rels part is otherwise mostly repetition.
fn external_targets(sheet: &Sheet) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for link in &sheet.hyperlinks {
        if let Some(target) = &link.target
            && !out.contains(target)
        {
            out.push(target.clone());
        }
    }
    out
}

/// The relationship id for the nth external hyperlink target. Numbered apart
/// from the fixed rIds the comment parts use so the two cannot collide.
fn hyperlink_rel_id(index: usize) -> String {
    format!("rIdHl{}", index + 1)
}

/// The workbook-wide 1-based number of a table part.
fn table_index(workbook: &Workbook, sheet: usize, within: usize) -> usize {
    workbook.sheets[..sheet]
        .iter()
        .map(|s| s.tables.len())
        .sum::<usize>()
        + within
        + 1
}

/// The relationship id for a sheet's nth table. Numbered apart from the fixed
/// ids the comment parts use so the two cannot collide.
fn table_rel_id(within: usize) -> String {
    format!("rIdTbl{}", within + 1)
}

/// One `xl/tables/table{n}.xml` part.
fn table_xml(table: &Table) -> String {
    let mut s = format!(
        "{DECL}<table xmlns=\"{NS_MAIN}\" id=\"{}\" name=\"{}\" displayName=\"{}\" ref=\"{}\"",
        table.id,
        escape_attr(&table.name),
        escape_attr(&table.display_name),
        range_a1(&table.range)
    );
    if table.header_row_count != 1 {
        s.push_str(&format!(" headerRowCount=\"{}\"", table.header_row_count));
    }
    if table.totals_row_count != 0 {
        s.push_str(&format!(" totalsRowCount=\"{}\"", table.totals_row_count));
    }
    for (k, v) in &table.attrs {
        s.push_str(&format!(" {k}=\"{}\"", escape_attr(v)));
    }
    s.push('>');
    if let Some(filter) = &table.auto_filter {
        write_auto_filter(&mut s, filter);
    }
    s.push_str(&format!("<tableColumns count=\"{}\">", table.columns.len()));
    for column in &table.columns {
        s.push_str(&format!(
            "<tableColumn id=\"{}\" name=\"{}\"",
            column.id,
            escape_attr(&column.name)
        ));
        if let Some(f) = &column.totals_row_function {
            s.push_str(&format!(" totalsRowFunction=\"{}\"", escape_attr(f)));
        }
        if let Some(l) = &column.totals_row_label {
            s.push_str(&format!(" totalsRowLabel=\"{}\"", escape_attr(l)));
        }
        // Both formulas are element text, not attributes.
        let body: Vec<String> = [
            column.calculated_column_formula.as_ref().map(|f| {
                format!(
                    "<calculatedColumnFormula>{}</calculatedColumnFormula>",
                    escape_text(f)
                )
            }),
            column
                .totals_row_formula
                .as_ref()
                .map(|f| format!("<totalsRowFormula>{}</totalsRowFormula>", escape_text(f))),
        ]
        .into_iter()
        .flatten()
        .collect();
        if body.is_empty() {
            s.push_str("/>");
        } else {
            s.push('>');
            s.push_str(&body.concat());
            s.push_str("</tableColumn>");
        }
    }
    s.push_str("</tableColumns>");
    if !table.style.is_empty() {
        s.push_str("<tableStyleInfo");
        for (k, v) in &table.style {
            s.push_str(&format!(" {k}=\"{}\"", escape_attr(v)));
        }
        s.push_str("/>");
    }
    s.push_str("</table>");
    s
}

/// The `.rels` path for a part: `a/b/c.xml` becomes `a/b/_rels/c.xml.rels`.
fn rels_path_for(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// The retained relationships declared by one part.
fn workbook_rels_for<'a>(rels: &'a [RetainedRel], source: &str) -> Vec<&'a RetainedRel> {
    rels.iter().filter(|r| r.source == source).collect()
}

/// One retained `<Relationship>`, re-emitted rather than re-minted.
///
/// Every `.rels` this writer produces goes through here, because the one
/// attribute easiest to forget is the one that changes what the target *means*:
/// without `TargetMode="External"` the `file:///other.xlsx` of an external link
/// is read back as a part path, and the reference to the other workbook is
/// destroyed on the very save that was meant to preserve it.
pub(crate) fn retained_rel_xml(rel: &RetainedRel) -> String {
    let mode = if rel.external {
        " TargetMode=\"External\""
    } else {
        ""
    };
    format!(
        "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{mode}/>",
        escape_attr(&rel.id),
        escape_attr(&rel.rel_type),
        escape_attr(&rel.target)
    )
}

/// Whether a retained relationship's source is the workbook part.
///
/// Matched by suffix rather than by equality because the source path comes from
/// the file that was imported, which is free to keep its workbook somewhere
/// other than `xl/workbook.xml`, while this writer always emits it there. The
/// three places that split retained relationships by source have to agree on
/// this test, or a relationship is written twice or not at all.
fn is_workbook(source: &str) -> bool {
    source.ends_with("workbook.xml")
}

/// Mints `rId`s that no retained relationship on the same part already claims.
///
/// A retained id cannot move: it came from the file, and the content naming it —
/// `<externalReference r:id="rId1"/>` — is re-emitted verbatim, so a renumbered
/// relationship would leave that element pointing at nothing. The minted ids are
/// named by nothing but their own part, so they are the ones that step aside.
/// `root_rels` does the same for the single relationship it mints.
struct RelIdMinter<'a> {
    taken: BTreeSet<&'a str>,
    n: u32,
}

impl<'a> RelIdMinter<'a> {
    fn avoiding(taken: impl Iterator<Item = &'a str>) -> Self {
        Self {
            taken: taken.collect(),
            n: 0,
        }
    }

    fn mint(&mut self) -> String {
        loop {
            self.n += 1;
            let id = format!("rId{}", self.n);
            if !self.taken.contains(id.as_str()) {
                return id;
            }
        }
    }
}

/// The ids the workbook part mints for the pieces this writer produces itself.
///
/// Minted once and handed to both writers, because two parts have to agree on
/// them: `workbook.xml` names each sheet by id and `workbook.xml.rels` defines
/// it, and a sheet whose `r:id` matches no relationship is a sheet Excel cannot
/// open. Deriving the same sequence independently in each writer would hold
/// until the day one of them grew a case the other did not.
struct WorkbookRelIds {
    sheets: Vec<String>,
    styles: String,
    strings: String,
    theme: String,
    person: String,
}

impl WorkbookRelIds {
    /// Ids are allocated for the optional parts whether or not they are
    /// written. The gaps that leaves are legal — `root_rels` already produces
    /// them — and the alternative is a numbering that shifts depending on
    /// whether a particular workbook happened to need a theme.
    fn mint(workbook: &Workbook) -> Self {
        let mut minter = RelIdMinter::avoiding(
            workbook
                .retained_rels
                .iter()
                .filter(|r| is_workbook(&r.source))
                .map(|r| r.id.as_str()),
        );
        Self {
            sheets: (0..workbook.sheets.len()).map(|_| minter.mint()).collect(),
            styles: minter.mint(),
            strings: minter.mint(),
            theme: minter.mint(),
            person: minter.mint(),
        }
    }
}

/// The ids a worksheet's `.rels` mints for the parts behind a note.
///
/// Minted once per sheet and handed to both writers for the same reason as
/// [`WorkbookRelIds`]: the worksheet's `<legacyDrawing r:id>` names the VML that
/// draws the note markers, so the two parts have to agree. A disagreement here
/// does not fail — it produces a note that is in the file and has no marker to
/// click, which is the shape of bug that survives a release.
///
/// Tables, hyperlinks and new drawings do not appear because their ids are
/// deliberately prefixed (`rIdTbl1`, `rIdHl1`, `rIdDrawing1`) and so cannot
/// collide with a producer's `rId{n}` in the first place.
struct SheetRelIds {
    vml: String,
    comments: String,
    threaded: String,
}

impl SheetRelIds {
    fn mint(retained: &[RetainedRel], sheet_part: &str) -> Self {
        let mut minter = RelIdMinter::avoiding(
            retained
                .iter()
                .filter(|r| r.source == sheet_part)
                .map(|r| r.id.as_str()),
        );
        Self {
            vml: minter.mint(),
            comments: minter.mint(),
            threaded: minter.mint(),
        }
    }
}

/// Whether any sheet holds a thread that needs the 2018 parts.
fn any_threaded(workbook: &Workbook) -> bool {
    workbook
        .sheets
        .iter()
        .any(|s| s.comments.iter().any(|c| c.is_threaded()))
}

/// The `_rels/.rels` part: the workbook, plus everything else the package hangs
/// off its root.
///
/// `docProps/core.xml`, `docProps/app.xml` and `customXml` are attached here and
/// nowhere else, so a root `.rels` holding only the workbook relationship leaves
/// their parts in the zip with nothing pointing at them — which Excel reports as
/// a package needing repair, and which loses the author, the title and the
/// company on the read after that.
/// `docProps/core.xml` written from the model.
///
/// **The reader parsed these and the writer never emitted them.** `docProps` was
/// handled only as a *retained* part — carried through from an imported file
/// byte for byte — so a workbook created here went out with no properties at
/// all, and one whose title was edited here saved the title it arrived with.
/// The dialog that edits them would have been a lie about what was stored
/// (`UX-META-01`).
///
/// Emitted as a **generated** part, which the writer already prefers over a
/// retained one at the same path — the same mechanism `FID-27` used to make an
/// edited chart's shifted series reach the file. So an untouched workbook still
/// round-trips its original bytes, and an edited one saves what it now says.
///
/// `modified` is stamped by the caller rather than read from a clock here: two
/// builds of the same workbook must produce the same bytes, which is the
/// property the whole writer is built on.
fn core_properties_xml(workbook: &Workbook) -> String {
    let p = &workbook.properties;
    let mut s = String::from(DECL);
    s.push_str(
        "<cp:coreProperties \
xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
xmlns:dcterms=\"http://purl.org/dc/terms/\" \
xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">",
    );
    // Only what the model holds. An empty element is not the same as an absent
    // one to every reader, and writing `<dc:title/>` for a workbook that has no
    // title asserts that it has an empty one.
    let put = |s: &mut String, tag: &str, value: &str| {
        if !value.trim().is_empty() {
            s.push_str(&format!("<{tag}>{}</{tag}>", escape_text(value)));
        }
    };
    put(&mut s, "dc:title", &p.title);
    put(&mut s, "dc:subject", &p.subject);
    put(&mut s, "dc:creator", &p.creator);
    if !p.keywords.is_empty() {
        put(&mut s, "cp:keywords", &p.keywords.join(", "));
    }
    put(&mut s, "dc:description", &p.description);
    put(&mut s, "cp:lastModifiedBy", &p.last_modified_by);
    put(&mut s, "dc:language", &p.language);
    // The two dates carry the `xsi:type` the schema requires; without it Excel
    // reads them as text and shows nothing in its own properties pane.
    for (tag, value) in [
        ("dcterms:created", &p.created),
        ("dcterms:modified", &p.modified),
    ] {
        if !value.trim().is_empty() {
            s.push_str(&format!(
                "<{tag} xsi:type=\"dcterms:W3CDTF\">{}</{tag}>",
                escape_text(value)
            ));
        }
    }
    s.push_str("</cp:coreProperties>");
    s
}

/// Whether the model holds any document property at all.
///
/// A workbook with none gets no part, so a file that never had properties does
/// not gain an empty one — a difference in the bytes for no change in meaning.
fn has_core_properties(workbook: &Workbook) -> bool {
    let p = &workbook.properties;
    !(p.title.is_empty()
        && p.subject.is_empty()
        && p.description.is_empty()
        && p.keywords.is_empty()
        && p.creator.is_empty()
        && p.last_modified_by.is_empty()
        && p.created.is_empty()
        && p.modified.is_empty()
        && p.language.is_empty())
}

/// The `docProps/core.xml` to write: the retained one with the model's values
/// put back into it, or a fresh one when the file had none.
///
/// **Not a wholesale replacement**, which is what the first version did and what
/// `parts_attached_at_the_package_root_survive_a_save` caught. A generated part
/// wins over a retained one at the same path, so writing ours unconditionally
/// swapped an imported file's metadata part for a reduction of it — `core.xml`
/// carries producer extensions this model does not hold, and they went every
/// time an untouched workbook was saved.
///
/// So the retained bytes are edited in place: each element the model holds has
/// its text replaced, everything else survives untouched, and a workbook nobody
/// edited produces the bytes it arrived with. That is `FID-27`'s rule and its
/// method — the same surgical treatment a retained chart part gets.
fn core_properties_part(workbook: &Workbook) -> String {
    let retained = workbook
        .retained_parts
        .iter()
        .find(|r| r.path.eq_ignore_ascii_case("docProps/core.xml"))
        .and_then(|r| String::from_utf8(r.bytes.clone()).ok());
    let Some(existing) = retained else {
        return core_properties_xml(workbook);
    };
    let p = &workbook.properties;
    let mut out = existing;
    for (tag, value) in [
        ("dc:title", p.title.clone()),
        ("dc:subject", p.subject.clone()),
        ("dc:description", p.description.clone()),
        ("dc:creator", p.creator.clone()),
        ("cp:lastModifiedBy", p.last_modified_by.clone()),
        ("dc:language", p.language.clone()),
        ("cp:keywords", p.keywords.join(", ")),
    ] {
        out = put_element(&out, tag, &value);
    }
    out
}

/// Replace one element's text, add it before the closing tag if it is absent,
/// or remove it when the model has cleared it.
///
/// Prefix-agnostic on the local name, for the reason `FID-31` records: matching
/// `title` as a prefix would also match a longer name, and OOXML orders these
/// however the producer liked.
fn put_element(xml: &str, qname: &str, value: &str) -> String {
    let local = qname.rsplit(':').next().unwrap_or(qname);
    let open = format!("<{qname}>");
    let close = format!("</{qname}>");
    if let (Some(a), Some(b)) = (xml.find(&open), xml.find(&close))
        && a < b
    {
        let (head, tail) = (&xml[..a], &xml[b + close.len()..]);
        if value.trim().is_empty() {
            return format!("{head}{tail}");
        }
        return format!("{head}{open}{}{close}{tail}", escape_text(value));
    }
    if value.trim().is_empty() {
        return xml.to_owned();
    }
    // Absent: insert before the root's closing tag, which is the only place a
    // child of `coreProperties` can legally go.
    let _ = local;
    match xml.rfind("</cp:coreProperties>") {
        Some(at) => format!(
            "{}{open}{}{close}{}",
            &xml[..at],
            escape_text(value),
            &xml[at..]
        ),
        None => xml.to_owned(),
    }
}

fn root_rels(workbook: &Workbook) -> String {
    let retained = workbook_rels_for(&workbook.retained_rels, "");
    // Nothing numbers the root relationships, so a producer is free to have
    // given `rId1` to `docProps/core.xml`. Two `Id="rId1"` in one `.rels` is a
    // package Excel repairs — `Id` is an xsd:ID — and the retained one is the id
    // that must not move, because it came from the file. The workbook's own is
    // named by nothing but its type, so it is the one that steps aside.
    let workbook_id = RelIdMinter::avoiding(retained.iter().map(|r| r.id.as_str())).mint();
    let mut s = format!(
        "{DECL}<Relationships xmlns=\"{NS_REL}\"><Relationship Id=\"{workbook_id}\" Type=\"{NS_R}/officeDocument\" Target=\"xl/workbook.xml\"/>"
    );
    // Ids travel verbatim for the same reason they do everywhere else: the
    // relationship is re-emitted, not re-minted, so anything naming it still
    // names the same part.
    for rel in &retained {
        s.push_str(&retained_rel_xml(rel));
    }
    // Our own `docProps/core.xml`, when we are writing one and the file did not
    // arrive with a relationship for it already. Two relationships to one target
    // is a package Excel repairs.
    if has_core_properties(workbook) && !retained.iter().any(|r| r.target.contains("core.xml")) {
        let id = RelIdMinter::avoiding(
            retained
                .iter()
                .map(|r| r.id.as_str())
                .chain(std::iter::once(workbook_id.as_str())),
        )
        .mint();
        s.push_str(&format!(
            "<Relationship Id=\"{id}\" Type=\"{NS_R}/metadata/core-properties\" Target=\"docProps/core.xml\"/>"
        ));
    }
    s.push_str("</Relationships>");
    s
}

/// `<pivotCaches>`: the retained entries and the authored ones, in one wrapper.
///
/// Both kinds have to appear inside a *single* element — a second
/// `<pivotCaches>` is a schema violation and Excel refuses the package rather
/// than ignoring the stray one, which is the same trap `write_retained_refs`
/// exists to avoid for external references.
fn write_pivot_caches(s: &mut String, workbook: &Workbook, pivots: &[pivot::SheetPivots]) {
    let authored: Vec<&(String, String, u32)> =
        pivots.iter().flat_map(|p| p.caches.iter()).collect();
    if authored.is_empty() {
        write_retained_refs(s, workbook, "pivotCache", "pivotCaches");
        return;
    }
    s.push_str("<pivotCaches>");
    write_retained_refs_inner(s, workbook, "pivotCache");
    for (id, _, cache_id) in authored {
        s.push_str(&format!(
            "<pivotCache cacheId=\"{cache_id}\" r:id=\"{id}\"/>"
        ));
    }
    s.push_str("</pivotCaches>");
}

fn workbook_xml(
    workbook: &Workbook,
    ids: &WorkbookRelIds,
    pivots: &[pivot::SheetPivots],
) -> String {
    let mut s = format!("{DECL}<workbook xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_R}\">");
    // CT_Workbook's sequence: fileVersion, fileSharing, workbookPr, bookViews,
    // sheets, … calcPr. The settings travel verbatim; only `date1904` is
    // interpreted, and it is merged back in here so the two cannot disagree —
    // written even when the map already holds it, since a workbook built from
    // scratch has no map at all but may still be on the 1904 epoch.
    let settings = &workbook.settings;
    write_attr_element(&mut s, "fileVersion", &settings.file_version);
    write_attr_element(&mut s, "fileSharing", &settings.file_sharing);
    let mut workbook_pr = settings.workbook_pr.clone();
    if workbook.date1904 {
        workbook_pr.insert("date1904".to_owned(), "1".to_owned());
    } else {
        // 1900 is the default, so the attribute is written by omission — but a
        // stale `date1904="1"` left in the map would shift every serial by 1462
        // days.
        workbook_pr.remove("date1904");
    }
    write_attr_element(&mut s, "workbookPr", &workbook_pr);
    write_attr_element(&mut s, "workbookProtection", &settings.protection);
    if !settings.views.is_empty() {
        s.push_str("<bookViews>");
        for view in &settings.views {
            write_attr_element(&mut s, "workbookView", view);
        }
        s.push_str("</bookViews>");
    }
    s.push_str("<sheets>");
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        // Visible is the schema default, so it is written by omission.
        let state = sheet
            .visibility
            .ooxml()
            .map(|v| format!(" state=\"{v}\""))
            .unwrap_or_default();
        s.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{}\"{state} r:id=\"{}\"/>",
            escape_attr(&sheet.name),
            i + 1,
            ids.sheets[i]
        ));
    }
    s.push_str("</sheets>");
    // `<externalReferences>` follows `<sheets>` in CT_Workbook's sequence;
    // `<pivotCaches>` comes after `<calcPr>`, so the two kinds of retained
    // reference are written in different places and each needs its own
    // synthesized wrapper. Writing a `<pivotCache>` inside `<externalReferences>`
    // is not a stylistic slip — the sequence is validated, and Excel refuses
    // the package rather than ignoring the stray child.
    write_retained_refs(&mut s, workbook, "externalReference", "externalReferences");
    if !workbook.defined_names.is_empty() {
        s.push_str("<definedNames>");
        for name in &workbook.defined_names {
            let scope = name
                .sheet
                .and_then(|id| sheet_index(workbook, id))
                .map(|i| format!(" localSheetId=\"{i}\""))
                .unwrap_or_default();
            s.push_str(&format!(
                "<definedName name=\"{}\"{scope}>{}</definedName>",
                escape_attr(&name.name),
                escape_text(&formula_text(&name.formula, ABSOLUTE))
            ));
        }
        s.push_str("</definedNames>");
    }
    write_attr_element(&mut s, "calcPr", &workbook.settings.calc);
    write_pivot_caches(&mut s, workbook, pivots);
    s.push_str("</workbook>");
    s
}

/// Write the retained references of one element name inside their wrapper,
/// which is synthesized because it carries nothing itself and so was never
/// read.
fn write_retained_refs(s: &mut String, workbook: &Workbook, element: &str, wrapper: &str) {
    let refs: Vec<_> = workbook
        .retained_refs
        .iter()
        .filter(|(name, _)| name == element)
        .collect();
    if refs.is_empty() {
        return;
    }
    s.push_str(&format!("<{wrapper}>"));
    write_retained_refs_inner(s, workbook, element);
    s.push_str(&format!("</{wrapper}>"));
}

/// The retained entries alone, with no wrapper — so a caller that has authored
/// entries of its own can put both inside one element.
fn write_retained_refs_inner(s: &mut String, workbook: &Workbook, element: &str) {
    let refs: Vec<_> = workbook
        .retained_refs
        .iter()
        .filter(|(name, _)| name == element)
        .collect();
    for (name, attrs) in refs {
        s.push_str(&format!("<{name}"));
        for (k, v) in attrs {
            // `r:id` collapses to the local name `id` on the way in, since the
            // reader strips namespace prefixes; it has to be restored or the
            // reference names an attribute nothing defines.
            let key = if k == "id" { "r:id" } else { k.as_str() };
            s.push_str(&format!(" {key}=\"{}\"", escape_attr(v)));
        }
        s.push_str("/>");
    }
}

fn workbook_rels(
    workbook: &Workbook,
    ids: &WorkbookRelIds,
    has_styles: bool,
    has_strings: bool,
    has_theme: bool,
    pivots: &[pivot::SheetPivots],
) -> String {
    let mut s = format!("{DECL}<Relationships xmlns=\"{NS_REL}\">");
    // Every cache `<pivotCaches>` names has to resolve here, or the workbook
    // points at a part the package cannot find and Excel calls the file
    // unreadable rather than dropping the pivot.
    for (id, target, _) in pivots.iter().flat_map(|p| p.caches.iter()) {
        s.push_str(&format!(
            "<Relationship Id=\"{id}\" Type=\"{NS_R}/pivotCacheDefinition\" Target=\"{target}\"/>"
        ));
    }
    for (i, id) in ids.sheets.iter().enumerate() {
        s.push_str(&format!(
            "<Relationship Id=\"{id}\" Type=\"{NS_R}/worksheet\" Target=\"worksheets/sheet{}.xml\"/>",
            i + 1
        ));
    }
    if has_styles {
        s.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"{NS_R}/styles\" Target=\"styles.xml\"/>",
            ids.styles
        ));
    }
    if has_strings {
        s.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"{NS_R}/sharedStrings\" Target=\"sharedStrings.xml\"/>",
            ids.strings
        ));
    }
    if has_theme {
        s.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"{NS_R}/theme\" Target=\"theme/theme1.xml\"/>",
            ids.theme
        ));
    }
    if any_threaded(workbook) {
        s.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"{NS_R_PERSON}\" Target=\"persons/person1.xml\"/>",
            ids.person
        ));
    }
    // Retained relationships keep their original ids: the element that names
    // one — `<externalReference r:id="rId4"/>` — travels verbatim too, and a
    // re-minted id would point at nothing.
    //
    // Only the ones the workbook part itself declared. A target is relative to
    // the part that declares it, so a root relationship copied in here would
    // resolve `docProps/core.xml` against `xl/` and reach nothing, and a sheet's
    // would be written twice — once correctly, once dangling.
    for rel in workbook
        .retained_rels
        .iter()
        .filter(|r| is_workbook(&r.source))
    {
        s.push_str(&retained_rel_xml(rel));
    }
    s.push_str("</Relationships>");
    s
}

fn shared_strings_xml(workbook: &Workbook, strings: &SharedStrings) -> String {
    let count = strings.order.len();
    let mut s =
        format!("{DECL}<sst xmlns=\"{NS_MAIN}\" count=\"{count}\" uniqueCount=\"{count}\">");
    for &index in &strings.order {
        let Some(id) = workbook.strings.id_at(index as usize) else {
            continue;
        };
        let text = workbook.strings.get(id).unwrap_or_default();
        match workbook.strings.runs(id) {
            // Rich text: one `<r>` per run, each carrying its own `<rPr>`.
            // Writing the flattened `<t>` instead is what loses the formatting.
            Some(runs) => {
                s.push_str("<si>");
                for run in runs {
                    s.push_str("<r>");
                    if let Some(font) = &run.font {
                        s.push_str(&run_font_xml(font));
                    }
                    s.push_str(&format!(
                        "<t xml:space=\"preserve\">{}</t>",
                        escape_text(&run.text)
                    ));
                    s.push_str("</r>");
                }
                s.push_str("</si>");
            }
            None => s.push_str(&format!(
                "<si><t xml:space=\"preserve\">{}</t></si>",
                escape_text(text)
            )),
        }
    }
    s.push_str("</sst>");
    s
}

/// One `<rPr>`. Children are written in the order the schema declares them,
/// which is what Excel emits — though `CT_RPrElt` is an `xsd:choice`, so the
/// order is conventional rather than required. Matching it keeps our output
/// diffable against a file Excel wrote.
fn run_font_xml(font: &RunFont) -> String {
    let mut s = String::from("<rPr>");
    if font.bold {
        s.push_str("<b/>");
    }
    if font.italic {
        s.push_str("<i/>");
    }
    if font.strike {
        s.push_str("<strike/>");
    }
    for (on, tag) in [
        (font.outline, "outline"),
        (font.shadow, "shadow"),
        (font.condense, "condense"),
        (font.extend, "extend"),
    ] {
        if on {
            s.push_str(&format!("<{tag}/>"));
        }
    }
    if let Some(u) = font.underline {
        s.push_str(&format!("<u val=\"{}\"/>", u.ooxml()));
    }
    if let Some(v) = font.vert_align {
        s.push_str(&format!("<vertAlign val=\"{}\"/>", v.ooxml()));
    }
    if let Some(sz) = font.size_hp {
        s.push_str(&format!("<sz val=\"{}\"/>", fmt_half_points(sz)));
    }
    if let Some(color) = &font.color {
        s.push_str(&color_element("color", color, font.color_theme.as_ref()));
    }
    if let Some(name) = &font.name {
        s.push_str(&format!("<rFont val=\"{}\"/>", escape_attr(name)));
    }
    if let Some(family) = font.family {
        s.push_str(&format!("<family val=\"{family}\"/>"));
    }
    if let Some(charset) = font.charset {
        s.push_str(&format!("<charset val=\"{charset}\"/>"));
    }
    if let Some(scheme) = &font.scheme {
        s.push_str(&format!("<scheme val=\"{}\"/>", escape_attr(scheme)));
    }
    s.push_str("</rPr>");
    s
}

/// The deduplicated conditional-format fill colors across the workbook — each
/// becomes one `<dxf>` (differential format), indexed by position (the dxfId).
/// One `<dxf>`: the whole differential format a rule paints with.
///
/// Keyed on all three parts, not on the fill. Keying on the fill alone meant two
/// rules sharing a colour shared one `<dxf>`, so the second silently took the
/// first's text colour — and a rule with no fill matched nothing and fell to
/// `unwrap_or(0)`, which is some other rule's format entirely. Excel's "Red
/// Text" preset has no fill, so that was not an edge case.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Dxf {
    fill: String,
    font_color: Option<String>,
    bold: bool,
}

fn collect_dxfs(workbook: &Workbook) -> Vec<Dxf> {
    let mut out: Vec<Dxf> = Vec::new();
    for sheet in &workbook.sheets {
        for cf in &sheet.conditional_formats {
            let dxf = Dxf {
                fill: cf.fill.clone(),
                font_color: cf.font_color.clone(),
                bold: cf.bold,
            };
            if !out.contains(&dxf) {
                out.push(dxf);
            }
        }
    }
    out
}

/// Write one `<xf>`. Shared by `cellStyleXfs` (which carries no `xfId`) and
/// `cellXfs` (which points at the named style it belongs to), so the two can
/// never disagree about how a format is spelled.
fn write_xf(s: &mut String, ids: &StyleIds, xf_id: Option<usize>) {
    let flag = |on: bool, attr: &'static str| if on { attr } else { "" };
    let has_align = ids.align.is_some()
        || ids.valign.is_some()
        || ids.wrap
        || ids.indent != 0
        || ids.rotation != 0
        || ids.shrink_to_fit
        || ids.justify_last_line
        || ids.reading_order.is_some()
        || ids.relative_indent.is_some();
    let xf_attr = xf_id
        .map(|id| format!(" xfId=\"{id}\""))
        .unwrap_or_default();
    s.push_str(&format!(
        "<xf numFmtId=\"{}\" fontId=\"{}\" fillId=\"{}\" borderId=\"{}\"{xf_attr}{}{}{}{}{}",
        ids.num_fmt_id,
        ids.font_id,
        ids.fill_id,
        ids.border_id,
        flag(ids.num_fmt_id != 0, " applyNumberFormat=\"1\""),
        flag(ids.font_id != 0, " applyFont=\"1\""),
        flag(ids.fill_id != 0, " applyFill=\"1\""),
        flag(ids.border_id != 0, " applyBorder=\"1\""),
        flag(has_align, " applyAlignment=\"1\""),
    ));
    if ids.quote_prefix {
        s.push_str(" quotePrefix=\"1\"");
    }
    let has_protection = ids.locked.is_some() || ids.formula_hidden.is_some();
    if has_protection {
        // Excel honours a <protection> child only when applyProtection says to.
        // Writing the child without the flag stores the setting and ignores it,
        // which is indistinguishable from losing it.
        s.push_str(" applyProtection=\"1\"");
    }
    if !has_align && !has_protection {
        s.push_str("/>");
        return;
    }
    s.push('>');
    if has_protection {
        s.push_str("<protection");
        if let Some(locked) = ids.locked {
            s.push_str(&format!(" locked=\"{}\"", u8::from(locked)));
        }
        if let Some(hidden) = ids.formula_hidden {
            s.push_str(&format!(" hidden=\"{}\"", u8::from(hidden)));
        }
        s.push_str("/>");
    }
    if !has_align {
        s.push_str("</xf>");
        return;
    }
    s.push_str("<alignment");
    if let Some(align) = ids.align {
        s.push_str(&format!(" horizontal=\"{}\"", align.ooxml()));
    }
    if let Some(valign) = ids.valign {
        s.push_str(&format!(" vertical=\"{}\"", valign.ooxml()));
    }
    if ids.wrap {
        s.push_str(" wrapText=\"1\"");
    }
    if ids.indent != 0 {
        s.push_str(&format!(" indent=\"{}\"", ids.indent));
    }
    if ids.rotation != 0 {
        s.push_str(&format!(" textRotation=\"{}\"", ids.rotation));
    }
    if ids.shrink_to_fit {
        s.push_str(" shrinkToFit=\"1\"");
    }
    if ids.justify_last_line {
        s.push_str(" justifyLastLine=\"1\"");
    }
    if let Some(order) = ids.reading_order {
        s.push_str(&format!(" readingOrder=\"{order}\""));
    }
    if let Some(indent) = ids.relative_indent {
        s.push_str(&format!(" relativeIndent=\"{indent}\""));
    }
    s.push_str("/></xf>");
}

fn styles_xml(workbook: &Workbook, dxfs: &[Dxf]) -> String {
    let styles: Vec<_> = workbook.styles.iter().collect();

    // Deduplicate fonts, solid fills, and custom number formats, and record the
    // (fontId, fillId, numFmtId) each interned style resolves to. Fill ids 0 and
    // 1 are reserved (none / gray125); font id 0 is the default font.
    // Font key: (bold, italic, underline, strike, color, name, size_hp).
    let mut fonts: Vec<FontKey> = vec![FontKey::default()];
    let mut fills: Vec<FillKey> = Vec::new();
    let mut num_codes: Vec<String> = Vec::new();
    // Border id 0 is reserved for the empty border; interned borders start at 1.
    let mut borders: Vec<Borders> = Vec::new();
    let mut per_style: Vec<StyleIds> = Vec::with_capacity(styles.len());

    let mut intern = |style: &Style| {
        let font_key = FontKey {
            bold: style.bold,
            italic: style.italic,
            underline: style.underline,
            strike: style.strike,
            vert_align: style.vert_align,
            color: style.font_color.clone(),
            color_theme: style.font_theme,
            name: style.font_name.clone(),
            size_hp: style.font_size_hp,
            family: style.font_family,
            scheme: style.font_scheme.clone(),
            charset: style.font_charset,
            outline: style.font_outline,
            shadow: style.font_shadow,
            condense: style.font_condense,
            extend: style.font_extend,
        };
        let font_id = fonts
            .iter()
            .position(|f| f == &font_key)
            .unwrap_or_else(|| {
                fonts.push(font_key.clone());
                fonts.len() - 1
            });
        let fill_id = if style.fill_color.is_some() || style.fill_gradient.is_some() {
            let key: FillKey = (
                style.fill_color.clone(),
                style.fill_theme,
                style.fill_pattern.clone(),
                style.fill_bg_color.clone(),
                style.fill_bg_theme,
                style.fill_gradient.clone(),
            );
            2 + fills.iter().position(|f| *f == key).unwrap_or_else(|| {
                fills.push(key.clone());
                fills.len() - 1
            })
        } else {
            0
        };
        let num_fmt_id = match &style.number_format {
            Some(code) => {
                let idx = num_codes.iter().position(|c| c == code).unwrap_or_else(|| {
                    num_codes.push(code.clone());
                    num_codes.len() - 1
                });
                FIRST_CUSTOM_NUM_FMT + idx as u32
            }
            None => 0,
        };
        let border_id = match &style.border {
            Some(b) if !b.is_empty() => {
                1 + borders.iter().position(|x| x == b).unwrap_or_else(|| {
                    borders.push(b.clone());
                    borders.len() - 1
                })
            }
            _ => 0,
        };
        StyleIds {
            font_id,
            fill_id,
            num_fmt_id,
            border_id,
            align: style.align,
            valign: style.valign,
            wrap: style.wrap,
            indent: style.indent,
            rotation: style.rotation,
            shrink_to_fit: style.shrink_to_fit,
            justify_last_line: style.justify_last_line,
            reading_order: style.reading_order,
            relative_indent: style.relative_indent,
            locked: style.locked,
            formula_hidden: style.formula_hidden,
            quote_prefix: style.quote_prefix,
        }
    };
    for style in &styles {
        per_style.push(intern(style));
    }
    // Named styles resolve through the same tables, so a font only a named style
    // uses still lands in `<fonts>`.
    //
    // OOXML requires `cellStyleXfs[0]` to be the Normal style — it is what every
    // unlinked cell's `xfId="0"` points at. Our own ordering is whatever the
    // source file's `<cellStyles>` happened to be, so emit Normal first and remap
    // the links rather than assuming it was already there.
    let normal = workbook
        .cell_styles
        .iter()
        .position(|cs| cs.builtin_id == Some(0) || cs.name.eq_ignore_ascii_case("Normal"));
    let mut order: Vec<usize> = normal.into_iter().collect();
    order.extend((0..workbook.cell_styles.len()).filter(|i| Some(*i) != normal));
    // `slot[i]` is where cell_styles[i] ends up in cellStyleXfs.
    let mut slot = vec![0usize; workbook.cell_styles.len()];
    for (pos, &i) in order.iter().enumerate() {
        slot[i] = pos;
    }
    let per_named: Vec<StyleIds> = order
        .iter()
        .map(|&i| intern(&workbook.cell_styles[i].style))
        .collect();

    let mut s = format!("{DECL}<styleSheet xmlns=\"{NS_MAIN}\">");
    if !num_codes.is_empty() {
        s.push_str(&format!("<numFmts count=\"{}\">", num_codes.len()));
        for (i, code) in num_codes.iter().enumerate() {
            s.push_str(&format!(
                "<numFmt numFmtId=\"{}\" formatCode=\"{}\"/>",
                FIRST_CUSTOM_NUM_FMT + i as u32,
                escape_attr(code)
            ));
        }
        s.push_str("</numFmts>");
    }

    s.push_str(&format!("<fonts count=\"{}\">", fonts.len()));
    for font in &fonts {
        s.push_str("<font>");
        if font.bold {
            s.push_str("<b/>");
        }
        if font.italic {
            s.push_str("<i/>");
        }
        if let Some(u) = font.underline {
            s.push_str(&format!("<u val=\"{}\"/>", u.ooxml()));
        }
        if font.strike {
            s.push_str("<strike/>");
        }
        for (on, tag) in [
            (font.outline, "outline"),
            (font.shadow, "shadow"),
            (font.condense, "condense"),
            (font.extend, "extend"),
        ] {
            if on {
                s.push_str(&format!("<{tag}/>"));
            }
        }
        if let Some(v) = font.vert_align {
            s.push_str(&format!("<vertAlign val=\"{}\"/>", v.ooxml()));
        }
        if let Some(f) = font.family {
            s.push_str(&format!("<family val=\"{f}\"/>"));
        }
        if let Some(c) = font.charset {
            s.push_str(&format!("<charset val=\"{c}\"/>"));
        }
        if let Some(sc) = &font.scheme {
            s.push_str(&format!("<scheme val=\"{}\"/>", escape_attr(sc)));
        }
        if let Some(c) = &font.color {
            s.push_str(&color_element("color", c, font.color_theme.as_ref()));
        }
        // Default font is Calibri 11pt (22 half-points) when unset.
        s.push_str(&format!(
            "<sz val=\"{}\"/>",
            fmt_half_points(font.size_hp.unwrap_or(22))
        ));
        s.push_str(&format!(
            "<name val=\"{}\"/>",
            escape_attr(font.name.as_deref().unwrap_or("Calibri"))
        ));
        s.push_str("</font>");
    }
    s.push_str("</fonts>");

    s.push_str(&format!("<fills count=\"{}\">", fills.len() + 2));
    s.push_str("<fill><patternFill patternType=\"none\"/></fill>");
    s.push_str("<fill><patternFill patternType=\"gray125\"/></fill>");
    for (color, theme, pattern, bg, bg_theme, gradient) in &fills {
        s.push_str("<fill>");
        match gradient {
            // A gradient replaces the pattern rather than joining it: `<fill>`
            // holds one or the other, never both.
            Some(g) => s.push_str(&gradient_fill_xml(g)),
            None => {
                s.push_str(&format!(
                    "<patternFill patternType=\"{}\">",
                    escape_attr(pattern.as_deref().unwrap_or("solid"))
                ));
                if let Some(color) = color {
                    s.push_str(&color_element("fgColor", color, theme.as_ref()));
                }
                if let Some(bg) = bg {
                    s.push_str(&color_element("bgColor", bg, bg_theme.as_ref()));
                }
                s.push_str("</patternFill>");
            }
        }
        s.push_str("</fill>");
    }
    s.push_str("</fills>");

    s.push_str(&format!("<borders count=\"{}\">", borders.len() + 1));
    s.push_str("<border><left/><right/><top/><bottom/><diagonal/></border>");
    for border in &borders {
        write_border(&mut s, border);
    }
    s.push_str("</borders>");
    // `<cellStyleXfs>`: the formats the named styles stand for. With no named
    // styles the single default entry is still required — `cellXfs` entries all
    // point at index 0.
    if per_named.is_empty() {
        s.push_str("<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>");
    } else {
        s.push_str(&format!("<cellStyleXfs count=\"{}\">", per_named.len()));
        for ids in &per_named {
            write_xf(&mut s, ids, None);
        }
        s.push_str("</cellStyleXfs>");
    }

    s.push_str(&format!("<cellXfs count=\"{}\">", styles.len() + 1));
    s.push_str("<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>");
    for (i, ids) in per_style.iter().enumerate() {
        // The named style this cell format belongs to, remapped to its emitted
        // slot. Absent or out of range falls back to Normal.
        let xf_id = styles[i]
            .style_ref
            .and_then(|r| slot.get(r as usize).copied())
            .unwrap_or(0);
        write_xf(&mut s, ids, Some(xf_id));
    }
    s.push_str("</cellXfs>");

    // `<cellStyles>` names the cellStyleXfs entries. It sits between cellXfs and
    // dxfs in the CT_Stylesheet sequence.
    if !order.is_empty() {
        s.push_str(&format!("<cellStyles count=\"{}\">", order.len()));
        for (pos, &i) in order.iter().enumerate() {
            let cs = &workbook.cell_styles[i];
            let builtin = cs
                .builtin_id
                .map(|b| format!(" builtinId=\"{b}\""))
                .unwrap_or_default();
            s.push_str(&format!(
                "<cellStyle name=\"{}\" xfId=\"{pos}\"{builtin}/>",
                escape_attr(&cs.name)
            ));
        }
        s.push_str("</cellStyles>");
    }

    // Differential formats (conditional-format fills), after cellXfs.
    if !dxfs.is_empty() {
        s.push_str(&format!("<dxfs count=\"{}\">", dxfs.len()));
        for dxf in dxfs {
            s.push_str("<dxf>");
            // Font first: OOXML's CT_Dxf sequence is font, numFmt, fill, …, and
            // Excel rejects a dxf whose children are out of order.
            if dxf.font_color.is_some() || dxf.bold {
                s.push_str("<font>");
                if dxf.bold {
                    s.push_str("<b/>");
                }
                if let Some(rgb) = &dxf.font_color {
                    s.push_str(&format!("<color rgb=\"FF{rgb}\"/>"));
                }
                s.push_str("</font>");
            }
            // A rule may legitimately have no fill — Excel's "Red Text" preset
            // is exactly that — and writing an empty one paints the cell.
            if !dxf.fill.is_empty() {
                s.push_str(&format!(
                    "<fill><patternFill><bgColor rgb=\"FF{}\"/></patternFill></fill>",
                    dxf.fill
                ));
            }
            s.push_str("</dxf>");
        }
        s.push_str("</dxfs>");
    }
    s.push_str("</styleSheet>");
    s
}

/// A deduplication key for a `<font>`: (bold, italic, underline, strike, color,
/// name, size in half-points).
/// Everything that distinguishes one `<font>` from another.
///
/// A struct rather than a tuple: a font has sixteen distinguishing properties
/// and Rust derives the comparison traits for tuples only up to twelve, so the
/// tuple form stopped compiling the moment the legacy effects were added. The
/// named fields are also what keeps the dedup key and the writer in step —
/// with a tuple, adding a property and forgetting the key silently merges two
/// different fonts.
#[derive(Clone, Default, PartialEq)]
struct FontKey {
    bold: bool,
    italic: bool,
    underline: Option<Underline>,
    strike: bool,
    vert_align: Option<VertAlign>,
    color: Option<String>,
    color_theme: Option<ThemeTint>,
    name: Option<String>,
    size_hp: Option<u32>,
    family: Option<u32>,
    scheme: Option<String>,
    charset: Option<u32>,
    outline: bool,
    shadow: bool,
    condense: bool,
    extend: bool,
}

/// Everything that distinguishes one `<fill>` from another.
///
/// The pattern, the background colour and the gradient are all part of the key:
/// two cells whose foregrounds match but whose patterns differ are different
/// fills, and keying on the foreground alone would give the second one the
/// first one's pattern.
type FillKey = (
    Option<String>,
    Option<ThemeTint>,
    Option<String>,
    Option<String>,
    Option<ThemeTint>,
    Option<GradientFill>,
);

/// Write a colour element, as a theme reference when the colour is linked to
/// one and as a literal `rgb` otherwise. Writing `rgb` for a theme colour is
/// what breaks re-theming: the file then states a fixed colour, and Excel has
/// no reason to move it when the palette changes.
fn color_element(tag: &str, rgb: &str, theme: Option<&ThemeTint>) -> String {
    match theme {
        Some(t) if t.tint_micro == 0 => format!("<{tag} theme=\"{}\"/>", t.slot),
        Some(t) => format!(
            "<{tag} theme=\"{}\" tint=\"{}\"/>",
            t.slot,
            fmt_tint(t.tint())
        ),
        None => format!("<{tag} rgb=\"FF{}\"/>", escape_attr(rgb)),
    }
}

/// The `xl/theme/theme1.xml` part.
///
/// Written whenever any style references a theme slot. Without it a `theme="4"`
/// resolves against whatever palette the reader defaults to, so a re-themed
/// workbook would come back in Office's stock colours rather than its own — and
/// our own importer would re-read different RGB than we wrote, which breaks the
/// round-trip fixed point.
///
/// Only `<a:clrScheme>` is meaningful here; the font and format schemes are the
/// stock Office ones, present because the schema requires them.
fn theme_xml(workbook: &Workbook) -> String {
    let slot = |i: usize| workbook.theme_slot(i);
    // `<a:clrScheme>` lists dk1/lt1/dk2/lt2 before the accents, while a
    // `theme="N"` attribute indexes lt1/dk1/lt2/dk2 — the first two pairs are
    // swapped. Emitting them in slot order would invert black and white text.
    let mut s = format!(
        "{DECL}<a:theme xmlns:a=\"{NS_DRAWING}\" name=\"Office Theme\"><a:themeElements><a:clrScheme name=\"Office\">"
    );
    for (element, index) in [
        ("dk1", 1),
        ("lt1", 0),
        ("dk2", 3),
        ("lt2", 2),
        ("accent1", 4),
        ("accent2", 5),
        ("accent3", 6),
        ("accent4", 7),
        ("accent5", 8),
        ("accent6", 9),
        ("hlink", 10),
        ("folHlink", 11),
    ] {
        s.push_str(&format!(
            "<a:{element}><a:srgbClr val=\"{}\"/></a:{element}>",
            escape_attr(slot(index))
        ));
    }
    s.push_str("</a:clrScheme>");
    s.push_str(
        "<a:fontScheme name=\"Office\">\
<a:majorFont><a:latin typeface=\"Calibri Light\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont>\
<a:minorFont><a:latin typeface=\"Calibri\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont>\
</a:fontScheme>\
<a:fmtScheme name=\"Office\">\
<a:fillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:fillStyleLst>\
<a:lnStyleLst><a:ln><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln><a:ln><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln><a:ln><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln></a:lnStyleLst>\
<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>\
<a:bgFillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:bgFillStyleLst>\
</a:fmtScheme>",
    );
    s.push_str("</a:themeElements></a:theme>");
    s
}

/// Whether any style references a theme colour, and so needs the theme part.
fn any_theme_link(workbook: &Workbook) -> bool {
    workbook
        .styles
        .iter()
        .any(|s| s.font_theme.is_some() || s.fill_theme.is_some())
}

/// A self-closing element carrying attributes that travel verbatim. Nothing is
/// written when there are none, so an untouched sheet gains no empty elements.
fn write_attr_element(s: &mut String, name: &str, attrs: &BTreeMap<String, String>) {
    if attrs.is_empty() {
        return;
    }
    s.push_str(&format!("<{name}"));
    for (key, value) in attrs {
        s.push_str(&format!(" {key}=\"{}\"", escape_attr(value)));
    }
    s.push_str("/>");
}

/// One `<gradientFill>` with its stops.
fn gradient_fill_xml(g: &GradientFill) -> String {
    let mut s = String::from("<gradientFill");
    if let Some(kind) = &g.kind {
        s.push_str(&format!(" type=\"{}\"", escape_attr(kind)));
    }
    for (name, micro) in [
        ("degree", g.degree_micro),
        ("left", g.left_micro),
        ("right", g.right_micro),
        ("top", g.top_micro),
        ("bottom", g.bottom_micro),
    ] {
        if micro != 0 {
            s.push_str(&format!(" {name}=\"{}\"", fmt_micro(micro)));
        }
    }
    s.push('>');
    for stop in &g.stops {
        s.push_str(&format!(
            "<stop position=\"{}\">{}</stop>",
            fmt_micro(stop.position_micro),
            color_element("color", &stop.color, stop.color_theme.as_ref())
        ));
    }
    s.push_str("</gradientFill>");
    s
}

/// Integer millionths back to the plain decimal OOXML writes.
fn fmt_micro(micro: i32) -> String {
    let mut s = format!("{:.6}", from_micro(micro));
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

/// A tint as OOXML writes it: plain decimal, no exponent, no trailing zeros.
fn fmt_tint(tint: f64) -> String {
    let mut s = format!("{tint:.6}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

/// The resolved OOXML style-collection ids a single interned style maps to.
struct StyleIds {
    font_id: usize,
    fill_id: usize,
    num_fmt_id: u32,
    border_id: usize,
    align: Option<HAlign>,
    valign: Option<VAlign>,
    wrap: bool,
    indent: u8,
    rotation: u16,
    shrink_to_fit: bool,
    justify_last_line: bool,
    reading_order: Option<u8>,
    relative_indent: Option<i16>,
    locked: Option<bool>,
    formula_hidden: Option<bool>,
    quote_prefix: bool,
}

/// The per-column attributes coalesced into one `<col>` span: a custom width
/// (twips), a hidden flag, an outline nesting level, and a collapsed flag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ColAttrs {
    width: Option<i64>,
    hidden: bool,
    outline_level: u8,
    collapsed: bool,
}

fn write_border(s: &mut String, border: &Borders) {
    // The diagonal directions are attributes of `<border>`; the line's style and
    // colour live in the `<diagonal>` child.
    s.push_str("<border");
    if border.diagonal_up {
        s.push_str(" diagonalUp=\"1\"");
    }
    if border.diagonal_down {
        s.push_str(" diagonalDown=\"1\"");
    }
    s.push('>');
    write_border_edge(s, "left", &border.left);
    write_border_edge(s, "right", &border.right);
    write_border_edge(s, "top", &border.top);
    write_border_edge(s, "bottom", &border.bottom);
    write_border_edge(s, "diagonal", &border.diagonal);
    // CT_Border's sequence puts the inside rules after the diagonal.
    write_border_edge(s, "vertical", &border.inside_vertical);
    write_border_edge(s, "horizontal", &border.inside_horizontal);
    s.push_str("</border>");
}

fn write_border_edge(s: &mut String, name: &str, edge: &Option<BorderEdge>) {
    match edge {
        Some(edge) => {
            s.push_str(&format!("<{name} style=\"{}\">", escape_attr(&edge.style)));
            if let Some(color) = &edge.color {
                s.push_str(&format!("<color rgb=\"FF{color}\"/>"));
            }
            s.push_str(&format!("</{name}>"));
        }
        None => s.push_str(&format!("<{name}/>")),
    }
}

fn cell_a1(row: u32, col: u32) -> String {
    format!("{}{}", column_to_letters(col), row + 1)
}

pub(crate) fn range_a1(range: &CellRange) -> String {
    format!(
        "{}:{}",
        cell_a1(range.start.row, range.start.col),
        cell_a1(range.end.row, range.end.col)
    )
}

/// Reverse of the importer's column-width conversion: twips → Excel character
/// width. Chosen so `read(write(x)) == x` for import-derived widths.
fn twips_to_col_chars(twips: i64) -> f64 {
    ((twips as f64 / 15.0) - 5.0) / 7.0
}

/// Reverse of the importer's row-height conversion: twips → points.
fn twips_to_row_points(twips: i64) -> f64 {
    twips as f64 / 20.0
}

/// Format a float for an XML attribute using the shortest round-trippable form.
fn fmt_f64(value: f64) -> String {
    format!("{value}")
}

/// Render a half-point font size as OOXML points: an integral number of points
/// prints with no fraction (`22` → `11`), a half-point keeps `.5` (`23` → `11.5`).
fn fmt_half_points(size_hp: u32) -> String {
    if size_hp.is_multiple_of(2) {
        format!("{}", size_hp / 2)
    } else {
        format!("{}.5", size_hp / 2)
    }
}

fn worksheet_xml(
    workbook: &Workbook,
    sheet_index: usize,
    dxfs: &[Dxf],
    charts: &chart::SheetCharts,
    ids: &SheetRelIds,
    strings: &SharedStrings,
) -> String {
    let sheet = &workbook.sheets[sheet_index];
    let mut s = format!("{DECL}<worksheet xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_R}\">");

    // `<sheetPr>` is first in the CT_Worksheet sequence, and within it the schema
    // order is `tabColor`, `outlinePr`, then `pageSetUpPr`. Excel stores the tab color as 8-hex
    // ARGB; the model keeps `RRGGBB`, so we prepend an opaque `FF` alpha on the
    // way out. `<outlinePr>` is emitted only for non-default summary positions.
    // Some of these are children of a wrapper element that carries nothing
    // itself; writing the child without it is invalid, so the wrapper is
    // synthesized here rather than carried.
    let carried = |s: &mut String, name: &str, wrapper: Option<&str>| {
        let items: Vec<_> = sheet.carried.iter().filter(|(n, _)| n == name).collect();
        if items.is_empty() {
            return;
        }
        if let Some(wrapper) = wrapper {
            s.push_str(&format!("<{wrapper}>"));
        }
        for (element, attrs) in items {
            write_attr_element(s, element, attrs);
        }
        if let Some(wrapper) = wrapper {
            s.push_str(&format!("</{wrapper}>"));
        }
    };
    let has_outline_pr = !sheet.outline.is_default();
    let has_setup_pr = !sheet.print.setup_pr.is_empty();
    if sheet.tab_color.is_some() || has_outline_pr || has_setup_pr {
        s.push_str("<sheetPr>");
        if let Some(rgb) = &sheet.tab_color {
            s.push_str(&format!(
                "<tabColor rgb=\"FF{}\"/>",
                rgb.to_ascii_uppercase()
            ));
        }
        if has_outline_pr {
            s.push_str("<outlinePr");
            if !sheet.outline.summary_below {
                s.push_str(" summaryBelow=\"0\"");
            }
            if !sheet.outline.summary_right {
                s.push_str(" summaryRight=\"0\"");
            }
            s.push_str("/>");
        }
        write_attr_element(&mut s, "pageSetUpPr", &sheet.print.setup_pr);
        s.push_str("</sheetPr>");
    }

    // CT_Worksheet's sequence is sheetPr, dimension, sheetViews, … — dimension
    // precedes the views rather than sitting just before sheetData.
    carried(&mut s, "dimension", None);

    // `<sheetView>` carries the zoom scale (an attribute), the frozen `<pane>`
    // and the carried `<selection>`; any one alone is enough to emit it. A
    // default view with only a selection still needs the element, or the
    // selection has nowhere to go and is silently dropped.
    let has_selection = sheet.carried.iter().any(|(n, _)| n == "selection");
    if !sheet.view.is_default() || has_selection {
        let zoom_attr = if sheet.view.zoom != 0 {
            format!(" zoomScale=\"{}\"", sheet.view.zoom)
        } else {
            String::new()
        };
        // Grid lines and headers show by default; only emit an attribute to
        // hide them, so a normal sheet writes neither.
        let grid_attr = if sheet.view.hide_gridlines {
            " showGridLines=\"0\""
        } else {
            ""
        };
        let headers_attr = if sheet.view.hide_headers {
            " showRowColHeaders=\"0\""
        } else {
            ""
        };
        let flags = [
            (sheet.view.right_to_left, " rightToLeft=\"1\""),
            (sheet.view.show_formulas, " showFormulas=\"1\""),
            // showZeros defaults to true, so hiding them is written as "0".
            (sheet.view.hide_zeros, " showZeros=\"0\""),
            (sheet.view.tab_selected, " tabSelected=\"1\""),
        ]
        .into_iter()
        .filter_map(|(on, attr)| on.then_some(attr))
        .collect::<String>();
        s.push_str(&format!(
            "<sheetViews><sheetView{grid_attr}{headers_attr}{flags}{zoom_attr} workbookViewId=\"0\">"
        ));
        if sheet.view.frozen_rows != 0 || sheet.view.frozen_cols != 0 {
            let top_left = cell_a1(sheet.view.frozen_rows, sheet.view.frozen_cols);
            s.push_str(&format!(
                "<pane xSplit=\"{}\" ySplit=\"{}\" topLeftCell=\"{}\" state=\"frozen\" activePane=\"bottomRight\"/>",
                sheet.view.frozen_cols, sheet.view.frozen_rows, top_left
            ));
        }
        // `<selection>` follows `<pane>` inside `<sheetView>`; it records the
        // cursor position, which is worth keeping so reopening a file lands
        // where the author left it.
        for (element, attrs) in sheet.carried.iter().filter(|(n, _)| n == "selection") {
            write_attr_element(&mut s, element, attrs);
        }
        s.push_str("</sheetView></sheetViews>");
    }

    // Axis defaults, then per-column overrides (schema order: before sheetData).
    if sheet.columns.default.is_some()
        || sheet.rows.default.is_some()
        || !sheet.format_pr.is_empty()
    {
        s.push_str("<sheetFormatPr");
        if let Some(w) = sheet.columns.default {
            s.push_str(&format!(
                " defaultColWidth=\"{}\"",
                fmt_f64(twips_to_col_chars(w))
            ));
        }
        if let Some(h) = sheet.rows.default {
            s.push_str(&format!(
                " defaultRowHeight=\"{}\"",
                fmt_f64(twips_to_row_points(h))
            ));
        }
        // The rest travel verbatim; the two defaults above were interpreted out
        // of this map on import so they cannot be written twice.
        for (k, v) in &sheet.format_pr {
            s.push_str(&format!(" {k}=\"{}\"", escape_attr(v)));
        }
        s.push_str("/>");
    }
    if !sheet.columns.sizes.is_empty()
        || !sheet.hidden_cols.is_empty()
        || !sheet.col_outline_levels.is_empty()
        || !sheet.collapsed_cols.is_empty()
    {
        // Union the width overrides, hidden flags, outline levels, and collapsed
        // flags, keyed by zero-based column, so a column can carry any mix.
        let mut columns: BTreeMap<u32, ColAttrs> = BTreeMap::new();
        for (&col, &width) in &sheet.columns.sizes {
            columns.entry(col).or_default().width = Some(width);
        }
        for &col in &sheet.hidden_cols {
            columns.entry(col).or_default().hidden = true;
        }
        for (&col, &level) in &sheet.col_outline_levels {
            columns.entry(col).or_default().outline_level = level;
        }
        for &col in &sheet.collapsed_cols {
            columns.entry(col).or_default().collapsed = true;
        }
        s.push_str("<cols>");
        let entries: Vec<(u32, ColAttrs)> = columns.iter().map(|(&k, &v)| (k, v)).collect();
        let mut i = 0;
        while i < entries.len() {
            let (start, attrs) = entries[i];
            let mut end = start;
            let mut j = i + 1;
            // Coalesce a run of consecutive columns with identical attributes.
            while j < entries.len() && entries[j].0 == end + 1 && entries[j].1 == attrs {
                end = entries[j].0;
                j += 1;
            }
            let width_attr = attrs
                .width
                .map(|w| {
                    format!(
                        " width=\"{}\" customWidth=\"1\"",
                        fmt_f64(twips_to_col_chars(w))
                    )
                })
                .unwrap_or_default();
            let hidden_attr = if attrs.hidden { " hidden=\"1\"" } else { "" };
            let outline_attr = if attrs.outline_level != 0 {
                format!(" outlineLevel=\"{}\"", attrs.outline_level)
            } else {
                String::new()
            };
            let collapsed_attr = if attrs.collapsed {
                " collapsed=\"1\""
            } else {
                ""
            };
            s.push_str(&format!(
                "<col min=\"{}\" max=\"{}\"{width_attr}{hidden_attr}{outline_attr}{collapsed_attr}/>",
                start + 1,
                end + 1,
            ));
            i = j;
        }
        s.push_str("</cols>");
    }

    s.push_str("<sheetData>");
    // The rows to emit: every row with cells, plus any row carrying a custom
    // height, hidden flag, outline level, or collapsed flag (even if it has no
    // cells). Cells iterate in row-major order.
    let mut rows: BTreeSet<u32> = sheet.rows.sizes.keys().copied().collect();
    rows.extend(sheet.hidden_rows.iter().copied());
    rows.extend(sheet.filter_hidden.iter().copied());
    rows.extend(sheet.row_outline_levels.keys().copied());
    rows.extend(sheet.collapsed_rows.iter().copied());
    for (at, _) in sheet.cells.iter() {
        rows.insert(at.row);
    }
    let mut cells = sheet.cells.iter().peekable();
    for row in rows {
        let ht_attr = sheet
            .rows
            .sizes
            .get(&row)
            .map(|&t| {
                format!(
                    " ht=\"{}\" customHeight=\"1\"",
                    fmt_f64(twips_to_row_points(t))
                )
            })
            .unwrap_or_default();
        // Filtered-out rows carry hidden="1" exactly like hand-hidden ones —
        // OOXML has no separate marker, which is why the two sets are kept
        // apart in the model rather than here.
        let hidden_attr = if sheet.is_row_hidden(row) {
            " hidden=\"1\""
        } else {
            ""
        };
        let outline_attr = sheet
            .row_outline_levels
            .get(&row)
            .filter(|&&l| l != 0)
            .map(|&l| format!(" outlineLevel=\"{l}\""))
            .unwrap_or_default();
        let collapsed_attr = if sheet.collapsed_rows.contains(&row) {
            " collapsed=\"1\""
        } else {
            ""
        };
        s.push_str(&format!(
            "<row r=\"{}\"{ht_attr}{hidden_attr}{outline_attr}{collapsed_attr}>",
            row + 1
        ));
        while cells.peek().is_some_and(|(at, _)| at.row == row) {
            let (at, cell) = cells.next().unwrap();
            write_cell(&mut s, workbook, at.row, at.col, cell, strings);
        }
        s.push_str("</row>");
    }
    s.push_str("</sheetData>");

    // `<sheetProtection>` precedes `<autoFilter>` in the CT_Worksheet sequence.
    // Attributes are written back exactly as they were read, in sorted order for
    // determinism — including the password hash, which must never be
    // regenerated here.
    if let Some(p) = &sheet.protection
        && !p.attrs.is_empty()
    {
        s.push_str("<sheetProtection");
        for (k, v) in &p.attrs {
            s.push_str(&format!(" {k}=\"{}\"", escape_attr(v)));
        }
        s.push_str("/>");
    }

    // <autoFilter> precedes <mergeCells> in the CT_Worksheet sequence.
    if let Some(filter) = &sheet.auto_filter {
        write_auto_filter(&mut s, filter);
    }

    if !sheet.merges.is_empty() {
        s.push_str(&format!("<mergeCells count=\"{}\">", sheet.merges.len()));
        for range in &sheet.merges {
            s.push_str(&format!("<mergeCell ref=\"{}\"/>", range_a1(range)));
        }
        s.push_str("</mergeCells>");
    }

    // Conditional formatting — one <conditionalFormatting> per rule, its <dxf>
    // referenced by the fill's index in the workbook dxfs list.
    for (i, cf) in sheet.conditional_formats.iter().enumerate() {
        let dxf_id = dxfs
            .iter()
            .position(|d| d.fill == cf.fill && d.font_color == cf.font_color && d.bold == cf.bold)
            .unwrap_or(0);
        s.push_str(&format!(
            "<conditionalFormatting sqref=\"{}\">",
            range_a1(&cf.range)
        ));
        // An explicit priority wins; zero means the rule never carried one, so
        // fall back to document order rather than writing an invalid 0.
        let priority = if cf.priority > 0 {
            cf.priority as usize
        } else {
            i + 1
        };
        s.push_str(&cf_rule_xml(cf, dxf_id, priority));
        s.push_str("</conditionalFormatting>");
    }

    // Data validations come after conditionalFormatting in the CT_Worksheet
    // sequence. A list's allowed values are an inline quoted CSV in <formula1>.
    if !sheet.validations.is_empty() {
        s.push_str(&format!(
            "<dataValidations count=\"{}\">",
            sheet.validations.len()
        ));
        for v in &sheet.validations {
            s.push_str(&format!("<dataValidation type=\"{}\"", v.kind.ooxml()));
            // `between` and `allowBlank=1` are the schema defaults, written by
            // omission. The operator is meaningless for list and custom rules.
            if !matches!(v.kind, DvKind::List | DvKind::Custom | DvKind::None)
                && v.operator != DvOperator::Between
            {
                s.push_str(&format!(" operator=\"{}\"", v.operator.ooxml()));
            }
            if v.allow_blank {
                s.push_str(" allowBlank=\"1\"");
            }
            // Author-set wording is preserved; the flags follow whether there is
            // anything to show.
            if !v.prompt_title.is_empty() || !v.prompt_text.is_empty() {
                s.push_str(" showInputMessage=\"1\"");
                if !v.prompt_title.is_empty() {
                    s.push_str(&format!(
                        " promptTitle=\"{}\"",
                        escape_attr(&v.prompt_title)
                    ));
                }
                if !v.prompt_text.is_empty() {
                    s.push_str(&format!(" prompt=\"{}\"", escape_attr(&v.prompt_text)));
                }
            }
            if let Some(style) = &v.error_style {
                s.push_str(&format!(" errorStyle=\"{}\"", escape_attr(style)));
            }
            if v.hide_dropdown {
                s.push_str(" showDropDown=\"1\"");
            }
            if let Some(ime) = &v.ime_mode {
                s.push_str(&format!(" imeMode=\"{}\"", escape_attr(ime)));
            }
            s.push_str(" showErrorMessage=\"1\"");
            if !v.error_title.is_empty() {
                s.push_str(&format!(" errorTitle=\"{}\"", escape_attr(&v.error_title)));
            }
            if !v.error_text.is_empty() {
                s.push_str(&format!(" error=\"{}\"", escape_attr(&v.error_text)));
            }
            s.push_str(&format!(" sqref=\"{}\">", range_a1(&v.range)));
            // A list's values are an inline quoted CSV; every other kind keeps
            // the operand text it came in with.
            let f1 = if v.kind == DvKind::List && !v.values.is_empty() {
                format!("\"{}\"", v.values.join(","))
            } else {
                v.formula1.clone()
            };
            if !f1.is_empty() {
                s.push_str(&format!("<formula1>{}</formula1>", escape_text(&f1)));
            }
            if !v.formula2.is_empty() {
                s.push_str(&format!(
                    "<formula2>{}</formula2>",
                    escape_text(&v.formula2)
                ));
            }
            s.push_str("</dataValidation>");
        }
        s.push_str("</dataValidations>");
    }

    carried(&mut s, "protectedRange", Some("protectedRanges"));
    carried(&mut s, "sheetCalcPr", None);
    if let Some(sort) = &sheet.sort_state {
        // Follows `<autoFilter>` in CT_Worksheet's sequence. A saved sort
        // records an order already applied to the cells, so nothing is
        // re-sorted on load; only the record would be lost.
        s.push_str("<sortState");
        for (k, v) in &sort.attrs {
            s.push_str(&format!(" {k}=\"{}\"", escape_attr(v)));
        }
        if sort.conditions.is_empty() {
            s.push_str("/>");
        } else {
            s.push('>');
            for condition in &sort.conditions {
                write_attr_element(&mut s, "sortCondition", condition);
            }
            s.push_str("</sortState>");
        }
    }

    // `<hyperlinks>` sits after `dataValidations` and before `printOptions` in
    // CT_Worksheet's sequence; out of order the package fails validation.
    if !sheet.hyperlinks.is_empty() {
        let targets = external_targets(sheet);
        s.push_str("<hyperlinks>");
        for link in &sheet.hyperlinks {
            s.push_str(&format!("<hyperlink ref=\"{}\"", range_a1(&link.range)));
            if let Some(target) = &link.target
                && let Some(i) = targets.iter().position(|t| t == target)
            {
                s.push_str(&format!(" r:id=\"{}\"", hyperlink_rel_id(i)));
            }
            if let Some(location) = &link.location {
                s.push_str(&format!(" location=\"{}\"", escape_attr(location)));
            }
            if let Some(display) = &link.display {
                s.push_str(&format!(" display=\"{}\"", escape_attr(display)));
            }
            if let Some(tooltip) = &link.tooltip {
                s.push_str(&format!(" tooltip=\"{}\"", escape_attr(tooltip)));
            }
            s.push_str("/>");
        }
        s.push_str("</hyperlinks>");
    }

    // Print layout. CT_Worksheet is an xsd:sequence here, unlike the font
    // types, so this order is required rather than conventional: printOptions,
    // pageMargins, pageSetup, headerFooter, rowBreaks, colBreaks.
    let print = &sheet.print;
    write_attr_element(&mut s, "printOptions", &print.options);
    write_attr_element(&mut s, "pageMargins", &print.margins);
    write_attr_element(&mut s, "pageSetup", &print.page);
    if !print.header_footer.is_empty() || !print.header_footer_text.is_empty() {
        s.push_str("<headerFooter");
        for (k, v) in &print.header_footer {
            s.push_str(&format!(" {k}=\"{}\"", escape_attr(v)));
        }
        s.push('>');
        // Child order within CT_HeaderFooter is also a sequence.
        for tag in [
            "oddHeader",
            "oddFooter",
            "evenHeader",
            "evenFooter",
            "firstHeader",
            "firstFooter",
        ] {
            if let Some(text) = print.header_footer_text.get(tag) {
                s.push_str(&format!("<{tag}>{}</{tag}>", escape_text(text)));
            }
        }
        s.push_str("</headerFooter>");
    }
    for (tag, breaks) in [
        ("rowBreaks", &print.row_breaks),
        ("colBreaks", &print.col_breaks),
    ] {
        if breaks.is_empty() {
            continue;
        }
        let manual = breaks
            .iter()
            .filter(|b| b.get("man").is_some_and(|v| v == "1" || v == "true"))
            .count();
        s.push_str(&format!(
            "<{tag} count=\"{}\" manualBreakCount=\"{manual}\">",
            breaks.len()
        ));
        for brk in breaks {
            write_attr_element(&mut s, "brk", brk);
        }
        s.push_str(&format!("</{tag}>"));
    }

    // `<drawing>` precedes `<legacyDrawing>` in CT_Worksheet's sequence, and
    // `<oleObjects>`/`<controls>` precede both.
    for name in ["controls", "oleObjects", "drawing", "picture"] {
        for (element, attrs) in sheet.retained_refs.iter().filter(|(n, _)| n == name) {
            s.push_str(&format!("<{element}"));
            for (k, v) in attrs {
                let key = if k == "id" { "r:id" } else { k.as_str() };
                s.push_str(&format!(" {key}=\"{}\"", escape_attr(v)));
            }
            s.push_str("/>");
        }
        // A sheet that had no drawing gains one for the charts made here. A
        // sheet that already had one keeps its own element above: its drawing
        // was spliced rather than replaced, so the reference still holds.
        if name == "drawing"
            && let Some((id, _)) = &charts.sheet_rel
        {
            s.push_str(&format!("<drawing r:id=\"{}\"/>", escape_attr(id)));
        }
    }

    // Legacy drawing ref (the VML holding note markers).
    if !sheet.comments.is_empty() {
        s.push_str(&format!("<legacyDrawing r:id=\"{}\"/>", ids.vml));
    }

    carried(&mut s, "ignoredError", Some("ignoredErrors"));
    // `<tableParts>` closes CT_Worksheet. Without it the table parts are in the
    // package but attached to no sheet, so Excel shows a plain range.
    if !sheet.tables.is_empty() {
        s.push_str(&format!("<tableParts count=\"{}\">", sheet.tables.len()));
        for j in 0..sheet.tables.len() {
            s.push_str(&format!("<tablePart r:id=\"{}\"/>", table_rel_id(j)));
        }
        s.push_str("</tableParts>");
    }

    s.push_str("</worksheet>");
    s
}

/// A single `<cfRule>` for a highlight-cells rule.
fn cf_rule_xml(cf: &ConditionalFormat, dxf_id: usize, priority: usize) -> String {
    let xml = cf_rule_body(cf, dxf_id, priority);
    if !cf.stop_if_true {
        return xml;
    }
    // `stopIfTrue` is an attribute of every rule form, so it is spliced in here
    // rather than repeated across a dozen format! calls.
    xml.replacen("<cfRule ", "<cfRule stopIfTrue=\"1\" ", 1)
}

fn cf_rule_body(cf: &ConditionalFormat, dxf_id: usize, priority: usize) -> String {
    match &cf.rule {
        CfRule::GreaterThan(x) => format!(
            "<cfRule type=\"cellIs\" dxfId=\"{dxf_id}\" priority=\"{priority}\" operator=\"greaterThan\"><formula>{}</formula></cfRule>",
            fmt_f64(*x)
        ),
        CfRule::LessThan(x) => format!(
            "<cfRule type=\"cellIs\" dxfId=\"{dxf_id}\" priority=\"{priority}\" operator=\"lessThan\"><formula>{}</formula></cfRule>",
            fmt_f64(*x)
        ),
        CfRule::EqualTo(x) => format!(
            "<cfRule type=\"cellIs\" dxfId=\"{dxf_id}\" priority=\"{priority}\" operator=\"equal\"><formula>{}</formula></cfRule>",
            fmt_f64(*x)
        ),
        CfRule::Between(lo, hi) => format!(
            "<cfRule type=\"cellIs\" dxfId=\"{dxf_id}\" priority=\"{priority}\" operator=\"between\"><formula>{}</formula><formula>{}</formula></cfRule>",
            fmt_f64(*lo),
            fmt_f64(*hi)
        ),
        CfRule::GreaterThanOrEqual(x) => format!(
            "<cfRule type=\"cellIs\" dxfId=\"{dxf_id}\" priority=\"{priority}\" operator=\"greaterThanOrEqual\"><formula>{}</formula></cfRule>",
            fmt_f64(*x)
        ),
        CfRule::LessThanOrEqual(x) => format!(
            "<cfRule type=\"cellIs\" dxfId=\"{dxf_id}\" priority=\"{priority}\" operator=\"lessThanOrEqual\"><formula>{}</formula></cfRule>",
            fmt_f64(*x)
        ),
        CfRule::NotEqualTo(x) => format!(
            "<cfRule type=\"cellIs\" dxfId=\"{dxf_id}\" priority=\"{priority}\" operator=\"notEqual\"><formula>{}</formula></cfRule>",
            fmt_f64(*x)
        ),
        CfRule::NotBetween(lo, hi) => format!(
            "<cfRule type=\"cellIs\" dxfId=\"{dxf_id}\" priority=\"{priority}\" operator=\"notBetween\"><formula>{}</formula><formula>{}</formula></cfRule>",
            fmt_f64(*lo),
            fmt_f64(*hi)
        ),
        // Colour scales and data bars carry their own presentation, so they take
        // no dxfId — the `<cfvo>` stops describe the range's own min and max.
        CfRule::ColorScale(colors) => {
            let stops: Vec<&str> = colors.iter().map(String::as_str).collect();
            let cfvo = if stops.len() >= 3 {
                "<cfvo type=\"min\"/><cfvo type=\"percentile\" val=\"50\"/><cfvo type=\"max\"/>"
            } else {
                "<cfvo type=\"min\"/><cfvo type=\"max\"/>"
            };
            let mut colours = String::new();
            for c in &stops {
                colours.push_str(&format!("<color rgb=\"FF{}\"/>", escape_attr(c)));
            }
            format!(
                "<cfRule type=\"colorScale\" priority=\"{priority}\"><colorScale>{cfvo}{colours}</colorScale></cfRule>"
            )
        }
        CfRule::DataBar(color) => format!(
            "<cfRule type=\"dataBar\" priority=\"{priority}\"><dataBar><cfvo type=\"min\"/><cfvo type=\"max\"/><color rgb=\"FF{}\"/></dataBar></cfRule>",
            escape_attr(color)
        ),
        CfRule::Top10 {
            rank,
            bottom,
            percent,
        } => format!(
            "<cfRule type=\"top10\" dxfId=\"{dxf_id}\" priority=\"{priority}\"{}{} rank=\"{rank}\"/>",
            if *bottom { " bottom=\"1\"" } else { "" },
            if *percent { " percent=\"1\"" } else { "" },
        ),
        // The schema defaults `aboveAverage` to true, so only the "below" case
        // needs writing out.
        CfRule::AboveAverage { below, equal } => format!(
            "<cfRule type=\"aboveAverage\" dxfId=\"{dxf_id}\" priority=\"{priority}\"{}{}/>",
            if *below { " aboveAverage=\"0\"" } else { "" },
            if *equal { " equalAverage=\"1\"" } else { "" },
        ),
        CfRule::DuplicateValues { unique } => format!(
            "<cfRule type=\"{}\" dxfId=\"{dxf_id}\" priority=\"{priority}\"/>",
            if *unique {
                "uniqueValues"
            } else {
                "duplicateValues"
            },
        ),
        // The formula goes out exactly as the model holds it, because the model
        // holds exactly what the file said: A1 text anchored to the top-left of
        // this rule's range (`CfRule::Expression`). Re-anchoring it here — to
        // `A1`, or to anything else — would move every highlight by the
        // difference, and nothing on the way back in would notice.
        CfRule::Expression(formula) => format!(
            "<cfRule type=\"expression\" dxfId=\"{dxf_id}\" priority=\"{priority}\"><formula>{}</formula></cfRule>",
            escape_text(formula)
        ),
        CfRule::TextContains(text) => {
            let top = cell_a1(cf.range.start.row, cf.range.start.col);
            format!(
                "<cfRule type=\"containsText\" dxfId=\"{dxf_id}\" priority=\"{priority}\" operator=\"containsText\" text=\"{}\"><formula>NOT(ISERROR(SEARCH(\"{}\",{top})))</formula></cfRule>",
                escape_attr(text),
                escape_text(text)
            )
        }
    }
}

fn write_cell(
    s: &mut String,
    workbook: &Workbook,
    row: u32,
    col: u32,
    cell: &Cell,
    strings: &SharedStrings,
) {
    let reference = cell_a1(row, col);
    let style_attr = cell
        .style
        .and_then(|id| workbook.styles.index_of(id))
        .map(|i| format!(" s=\"{}\"", i + 1))
        .unwrap_or_default();

    let has_formula = cell.formula.is_some();
    // A non-finite number (inf/NaN — e.g. arithmetic overflow, or a `1e999`
    // literal) is not a valid OOXML numeric literal and would emit `<v>inf</v>`,
    // corrupting the file. Treat it as a #NUM! error cell for both the type
    // attribute and the value below.
    let num_error = CellValue::Error(ErrorValue::Num);
    let effective_value = match &cell.value {
        CellValue::Number(n) if !n.is_finite() => &num_error,
        other => other,
    };
    // A formula cell whose cached result is a string is a `str` type with the
    // text in `<v>` — OOXML does not allow `<is>`/shared-string on a formula
    // cell (Excel would drop the formula or repair the file). Only a *literal*
    // inline string (no formula) uses `t="inlineStr"` with `<is>`.
    let type_attr = match effective_value {
        CellValue::Bool(_) => " t=\"b\"",
        CellValue::Error(_) => " t=\"e\"",
        CellValue::SharedString(_) if has_formula => " t=\"str\"",
        CellValue::InlineString(_) if has_formula => " t=\"str\"",
        CellValue::SharedString(_) => " t=\"s\"",
        CellValue::InlineString(_) => " t=\"inlineStr\"",
        _ => "",
    };

    s.push_str(&format!("<c r=\"{reference}\"{style_attr}{type_attr}>"));

    if let Some(handle) = cell.formula
        && let Some(expr) = workbook.formula(handle)
    {
        s.push_str(&format!(
            "<f>{}</f>",
            escape_text(&formula_text(expr, Origin::at(row, col)))
        ));
    }

    match effective_value {
        CellValue::Empty => {}
        CellValue::Number(n) => s.push_str(&format!("<v>{n}</v>")),
        CellValue::Bool(b) => s.push_str(&format!("<v>{}</v>", if *b { 1 } else { 0 })),
        CellValue::Error(e) => s.push_str(&format!("<v>{e}</v>")),
        // Formula string result: emit the text in <v> (t="str"); otherwise a
        // literal shared string is emitted as its shared-table index.
        CellValue::SharedString(id) if has_formula => {
            let text = workbook.strings.get(*id).unwrap_or("");
            s.push_str(&format!("<v>{}</v>", escape_text(text)));
        }
        CellValue::SharedString(id) => {
            // The index in the *written* table, which is not the model's once
            // strings this session abandoned have been left out (`FID-36`).
            // Every cell reference is emitted by construction, so `None` here
            // means an id that does not resolve in this workbook at all — the
            // same case the previous code answered with 0.
            let index = strings.index_of(workbook, *id).unwrap_or(0);
            s.push_str(&format!("<v>{index}</v>"));
        }
        CellValue::InlineString(id) => {
            let text = workbook.strings.get(*id).unwrap_or("");
            if has_formula {
                s.push_str(&format!("<v>{}</v>", escape_text(text)));
            } else {
                s.push_str(&format!(
                    "<is><t xml:space=\"preserve\">{}</t></is>",
                    escape_text(text)
                ));
            }
        }
    }

    s.push_str("</c>");
}

fn sheet_index(workbook: &Workbook, id: SheetId) -> Option<usize> {
    workbook.sheets.iter().position(|s| s.id == id)
}

#[cfg(test)]
mod tests;

/// Write an `<autoFilter>` element with its per-column rules.
///
/// Shared by the worksheet and table writers: a table filters independently of
/// the sheet it sits on, and the element is identical in both places.
fn write_auto_filter(s: &mut String, filter: &AutoFilter) {
    s.push_str(&format!("<autoFilter ref=\"{}\">", range_a1(&filter.range)));
    for (&col_id, rule) in &filter.rules {
        s.push_str(&format!("<filterColumn colId=\"{col_id}\">"));
        match rule {
            // Re-emitted exactly as read, so Excel applies the filter it
            // wrote even though we never evaluated it.
            FilterRule::Unevaluated { element, attrs } => {
                write_attr_element(s, element, attrs);
            }
            FilterRule::Values(vals) => {
                // A blank is not a <filter val="">; OOXML carries it as an
                // attribute on the container.
                let blank = vals.iter().any(|v| v.is_empty());
                s.push_str(if blank {
                    "<filters blank=\"1\">"
                } else {
                    "<filters>"
                });
                for v in vals.iter().filter(|v| !v.is_empty()) {
                    s.push_str(&format!("<filter val=\"{}\"/>", escape_attr(v)));
                }
                s.push_str("</filters>");
            }
            FilterRule::Custom { first, second, and } => {
                s.push_str(&format!(
                    "<customFilters{}>",
                    if *and { " and=\"1\"" } else { "" }
                ));
                for f in std::iter::once(first).chain(second.as_ref()) {
                    s.push_str(&format!(
                        "<customFilter operator=\"{}\" val=\"{}\"/>",
                        f.op.as_ooxml(),
                        escape_attr(&f.value)
                    ));
                }
                s.push_str("</customFilters>");
            }
        }
        s.push_str("</filterColumn>");
    }
    s.push_str("</autoFilter>");
}
