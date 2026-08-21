//! `casual-calc-import` — SpreadsheetML semantic import into the normalized
//! model.
//!
//! Phase 1A: maps a SpreadsheetML package into a [`Workbook`] — cell values
//! (number, bool, shared/inline string, error), **formulas parsed to an AST**
//! (`casual-calc-formula`) with the cached value preserved, **number formats**
//! (from `styles.xml` `cellXfs`), **merged ranges**, **frozen panes**, and
//! **defined names**. A [`CompatibilityReport`] records anything not fully
//! mapped (e.g. an unparseable formula is `Degraded`, keeping its cached value).
//! Import is deterministic: fixed workbook id, sequential sheet ids, and
//! insertion-ordered interning.
//!
//! See `docs/34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md` and
//! `docs/22-NORMALIZED-SCHEMA.md`.

mod a1;
mod chart;
mod error;
mod pivot;
mod read;
mod report;
mod styles;
mod theme;

pub use error::{ImportError, Overrun};
pub use report::{CompatibilityEntry, CompatibilityReport, ModelOutcome, RetentionOutcome};
pub use theme::stock_theme_slots;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use casual_calc_formula::stored::{ABSOLUTE, Origin};
use casual_calc_formula::{Expr, FormulaError, strip_bound_name_prefixes, strip_future_prefixes};

/// Parse a formula **as the file writes it**, which is not quite the language.
///
/// SpreadsheetML prefixes any function it postdates with `_xlfn.`, so a file
/// says `_xlfn.CONCAT` where the formula says `CONCAT`. Everything downstream —
/// the evaluator, the formula bar, the transform — works in the language, so the
/// prefix comes off here, at the one point where file text becomes an `Expr`,
/// and nowhere else has to know about it.
fn parse_formula(text: &str) -> Result<Expr, FormulaError> {
    let mut expr = casual_calc_formula::parse(text)?;
    strip_future_prefixes(&mut expr);
    strip_bound_name_prefixes(&mut expr);
    Ok(expr)
}
use casual_calc_model::{
    AutoFilter, Cell, CellComment, CellRef, CellValue, CfRule, CommentReply, ConditionalFormat,
    CustomFilter, DataValidation, DefinedName, DvKind, DvOperator, ErrorValue, FilterOp,
    FilterRule, Id, IdGenerator, RetainedPart, RetainedRel, Sheet, SheetId, SheetVisibility,
    StringId, Workbook,
};
use casual_calc_ooxml::{OoxmlLimits, SpreadsheetPackage};

use a1::{Parsed, parse_a1, parse_a1_classified, parse_range_classified};
use read::{
    RawCell, RawThreadedComment, parse_comments, parse_date1904, parse_defined_names,
    parse_persons, parse_retained_refs, parse_shared_strings, parse_table, parse_table_parts,
    parse_threaded_comments, parse_workbook_settings, parse_worksheet,
};
use styles::{StyleSheet, parse_styles};
use theme::{ThemePalette, parse_theme};

const WORKBOOK_NAMESPACE: u64 = 0x574b_0000_0000_0000; // "WK"
const SHEET_NAMESPACE: u64 = 0x5348_0000_0000_0000; // "SH"
const SHARED_STRINGS_PART: &str = "xl/sharedStrings.xml";
const STYLES_PART: &str = "xl/styles.xml";
const THEME_PART: &str = "xl/theme/theme1.xml";
/// Relationship type suffix binding a worksheet to its comments part.
const COMMENTS_REL_SUFFIX: &str = "/comments";
/// Relationship type suffix binding a worksheet to its threaded-comments part.
/// Deliberately distinct from [`COMMENTS_REL_SUFFIX`]: the two are matched by
/// suffix, and `…/threadedComment` does not end in `/comments`, so a package
/// carrying both binds each to the right reader.
const THREADED_COMMENTS_REL_SUFFIX: &str = "/threadedComment";
/// A worksheet's drawing, which is what anchors its charts and pictures.
const DRAWING_REL_SUFFIX: &str = "/drawing";
/// A drawing's chart, one hop further out.
const CHART_REL_SUFFIX: &str = "/chart";
/// A drawing's picture.
const IMAGE_REL_SUFFIX: &str = "/image";
/// Relationship type suffix binding the workbook to its persons part.
const PERSONS_REL_SUFFIX: &str = "/person";
/// Most areas honoured from one `sqref`. Each area materializes its own model
/// entry (a validation copies its whole value list), so an adversarial part
/// with a huge area list must not become unbounded allocation.
const MAX_SQREF_AREAS: usize = 1024;
/// Most parts named individually in the compatibility report before the rest
/// are folded into one `(overflow)` bucket. The names come from the file, so an
/// adversarial package with a relationship per part must not be able to grow the
/// report without bound; the host still gets a count of what it did not get.
const MAX_REPORT_PART_FEATURES: usize = 256;

/// Note in the report that the grid bound refused a reference (FID-18).
///
/// Only the out-of-grid case is recorded here. A *malformed* reference is a
/// failure every one of these call sites has had an answer for since Phase 1A —
/// some report it, some skip it as noise — and rerouting that through here would
/// have changed behaviour nothing asked to change. What is new is the address
/// that is perfectly well-formed and simply does not exist, and docs/34 is
/// explicit that this is the one thing that may not happen quietly: `Omitted` +
/// `NotRetained`, counted, named, per construct.
fn note_out_of_grid<T>(report: &mut CompatibilityReport, feature: &str, parsed: &Parsed<T>) {
    if parsed.is_out_of_grid() {
        report.record(
            feature,
            ModelOutcome::Omitted,
            RetentionOutcome::NotRetained,
        );
    }
}

/// Whether every reference in `expr` is inside the addressable grid.
///
/// A formula is a reference too, and one that says `SUM(A1:ZZZZ4294967295)` is
/// written back into `<f>` exactly as it arrived — the same corrupt package by a
/// different route. The AST is checked rather than the text because by the time
/// this is asked the text has already been through `_xlfn.` stripping and,
/// for a shared-formula follower, a row/column shift that can push an in-range
/// master out of the grid on its own.
fn expr_within_grid(expr: &Expr, origin: Origin) -> bool {
    let within = |r: &casual_calc_formula::stored::StoredRef| {
        // Resolved against the cell that will hold it: a stored reference is an
        // offset, and a follower's offsets only name an address once its own
        // origin is applied. One that resolves off the sheet is `#REF!` and is
        // *not* inside the grid.
        r.resolve(origin).is_some_and(|at| {
            at.row <= casual_calc_model::GRID_MAX_ROW && at.col <= casual_calc_model::GRID_MAX_COL
        })
    };
    match expr {
        Expr::Reference(r) => within(r),
        Expr::Range(a, b) => within(a) && within(b),
        Expr::Unary { operand, .. } => expr_within_grid(operand, origin),
        Expr::Binary { left, right, .. } => {
            expr_within_grid(left, origin) && expr_within_grid(right, origin)
        }
        Expr::Function { args, .. } => args.iter().all(|a| expr_within_grid(a, origin)),
        Expr::Call { callee, args } => {
            expr_within_grid(callee, origin) && args.iter().all(|a| expr_within_grid(a, origin))
        }
        // No address of its own: a literal, a name, a structured reference (the
        // evaluator resolves that against a table, and a table's range came
        // through `parse_range` above), or text this parser never read.
        _ => true,
    }
}

/// Whether every address in a verbatim element's attributes is inside the grid.
///
/// Four attribute names hold one: `ref` and `sqref` (a space-separated list of
/// areas), and `activeCell` / `topLeftCell`, which are single cells. Anything
/// that is not an address is left alone — `<protectedRange name="…">` is a name,
/// not a reference, and this is not the place to judge it.
fn carried_within_grid(attrs: &BTreeMap<String, String>) -> bool {
    attrs.iter().all(|(key, value)| {
        match key.as_str() {
            "ref" | "sqref" | "activeCell" | "topLeftCell" => value
                .split_whitespace()
                .all(|area| !parse_range_classified(area).is_out_of_grid()),
            // Not an address; nothing here to bound.
            _ => true,
        }
    })
}

/// The result of importing a package: the model plus its compatibility report.
#[derive(Debug)]
pub struct Import {
    /// The normalized workbook.
    pub workbook: Workbook,
    /// What was mapped, degraded, or omitted.
    pub report: CompatibilityReport,
}

/// Copy the parts nothing above modelled into the workbook's retention set.
///
/// A part is retained when it is reachable from the package root, `workbook.xml`
/// or a sheet by a relationship whose type we do not handle. The reference
/// element inside `workbook.xml` (`<externalReference r:id=…>`) travels too: a
/// retained part nothing points at is invisible to Excel, which is
/// indistinguishable from having dropped it.
fn retain_unmodelled(
    package: &mut SpreadsheetPackage,
    workbook: &mut Workbook,
    sheet_parts: &[String],
    report: &mut CompatibilityReport,
) -> Result<(), ImportError> {
    let limits = *package.limits();
    // Both halves of `[Content_Types].xml`, not just the `<Override>`s. A real
    // file declares its repeated binary parts by extension — every
    // `printerSettings*.bin`, `image1.emf`, `image1.jpeg` in the corpus — and
    // reading only the overrides recorded `None` for all of them. The part was
    // then written back with no content type at all, which is a package Excel
    // refuses or offers to repair, and repairing it discards them (FID-17).
    let content_types = package.content_types()?;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    // Everything already generated by the writer, which must not be retained as
    // well — a stale copy would be written beside the fresh one.
    let generated: BTreeSet<String> = sheet_parts.iter().cloned().collect();

    // Breadth-first from the parts we do parse, **and from the package root**. A
    // drawing reaches its charts and images through its *own* relationships, so
    // retention has to follow them: keeping `drawing1.xml` while dropping the
    // chart it references leaves a reference to nothing, which Excel reports as
    // a repair.
    //
    // The root (`""`, whose relationships live in `_rels/.rels`) was missing
    // from this list, and it is where Excel hangs four relationships on every
    // file it writes: the workbook, `docProps/core.xml`, `docProps/app.xml` and
    // `customXml`. None of them is reachable from `workbook.xml`, so opening any
    // ordinary workbook and saving it silently dropped its author, its title,
    // its company — and every custom XML payload a host was round-tripping —
    // with an empty report to say so.
    let workbook_part = package.workbook_part().to_owned();
    let mut queue: Vec<String> = std::iter::once(String::new())
        .chain(std::iter::once(workbook_part.clone()))
        .chain(sheet_parts.iter().cloned())
        .collect();
    while let Some(source) = queue.pop() {
        let kind = PartKind::of(&source, &workbook_part, sheet_parts);
        for rel in package.relationships_of(&source, &limits)? {
            if is_modelled(kind, &rel.rel_type) {
                continue;
            }
            // `TargetMode="External"` names something outside the package — the
            // `externalLink` to another workbook, which is data the author put
            // there and which nothing here models. It is retained like any other
            // unmodelled relationship, but the retention stops at the
            // relationship: a URI is not a path, so nothing below may resolve it
            // against the source part or ask the package whether it holds it.
            // `file:///other.xlsx` under `xl/workbook.xml` would "resolve" to
            // `xl/file:/other.xlsx`, and the package saying it has no such part
            // is the report of a loss that never happened (FID-19).
            //
            // Nothing is lost either way, so nothing is reported: the
            // relationship is Preserved, and the `<externalReference r:id>` that
            // names it travels with it.
            if rel.external {
                workbook.retained_rels.push(RetainedRel {
                    source: source.clone(),
                    id: rel.id,
                    rel_type: rel.rel_type,
                    target: rel.target,
                    external: true,
                });
                continue;
            }
            let target = resolve_part(&source, &rel.target);
            workbook.retained_rels.push(RetainedRel {
                source: source.clone(),
                id: rel.id,
                rel_type: rel.rel_type,
                target: rel.target,
                external: false,
            });
            if generated.contains(&target) || !seen.insert(target.clone()) {
                continue;
            }
            if !package.contains(&target) {
                // A part the file names but does not carry: there are no bytes
                // to retain, and `Omitted` + `NotRetained` is the one way data
                // leaves the system, so it is counted rather than skipped in
                // silence (docs/34). Keyed by path, capped, because the count of
                // distinct names here comes from the file.
                if seen.len() <= MAX_REPORT_PART_FEATURES {
                    report.record(
                        &target,
                        ModelOutcome::Omitted,
                        RetentionOutcome::NotRetained,
                    );
                } else {
                    report.record(
                        "(overflow)",
                        ModelOutcome::Omitted,
                        RetentionOutcome::NotRetained,
                    );
                }
                continue;
            }
            let bytes = package.read_part(&target)?;
            workbook.retained_parts.push(RetainedPart {
                // Carried from the file, never inferred from the extension:
                // `.bin` is printer settings in one workbook and an OLE object
                // or a pivot record stream in the next, and a guess would make
                // the saved file assert something the source never said.
                content_type: content_types.resolve(&target).map(str::to_owned),
                path: target.clone(),
                bytes,
            });
            // Follow this part's own relationships in turn.
            queue.push(target);
        }
    }
    // Deterministic order: the queue is LIFO, so without this two runs over the
    // same package could emit the parts in different orders.
    workbook.retained_parts.sort_by(|a, b| a.path.cmp(&b.path));
    workbook
        .retained_rels
        .sort_by(|a, b| (&a.source, &a.id).cmp(&(&b.source, &b.id)));
    workbook.retained_rels.dedup();
    Ok(())
}

/// A worksheet's pivot table.
const PIVOT_TABLE_REL_SUFFIX: &str = "/pivotTable";
/// A pivot table's cache, one hop further out.
const PIVOT_CACHE_REL_SUFFIX: &str = "/pivotCacheDefinition";

/// Read every sheet's pivot tables into the model.
///
/// The parts stay retained either way: this makes an imported pivot live —
/// listed, reconfigurable, refreshable — without changing what is written back
/// until the user edits it. A pivot that fails to resolve is skipped in
/// silence for the same reason a chart is: the part is still there and still
/// written, so the cost is a pivot the panel does not list, not a lost one.
fn read_pivots(
    package: &mut SpreadsheetPackage,
    workbook: &mut Workbook,
    report: &mut CompatibilityReport,
) -> Result<(), ImportError> {
    let limits = *package.limits();
    let sheet_parts: Vec<String> = package.sheets().iter().map(|s| s.part.clone()).collect();
    let mut next_id = 1u32;

    for (index, part) in sheet_parts.iter().enumerate() {
        let rels: Vec<_> = package
            .relationships_of(part, &limits)?
            .into_iter()
            .filter(|r| !r.external && r.rel_type.ends_with(PIVOT_TABLE_REL_SUFFIX))
            .collect();
        for rel in rels {
            let target = resolve_part(part, &rel.target);
            if !package.contains(&target) {
                continue;
            }
            let spec = pivot::parse_pivot_table(&package.read_part(&target)?)?;
            let cache = match package.related_part(&target, PIVOT_CACHE_REL_SUFFIX, &limits)? {
                Some(cache_part) if package.contains(&cache_part) => {
                    pivot::parse_pivot_cache(&package.read_part(&cache_part)?)?
                }
                // Without the cache the field indices name nothing, so there is
                // no definition to reconstruct — only a preserved part.
                _ => {
                    report.record(
                        "pivotTable",
                        ModelOutcome::Degraded,
                        RetentionOutcome::Preserved,
                    );
                    continue;
                }
            };

            let Some((source_sheet, source)) = resolve_pivot_source(workbook, &cache) else {
                report.record(
                    "pivotTable",
                    ModelOutcome::Degraded,
                    RetentionOutcome::Preserved,
                );
                continue;
            };
            workbook.sheets[index].pivots.push(pivot::to_model(
                &spec,
                &cache,
                next_id,
                source_sheet,
                source,
                target,
            ));
            next_id += 1;
            report.record(
                "pivotTable",
                ModelOutcome::Mapped,
                RetentionOutcome::Preserved,
            );
        }
    }
    Ok(())
}

/// Where a cache's records live: a sheet and a rectangle.
///
/// `<worksheetSource>` names them either directly (`sheet` + `ref`) or through
/// a `name`, which is a table or a defined name. Both forms are common — Excel
/// writes the second whenever the pivot was built from a table — so resolving
/// only the first would leave every table-sourced pivot unreadable.
fn resolve_pivot_source(
    workbook: &Workbook,
    cache: &pivot::CacheSpec,
) -> Option<(casual_calc_model::SheetId, casual_calc_model::CellRange)> {
    if let Some(name) = &cache.name {
        for sheet in &workbook.sheets {
            if let Some(table) = sheet.tables.iter().find(|t| &t.name == name) {
                // Only the header and body: a totals row is a summary of the
                // records, not one of them, and aggregating it would double
                // every figure.
                let mut range = table.range;
                range.end.row = range.end.row.saturating_sub(table.totals_row_count);
                return Some((sheet.id, range));
            }
        }
    }
    let range = cache.range?;
    let sheet_name = cache.sheet.as_deref()?;
    let sheet = workbook.sheets.iter().find(|s| s.name == sheet_name)?;
    Some((sheet.id, range))
}

/// Which part declared a relationship, as far as retention is concerned.
///
/// Retention needs this because a relationship *type* alone does not say
/// whether the model already carries it — the part it hangs off decides.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PartKind {
    /// The package root, whose relationships live in `_rels/.rels`.
    Root,
    /// `xl/workbook.xml`, wherever this particular file keeps it.
    Workbook,
    /// One of the worksheets listed in the workbook.
    Worksheet,
    /// Everything reached by following the two above: drawings, charts,
    /// images, custom XML. Nothing declared here is modelled.
    Other,
}

impl PartKind {
    fn of(source: &str, workbook_part: &str, sheet_parts: &[String]) -> Self {
        if source.is_empty() {
            Self::Root
        } else if source == workbook_part {
            Self::Workbook
        } else if sheet_parts.iter().any(|p| p == source) {
            Self::Worksheet
        } else {
            Self::Other
        }
    }
}

/// Whether `import_package` already turns this relationship into model state,
/// and so must not also retain it.
///
/// **Paired with the part that declares it, not matched on type alone.** The
/// same type means different things depending on where it hangs: `/hyperlink`
/// from a worksheet is `Sheet::hyperlinks`, read into the model and re-minted on
/// write, so retaining it as well would write every cell link twice. `/hyperlink`
/// from a **drawing** — the address behind a clickable picture — is modelled
/// nowhere, and matching on the type alone dropped it along with the other
/// (FID-20). The drawing's bytes are retained either way, so the loss showed up
/// as an `<a:hlinkClick r:id>` naming a relationship that no longer existed:
/// data gone from a package that still claimed to reference it, and no entry in
/// the report to say so.
///
/// Anything not listed is retained, which is the safe direction: a relationship
/// kept twice is a bug that is visible in the file, and one dropped is a bug
/// that is visible only to whoever opens it next.
fn is_modelled(kind: PartKind, rel_type: &str) -> bool {
    let modelled: &[&str] = match kind {
        // The root's link to `workbook.xml`. The workbook is read and
        // regenerated, and the writer emits this relationship itself: retaining
        // it would put a stale copy of the workbook beside the fresh one and a
        // second `rId1` in `_rels/.rels`, and the older of the two would win on
        // the next read.
        PartKind::Root => &["/officeDocument"],
        PartKind::Workbook => &[
            "/worksheet",
            "/sharedStrings",
            "/styles",
            "/theme",
            "/calcChain",
            PERSONS_REL_SUFFIX,
        ],
        PartKind::Worksheet => &[
            "/vmlDrawing",
            "/hyperlink",
            "/table",
            COMMENTS_REL_SUFFIX,
            THREADED_COMMENTS_REL_SUFFIX,
        ],
        PartKind::Other => &[],
    };
    modelled.iter().any(|s| rel_type.ends_with(s))
}

/// Resolve a relationship target against the part that declared it.
/// The charts and pictures anchored on a sheet, resolved through its drawing.
///
/// A sheet points at one drawing; the drawing's anchors point at charts through
/// its *own* relationships, so two hops are needed. Anything that fails to
/// resolve is skipped rather than reported: the part is still retained and
/// still written back, so the only cost is a chart that does not appear.
#[allow(clippy::type_complexity)]
fn read_sheet_drawings(
    package: &mut SpreadsheetPackage,
    sheet_part: &str,
) -> Result<
    (
        Vec<casual_calc_model::ChartView>,
        Vec<casual_calc_model::ImageView>,
    ),
    ImportError,
> {
    let limits = *package.limits();
    let Some(drawing_part) = package
        .related_part(sheet_part, DRAWING_REL_SUFFIX, &limits)?
        .filter(|p| package.contains(p))
    else {
        return Ok((Vec::new(), Vec::new()));
    };
    let drawing_xml = package.read_part(&drawing_part)?;
    let anchors = chart::parse_drawing(&drawing_xml)?;
    if anchors.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let rels = package.relationships_of(&drawing_part, &limits)?;

    let mut charts = Vec::new();
    let mut images = Vec::new();
    for anchor in anchors {
        let Some(rel_id) = anchor.rel_id else {
            continue;
        };
        let Some(rel) = rels.iter().find(|r| r.id == rel_id) else {
            continue;
        };
        if rel.external {
            continue;
        }
        let target = resolve_part(&drawing_part, &rel.target);
        if !package.contains(&target) {
            continue;
        }
        if rel.rel_type.ends_with(CHART_REL_SUFFIX) {
            let spec = chart::parse_chart(&package.read_part(&target)?)?;
            charts.push(casual_calc_model::ChartView {
                // Numbered below, once the whole list is known, so the ids
                // follow document order rather than relationship order.
                id: 0,
                anchor: anchor.range,
                from_offset: anchor.from_offset,
                to_offset: anchor.to_offset,
                kind: spec
                    .kind
                    .unwrap_or(casual_calc_model::ChartKind::Unsupported),
                title: spec.title,
                series: spec.series,
                legend: spec.legend,
                x_title: spec.x_title,
                y_title: spec.y_title,
                // The part stays authoritative until the chart is edited: what
                // is read here is a fraction of what it holds.
                part: Some(target),
            });
        } else if rel.rel_type.ends_with(IMAGE_REL_SUFFIX) {
            // Only the path: the bytes are already retained under it, and
            // copying them here would store every picture twice.
            images.push(casual_calc_model::ImageView {
                anchor: anchor.range,
                from_offset: anchor.from_offset,
                to_offset: anchor.to_offset,
                extent: anchor.extent,
                part: target,
            });
        }
    }
    Ok((charts, images))
}

fn resolve_part(source: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_owned();
    }
    let dir = source.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for segment in target.split('/') {
        match segment {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// Fold parsed `<threadedComment>` elements into a sheet's comment list.
///
/// The schema is flat: a root and its replies are siblings sharing a `ref`,
/// linked by `parentId`. Replies are attached in document order, which is the
/// order Excel writes and reads them.
///
/// A cell that already has a legacy note is **replaced**, not appended to: Excel
/// writes both parts for the same thread so that readers predating the 2018
/// schema still see something, and keeping both would show the opening remark
/// twice.
fn merge_threaded_comments(
    sheet: &mut Sheet,
    raw: Vec<RawThreadedComment>,
    persons: &BTreeMap<String, String>,
) {
    let author_of = |person_id: &Option<String>| -> Option<String> {
        person_id
            .as_ref()
            .and_then(|id| persons.get(id))
            .filter(|name| !name.is_empty())
            .cloned()
    };

    // Roots first, so a reply can never arrive before the thread it joins even
    // if a writer emitted them out of order.
    let (roots, replies): (Vec<_>, Vec<_>) = raw.into_iter().partition(|c| c.parent_id.is_none());

    let mut index_of: HashMap<String, usize> = HashMap::new();
    for root in roots {
        let Some(at) = parse_a1(&root.reference) else {
            continue;
        };
        let thread = CellComment {
            at,
            text: root.text,
            author: author_of(&root.person_id),
            created: root.date,
            resolved: root.done,
            replies: Vec::new(),
        };
        // Replace the legacy note for this cell if one was already read.
        match sheet
            .comments
            .iter()
            .position(|c| c.at.row == at.row && c.at.col == at.col)
        {
            Some(i) => {
                sheet.comments[i] = thread;
                index_of.insert(root.id, i);
            }
            None => {
                index_of.insert(root.id, sheet.comments.len());
                sheet.comments.push(thread);
            }
        }
    }

    for reply in replies {
        // A reply whose parent is missing has nowhere to go; dropping it is the
        // only option that does not invent a thread the file never had.
        let Some(&i) = reply.parent_id.as_ref().and_then(|p| index_of.get(p)) else {
            continue;
        };
        sheet.comments[i].replies.push(CommentReply {
            text: reply.text,
            author: author_of(&reply.person_id),
            created: reply.date,
        });
    }
}

/// Import a SpreadsheetML package into the normalized model.
/// What this document has spent, against what one document may spend.
///
/// One of these per import, never per part — which is the whole point. Each
/// per-part limit was already enforced and each was passing; nothing added them
/// up, so a package of many parts multiplied a bound nobody had agreed to
/// (`SEC-002`).
///
/// Every method fails closed on the way past the line, before the thing is
/// stored, so the model is never left holding a fraction of a document.
struct Budget {
    limits: casual_calc_ooxml::SpreadsheetLimits,
    cells: usize,
    merges: usize,
}

impl Budget {
    fn new(limits: casual_calc_ooxml::SpreadsheetLimits) -> Self {
        Self {
            limits,
            cells: 0,
            merges: 0,
        }
    }

    fn cell(&mut self) -> Result<(), ImportError> {
        self.cells += 1;
        if self.cells > self.limits.max_populated_cells {
            return Err(ImportError::OverBudget {
                what: crate::Overrun::PopulatedCells,
                limit: self.limits.max_populated_cells,
            });
        }
        Ok(())
    }

    fn merge(&mut self) -> Result<(), ImportError> {
        self.merges += 1;
        if self.merges > self.limits.max_merged_ranges {
            return Err(ImportError::OverBudget {
                what: crate::Overrun::MergedRanges,
                limit: self.limits.max_merged_ranges,
            });
        }
        Ok(())
    }

    /// Checked before interning, because the table's size is known up front and
    /// there is no reason to build it first.
    fn shared_strings(&self, count: usize) -> Result<(), ImportError> {
        if count > self.limits.max_shared_strings {
            return Err(ImportError::OverBudget {
                what: crate::Overrun::SharedStrings,
                limit: self.limits.max_shared_strings,
            });
        }
        Ok(())
    }

    fn defined_names(&self, count: usize) -> Result<(), ImportError> {
        if count > self.limits.max_defined_names {
            return Err(ImportError::OverBudget {
                what: crate::Overrun::DefinedNames,
                limit: self.limits.max_defined_names,
            });
        }
        Ok(())
    }
}

pub fn import_package(bytes: Vec<u8>) -> Result<Import, ImportError> {
    import_package_with(bytes, OoxmlLimits::default())
}

/// Import a package under caller-supplied admission limits.
///
/// The limits are a **security bound**, not a tuning knob: they cap what an
/// untrusted file can make this allocate before it is rejected. A desktop host
/// opening a file its user chose can afford the defaults; a service admitting
/// uploads cannot, and had no way to say so — every caller got the same numbers
/// because they were written into this function.
pub fn import_package_with(bytes: Vec<u8>, limits: OoxmlLimits) -> Result<Import, ImportError> {
    import_package_cancellable(bytes, limits, &casual_calc_model::Never)
}

/// The same, with a way to stop it.
///
/// Admission is the longest thing this crate does and, until `SEC-012`, the
/// only way out of it was for it to finish. docs/07 and docs/21 both promised
/// otherwise. `cancel` is asked periodically — see
/// [`CANCEL_CHECK_INTERVAL`](casual_calc_model::CANCEL_CHECK_INTERVAL) — and a
/// cancelled import returns [`ImportError::Cancelled`] having built nothing,
/// which is the same fail-closed rule the limits follow: a half-admitted
/// workbook is one that gets saved back over the original.
///
/// # Errors
///
/// As [`import_package_with`], plus [`ImportError::Cancelled`].
pub fn import_package_cancellable(
    bytes: Vec<u8>,
    limits: OoxmlLimits,
    cancel: &dyn casual_calc_model::Cancel,
) -> Result<Import, ImportError> {
    let mut package = SpreadsheetPackage::open(bytes, limits)?;
    let mut report = CompatibilityReport::default();
    let mut workbook = Workbook::new(Id::from_parts(WORKBOOK_NAMESPACE, 1));
    let mut budget = Budget::new(limits.spreadsheet);

    // Shared strings → interned into the workbook, keeping index → StringId.
    let mut shared_ids: Vec<StringId> = Vec::new();
    if package.contains(SHARED_STRINGS_PART) {
        let xml = package.read_part(SHARED_STRINGS_PART)?;
        let values = parse_shared_strings(&xml)?;
        budget.shared_strings(values.len())?;
        for value in values {
            shared_ids.push(workbook.intern_rich_text(value));
        }
    }

    // Styles: the number-format code per cellXfs index. Pre-intern every xf in
    // order so the style-table order is canonical (cellXfs order) — this is what
    // lets the writer round-trip styles deterministically.
    // The theme palette must be read first: `styles.xml` states most colors as
    // a theme slot plus a tint, and those are exactly the colors Excel's
    // built-in cell styles use.
    let palette = if package.contains(THEME_PART) {
        let xml = package.read_part(THEME_PART)?;
        parse_theme(&xml)?
    } else {
        ThemePalette::default()
    };
    let stylesheet = if package.contains(STYLES_PART) {
        let xml = package.read_part(STYLES_PART)?;
        parse_styles(&xml, &palette)?
    } else {
        StyleSheet::default()
    };
    // Keep the theme itself, not just its resolved colours: a host offering a
    // colour picker should offer *this file's* theme, and the writer will need
    // it once theme linkage round-trips.
    workbook.theme_colors = palette.slots().to_vec();
    workbook.default_font_name = stylesheet.default_font_name.clone();
    workbook.default_font_size_hp = stylesheet.default_font_size_hp;
    workbook.cell_styles = stylesheet.cell_styles.clone();
    let xf_style_ids: Vec<Option<_>> = stylesheet
        .xf_styles
        .iter()
        .enumerate()
        .map(|(i, style)| {
            let mut style = style.clone();
            // Carry the named-style association. `xfId` 0 is Normal, which every
            // cell points at by default and which says nothing, so only a
            // non-zero link is worth keeping — otherwise every plain cell would
            // become "styled" and stop deduplicating.
            style.style_ref = stylesheet
                .xf_style_refs
                .get(i)
                .copied()
                .flatten()
                .filter(|id| *id != 0);
            if style.is_default() {
                None
            } else {
                Some(workbook.intern_style(style))
            }
        })
        .collect();

    // Own the sheet metadata so the package can be mutated (read) while looping.
    let sheet_meta: Vec<(String, String, String)> = package
        .sheets()
        .iter()
        .map(|s| (s.name.clone(), s.part.clone(), s.state.clone()))
        .collect();

    // The persons part is workbook-level and shared by every sheet's threads,
    // so it is read once here rather than per sheet.
    let workbook_part_name = package.workbook_part().to_owned();
    // Cloned rather than borrowed: `related_part` takes `&mut self`, and the
    // limits live on the same package.
    let limits = *package.limits();
    let persons: BTreeMap<String, String> = match package
        .related_part(&workbook_part_name, PERSONS_REL_SUFFIX, &limits)?
        .filter(|p| package.contains(p))
    {
        Some(part) => {
            let pxml = package.read_part(&part)?;
            parse_persons(&pxml)?.into_iter().collect()
        }
        None => BTreeMap::new(),
    };

    let mut sheet_ids = IdGenerator::new(SHEET_NAMESPACE);
    let mut sheet_ids_by_index: Vec<SheetId> = Vec::new();
    for (name, part, state) in sheet_meta {
        let xml = package.read_part(&part)?;
        let worksheet = parse_worksheet(&xml, &palette)?;
        let sheet_id = SheetId(sheet_ids.next_id());
        sheet_ids_by_index.push(sheet_id);
        let mut sheet = Sheet::new(sheet_id, name);
        // A hidden sheet that comes back visible exposes data its author put
        // away on purpose, so the state travels with the sheet.
        sheet.visibility = SheetVisibility::from_ooxml(&state);
        sheet.print = worksheet.print.clone();
        sheet.sort_state = worksheet.sort_state.clone();
        // Verbatim elements travel with their attributes, and several of them
        // carry an address: `<dimension ref>`, `<selection sqref activeCell>`,
        // `<ignoredError sqref>`, `<protectedRange sqref>`. The writer re-emits
        // them unchanged, so `<dimension ref="A1:ZZZZ4294967295"/>` walks past
        // every parsed path above and straight out into the saved file.
        // "Carried verbatim" cannot mean "exempt from the grid" when verbatim is
        // what gets written — an element addressing a cell that does not exist
        // is dropped and named, like every other route out (FID-18).
        sheet.carried = worksheet
            .carried
            .iter()
            .filter(|(name, attrs)| {
                let ok = carried_within_grid(attrs);
                if !ok {
                    report.record(
                        &format!("{name}/outOfGrid"),
                        ModelOutcome::Omitted,
                        RetentionOutcome::NotRetained,
                    );
                }
                ok
            })
            .cloned()
            .collect();
        sheet.format_pr = worksheet.format_pr.clone();
        sheet.view.right_to_left = worksheet.right_to_left;
        sheet.view.show_formulas = worksheet.show_formulas;
        sheet.view.hide_zeros = worksheet.hide_zeros;
        sheet.view.tab_selected = worksheet.tab_selected;
        sheet.retained_refs = worksheet.retained_refs.clone();
        sheet.protection = worksheet
            .protection
            .clone()
            .map(|attrs| casual_calc_model::SheetProtection { attrs });

        // Shared formulas: Excel's fill-down writes the expression once, on the
        // group's master cell, and leaves every follower's `<f>` empty. Without
        // expanding them a filled column imports as one formula plus a stack of
        // cached constants — the formulas are simply gone. Collect the masters
        // first (document order puts them before their followers, but a
        // pre-pass keeps that from being load-bearing).
        let mut shared_masters: HashMap<u32, (CellRef, Expr)> = HashMap::new();
        for raw in &worksheet.cells {
            let (Some(si), Some(text)) = (raw.shared_index, raw.formula.as_deref()) else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            if let Some(at) = parse_a1(&raw.reference)
                && let Ok(expr) = parse_formula(text)
            {
                shared_masters.entry(si).or_insert((at, expr));
            }
        }

        for raw in worksheet.cells {
            let parsed = parse_a1_classified(&raw.reference);
            let Some(cell_ref) = parsed.ok() else {
                // Two distinct features, because they are two distinct events: a
                // reference this parser could not read, and a reference naming a
                // cell that does not exist. The second one used to be admitted
                // and written back, which is how a `<v>7</v>` at row
                // 4,294,967,295 became a package Excel refuses to open.
                report.record(
                    if parsed.is_out_of_grid() {
                        "cellRef/outOfGrid"
                    } else {
                        "cellRef"
                    },
                    ModelOutcome::Omitted,
                    RetentionOutcome::NotRetained,
                );
                continue;
            };
            let value = map_value(&raw, &shared_ids, &mut workbook, &mut report);
            let mut cell = Cell::value(value);
            if let Some(index) = raw.style_index
                && let Some(Some(style_id)) = xf_style_ids.get(index as usize)
            {
                cell.style = Some(*style_id);
            }
            match raw.formula.as_deref() {
                Some(text) if !text.trim().is_empty() => match parse_formula(text) {
                    // A formula addressing a cell that does not exist keeps its
                    // cached value and loses the expression — the same trade the
                    // unparseable-formula arm below makes, for the same reason:
                    // what the file last computed is real, and re-emitting the
                    // reference is what corrupts the package.
                    Ok(expr) if !expr_within_grid(&expr, ABSOLUTE) => {
                        report.record(
                            "f/outOfGrid",
                            ModelOutcome::Omitted,
                            RetentionOutcome::NotRetained,
                        );
                    }
                    Ok(expr) => {
                        cell.formula = Some(
                            workbook.store_formula_at(expr, Origin::at(cell_ref.row, cell_ref.col)),
                        );
                        report.record("f", ModelOutcome::Mapped, RetentionOutcome::NotApplicable);
                    }
                    Err(_) => {
                        // Cached value kept; the formula text did not parse.
                        report.record("f", ModelOutcome::Degraded, RetentionOutcome::NotRetained);
                    }
                },
                // An empty `<f>` is a shared-formula follower: it takes its
                // master's tree unchanged.
                Some(_) => {
                    // **No shift.** A shared formula's master, stored against
                    // the master's own cell, is already what every follower
                    // needs: its references are offsets and the follower's own
                    // origin applies them. That is what `<f t="shared">` means
                    // in the file, and since `PERF-11` it is what the model
                    // means too — so a whole shared group lands on *one* tree.
                    let rebuilt = raw.shared_index.and_then(|si| shared_masters.get(&si)).map(
                        |(at, expr)| {
                            casual_calc_formula::restore_at(
                                expr,
                                ABSOLUTE,
                                Origin::at(at.row, at.col),
                            )
                        },
                    );
                    // The shift is where a follower can leave the grid on its
                    // own: the master's references are in range and the delta
                    // carries them out, so the check belongs after the rebuild,
                    // not on the master. Either way the follower keeps its
                    // cached value and is reported `Degraded` below.
                    match rebuilt
                        .filter(|e| expr_within_grid(e, Origin::at(cell_ref.row, cell_ref.col)))
                    {
                        Some(expr) => {
                            cell.formula = Some(workbook.store_formula(expr));
                            report.record(
                                "f",
                                ModelOutcome::Mapped,
                                RetentionOutcome::NotApplicable,
                            );
                        }
                        None => {
                            report.record(
                                "f",
                                ModelOutcome::Degraded,
                                RetentionOutcome::NotRetained,
                            );
                        }
                    }
                }
                None => {}
            }
            if !cell.is_blank() {
                // Across every sheet, not this one: the sum is the thing being
                // bounded, and it is charged before the cell is stored.
                budget.cell()?;
                // The one loop long enough to need stopping. Asking here rather
                // than per part is the difference between cancelling a workbook
                // and cancelling a workbook with one enormous sheet in it.
                if casual_calc_model::should_check(budget.cells) && cancel.cancelled() {
                    return Err(ImportError::Cancelled);
                }
                sheet.cells.set(cell_ref, cell);
            }
        }

        for reference in &worksheet.merges {
            // A merge is refused whole when either corner is out of the grid.
            // `A1:ZZZZ4294967295` is 475,254 columns by 4 billion rows: the
            // layout walked it, and the writer emitted it verbatim.
            match parse_range_classified(reference) {
                Parsed::Ok(range) => {
                    budget.merge()?;
                    sheet.merges.push(range);
                }
                Parsed::OutOfGrid => report.record(
                    "mergeCell/outOfGrid",
                    ModelOutcome::Omitted,
                    RetentionOutcome::NotRetained,
                ),
                Parsed::Malformed => report.record(
                    "mergeCell",
                    ModelOutcome::Omitted,
                    RetentionOutcome::NotRetained,
                ),
            }
        }
        // Row and column spans the reader clipped or dropped at the grid edge
        // (`read_col` / `read_row`). They are counted there and named here,
        // because the reader has no report to write to — but an axis that
        // silently stopped existing is exactly the silence docs/34 forbids.
        for _ in 0..worksheet.out_of_grid_cols {
            report.record(
                "col/outOfGrid",
                ModelOutcome::Omitted,
                RetentionOutcome::NotRetained,
            );
        }
        for _ in 0..worksheet.out_of_grid_rows {
            report.record(
                "row/outOfGrid",
                ModelOutcome::Omitted,
                RetentionOutcome::NotRetained,
            );
        }
        if let Some((frozen_rows, frozen_cols)) = worksheet.frozen {
            sheet.view.frozen_rows = frozen_rows;
            sheet.view.frozen_cols = frozen_cols;
        }
        if let Some(zoom) = worksheet.zoom {
            sheet.view.zoom = zoom;
        }
        sheet.view.hide_gridlines = worksheet.hide_gridlines;
        sheet.view.hide_headers = worksheet.hide_headers;
        sheet.columns.default = worksheet.col_default;
        sheet.columns.sizes = worksheet.col_sizes;
        sheet.rows.default = worksheet.row_default;
        sheet.rows.sizes = worksheet.row_sizes;
        sheet.hidden_rows = worksheet.hidden_rows;
        sheet.hidden_cols = worksheet.hidden_cols;
        sheet.row_outline_levels = worksheet.row_outline_levels;
        sheet.col_outline_levels = worksheet.col_outline_levels;
        sheet.collapsed_rows = worksheet.collapsed_rows;
        sheet.collapsed_cols = worksheet.collapsed_cols;
        if let Some(outline) = worksheet.outline {
            sheet.outline = outline;
        }
        sheet.tab_color = worksheet.tab_color;

        // Autofilter. The rows it hides arrive as ordinary `hidden="1"` rows —
        // OOXML has no separate marker — so they land in `hidden_rows` here and
        // the session re-derives `filter_hidden` from the rules once formatting
        // is available (display text is what a checklist matches on).
        if let Some(reference) = worksheet.auto_filter.as_deref() {
            note_out_of_grid(
                &mut report,
                "autoFilter/outOfGrid",
                &parse_range_classified(reference),
            );
            sheet.auto_filter = build_auto_filter(reference, worksheet.filter_columns);
        }

        // Data validations, every kind. Only a `list` rule's inline quoted CSV is
        // expanded into values; the other kinds keep their operands as the raw
        // formula text, which is what they are.
        for raw in worksheet.validations {
            let kind = DvKind::from_ooxml(&raw.kind);
            let trimmed = raw.formula1.trim();
            let values: Vec<String> = if kind == DvKind::List
                && trimmed.len() >= 2
                && trimmed.starts_with('"')
                && trimmed.ends_with('"')
            {
                trimmed[1..trimmed.len() - 1]
                    .split(',')
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect()
            } else {
                Vec::new()
            };
            // A list whose values are a range reference rather than an inline
            // CSV keeps its formula, so the rule survives even though the editor
            // cannot offer the dropdown yet.
            if kind == DvKind::None && values.is_empty() && raw.formula1.trim().is_empty() {
                continue;
            }
            // An sqref is a space-separated list of areas; taking only the first
            // silently dropped the validation from every other area it covers.
            // Bounded, because each area copies the value list: a hand-written
            // sqref with tens of thousands of areas would otherwise turn a small
            // part into millions of heap strings.
            for area in raw.sqref.split_whitespace().take(MAX_SQREF_AREAS) {
                let parsed = parse_range_classified(area);
                note_out_of_grid(&mut report, "dataValidation/outOfGrid", &parsed);
                if let Some(range) = parsed.ok() {
                    sheet.validations.push(DataValidation {
                        values: values.clone(),
                        kind,
                        operator: DvOperator::from_ooxml(&raw.operator),
                        formula1: raw.formula1.clone(),
                        formula2: raw.formula2.clone(),
                        allow_blank: raw.allow_blank,
                        error_style: raw.error_style.clone(),
                        hide_dropdown: raw.hide_dropdown,
                        ime_mode: raw.ime_mode.clone(),
                        error_title: raw.error_title.clone(),
                        error_text: raw.error_text.clone(),
                        prompt_title: raw.prompt_title.clone(),
                        prompt_text: raw.prompt_text.clone(),
                        ..DataValidation::none(range)
                    });
                }
            }
            let sqref = raw.sqref;
            if sqref.split_whitespace().count() > MAX_SQREF_AREAS {
                report.record(
                    "dataValidation/sqref",
                    ModelOutcome::Degraded,
                    RetentionOutcome::NotRetained,
                );
            }
        }

        // Conditional formatting: resolve each cfRule's fill via its dxfId, its
        // range via the sqref, and its predicate via type/operator/formulas.
        // Rules without a solid fill (the only kind modeled) are skipped.
        for raw in worksheet.conditional_formats {
            // Colour scales and data bars carry their own colours and have no
            // dxfId, so the fill lookup must not gate them out.
            let scale_or_bar = matches!(raw.kind.as_str(), "colorScale" | "dataBar");
            let fill = match raw
                .dxf_id
                .and_then(|id| stylesheet.dxf_fills.get(id).cloned().flatten())
            {
                Some(f) => f,
                None if scale_or_bar => String::new(),
                None => continue,
            };
            let num = |i: usize| {
                raw.formulas
                    .get(i)
                    .and_then(|s| s.trim().parse::<f64>().ok())
            };
            let rule = match (raw.kind.as_str(), raw.operator.as_str()) {
                ("cellIs", "greaterThan") => num(0).map(CfRule::GreaterThan),
                ("cellIs", "lessThan") => num(0).map(CfRule::LessThan),
                ("cellIs", "equal") => num(0).map(CfRule::EqualTo),
                ("cellIs", "between") => match (num(0), num(1)) {
                    (Some(a), Some(b)) => Some(CfRule::Between(a, b)),
                    _ => None,
                },
                ("containsText", _) => raw.text.clone().map(CfRule::TextContains),
                ("colorScale", _) if raw.colors.len() >= 2 => {
                    Some(CfRule::ColorScale(raw.colors.clone()))
                }
                ("dataBar", _) => raw.colors.first().cloned().map(CfRule::DataBar),
                ("top10", _) => Some(CfRule::Top10 {
                    // A rank of zero would select nothing; Excel's minimum is 1.
                    rank: raw.rank.max(1),
                    bottom: raw.bottom,
                    percent: raw.percent,
                }),
                ("aboveAverage", _) => Some(CfRule::AboveAverage {
                    below: !raw.above_average,
                    equal: raw.equal_average,
                }),
                ("duplicateValues", _) => Some(CfRule::DuplicateValues { unique: false }),
                ("uniqueValues", _) => Some(CfRule::DuplicateValues { unique: true }),
                _ => None,
            };
            if let Some(rule) = rule {
                // One rule per area of the sqref — a cfRule covering "A1:A9 C1:C9"
                // used to apply to the first area only. Bounded as above.
                for area in raw.sqref.split_whitespace().take(MAX_SQREF_AREAS) {
                    let parsed = parse_range_classified(area);
                    note_out_of_grid(&mut report, "conditionalFormatting/outOfGrid", &parsed);
                    let Some(range) = parsed.ok() else {
                        continue;
                    };
                    sheet.conditional_formats.push(ConditionalFormat {
                        range,
                        rule: rule.clone(),
                        fill: fill.clone(),
                        font_color: None,
                        bold: false,
                        priority: raw.priority,
                        stop_if_true: raw.stop_if_true,
                    });
                }
            }
        }

        // Cell comments: follow the sheet's own relationships. Guessing
        // `xl/comments{index+1}.xml` only agrees with files this writer
        // produced — in anyone else's package the numbering follows which
        // sheets *have* comments, so sheet 2's notes landed on sheet 1 (or on
        // no sheet at all).
        // (A comments part is only reachable through a relationship, so a sheet
        // without one simply has no notes — there is nothing to fall back to.)
        let comments_part = package
            .related_part(&part, COMMENTS_REL_SUFFIX, &limits)?
            .filter(|p| package.contains(p));
        if let Some(comments_part) = comments_part {
            let cxml = package.read_part(&comments_part)?;
            for (reference, author, text) in parse_comments(&cxml)? {
                if text.is_empty() {
                    continue;
                }
                let parsed = parse_a1_classified(&reference);
                note_out_of_grid(&mut report, "comment/outOfGrid", &parsed);
                if let Some(at) = parsed.ok() {
                    sheet.comments.push(CellComment::note(at, text, author));
                }
            }
        }

        // Hyperlinks: the `ref` and `location` are in the worksheet, but an
        // external target lives in the sheet's relationships and is reachable
        // only through the link's own `r:id`. Resolving the two here keeps the
        // model free of relationship ids, which are a packaging detail with no
        // meaning once the file is open.
        if !worksheet.hyperlinks.is_empty() {
            let rels = package.relationships_of(&part, &limits)?;
            for raw in &worksheet.hyperlinks {
                let parsed = parse_range_classified(&raw.reference);
                note_out_of_grid(&mut report, "hyperlink/outOfGrid", &parsed);
                let Some(range) = parsed.ok() else {
                    continue;
                };
                let target = raw.rel_id.as_ref().and_then(|id| {
                    rels.iter()
                        .find(|r| &r.id == id)
                        // Only an external relationship is a link destination;
                        // an internal one points at a part in the package,
                        // which is not something to navigate to.
                        .filter(|r| r.external)
                        .map(|r| r.target.clone())
                });
                if target.is_none() && raw.location.is_none() {
                    // Neither destination resolved, so there is nothing to link
                    // to; keeping it would render as a dead link.
                    continue;
                }
                sheet.hyperlinks.push(casual_calc_model::Hyperlink {
                    range,
                    target,
                    location: raw.location.clone(),
                    tooltip: raw.tooltip.clone(),
                    display: raw.display.clone(),
                });
            }
        }

        // Tables (ListObjects). Each `<tablePart r:id>` in the worksheet names
        // a part through the sheet's own relationships, the same indirection
        // comments use — guessing `xl/tables/table{n}.xml` would bind sheet 2's
        // table to sheet 1 in anyone else's package.
        let table_ids = parse_table_parts(&xml)?;
        if !table_ids.is_empty() {
            let rels = package.relationships_of(&part, &limits)?;
            for id in table_ids {
                let Some(rel) = rels.iter().find(|r| r.id == id) else {
                    continue;
                };
                let target = resolve_part(&part, &rel.target);
                if !package.contains(&target) {
                    continue;
                }
                let txml = package.read_part(&target)?;
                let Some(raw) = parse_table(&txml)? else {
                    continue;
                };
                let parsed = raw
                    .attrs
                    .get("ref")
                    .map_or(Parsed::Malformed, |r| parse_range_classified(r));
                note_out_of_grid(&mut report, "table/outOfGrid", &parsed);
                let Some(range) = parsed.ok() else {
                    continue;
                };
                let attr_u32 = |k: &str, default: u32| {
                    raw.attrs
                        .get(k)
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(default)
                };
                let name = raw.attrs.get("name").cloned().unwrap_or_default();
                let mut attrs = raw.attrs.clone();
                // The interpreted attributes are removed from the verbatim map,
                // or they would be written twice — once from the field and once
                // from the map, with the stale copy winning on the next read.
                for key in [
                    "ref",
                    "name",
                    "displayName",
                    "id",
                    "headerRowCount",
                    "totalsRowCount",
                ] {
                    attrs.remove(key);
                }
                sheet.tables.push(casual_calc_model::Table {
                    id: attr_u32("id", 1),
                    display_name: raw
                        .attrs
                        .get("displayName")
                        .cloned()
                        .unwrap_or_else(|| name.clone()),
                    name,
                    range,
                    header_row_count: attr_u32("headerRowCount", 1),
                    totals_row_count: attr_u32("totalsRowCount", 0),
                    columns: raw.columns,
                    auto_filter: raw
                        .auto_filter_ref
                        .as_deref()
                        .and_then(|r| build_auto_filter(r, raw.filter_columns)),
                    style: raw.style,
                    attrs,
                });
            }
        }

        // Threaded comments (the 2018 parts) carry the timestamps, the replies
        // and the resolved flag. Excel writes a legacy note alongside them for
        // readers that predate the schema, so a cell can appear in both parts —
        // the threaded one is the fuller record and replaces what the legacy
        // pass just read, rather than adding a duplicate beside it.
        let threaded_part = package
            .related_part(&part, THREADED_COMMENTS_REL_SUFFIX, &limits)?
            .filter(|p| package.contains(p));
        if let Some(threaded_part) = threaded_part {
            let txml = package.read_part(&threaded_part)?;
            let raw = parse_threaded_comments(&txml)?;
            merge_threaded_comments(&mut sheet, raw, &persons);
        }

        // Charts, read for display only. The parts stay retained and are
        // written back from their own bytes; this is the projection a renderer
        // needs, so a chart it cannot decode simply does not draw.
        let drawn = read_sheet_drawings(&mut package, &part)?;
        sheet.charts = drawn.0;
        sheet.images = drawn.1;
        // Identity, in document order, so opening the same file twice — or on
        // two machines — numbers the charts identically. Anything that refers
        // to a chart by index instead stops meaning the same chart as soon as
        // one is inserted before it.
        sheet.assign_chart_ids();

        workbook.sheets.push(sheet);
    }

    // Retention: keep every part the semantic reader did not consume, plus the
    // relationship that reaches it and the reference that names it. This is what
    // separates "we do not model charts" from "we delete charts" — a workbook
    // people already have work in must survive a save even where we understand
    // nothing about the feature.
    let sheet_parts: Vec<String> = package.sheets().iter().map(|s| s.part.clone()).collect();
    retain_unmodelled(&mut package, &mut workbook, &sheet_parts, &mut report)?;

    // Defined names, resolved against the sheet ids assigned above.
    let workbook_part = package.workbook_part().to_owned();
    let workbook_xml = package.read_part(&workbook_part)?;
    // The date epoch has to be known before any date is displayed, and has to
    // be written back or the serials silently change meaning.
    workbook.date1904 = parse_date1904(&workbook_xml)?;
    workbook.settings = parse_workbook_settings(&workbook_xml)?;
    workbook.retained_refs = parse_retained_refs(&workbook_xml)?;
    let names = parse_defined_names(&workbook_xml)?;
    budget.defined_names(names.len())?;
    for (name, local_sheet, refers_to) in names {
        // A target this parser cannot read is kept verbatim rather than
        // dropped. Discarding it lost the name from the file entirely, and the
        // commonest casualty was `Print_Titles`, whose value is a whole-row
        // reference (`Sheet1!$1:$2`) that the parser does not support — so
        // every workbook with repeating print titles lost them on save.
        let (formula, outcome) = match parse_formula(&refers_to) {
            // A name whose target is outside the grid is dropped rather than
            // kept verbatim. `Expr::Raw` is the right answer for a target this
            // parser cannot *read* — it prints back unchanged and the file
            // survives — and the wrong one here, because printing it back
            // unchanged is precisely what writes the unopenable package.
            Ok(formula) if !expr_within_grid(&formula, ABSOLUTE) => {
                report.record(
                    "definedName/outOfGrid",
                    ModelOutcome::Omitted,
                    RetentionOutcome::NotRetained,
                );
                continue;
            }
            Ok(formula) => (
                formula,
                (ModelOutcome::Mapped, RetentionOutcome::NotApplicable),
            ),
            Err(_) => (
                Expr::Raw(refers_to.clone()),
                (ModelOutcome::Degraded, RetentionOutcome::Preserved),
            ),
        };
        let sheet = local_sheet.and_then(|i| sheet_ids_by_index.get(i as usize).copied());
        workbook.defined_names.push(DefinedName {
            name,
            sheet,
            formula,
        });
        report.record("definedName", outcome.0, outcome.1);
    }

    // Pivots last: resolving one needs the sheet ids, and a cache whose source
    // is a table needs that table already read.
    read_pivots(&mut package, &mut workbook, &mut report)?;

    workbook.validate()?;
    Ok(Import { workbook, report })
}

fn map_value(
    raw: &RawCell,
    shared: &[StringId],
    workbook: &mut Workbook,
    report: &mut CompatibilityReport,
) -> CellValue {
    match raw.cell_type.as_deref() {
        None | Some("n") => raw
            .value
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok())
            .map(CellValue::Number)
            .unwrap_or(CellValue::Empty),
        Some("b") => CellValue::Bool(raw.value.as_deref() == Some("1")),
        Some("s") => match raw
            .value
            .as_deref()
            .and_then(|s| s.parse::<usize>().ok())
            .and_then(|i| shared.get(i).copied())
        {
            Some(id) => CellValue::SharedString(id),
            None => {
                report.record("s", ModelOutcome::Omitted, RetentionOutcome::NotRetained);
                CellValue::Empty
            }
        },
        Some("str") => raw
            .value
            .as_deref()
            .map(|s| CellValue::InlineString(workbook.intern_string(s)))
            .unwrap_or(CellValue::Empty),
        Some("inlineStr") => raw
            .inline
            .as_deref()
            .map(|s| CellValue::InlineString(workbook.intern_string(s)))
            .unwrap_or(CellValue::Empty),
        Some("e") => match raw.value.as_deref().and_then(parse_error) {
            Some(error) => CellValue::Error(error),
            None => {
                report.record("e", ModelOutcome::Omitted, RetentionOutcome::NotRetained);
                CellValue::Empty
            }
        },
        Some(other) => {
            report.record(other, ModelOutcome::Omitted, RetentionOutcome::NotRetained);
            CellValue::Empty
        }
    }
}

fn parse_error(token: &str) -> Option<ErrorValue> {
    Some(match token {
        "#REF!" => ErrorValue::Ref,
        "#VALUE!" => ErrorValue::Value,
        "#DIV/0!" => ErrorValue::Div0,
        "#N/A" => ErrorValue::Na,
        "#NAME?" => ErrorValue::Name,
        "#NULL!" => ErrorValue::Null,
        "#NUM!" => ErrorValue::Num,
        "#SPILL!" => ErrorValue::Spill,
        "#CALC!" => ErrorValue::Calc,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;

/// Build an [`AutoFilter`] from a raw `ref` string and its `<filterColumn>`s.
///
/// Shared by the worksheet and the table: a table carries its own filter, with
/// the same element and the same rules, and building it in two places invited
/// the two to drift.
fn build_auto_filter(reference: &str, columns: Vec<read::RawFilterColumn>) -> Option<AutoFilter> {
    let range = a1::parse_range(reference)?;
    let mut filter = AutoFilter::new(range);
    for fc in columns {
        // A refinement takes precedence: a filterColumn holding one has
        // no <filters> or <customFilters> to read instead.
        let rule = if let Some((element, attrs)) = fc.unevaluated.clone() {
            FilterRule::Unevaluated { element, attrs }
        } else if fc.saw_filters {
            let mut values = fc.values;
            if fc.blank {
                // `blank="1"` is the checklist's "(Blanks)" entry, which
                // the model carries as the empty string.
                values.push(String::new());
            }
            // An empty checklist would select nothing at all; Excel does
            // not write one, and honouring it would blank the sheet.
            if values.is_empty() {
                continue;
            }
            FilterRule::Values(values)
        } else {
            let mut ops = fc.custom.into_iter().map(|(op, value)| CustomFilter {
                op: FilterOp::from_ooxml(&op),
                value,
            });
            let Some(first) = ops.next() else {
                continue; // a filterColumn with neither kind of child
            };
            FilterRule::Custom {
                first,
                second: ops.next(),
                and: fc.custom_and,
            }
        };
        filter.rules.insert(fc.col_id, rule);
    }
    Some(filter)
}
