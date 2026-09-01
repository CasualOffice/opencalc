//! Writing a chart that was made here.
//!
//! Only charts with no retained part reach this module. One that came from a
//! file is written back from its own bytes, because a chart part carries
//! hundreds of formatting elements and the model holds a handful; see
//! [`casual_calc_model::ChartView`].
//!
//! Two parts and three references. `xl/charts/chartN.xml` says what is plotted,
//! `xl/drawings/drawingN.xml` says where it sits, the drawing's rels name the
//! chart, the worksheet's rels name the drawing, and `<drawing r:id>` inside
//! the worksheet names that relationship. Miss any one and Excel reports a file
//! needing repair rather than a missing chart.
//!
//! **A worksheet may have only one drawing part.** So a sheet that already
//! carries a retained drawing — holding charts, pictures, text boxes, form
//! controls, anything — cannot simply be given a second one for the charts made
//! here. The retained drawing's bytes are therefore *spliced*: our anchors are
//! inserted before its closing tag and everything else travels untouched. The
//! alternative was to rebuild the drawing from the model, which would silently
//! delete every shape and text box the model does not know about.

use std::collections::BTreeMap;

use casual_calc_model::{ChartGrouping, ChartKind, ChartSeries, ChartView, Sheet, Workbook};

// Shared with the other `.rels` this crate writes, so that the one rule about
// `TargetMode` holds everywhere: a drawing hangs external relationships too —
// the web address behind a clickable picture.
use crate::retained_rel_xml as rel_xml;
use crate::xml::{escape_attr, escape_text};

const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";
const NS_C: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_XDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// Content type for a chart part.
pub const CHART_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
/// Content type for a drawing part.
pub const DRAWING_CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.drawing+xml";

/// Axis ids. Any two distinct numbers do, but they must match between the chart
/// group and the axis definitions or Excel cannot pair them.
const CAT_AX_ID: u32 = 111_111_111;
const VAL_AX_ID: u32 = 222_222_222;
/// The secondary pair, for the groups holding
/// [`ChartSeries::secondary_axis`](casual_calc_model::ChartSeries::secondary_axis)
/// series. A secondary axis in OOXML is a *second `<c:axId>` pair*, not a flag
/// on the first — so it needs two more ids and two more axis definitions, and
/// the category one is written deleted because a chart shows one set of
/// categories however many value axes it has.
const CAT2_AX_ID: u32 = 333_333_333;
const VAL2_AX_ID: u32 = 444_444_444;

/// Everything one sheet contributes to the package for its authored charts.
pub struct SheetCharts {
    /// The drawing part's path.
    pub drawing_part: String,
    /// The drawing XML, either fresh or the retained bytes with our anchors
    /// spliced in.
    pub drawing_xml: String,
    /// `(path, xml)` for each chart part.
    pub chart_parts: Vec<(String, String)>,
    /// The drawing's `.rels`, retained entries included.
    pub drawing_rels: String,
    /// The relationship id the worksheet's `<drawing>` element must name, when
    /// the sheet did not already have one.
    pub sheet_rel: Option<(String, String)>,
    /// Whether the drawing part is new, and so needs a content-type override.
    pub drawing_is_new: bool,
}

impl SheetCharts {
    /// The placeholder for a sheet with no authored chart, so the per-sheet
    /// vector stays index-aligned with `workbook.sheets`.
    pub fn none() -> Self {
        Self {
            drawing_part: String::new(),
            drawing_xml: String::new(),
            chart_parts: Vec::new(),
            drawing_rels: String::new(),
            sheet_rel: None,
            drawing_is_new: false,
        }
    }
}

/// The charts on a sheet that this module writes: the ones with no retained
/// part behind them.
pub fn authored(sheet: &Sheet) -> Vec<&ChartView> {
    sheet.charts.iter().filter(|c| c.part.is_none()).collect()
}

/// The drawing part a sheet already points at, if any.
///
/// An external one is not a candidate: `resolve_rel_target` would turn its URI
/// into a path, and the splice below would then look for bytes under a part
/// name no package contains.
fn retained_drawing(workbook: &Workbook, sheet_part: &str) -> Option<(String, String)> {
    workbook
        .retained_rels
        .iter()
        .find(|r| r.source == sheet_part && !r.external && r.rel_type.ends_with("/drawing"))
        .map(|r| (resolve_rel_target(&r.source, &r.target), r.id.clone()))
}

/// Resolve a relationship target against the part that declared it.
///
/// A worksheet's `../drawings/drawing1.xml` is `xl/drawings/drawing1.xml`.
/// Comparing raw targets instead would miss the drawing that has to be spliced
/// and write a second one the sheet cannot point at.
fn resolve_rel_target(source: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_owned();
    }
    let mut parts: Vec<&str> = source
        .rsplit_once('/')
        .map_or(Vec::new(), |(dir, _)| dir.split('/').collect());
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

/// Build every part a sheet's authored charts need.
///
/// `first_chart` is the 1-based number of this sheet's first chart part, so
/// numbering runs across the workbook the way Excel's does.
///
/// Also returns a rebuilt drawing for a sheet with *no* authored charts, when
/// its retained one holds an anchor whose relationship has gone — which is what
/// deleting or editing an imported chart leaves behind.
pub fn build(
    workbook: &Workbook,
    sheet: &Sheet,
    sheet_index: usize,
    first_chart: usize,
) -> Option<SheetCharts> {
    let charts = authored(sheet);
    let sheet_part = format!("xl/worksheets/sheet{}.xml", sheet_index + 1);
    let existing = retained_drawing(workbook, &sheet_part);
    let drawing_part = existing.as_ref().map_or_else(
        || format!("xl/drawings/drawing{}.xml", sheet_index + 1),
        |(path, _)| path.clone(),
    );

    // Relationship ids inside the drawing must not collide with the retained
    // ones, which keep their originals because the anchors already written name
    // them.
    let taken: Vec<&str> = workbook
        .retained_rels
        .iter()
        .filter(|r| r.source == drawing_part)
        .map(|r| r.id.as_str())
        .collect();
    let mut next = 1;
    let mut rel_ids: Vec<String> = Vec::new();
    for _ in &charts {
        loop {
            let candidate = format!("rId{next}");
            next += 1;
            if !taken.contains(&candidate.as_str()) {
                rel_ids.push(candidate);
                break;
            }
        }
    }

    let mut chart_parts = Vec::new();
    let mut anchors = String::new();
    for (i, chart) in charts.iter().enumerate() {
        let n = first_chart + i;
        chart_parts.push((format!("xl/charts/chart{n}.xml"), chart_xml(chart)));
        anchors.push_str(&anchor_xml(chart, &rel_ids[i], n));
    }

    // An *imported* chart is written back from its retained part, so a shift
    // that moved its series in the model never reached the file (`FID-27`). Emit
    // a corrected copy at the same path: the writer prefers a generated part
    // over a retained one, so this wins without the stored part being touched.
    // Unchanged bytes produce no part at all, which keeps an untouched chart
    // byte-identical.
    for chart in &sheet.charts {
        let Some(path) = chart.part.as_deref() else {
            continue;
        };
        let Some(bytes) = retained_bytes(workbook, path) else {
            continue;
        };
        let tuned = retune_grouping(&retune_series(&bytes, chart), chart);
        if tuned != bytes {
            chart_parts.push((path.to_owned(), tuned));
        }
    }

    // Every relationship the drawing will actually have once it is written.
    let known: Vec<String> = workbook
        .retained_rels
        .iter()
        .filter(|r| r.source == drawing_part)
        .map(|r| r.id.clone())
        .chain(rel_ids.iter().cloned())
        .collect();
    let retained = retained_bytes(workbook, &drawing_part);
    let drawing_xml = match &retained {
        Some(bytes) => splice(
            &retune_anchors(
                &strip_dangling_anchors(bytes, &known),
                workbook,
                &drawing_part,
                sheet,
            ),
            &anchors,
        ),
        None => format!(
            "{DECL}<xdr:wsDr xmlns:xdr=\"{NS_XDR}\" xmlns:a=\"{NS_A}\">{anchors}</xdr:wsDr>"
        ),
    };
    // Nothing to add and nothing to clean up: leave the retained drawing to be
    // written back byte for byte, which is what it deserves when untouched.
    if charts.is_empty()
        && chart_parts.is_empty()
        && retained.as_deref() == Some(drawing_xml.as_str())
    {
        return None;
    }
    if charts.is_empty() && chart_parts.is_empty() && retained.is_none() {
        return None;
    }

    let mut rels = format!("{DECL}<Relationships xmlns=\"{NS_REL}\">");
    for rel in workbook
        .retained_rels
        .iter()
        .filter(|r| r.source == drawing_part)
    {
        rels.push_str(&rel_xml(rel));
    }
    for (i, id) in rel_ids.iter().enumerate() {
        rels.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"{NS_R}/chart\" Target=\"../charts/chart{}.xml\"/>",
            escape_attr(id),
            first_chart + i
        ));
    }
    rels.push_str("</Relationships>");

    Some(SheetCharts {
        sheet_rel: match &existing {
            // The sheet already names this drawing; its rel and its `<drawing>`
            // element travel with the retained set.
            Some(_) => None,
            None => Some((drawing_rel_id(sheet), drawing_part.clone())),
        },
        drawing_is_new: existing.is_none(),
        drawing_part,
        drawing_xml,
        chart_parts,
        drawing_rels: rels,
    })
}

/// The relationship id a new `<drawing>` takes on a sheet.
///
/// Numbered above the fixed ids the comment and table parts use, so it cannot
/// land on one of theirs.
pub fn drawing_rel_id(sheet: &Sheet) -> String {
    format!("rIdDrawing{}", sheet.tables.len())
}

fn retained_bytes(workbook: &Workbook, path: &str) -> Option<String> {
    workbook
        .retained_parts
        .iter()
        .find(|p| p.path == path)
        .and_then(|p| String::from_utf8(p.bytes.clone()).ok())
}

/// Drop any anchor whose `r:id` no longer resolves.
///
/// An imported chart that is edited or deleted loses its part and the
/// relationship reaching it, but its anchor lives inside the retained drawing's
/// bytes — which are not rewritten. Left alone, that anchor names a
/// relationship that does not exist, and Excel reports the file as needing
/// repair rather than simply drawing nothing.
///
/// Only whole elements are removed, and only when their id is definitely
/// dangling. An anchor with no `r:id` at all is a shape or a text box, which is
/// exactly the content this must not touch. Anything that does not parse
/// cleanly is left as it was: leaving a stale anchor is a bad outcome, and
/// corrupting a drawing is a worse one.
fn strip_dangling_anchors(existing: &str, known: &[String]) -> String {
    const OPENERS: [&str; 3] = ["twoCellAnchor", "oneCellAnchor", "absoluteAnchor"];
    let mut out = String::with_capacity(existing.len());
    let mut rest = existing;
    // The next anchor element, whichever kind and whatever namespace prefix.
    let next_anchor = |haystack: &str| -> Option<(usize, &'static str)> {
        OPENERS
            .iter()
            .filter_map(|n| haystack.find(&format!("<{n}")).map(|i| (i, *n)))
            .chain(
                OPENERS
                    .iter()
                    .filter_map(|n| haystack.find(&format!(":{n}")).map(|i| (i, *n))),
            )
            .min_by_key(|(i, _)| *i)
    };
    while let Some((start, name)) = next_anchor(rest) {
        // Back up to the `<` that opens it, so a prefixed name is included.
        let Some(open) = rest[..=start].rfind('<') else {
            break;
        };
        // The search for the closing tag starts *past* the opening one. Two
        // near misses on the way here: `name>` alone matches the opening tag,
        // and `/name>` misses the real close because the namespace prefix sits
        // between the slash and the name (`</xdr:twoCellAnchor>`). Either way
        // the "anchor" came out as a one-tag fragment holding no `r:id`, so
        // nothing was ever stripped.
        let Some(gt) = rest[open..].find('>') else {
            break;
        };
        let after_open = open + gt + 1;
        let Some(end_rel) = rest[after_open..].find(&format!("{name}>")) else {
            // No closing tag: not something to edit.
            break;
        };
        let end = after_open + end_rel + name.len() + 1;
        let block = &rest[open..end];
        let dangling = anchor_rel_ids(block)
            .iter()
            .any(|id| !known.iter().any(|k| k == id));
        out.push_str(&rest[..open]);
        if !dangling {
            out.push_str(block);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Every `r:id` / `r:embed` an anchor names.
fn anchor_rel_ids(block: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for key in ["r:id=\"", "r:embed=\""] {
        let mut rest = block;
        while let Some(at) = rest.find(key) {
            let after = &rest[at + key.len()..];
            match after.find('"') {
                Some(close) => {
                    ids.push(after[..close].to_owned());
                    rest = &after[close..];
                }
                None => break,
            }
        }
    }
    ids
}

/// Rewrite a retained chart part's series references to match the model's.
///
/// A chart read from a file keeps its part, and that part — not the model — is
/// what gets written back. So when a row insert shifts `values` and
/// `categories` (`FID-26`), the picture on screen moves and the saved file does
/// not. This closes that gap without touching the stored part: the corrected
/// bytes are emitted as a *generated* part at the same path, and the writer
/// already prefers a generated part over a retained one. `RetainedPart` stays
/// inert, exactly as `workbook.rs` promises.
///
/// Only the text inside `<c:f>` changes, and only inside a series' `<c:cat>`,
/// `<c:val>`, `<c:xVal>` or `<c:yVal>`. Every other byte survives, which is the
/// whole point: a chart part carries hundreds of formatting elements and
/// rebuilding it from what we model would delete all of them. `<c:tx>` is left
/// alone — a series *name* is not a position.
///
/// The nth `<c:ser>` is the nth modelled series, which is how the importer read
/// them. A part with more series than the model leaves the extras untouched
/// rather than guessing.
/// Rewrite a retained chart part's `<c:grouping val="…"/>` in place.
///
/// `CHT-16`. `CHT-05` fixed what detaching *loses* — the model carries grouping
/// now, so rebuilding no longer discards it. It did not stop the detach, and a
/// rebuilt part still drops every formatting element the model has no field
/// for, which in a real chart is most of them. Editing the value where it sits
/// is the stronger form, and the one `docs/84` §7 D asks for.
///
/// **Only the attribute's own characters change.** The element is found, its
/// `val="…"` span located, and the new token written between the quotes; every
/// other byte — the gradient fills, the data-point overrides, the manual
/// layout — survives untouched, which is the entire reason this exists rather
/// than a rebuild.
///
/// The value is passed through [`ChartGrouping::from_val`] against the group
/// element that actually holds it, so a `clustered` arriving for a `lineChart`
/// leaves the file alone instead of writing a token the schema forbids — the
/// same rule [`group_head`] applies when generating.
fn retune_grouping(existing: &str, chart: &ChartView) -> String {
    let Some(wanted) = chart.grouping else {
        return existing.to_owned();
    };
    let mut out = existing.to_owned();
    let mut cursor = 0;
    // Every group element in the part, because a combo chart has more than one
    // and each states its own grouping.
    while let Some((open, body, _, _)) = element_span(&out, "grouping", cursor) {
        // Which group is this inside? The nearest enclosing `*Chart` element
        // decides which tokens the schema permits here.
        let element = out[..open]
            .rfind("Chart")
            .and_then(|end| {
                let start = out[..end].rfind('<')?;
                let name = &out[start + 1..end + "Chart".len()];
                name.rsplit(':').next().map(str::to_owned)
            })
            .unwrap_or_default();
        let Some(token) = ChartGrouping::from_val(&element, wanted.as_str()) else {
            cursor = body;
            continue;
        };
        let head = &out[open..body];
        let Some(v) = head.find("val=\"") else {
            cursor = body;
            continue;
        };
        let value_start = open + v + 5;
        let Some(close) = out[value_start..body].find('"') else {
            cursor = body;
            continue;
        };
        let value_end = value_start + close;
        out.replace_range(value_start..value_end, token.as_str());
        cursor = value_start + token.as_str().len();
    }
    out
}

fn retune_series(existing: &str, chart: &ChartView) -> String {
    let mut out = String::with_capacity(existing.len());
    let mut cursor = 0;
    for series in &chart.series {
        let Some((_, body, body_end, _)) = element_span(existing, "ser", cursor) else {
            break;
        };
        let mut inner = String::new();
        let mut at = body;
        // `xVal`/`yVal` are the scatter spellings of `cat`/`val`, and the
        // importer treats them the same way, so this must too.
        for (names, replacement) in [
            (["cat", "xVal"], series.categories.as_deref()),
            (["val", "yVal"], Some(series.values.as_str())),
        ] {
            let Some(text) = replacement else { continue };
            let Some((_, slot_body, slot_end, _)) = names
                .iter()
                .filter_map(|n| element_span(&existing[..body_end], n, at))
                .min_by_key(|(open, ..)| *open)
            else {
                continue;
            };
            let Some((_, f_body, f_end, _)) = element_span(&existing[..slot_end], "f", slot_body)
            else {
                continue;
            };
            inner.push_str(&existing[at..f_body]);
            inner.push_str(&escape_text(text));
            at = f_end;
        }
        out.push_str(&existing[cursor..body]);
        out.push_str(&inner);
        out.push_str(&existing[at..body_end]);
        cursor = body_end;
    }
    out.push_str(&existing[cursor..]);
    out
}

/// The span of the first element with local name `local` at or after `from`, as
/// `(open, body_start, body_end, close_end)`.
///
/// Prefix-agnostic, because a chart part may spell the same element `<c:ser>`
/// or `<ser>`. It will **not** match a longer name that merely starts the same
/// way, which is not a hypothetical: `val` sits next to `valAx`, `ser` next to
/// `serAx`, and `f` next to `formatCode`. Matching on a prefix instead of the
/// whole name would rewrite an axis definition as if it were a series.
fn element_span(hay: &str, local: &str, from: usize) -> Option<(usize, usize, usize, usize)> {
    let mut search = from;
    while let Some(rel) = hay[search..].find('<') {
        let lt = search + rel;
        let after = &hay[lt + 1..];
        // Closing tags, comments and processing instructions are not openers.
        if after.starts_with(['/', '!', '?']) {
            search = lt + 1;
            continue;
        }
        let name_end = after.find(|c: char| c.is_whitespace() || c == '/' || c == '>')?;
        let qname = &after[..name_end];
        let gt = after.find('>')?;
        let body_start = lt + 1 + gt + 1;
        if qname.rsplit(':').next() != Some(local) {
            search = lt + 1;
            continue;
        }
        // A self-closing element has no body and no closing tag.
        if after[..gt].ends_with('/') {
            return Some((lt, body_start, body_start, body_start));
        }
        let body_end = close_of(hay, local, body_start)?;
        let close_gt = hay[body_end..].find('>')?;
        return Some((lt, body_start, body_end, body_end + close_gt + 1));
    }
    None
}

/// Where the closing tag for local name `local` starts, at or after `from`.
///
/// None of the elements this is used for nest inside themselves in a chart
/// part, so the first matching close is the right one.
fn close_of(hay: &str, local: &str, from: usize) -> Option<usize> {
    let mut search = from;
    while let Some(rel) = hay[search..].find("</") {
        let lt = search + rel;
        let after = &hay[lt + 2..];
        let gt = after.find('>')?;
        let qname = after[..gt].trim();
        if qname.rsplit(':').next() == Some(local) {
            return Some(lt);
        }
        search = lt + 2 + gt;
    }
    None
}

/// Rewrite a retained drawing's anchors to the frames the model holds.
///
/// The counterpart of [`retune_series`] for position (`FID-29`). A chart's
/// series live in its own part, which `ChartView::part` names outright; its
/// *frame* lives in the retained drawing, where nothing names the chart at all.
/// The link is the anchor's `r:id`, so matching one to a modelled chart means
/// resolving that id through the drawing's own relationships to a part path and
/// comparing it with what the chart says it came from. Images work the same way
/// through `r:embed` and their media part.
///
/// Only the four cell coordinates change. Offsets stay exactly as the file had
/// them, because a row insert moves a frame by whole rows and leaves where it
/// sits *within* a row alone — and rewriting them from the model would snap
/// every edge that was ever dragged between gridlines.
///
/// `oneCellAnchor` has a `<xdr:from>` and an extent rather than a `<xdr:to>`,
/// so only its corner moves; `absoluteAnchor` names no cells and is left alone.
/// Anything that does not parse cleanly is left as it was, on the same rule
/// [`strip_dangling_anchors`] follows.
fn retune_anchors(
    existing: &str,
    workbook: &Workbook,
    drawing_part: &str,
    sheet: &Sheet,
) -> String {
    // What each retained part *should* be framed by, in document order. A media
    // part may be placed twice, so this is a queue per path rather than a plain
    // lookup: the nth anchor naming it is the nth view the importer read.
    let mut wanted: BTreeMap<String, Vec<(u32, u32, u32, u32)>> = BTreeMap::new();
    for chart in &sheet.charts {
        if let Some(part) = chart.part.as_deref() {
            let a = chart.anchor;
            wanted.entry(part.to_owned()).or_default().push((
                a.start.col,
                a.start.row,
                a.end.col + 1,
                a.end.row + 1,
            ));
        }
    }
    for image in &sheet.images {
        let a = image.anchor;
        wanted.entry(image.part.clone()).or_default().push((
            a.start.col,
            a.start.row,
            a.end.col + 1,
            a.end.row + 1,
        ));
    }
    if wanted.is_empty() {
        return existing.to_owned();
    }
    // Consumed front to back, so a part placed twice gets its two frames in the
    // order the anchors appear.
    let mut taken: BTreeMap<String, usize> = BTreeMap::new();

    const OPENERS: [&str; 2] = ["twoCellAnchor", "oneCellAnchor"];
    let mut out = String::with_capacity(existing.len());
    let mut cursor = 0;
    while let Some((_, body, body_end, close_end)) = OPENERS
        .iter()
        .filter_map(|n| element_span(existing, n, cursor))
        .min_by_key(|(open, ..)| *open)
    {
        let block = &existing[body..body_end];
        let frame = anchor_rel_ids(block).into_iter().find_map(|id| {
            let target = workbook
                .retained_rels
                .iter()
                .find(|r| r.source == drawing_part && r.id == id && !r.external)?;
            let path = resolve_rel_target(drawing_part, &target.target);
            let queue = wanted.get(&path)?;
            let n = taken.entry(path).or_insert(0);
            let frame = queue.get(*n).copied()?;
            *n += 1;
            Some(frame)
        });
        out.push_str(&existing[cursor..body]);
        match frame {
            Some((from_col, from_row, to_col, to_row)) => {
                let with_from = set_corner(block, "from", from_col, from_row);
                out.push_str(&set_corner(&with_from, "to", to_col, to_row));
            }
            None => out.push_str(block),
        }
        out.push_str(&existing[body_end..close_end]);
        cursor = close_end;
    }
    out.push_str(&existing[cursor..]);
    out
}

/// Set the `<xdr:col>` and `<xdr:row>` of one corner, leaving its offsets be.
///
/// `col` sits immediately beside `colOff` and `row` beside `rowOff`, so this is
/// the case that makes [`element_span`]'s whole-name matching load-bearing
/// rather than merely careful: a prefix match writes the frame's column into
/// the offset that positions it inside that column.
fn set_corner(block: &str, corner: &str, col: u32, row: u32) -> String {
    let Some((_, body, body_end, _)) = element_span(block, corner, 0) else {
        return block.to_owned();
    };
    let mut inner = block[body..body_end].to_owned();
    for (tag, value) in [("col", col), ("row", row)] {
        let Some((_, at, end, _)) = element_span(&inner, tag, 0) else {
            continue;
        };
        inner.replace_range(at..end, &value.to_string());
    }
    format!("{}{inner}{}", &block[..body], &block[body_end..])
}

/// Insert `anchors` just before the drawing's closing tag.
///
/// Byte-preserving for everything else, which is the point: the retained
/// drawing may hold text boxes, shapes and form controls nothing here models,
/// and rebuilding it from what we understand would delete them.
fn splice(existing: &str, anchors: &str) -> String {
    match existing.rfind("</xdr:wsDr>").or_else(|| {
        existing
            .rfind("</wsDr>")
            .filter(|_| !existing.contains("</xdr:wsDr>"))
    }) {
        Some(at) => format!("{}{anchors}{}", &existing[..at], &existing[at..]),
        // No closing tag to insert before means this is not a drawing we can
        // extend. Leaving it untouched loses the new chart, which is visible;
        // appending to a malformed part would lose the file.
        None => existing.to_owned(),
    }
}

/// One `<xdr:twoCellAnchor>` framing a chart over its cells.
///
/// The offsets are what let an edge sit between gridlines. Writing zeroes
/// instead snapped every frame to whole cells, so a chart never came back the
/// size it was dragged to.
fn anchor_xml(chart: &ChartView, rel_id: &str, n: usize) -> String {
    let range = chart.anchor;
    format!(
        "<xdr:twoCellAnchor>\
<xdr:from><xdr:col>{}</xdr:col><xdr:colOff>{}</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>{}</xdr:rowOff></xdr:from>\
<xdr:to><xdr:col>{}</xdr:col><xdr:colOff>{}</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>{}</xdr:rowOff></xdr:to>\
<xdr:graphicFrame macro=\"\">\
<xdr:nvGraphicFramePr><xdr:cNvPr id=\"{n}\" name=\"Chart {n}\"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr>\
<xdr:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/></xdr:xfrm>\
<a:graphic><a:graphicData uri=\"{NS_C}\">\
<c:chart xmlns:c=\"{NS_C}\" xmlns:r=\"{NS_R}\" r:id=\"{}\"/>\
</a:graphicData></a:graphic>\
</xdr:graphicFrame>\
<xdr:clientData/>\
</xdr:twoCellAnchor>",
        range.start.col,
        chart.from_offset.x,
        range.start.row,
        chart.from_offset.y,
        // The `to` corner is exclusive in a drawing anchor: a frame from column
        // 2 to column 2 has no width at all. Its offset is measured into that
        // cell, which is the same number as one measured past the cell before —
        // so `to_offset` travels unchanged.
        range.end.col + 1,
        chart.to_offset.x,
        range.end.row + 1,
        chart.to_offset.y,
        escape_attr(rel_id)
    )
}

/// A `<c:tx><c:rich>` title block, which is how every title in a chart part is
/// spelled — the chart's and each axis's alike.
fn title_xml(text: &str) -> String {
    format!(
        "<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>{}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val=\"0\"/></c:title>",
        escape_text(text)
    )
}

/// `xl/charts/chart{n}.xml`.
pub fn chart_xml(chart: &ChartView) -> String {
    let mut s = format!(
        "{DECL}<c:chartSpace xmlns:c=\"{NS_C}\" xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\"><c:chart>"
    );
    if chart.title.is_empty() {
        // Without this Excel invents a title from the single series' name, so
        // "no title" has to be said rather than left unsaid.
        s.push_str("<c:autoTitleDeleted val=\"1\"/>");
    } else {
        s.push_str(&title_xml(&chart.title));
        s.push_str("<c:autoTitleDeleted val=\"0\"/>");
    }
    s.push_str("<c:plotArea><c:layout/>");
    s.push_str(&plot_xml(chart));
    if uses_axes(chart.kind) {
        s.push_str(&axes_xml(chart));
    }
    s.push_str("</c:plotArea>");
    if let Some(pos) = &chart.legend {
        s.push_str(&format!(
            "<c:legend><c:legendPos val=\"{}\"/><c:overlay val=\"0\"/></c:legend>",
            escape_attr(pos)
        ));
    }
    // Without this Excel plots hidden rows too, so filtering a source range
    // would leave the chart showing what the sheet does not.
    s.push_str("<c:plotVisOnly val=\"1\"/><c:dispBlanksAs val=\"gap\"/>");
    s.push_str("</c:chart></c:chartSpace>");
    s
}

/// Whether this kind of chart has a category and a value axis. A pie does not:
/// writing axes for one is invalid, not merely redundant.
fn uses_axes(kind: ChartKind) -> bool {
    !matches!(kind, ChartKind::Pie | ChartKind::Doughnut)
}

/// Whether `kind` can share one plot area with a different kind.
///
/// The combination family, and only it. A pie has no axes to share and a
/// scatter's horizontal axis is a *value* axis, so neither can sit beside a
/// column group without describing a chart Excel refuses to open. A per-series
/// kind outside this set is ignored rather than written, which costs a picture
/// and never a package.
fn combinable(kind: ChartKind) -> bool {
    matches!(
        kind,
        ChartKind::Bar | ChartKind::Column | ChartKind::Line | ChartKind::Area
    )
}

/// The kind one series is drawn as: its own when the chart is a combination,
/// the chart's otherwise.
fn series_kind(chart: &ChartView, series: &ChartSeries) -> ChartKind {
    match series.kind {
        Some(k) if combinable(k) && combinable(chart.kind) => k,
        _ => chart.kind,
    }
}

/// The group element for a kind, and everything before its series.
///
/// Child order inside a group is fixed by the schema — `barDir`, `grouping`,
/// `varyColors`, the series, then `overlap`, then the axis ids — and Excel
/// refuses a package that gets it wrong rather than reordering it.
///
/// **The grouping is the model's, not a literal.** It used to be
/// `clustered` for every bar and column chart and `standard` for every line
/// and area one, so a stacked chart that had been retitled or dragged was
/// written back as a clustered chart: an edit that has nothing to do with what
/// the chart *is* converted it, in the file, with nothing said (`CHT-05`).
fn group_head(kind: ChartKind, grouping: Option<ChartGrouping>) -> (&'static str, String) {
    // `from_val` is the schema's own answer to whether this group's element
    // permits this value: `ST_Grouping`, which is what a line or area group
    // takes, has no `clustered`. So a grouping that does not belong falls back
    // to the element's default rather than being written into a package Excel
    // would refuse.
    let of = |element: &str, default: ChartGrouping| {
        grouping
            .and_then(|g| ChartGrouping::from_val(element, g.as_str()))
            .unwrap_or(default)
            .as_str()
    };
    match kind {
        ChartKind::Bar => (
            "barChart",
            format!(
                "<c:barDir val=\"bar\"/><c:grouping val=\"{}\"/><c:varyColors val=\"0\"/>",
                of("barChart", ChartGrouping::Clustered)
            ),
        ),
        ChartKind::Column | ChartKind::Unsupported => (
            "barChart",
            format!(
                "<c:barDir val=\"col\"/><c:grouping val=\"{}\"/><c:varyColors val=\"0\"/>",
                of("barChart", ChartGrouping::Clustered)
            ),
        ),
        ChartKind::Line => (
            "lineChart",
            format!(
                "<c:grouping val=\"{}\"/><c:varyColors val=\"0\"/>",
                of("lineChart", ChartGrouping::Standard)
            ),
        ),
        ChartKind::Area => (
            "areaChart",
            format!(
                "<c:grouping val=\"{}\"/><c:varyColors val=\"0\"/>",
                of("areaChart", ChartGrouping::Standard)
            ),
        ),
        // A pie varies colour per *point* rather than per series, which is the
        // only way one series reads as several slices.
        ChartKind::Pie => ("pieChart", "<c:varyColors val=\"1\"/>".to_owned()),
        ChartKind::Doughnut => ("doughnutChart", "<c:varyColors val=\"1\"/>".to_owned()),
        ChartKind::Scatter => (
            "scatterChart",
            "<c:scatterStyle val=\"lineMarker\"/><c:varyColors val=\"0\"/>".to_owned(),
        ),
    }
}

/// The chart groups and their series.
///
/// **One group per consecutive run of series sharing a kind and an axis.** A
/// combination chart is several `<c:*Chart>` elements in one `<c:plotArea>`,
/// and a secondary axis is a group naming a second `<c:axId>` pair — neither is
/// a property of a series in the file, so the flat model list has to be cut
/// back into groups here. Runs rather than a gather-by-kind, because a run
/// preserves the model's series order exactly: reading the part back gives the
/// list it was written from, where sorting three series into two groups would
/// not.
fn plot_xml(chart: &ChartView) -> String {
    let mut s = String::new();
    let mut at = 0usize;
    while at < chart.series.len() {
        let kind = series_kind(chart, &chart.series[at]);
        let secondary = chart.series[at].secondary_axis && uses_axes(chart.kind);
        let end = chart.series[at..]
            .iter()
            .position(|next| {
                series_kind(chart, next) != kind
                    || (next.secondary_axis && uses_axes(chart.kind)) != secondary
            })
            .map_or(chart.series.len(), |n| at + n);

        let (element, head) = group_head(kind, chart.grouping);
        s.push_str(&format!("<c:{element}>{head}"));
        for (i, series) in chart.series[at..end].iter().enumerate() {
            s.push_str(&series_xml(kind, at + i, series));
        }
        // Stacked bars that do not overlap sit side by side, which is the
        // clustered picture with a taller axis — the loss this whole change
        // exists to stop, reintroduced by an omitted element.
        if matches!(kind, ChartKind::Bar | ChartKind::Column)
            && chart.grouping.is_some_and(ChartGrouping::is_stacked)
        {
            s.push_str("<c:overlap val=\"100\"/>");
        }
        if uses_axes(kind) {
            let (cat, val) = if secondary {
                (CAT2_AX_ID, VAL2_AX_ID)
            } else {
                (CAT_AX_ID, VAL_AX_ID)
            };
            s.push_str(&format!("<c:axId val=\"{cat}\"/><c:axId val=\"{val}\"/>"));
        }
        s.push_str(&format!("</c:{element}>"));
        at = end;
    }
    if chart.series.is_empty() {
        // A chart with no series still needs a group, or there is nothing for
        // the axis ids to pair with and Excel reports a repair.
        let (element, head) = group_head(chart.kind, chart.grouping);
        s.push_str(&format!("<c:{element}>{head}"));
        if uses_axes(chart.kind) {
            s.push_str(&format!(
                "<c:axId val=\"{CAT_AX_ID}\"/><c:axId val=\"{VAL_AX_ID}\"/>"
            ));
        }
        s.push_str(&format!("</c:{element}>"));
    }
    s
}

/// One `<c:ser>`.
///
/// `idx` is its index in the *model's* list rather than in its group, so the
/// series keeps its palette slot and its plot order when a combination chart
/// splits it into a second group.
fn series_xml(kind: ChartKind, idx: usize, series: &ChartSeries) -> String {
    let mut s = format!("<c:ser><c:idx val=\"{idx}\"/><c:order val=\"{idx}\"/>");
    if !series.name.is_empty() {
        s.push_str(&format!(
            "<c:tx><c:v>{}</c:v></c:tx>",
            escape_text(&series.name)
        ));
    }
    // A line or scatter series carries its marker setting before the data,
    // and a scatter with no marker and no line plots nothing visible.
    if matches!(kind, ChartKind::Line | ChartKind::Scatter) {
        s.push_str("<c:marker><c:symbol val=\"circle\"/></c:marker>");
    }
    // After the marker and before the data, which is where every `CT_*Ser` in
    // the schema puts it. The five `show*` siblings are written explicitly and
    // off: their defaults differ by chart type — a pie shows percentages —
    // so leaving them unsaid means a label that says something else.
    if series.data_labels {
        s.push_str(
            "<c:dLbls><c:showLegendKey val=\"0\"/><c:showVal val=\"1\"/>\
<c:showCatName val=\"0\"/><c:showSerName val=\"0\"/><c:showPercent val=\"0\"/>\
<c:showBubbleSize val=\"0\"/></c:dLbls>",
        );
    }
    let (cat_tag, val_tag) = if kind == ChartKind::Scatter {
        ("xVal", "yVal")
    } else {
        ("cat", "val")
    };
    if let Some(categories) = &series.categories {
        // A scatter's horizontal values are numbers; every other chart's
        // categories are labels, and the wrong reference kind makes Excel
        // read the labels as zeroes.
        let inner = if kind == ChartKind::Scatter {
            "numRef"
        } else {
            "strRef"
        };
        s.push_str(&format!(
            "<c:{cat_tag}><c:{inner}><c:f>{}</c:f></c:{inner}></c:{cat_tag}>",
            escape_text(categories)
        ));
    }
    s.push_str(&format!(
        "<c:{val_tag}><c:numRef><c:f>{}</c:f></c:numRef></c:{val_tag}>",
        escape_text(&series.values)
    ));
    if kind == ChartKind::Line {
        s.push_str("<c:smooth val=\"0\"/>");
    }
    s.push_str("</c:ser>");
    s
}

/// The axis definitions, paired to the groups by id.
///
/// A scatter's horizontal axis is a *value* axis, not a category axis: it plots
/// numbers, and writing a `catAx` for it makes Excel space the points evenly
/// and lose the very relationship the chart is drawn to show.
///
/// **A second pair follows when any series is on the secondary axis.** Its
/// value axis sits on the right and crosses the category axis at its maximum,
/// which is what puts it opposite the primary one; its category axis is written
/// `delete="1"`, because a chart shows one set of category labels however many
/// value axes measure against them.
fn axes_xml(chart: &ChartView) -> String {
    let horizontal = if chart.kind == ChartKind::Scatter {
        "valAx"
    } else {
        "catAx"
    };
    let mut s = format!(
        "<c:{horizontal}><c:axId val=\"{CAT_AX_ID}\"/>\
<c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"b\"/>"
    );
    if !chart.x_title.is_empty() {
        s.push_str(&title_xml(&chart.x_title));
    }
    s.push_str(&format!(
        "<c:crossAx val=\"{VAL_AX_ID}\"/></c:{horizontal}>\
<c:valAx><c:axId val=\"{VAL_AX_ID}\"/>\
<c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"l\"/>\
<c:majorGridlines/>"
    ));
    if !chart.y_title.is_empty() {
        s.push_str(&title_xml(&chart.y_title));
    }
    s.push_str(&format!("<c:crossAx val=\"{CAT_AX_ID}\"/></c:valAx>"));
    if has_secondary(chart) {
        s.push_str(&format!(
            "<c:{horizontal}><c:axId val=\"{CAT2_AX_ID}\"/>\
<c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"1\"/><c:axPos val=\"b\"/>\
<c:crossAx val=\"{VAL2_AX_ID}\"/></c:{horizontal}>\
<c:valAx><c:axId val=\"{VAL2_AX_ID}\"/>\
<c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"r\"/>\
<c:crossAx val=\"{CAT2_AX_ID}\"/><c:crosses val=\"max\"/></c:valAx>"
        ));
    }
    s
}

/// Whether any series asks for the secondary value axis.
fn has_secondary(chart: &ChartView) -> bool {
    uses_axes(chart.kind) && chart.series.iter().any(|s| s.secondary_axis)
}

/// The content-type overrides an authored-chart package needs, keyed by part
/// path so a drawing shared by two passes is declared once.
pub fn content_types(built: &[SheetCharts]) -> BTreeMap<String, &'static str> {
    let mut out = BTreeMap::new();
    for sheet in built {
        if sheet.drawing_is_new {
            out.insert(sheet.drawing_part.clone(), DRAWING_CONTENT_TYPE);
        }
        for (path, _) in &sheet.chart_parts {
            out.insert(path.clone(), CHART_CONTENT_TYPE);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{close_of, element_span};

    /// `ser` must not match `serAx`, `val` must not match `valAx`, and `f` must
    /// not match `formatCode`. Valid OOXML happens to order these so that a
    /// prefix match usually lands on the right element anyway, which is exactly
    /// why this is tested here rather than left to an end-to-end case: the bug
    /// would be invisible until it met a file whose ordering differed.
    #[test]
    fn an_element_name_matches_whole_not_as_a_prefix() {
        let xml = "<c:serAx><c:f>axis</c:f></c:serAx><c:ser><c:f>data</c:f></c:ser>";
        let (open, body, body_end, _) = element_span(xml, "ser", 0).expect("finds the series");
        assert_eq!(
            &xml[open..body],
            "<c:ser>",
            "it must skip <c:serAx> and open the real series"
        );
        assert_eq!(&xml[body..body_end], "<c:f>data</c:f>");

        let valish = "<c:valAx>axis</c:valAx><c:val>data</c:val>";
        let (open, body, body_end, _) = element_span(valish, "val", 0).expect("finds the value");
        assert_eq!(&valish[open..body], "<c:val>");
        assert_eq!(&valish[body..body_end], "data");

        let fish = "<c:formatCode>General</c:formatCode><c:f>ref</c:f>";
        let (_, body, body_end, _) = element_span(fish, "f", 0).expect("finds the formula");
        assert_eq!(&fish[body..body_end], "ref");
    }

    /// A drawing anchor's corner, with the longer name written first.
    ///
    /// This is the ordering that actually bites. Where the shorter name comes
    /// first — as it does in every anchor Excel writes — a prefix matcher lands
    /// on the right element by luck, and no end-to-end case can tell the two
    /// matchers apart. Kept as its own test so it is proved on its own rather
    /// than shielded by an earlier assertion failing first.
    #[test]
    fn a_frame_coordinate_is_not_read_out_of_its_offset() {
        let corner = "<xdr:colOff>12700</xdr:colOff><xdr:col>5</xdr:col>";
        let (_, body, body_end, _) = element_span(corner, "col", 0).expect("finds the column");
        assert_eq!(
            &corner[body..body_end],
            "5",
            "the frame's column must not be read out of the offset that positions it"
        );

        let corner = "<xdr:rowOff>19050</xdr:rowOff><xdr:row>4</xdr:row>";
        let (_, body, body_end, _) = element_span(corner, "row", 0).expect("finds the row");
        assert_eq!(&corner[body..body_end], "4");
    }

    /// The closing tag has to match on the whole name too, or a span runs past
    /// its own element and swallows whatever follows.
    #[test]
    fn a_closing_tag_matches_whole_not_as_a_prefix() {
        let xml = "<c:ser>body</c:serAx></c:ser>";
        let at = close_of(xml, "ser", 0).expect("finds a close");
        assert_eq!(
            &xml[at..],
            "</c:ser>",
            "</c:serAx> is not the close of <c:ser>"
        );
    }

    /// A self-closing element has no body, and asking for one must not run off
    /// looking for a closing tag that will never come.
    #[test]
    fn a_self_closing_element_has_an_empty_body() {
        let xml = "<c:f/><c:v>after</c:v>";
        let (_, body, body_end, close_end) = element_span(xml, "f", 0).expect("finds it");
        assert_eq!(body, body_end);
        assert_eq!(close_end, body);
    }
}
