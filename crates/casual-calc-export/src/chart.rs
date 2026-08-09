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

use casual_calc_model::{CellRange, ChartKind, ChartView, RetainedRel, Sheet, Workbook};

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
fn retained_drawing(workbook: &Workbook, sheet_part: &str) -> Option<(String, String)> {
    workbook
        .retained_rels
        .iter()
        .find(|r| r.source == sheet_part && r.rel_type.ends_with("/drawing"))
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
pub fn build(
    workbook: &Workbook,
    sheet: &Sheet,
    sheet_index: usize,
    first_chart: usize,
) -> Option<SheetCharts> {
    let charts = authored(sheet);
    if charts.is_empty() {
        return None;
    }
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
        anchors.push_str(&anchor_xml(chart.anchor, &rel_ids[i], n));
    }

    let drawing_xml = match retained_bytes(workbook, &drawing_part) {
        Some(bytes) => splice(&bytes, &anchors),
        None => format!(
            "{DECL}<xdr:wsDr xmlns:xdr=\"{NS_XDR}\" xmlns:a=\"{NS_A}\">{anchors}</xdr:wsDr>"
        ),
    };

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

fn rel_xml(rel: &RetainedRel) -> String {
    format!(
        "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"/>",
        escape_attr(&rel.id),
        escape_attr(&rel.rel_type),
        escape_attr(&rel.target)
    )
}

fn retained_bytes(workbook: &Workbook, path: &str) -> Option<String> {
    workbook
        .retained_parts
        .iter()
        .find(|p| p.path == path)
        .and_then(|p| String::from_utf8(p.bytes.clone()).ok())
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
fn anchor_xml(range: CellRange, rel_id: &str, n: usize) -> String {
    format!(
        "<xdr:twoCellAnchor>\
<xdr:from><xdr:col>{}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>\
<xdr:to><xdr:col>{}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>\
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
        range.start.row,
        // The `to` corner is exclusive in a drawing anchor: a frame from column
        // 2 to column 2 has no width at all.
        range.end.col + 1,
        range.end.row + 1,
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

/// The chart-group element and its series.
///
/// Child order inside a group is fixed by the schema — `barDir`, `grouping`,
/// `varyColors`, the series, then the axis ids — and Excel refuses a package
/// that gets it wrong rather than reordering it.
fn plot_xml(chart: &ChartView) -> String {
    let (element, head) = match chart.kind {
        ChartKind::Bar => (
            "barChart",
            "<c:barDir val=\"bar\"/><c:grouping val=\"clustered\"/><c:varyColors val=\"0\"/>"
                .to_owned(),
        ),
        ChartKind::Column | ChartKind::Unsupported => (
            "barChart",
            "<c:barDir val=\"col\"/><c:grouping val=\"clustered\"/><c:varyColors val=\"0\"/>"
                .to_owned(),
        ),
        ChartKind::Line => (
            "lineChart",
            "<c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>".to_owned(),
        ),
        ChartKind::Area => (
            "areaChart",
            "<c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>".to_owned(),
        ),
        // A pie varies colour per *point* rather than per series, which is the
        // only way one series reads as several slices.
        ChartKind::Pie => ("pieChart", "<c:varyColors val=\"1\"/>".to_owned()),
        ChartKind::Doughnut => ("doughnutChart", "<c:varyColors val=\"1\"/>".to_owned()),
        ChartKind::Scatter => (
            "scatterChart",
            "<c:scatterStyle val=\"lineMarker\"/><c:varyColors val=\"0\"/>".to_owned(),
        ),
    };
    let mut s = format!("<c:{element}>{head}");
    for (i, series) in chart.series.iter().enumerate() {
        s.push_str(&format!(
            "<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/>"
        ));
        if !series.name.is_empty() {
            s.push_str(&format!(
                "<c:tx><c:v>{}</c:v></c:tx>",
                escape_text(&series.name)
            ));
        }
        // A line or scatter series carries its marker setting before the data,
        // and a scatter with no marker and no line plots nothing visible.
        if matches!(chart.kind, ChartKind::Line | ChartKind::Scatter) {
            s.push_str("<c:marker><c:symbol val=\"circle\"/></c:marker>");
        }
        let (cat_tag, val_tag) = if chart.kind == ChartKind::Scatter {
            ("xVal", "yVal")
        } else {
            ("cat", "val")
        };
        if let Some(categories) = &series.categories {
            // A scatter's horizontal values are numbers; every other chart's
            // categories are labels, and the wrong reference kind makes Excel
            // read the labels as zeroes.
            let inner = if chart.kind == ChartKind::Scatter {
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
        if chart.kind == ChartKind::Line {
            s.push_str("<c:smooth val=\"0\"/>");
        }
        s.push_str("</c:ser>");
    }
    if uses_axes(chart.kind) {
        s.push_str(&format!(
            "<c:axId val=\"{CAT_AX_ID}\"/><c:axId val=\"{VAL_AX_ID}\"/>"
        ));
    }
    s.push_str(&format!("</c:{element}>"));
    s
}

/// The two axis definitions, paired to the group by id.
///
/// A scatter's horizontal axis is a *value* axis, not a category axis: it plots
/// numbers, and writing a `catAx` for it makes Excel space the points evenly
/// and lose the very relationship the chart is drawn to show.
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
    s
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
