//! Reading charts far enough to draw them.
//!
//! Two parts are involved and neither is optional. `xl/drawings/drawingN.xml`
//! says *where* a chart sits — as cell coordinates, which is why a chart moves
//! with the rows under it — and names the chart part through a relationship.
//! `xl/charts/chartN.xml` says what it plots.
//!
//! Everything here feeds [`casual_calc_model::ChartView`], which is a display
//! projection: the parts themselves are retained byte for byte and written back
//! from those bytes. A field this parser misses costs a picture, not a file.

use casual_calc_model::{CellRange, CellRef, ChartGrouping, ChartKind, ChartSeries, Emu};
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::ImportError;
use crate::read::text_of;
use crate::read::{read_attr, ref_of, xml_err};

/// A `<xdr:*Anchor>`: the cells it covers and the relationship id of whatever
/// it frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingAnchor {
    /// The cells the frame spans, inclusive.
    pub range: CellRange,
    /// How far into the first cell the frame's top-left sits.
    pub from_offset: Emu,
    /// How far past the last cell's far edge its bottom-right sits.
    pub to_offset: Emu,
    /// `r:id` of the referenced part, when the anchor frames one.
    pub rel_id: Option<String>,
    /// `<xdr:ext cx cy>`, when the anchor states its size that way.
    ///
    /// A `twoCellAnchor` describes its size with the second cell and has none.
    /// The other two carry this instead, and it used to be read past: `range`
    /// was filled with a nominal span, and a guessed frame is a fabricated
    /// aspect ratio for anything scaled into it (`RND-13`).
    pub extent: Option<Emu>,
}

/// Parse a drawing part's anchors.
///
/// `oneCellAnchor` and `absoluteAnchor` carry an extent in EMUs rather than a
/// second cell. Rather than convert EMUs to a column count — which needs every
/// column's width and still only guesses — they get a nominal span; a chart
/// drawn a column or two out is far better than one not drawn at all, and the
/// file is unaffected either way.
pub fn parse_drawing(xml: &[u8]) -> Result<Vec<DrawingAnchor>, ImportError> {
    /// Columns and rows a one-cell or absolute anchor is assumed to cover.
    const NOMINAL_COLS: u32 = 8;
    const NOMINAL_ROWS: u32 = 15;

    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut out: Vec<DrawingAnchor> = Vec::new();

    // Which corner is being read, and the numbers collected so far. `<xdr:col>`
    // and `<xdr:row>` are element *text*, and both corners use the same element
    // names — so which corner is open has to be tracked.
    let mut corner: Option<bool> = None; // Some(true) = <from>, Some(false) = <to>
    // Which of the four numbers a corner holds is open: col, row, colOff or
    // rowOff. The offsets are what let a frame's edge sit anywhere rather than
    // only on a gridline, so dropping them snapped every chart to whole cells.
    let mut field: Option<u8> = None; // 0 col, 1 row, 2 colOff, 3 rowOff
    let mut text = String::new();
    let (mut fc, mut fr, mut tc, mut tr) = (0u32, 0u32, 0u32, 0u32);
    let (mut fcx, mut fcy, mut tcx, mut tcy) = (0i64, 0i64, 0i64, 0i64);
    let mut have_to = false;
    let mut rel_id: Option<String> = None;
    let mut extent: Option<Emu> = None;
    let mut open = false;

    loop {
        let event = reader.read_event_into(&mut buf).map_err(xml_err)?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => match e.local_name().as_ref() {
                b"twoCellAnchor" | b"oneCellAnchor" | b"absoluteAnchor" => {
                    open = true;
                    have_to = false;
                    rel_id = None;
                    extent = None;
                    (fc, fr, tc, tr) = (0, 0, 0, 0);
                    (fcx, fcy, tcx, tcy) = (0, 0, 0, 0);
                }
                b"from" => corner = Some(true),
                b"to" => {
                    corner = Some(false);
                    have_to = true;
                }
                b"col" => {
                    field = Some(0);
                    text.clear();
                }
                b"row" => {
                    field = Some(1);
                    text.clear();
                }
                b"colOff" => {
                    field = Some(2);
                    text.clear();
                }
                b"rowOff" => {
                    field = Some(3);
                    text.clear();
                }
                // `<c:chart r:id>` inside `<a:graphicData>`, and the same
                // attribute on a picture's `<a:blip r:embed>`.
                // `<xdr:ext cx cy>`: the picture's own size, which a
                // one-cell or absolute anchor uses in place of a second cell.
                b"ext" => {
                    let cx = read_attr(e, b"cx")?.and_then(|v| v.parse().ok());
                    let cy = read_attr(e, b"cy")?.and_then(|v| v.parse().ok());
                    if let (Some(x), Some(y)) = (cx, cy) {
                        extent = Some(Emu { x, y });
                    }
                }
                b"chart" | b"blip" => {
                    if let Some(id) = read_attr(e, b"id")?.or(read_attr(e, b"embed")?) {
                        rel_id = Some(id);
                    }
                }
                _ => {}
            },
            Event::Text(ref e) => {
                if field.is_some() {
                    text.push_str(&text_of(e)?);
                }
            }
            Event::GeneralRef(ref e) => {
                if field.is_some() {
                    text.push_str(&ref_of(e)?);
                }
            }
            Event::End(ref e) => match e.local_name().as_ref() {
                b"col" | b"row" | b"colOff" | b"rowOff" => {
                    let raw = text.trim();
                    let cells: u32 = raw.parse().unwrap_or(0);
                    let emu: i64 = raw.parse().unwrap_or(0);
                    match (corner, field) {
                        (Some(true), Some(0)) => fc = cells,
                        (Some(true), Some(1)) => fr = cells,
                        (Some(true), Some(2)) => fcx = emu,
                        (Some(true), Some(3)) => fcy = emu,
                        (Some(false), Some(0)) => tc = cells,
                        (Some(false), Some(1)) => tr = cells,
                        (Some(false), Some(2)) => tcx = emu,
                        (Some(false), Some(3)) => tcy = emu,
                        _ => {}
                    }
                    field = None;
                }
                b"from" | b"to" => corner = None,
                b"twoCellAnchor" | b"oneCellAnchor" | b"absoluteAnchor" if open => {
                    // `<xdr:to>` is **exclusive**: with `colOff` zero the
                    // frame's right edge sits on the left edge of that column,
                    // so the last column it covers is the one before. Reading
                    // it as inclusive drew every chart a row and a column too
                    // large, and — once the writer started emitting anchors —
                    // would have grown one on every save.
                    let (end_c, end_r) = if have_to {
                        (tc.saturating_sub(1), tr.saturating_sub(1))
                    } else {
                        (fc + NOMINAL_COLS - 1, fr + NOMINAL_ROWS - 1)
                    };
                    // A `to` that lands on or before `from` is degenerate; the
                    // frame collapses to one cell and the leftover offset would
                    // be measured from the wrong edge, so it is dropped rather
                    // than applied to a corner it does not belong to.
                    let degenerate = end_c < fc || end_r < fr;
                    out.push(DrawingAnchor {
                        range: CellRange::new(
                            CellRef::new(fr, fc),
                            CellRef::new(end_r.max(fr), end_c.max(fc)),
                        ),
                        from_offset: Emu { x: fcx, y: fcy },
                        to_offset: if degenerate || !have_to {
                            Emu::default()
                        } else {
                            Emu { x: tcx, y: tcy }
                        },
                        // Carried only when the file stated one, which is
                        // exactly the case where `range` above is a nominal
                        // guess rather than something the author wrote.
                        extent: if have_to { None } else { extent },
                        rel_id: rel_id.take(),
                    });
                    open = false;
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

/// Something a chart part carries that [`casual_calc_model::ChartView`] cannot
/// express, named so import can report it.
///
/// **A closed enum rather than a string taken from the file.** Feature keys
/// reach `CompatibilityReport::record` verbatim and a chart part's element
/// names are attacker-controlled — `<c:anythingChart>` satisfies the parser's
/// `ends_with("Chart")` test — so a key built from one would let a file spend
/// the report's whole `MAX_REPORT_FEATURES` budget. This is the same rule
/// `cf_report_feature` follows for a `<cfRule>` type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChartGap {
    /// A chart group element the model has no picture for.
    Type,
    /// A 3-D group, flattened to its two-dimensional picture.
    ThreeD,
    /// A second group stacks differently from the first. The model carries one
    /// grouping for the whole chart, which is the stated cost of holding a
    /// combination chart as a flat series list.
    PerGroupGrouping,
    /// A data label showing something other than the point's value — the
    /// category or series name, a percentage, the legend key, a bubble size.
    LabelKind,
    /// A label overridden for one point rather than for the series.
    PerPointLabel,
    /// `<c:trendline>`: a fitted curve the model does not carry.
    Trendline,
    /// `<c:errBars>`: error bars.
    ErrorBars,
    /// An axis with an explicit minimum, maximum or logarithmic base. The
    /// model has no axis object, so the plot's extent is always the data's.
    AxisScale,
}

impl ChartGap {
    /// The compatibility-report feature key. Fixed strings, never the file's.
    #[must_use]
    pub fn feature(self) -> &'static str {
        match self {
            Self::Type => "chart/unsupportedType",
            Self::ThreeD => "chart/3d",
            Self::PerGroupGrouping => "chart/grouping/perGroup",
            Self::LabelKind => "chart/dLbls/kind",
            Self::PerPointLabel => "chart/dLbls/perPoint",
            Self::Trendline => "chart/trendline",
            Self::ErrorBars => "chart/errBars",
            Self::AxisScale => "chart/axisScale",
        }
    }
}

/// What a chart part plots.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChartSpec {
    /// The chart kind, or `Unsupported`.
    pub kind: Option<ChartKind>,
    /// `<c:grouping val>` of the first chart group, when it states one the
    /// group's own element permits.
    pub grouping: Option<ChartGrouping>,
    /// The title text, empty when absent.
    pub title: String,
    /// Series in plot order.
    pub series: Vec<ChartSeries>,
    /// `<c:legendPos val>`, when the chart has a legend.
    pub legend: Option<String>,
    /// The category axis title.
    pub x_title: String,
    /// The value axis title.
    pub y_title: String,
    /// What was read and cannot be drawn, deduplicated and sorted so the same
    /// part always reports the same list.
    pub gaps: Vec<ChartGap>,
}

/// One `<c:*Chart>` group, while the part is being read.
///
/// A group is what carries `<c:grouping>` and the `<c:axId>` pair, and neither
/// is known until the group **ends** — `<c:axId>` follows the series it
/// applies to. So the group's series are recorded as a half-open range into
/// [`ChartSpec::series`] and stamped once the group closes.
#[derive(Debug, Default)]
struct Group {
    /// The element's local name, e.g. `barChart`.
    element: String,
    /// `<c:barDir val>` inside *this* group. Per group, not per part: a combo
    /// chart's `<c:lineChart>` has none, and reusing the `<c:barChart>`'s
    /// would make the line group read as a column group.
    bar_dir: Option<String>,
    /// `<c:grouping val>` as the file spelled it.
    grouping: Option<String>,
    /// The `<c:axId>` values pairing the group to its axes. Two of them, and
    /// what tells a secondary-axis group from a primary one.
    ax_ids: Vec<String>,
    /// Whether the group's own `<c:dLbls>` turns value labels on for every
    /// series in it.
    show_val: bool,
    /// Where this group's series start in [`ChartSpec::series`].
    from: usize,
}

/// Parse a chart part.
///
/// The shape that matters: `<c:plotArea>` holds one or more chart-group
/// elements (`<c:barChart>`, `<c:lineChart>`, …), each holding `<c:ser>`, each
/// holding `<c:tx>` (name), `<c:cat>` (categories) and `<c:val>` (values).
/// References live in a `<c:f>` element inside those.
pub fn parse_chart(xml: &[u8]) -> Result<ChartSpec, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut spec = ChartSpec::default();

    let mut group: Option<String> = None;
    // Every chart group in the part, in document order. The first decides the
    // chart's kind; the rest are what makes a combination chart and a secondary
    // axis readable, and both facts are only complete when a group closes.
    let mut groups: Vec<Group> = Vec::new();
    let mut open_group: Option<Group> = None;
    let mut gaps: Vec<ChartGap> = Vec::new();
    // `<c:dLbls>` nests: a group has one, each `<c:ser>` may have one, and each
    // `<c:dLbl>` inside those overrides a single point. Depth is what tells a
    // `<c:showVal>` that belongs to a series from one that belongs to a point.
    let mut dlbls_depth = 0usize;
    // `<c:scaling>` holds `<c:min>`, `<c:max>` and `<c:logBase>`; the same
    // element names appear nowhere else this reads, but the guard keeps the
    // report about axes rather than about whatever else grows a `min`.
    let mut in_scaling = false;
    // Inside a `<c:dLbl>`, whose `<c:showVal>` sits at the same depth as the
    // series' own and means something else entirely: this point, not this
    // series.
    let mut in_dlbl = false;
    let mut series: Option<ChartSeries> = None;
    // Which of a series' three reference slots is open.
    let mut slot: Option<&'static str> = None;
    let mut in_formula = false;
    let mut in_title = false;
    let mut text = String::new();
    // A title's text is `<a:t>` inside `<c:title>`; `<a:t>` also appears in
    // every other bit of rich text in the part, so the enclosing title has to
    // be tracked or the first axis label becomes the chart's name.
    let mut title_text = String::new();
    // `<c:tx><c:v>` is a literal series name; `<c:tx><c:strRef><c:f>` is a
    // reference to one. The literal is preferred where both appear, because it
    // is what Excel displays without resolving anything.
    let mut in_value = false;
    // `<c:title>` appears three times over — once for the chart and once inside
    // each axis — so which axis is open is what tells them apart. Without it
    // the first axis title becomes the chart's name.
    let mut axis: Option<&'static str> = None;

    loop {
        let event = reader.read_event_into(&mut buf).map_err(xml_err)?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"barDir" => {
                        let val = read_attr(e, b"val")?;
                        if let Some(g) = open_group.as_mut() {
                            g.bar_dir = val;
                        }
                    }
                    b"grouping" => {
                        let val = read_attr(e, b"val")?;
                        if let Some(g) = open_group.as_mut() {
                            g.grouping = val;
                        }
                    }
                    b"axId" => {
                        // The group's pairing, not an axis definition's own id:
                        // a group closes before `<c:catAx>` opens, and the
                        // `axis` guard says so rather than relying on it.
                        if axis.is_none()
                            && let Some(g) = open_group.as_mut()
                            && let Some(val) = read_attr(e, b"val")?
                        {
                            g.ax_ids.push(val);
                        }
                    }
                    b"dLbls" => dlbls_depth += 1,
                    b"dLbl" if dlbls_depth > 0 => {
                        in_dlbl = true;
                        gaps.push(ChartGap::PerPointLabel);
                    }
                    b"showVal" if dlbls_depth == 1 && !in_dlbl => {
                        if read_attr(e, b"val")?.as_deref().is_some_and(is_true) {
                            match series.as_mut() {
                                Some(s) => s.data_labels = true,
                                None => {
                                    if let Some(g) = open_group.as_mut() {
                                        g.show_val = true;
                                    }
                                }
                            }
                        }
                    }
                    b"showCatName" | b"showSerName" | b"showPercent" | b"showLegendKey"
                    | b"showBubbleSize"
                        if dlbls_depth > 0 && !in_dlbl =>
                    {
                        if read_attr(e, b"val")?.as_deref().is_some_and(is_true) {
                            gaps.push(ChartGap::LabelKind);
                        }
                    }
                    b"trendline" => gaps.push(ChartGap::Trendline),
                    b"errBars" => gaps.push(ChartGap::ErrorBars),
                    b"scaling" => in_scaling = true,
                    b"min" | b"max" | b"logBase" if in_scaling => {
                        gaps.push(ChartGap::AxisScale);
                    }
                    b"catAx" => axis = Some("cat"),
                    b"valAx" | b"dateAx" | b"serAx" => {
                        // A scatter chart has two value axes; the first is the
                        // horizontal one, so it takes the category slot.
                        axis = Some(
                            if axis.is_none()
                                && spec.x_title.is_empty()
                                && matches!(group.as_deref(), Some("scatterChart"))
                            {
                                "cat"
                            } else {
                                "val"
                            },
                        );
                    }
                    b"legendPos" => spec.legend = read_attr(e, b"val")?,
                    b"title" => in_title = true,
                    b"ser" => series = Some(ChartSeries::default()),
                    b"tx" => slot = Some("tx"),
                    b"cat" | b"xVal" => slot = Some("cat"),
                    b"val" | b"yVal" => slot = Some("val"),
                    b"f" => {
                        in_formula = true;
                        text.clear();
                    }
                    b"v" if slot == Some("tx") => {
                        in_value = true;
                        text.clear();
                    }
                    b"t" if in_title => {
                        in_formula = false;
                        text.clear();
                        in_value = true;
                    }
                    other => {
                        let n = String::from_utf8_lossy(other).into_owned();
                        if n.ends_with("Chart") {
                            // The first group still decides the *chart's* kind.
                            // Every group is now recorded as well, because a
                            // combination chart's later groups are what the
                            // per-series kind and the secondary axis come from.
                            group.get_or_insert(n.clone());
                            if let Some(previous) = open_group.take() {
                                // A group element that never closed. Keeping
                                // what it collected beats discarding a whole
                                // group's series' grouping over one bad tag.
                                groups.push(previous);
                            }
                            open_group = Some(Group {
                                element: n,
                                from: spec.series.len(),
                                ..Group::default()
                            });
                        }
                    }
                }
            }
            Event::Text(ref e) => {
                if in_formula || in_value {
                    text.push_str(&text_of(e)?);
                }
            }
            Event::GeneralRef(ref e) => {
                if in_formula || in_value {
                    text.push_str(&ref_of(e)?);
                }
            }
            Event::End(ref e) => match e.local_name().as_ref() {
                b"f" => {
                    in_formula = false;
                    if let Some(s) = series.as_mut() {
                        match slot {
                            Some("tx") if s.name.is_empty() => s.name = text.clone(),
                            Some("cat") => s.categories = Some(text.clone()),
                            Some("val") => s.values = text.clone(),
                            _ => {}
                        }
                    }
                }
                b"v" => {
                    if in_value
                        && slot == Some("tx")
                        && let Some(s) = series.as_mut()
                    {
                        s.name = text.clone();
                    }
                    in_value = false;
                }
                b"t" if in_title => {
                    let slot = match axis {
                        Some("cat") => &mut spec.x_title,
                        Some("val") => &mut spec.y_title,
                        _ => &mut title_text,
                    };
                    // First run only: a title split across formatting runs
                    // would otherwise concatenate every one of them, and the
                    // first is the whole of it in every file Excel writes.
                    if slot.is_empty() {
                        *slot = text.clone();
                    }
                    in_value = false;
                }
                b"title" => in_title = false,
                b"catAx" | b"valAx" | b"dateAx" | b"serAx" => axis = None,
                b"tx" | b"cat" | b"val" | b"xVal" | b"yVal" => slot = None,
                b"ser" => {
                    if let Some(s) = series.take() {
                        spec.series.push(s);
                    }
                }
                b"dLbls" => dlbls_depth = dlbls_depth.saturating_sub(1),
                b"dLbl" => in_dlbl = false,
                b"scaling" => in_scaling = false,
                other => {
                    let n = String::from_utf8_lossy(other);
                    if n.ends_with("Chart")
                        && let Some(mut g) = open_group.take()
                    {
                        g.element = n.into_owned();
                        groups.push(g);
                    }
                }
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if let Some(g) = open_group.take() {
        groups.push(g);
    }

    spec.title = title_text;
    let first_bar_dir = groups.first().and_then(|g| g.bar_dir.clone());
    spec.kind = group.map(|g| ChartKind::from_element(&g, first_bar_dir.as_deref()));
    stamp_groups(&mut spec, &groups, &mut gaps);

    gaps.sort_unstable();
    gaps.dedup();
    spec.gaps = gaps;
    Ok(spec)
}

/// Whether an OOXML boolean attribute says yes.
fn is_true(val: &str) -> bool {
    matches!(val, "1" | "true")
}

/// Carry each group's facts down onto the series it holds.
///
/// The importer flattens every `<c:ser>` from every group into one list, which
/// is why a combination chart's *data* has always survived import. What was
/// lost was which group each series came from, and this is where that is put
/// back: a series whose group's kind differs from the chart's carries its own
/// kind, and a series whose group pairs to a different `<c:axId>` pair than the
/// first group's is on the secondary axis.
fn stamp_groups(spec: &mut ChartSpec, groups: &[Group], gaps: &mut Vec<ChartGap>) {
    let Some(first) = groups.first() else {
        return;
    };
    let chart_kind = spec.kind.unwrap_or(ChartKind::Unsupported);
    spec.grouping = first
        .grouping
        .as_deref()
        .and_then(|v| ChartGrouping::from_val(&first.element, v));
    let first_stacked = effective_grouping(first).is_some_and(ChartGrouping::is_stacked);

    for (i, g) in groups.iter().enumerate() {
        if g.element.contains("3D") {
            gaps.push(ChartGap::ThreeD);
        }
        // The model carries one grouping for the whole chart, so a second group
        // that stacks differently from the first cannot be drawn as the file
        // asks. Named rather than dropped — this is the stated cost of holding
        // a combination chart as a flat series list.
        if i > 0 && effective_grouping(g).is_some_and(ChartGrouping::is_stacked) != first_stacked {
            gaps.push(ChartGap::PerGroupGrouping);
        }
        let kind = ChartKind::from_element(&g.element, g.bar_dir.as_deref());
        // An empty pairing is not a *different* pairing: a group that names no
        // axis is malformed, and putting it on a secondary axis the file never
        // asked for would invent a picture rather than fall back to one.
        let secondary = !g.ax_ids.is_empty() && g.ax_ids != first.ax_ids;
        let to = groups.get(i + 1).map_or(spec.series.len(), |n| n.from);
        for s in &mut spec.series[g.from.min(to)..to] {
            if kind != chart_kind {
                s.kind = Some(kind);
            }
            s.secondary_axis = secondary;
            s.data_labels |= g.show_val;
        }
    }
}

/// The grouping a group has in effect: what it states, or its element's own
/// schema default.
///
/// The default is not cosmetic. A `<c:barChart>` with no `<c:grouping>` is
/// clustered and a `<c:lineChart>` with none is standard, so comparing two
/// groups by what they *spelled* would call an unstated clustered group
/// different from a stated one.
fn effective_grouping(g: &Group) -> Option<ChartGrouping> {
    if let Some(stated) = g
        .grouping
        .as_deref()
        .and_then(|v| ChartGrouping::from_val(&g.element, v))
    {
        return Some(stated);
    }
    match g.element.as_str() {
        "barChart" | "bar3DChart" => Some(ChartGrouping::Clustered),
        "lineChart" | "line3DChart" | "areaChart" | "area3DChart" => Some(ChartGrouping::Standard),
        _ => None,
    }
}

#[cfg(test)]
mod chart_shape_tests {
    use super::{ChartGap, parse_chart};
    use casual_calc_model::{ChartGrouping, ChartKind};

    /// A `<c:ser>` naming one column.
    fn ser(name: &str, col: &str, dlbls: &str) -> String {
        format!(
            "<c:ser><c:tx><c:v>{name}</c:v></c:tx>{dlbls}\
<c:cat><c:strRef><c:f>S!$A$2:$A$4</c:f></c:strRef></c:cat>\
<c:val><c:numRef><c:f>S!${col}$2:${col}$4</c:f></c:numRef></c:val></c:ser>"
        )
    }

    fn part(plot: &str) -> Vec<u8> {
        format!(
            "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\">\
<c:chart><c:plotArea>{plot}</c:plotArea></c:chart></c:chartSpace>"
        )
        .into_bytes()
    }

    /// **`<c:grouping>` was never read.** A stacked column and a clustered one
    /// were the same `ChartView` in every field, so nothing downstream — the
    /// plotter or the writer — had anything to tell them apart by.
    #[test]
    fn a_stacked_group_is_read_as_stacked() {
        let xml = part(&format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"stacked\"/>{}{}\
<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>",
            ser("Rev", "B", ""),
            ser("Cost", "C", "")
        ));
        let spec = parse_chart(&xml).unwrap();
        assert_eq!(spec.kind, Some(ChartKind::Column));
        assert_eq!(spec.grouping, Some(ChartGrouping::Stacked));

        let xml = part(&format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"percentStacked\"/>{}\
<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>",
            ser("Rev", "B", "")
        ));
        assert_eq!(
            parse_chart(&xml).unwrap().grouping,
            Some(ChartGrouping::PercentStacked)
        );
    }

    /// `ST_Grouping` — what a line or area group takes — has no `clustered`.
    /// A file spelling one is refused rather than mapped to something near it,
    /// because writing it back would be a package Excel declines to open.
    #[test]
    fn a_line_group_cannot_be_clustered() {
        let xml = part(&format!(
            "<c:lineChart><c:grouping val=\"clustered\"/>{}\
<c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart>",
            ser("Rev", "B", "")
        ));
        assert_eq!(parse_chart(&xml).unwrap().grouping, None);
    }

    /// **A combination chart's second group was lost.** Its series all arrived,
    /// because the reader flattens every `<c:ser>` into one list — but every one
    /// of them was drawn as a column, so a line beside two bars became a third
    /// bar and the picture said something the file did not.
    #[test]
    fn a_second_group_gives_its_series_their_own_kind() {
        let xml = part(&format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}{}\
<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>\
<c:lineChart><c:grouping val=\"standard\"/>{}\
<c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart>",
            ser("Rev", "B", ""),
            ser("Cost", "C", ""),
            ser("Margin", "D", "")
        ));
        let spec = parse_chart(&xml).unwrap();
        assert_eq!(spec.kind, Some(ChartKind::Column));
        assert_eq!(spec.series.len(), 3);
        // The chart's own kind, so nothing is written for them.
        assert_eq!(spec.series[0].kind, None);
        assert_eq!(spec.series[1].kind, None);
        assert_eq!(spec.series[2].kind, Some(ChartKind::Line));
        // Sharing the first group's axis pair is what makes them all primary.
        assert!(spec.series.iter().all(|s| !s.secondary_axis));
    }

    /// A secondary axis is a group naming a **second `<c:axId>` pair**, and the
    /// pairing was never read — so the series measured against it shared the
    /// primary extent, where a margin percentage beside revenue in millions is
    /// drawn a rounding error tall.
    #[test]
    fn a_group_with_its_own_axis_pair_is_secondary() {
        let xml = part(&format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}\
<c:axId val=\"111\"/><c:axId val=\"222\"/></c:barChart>\
<c:lineChart><c:grouping val=\"standard\"/>{}\
<c:axId val=\"333\"/><c:axId val=\"444\"/></c:lineChart>",
            ser("Rev", "B", ""),
            ser("Margin", "D", "")
        ));
        let spec = parse_chart(&xml).unwrap();
        assert!(!spec.series[0].secondary_axis);
        assert!(spec.series[1].secondary_axis, "{:?}", spec.series[1]);
    }

    /// `<c:dLbls>` is read from the series that carries it and from the group
    /// that carries it for all of them.
    #[test]
    fn data_labels_are_read_per_series_and_per_group() {
        let show = "<c:dLbls><c:showVal val=\"1\"/></c:dLbls>";
        let xml = part(&format!(
            "<c:barChart><c:barDir val=\"col\"/>{}{}\
<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>",
            ser("Rev", "B", show),
            ser("Cost", "C", "")
        ));
        let spec = parse_chart(&xml).unwrap();
        assert!(spec.series[0].data_labels);
        assert!(!spec.series[1].data_labels);

        // The group's own `<c:dLbls>` follows its series, and turns them all on.
        let xml = part(&format!(
            "<c:barChart><c:barDir val=\"col\"/>{}{}{show}\
<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>",
            ser("Rev", "B", ""),
            ser("Cost", "C", "")
        ));
        let spec = parse_chart(&xml).unwrap();
        assert!(spec.series.iter().all(|s| s.data_labels));
    }

    /// What the model still cannot express is **named**, and the names are a
    /// closed set rather than the file's own element names — a chart part's
    /// element names are attacker-controlled, and a report key taken from one
    /// would let a file spend the whole report budget.
    #[test]
    fn what_cannot_be_drawn_is_reported() {
        let xml = part(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>\
<c:ser><c:tx><c:v>Rev</c:v></c:tx>\
<c:dLbls><c:showVal val=\"1\"/><c:showPercent val=\"1\"/></c:dLbls>\
<c:trendline><c:trendlineType val=\"linear\"/></c:trendline>\
<c:errBars><c:errBarType val=\"both\"/></c:errBars>\
<c:val><c:numRef><c:f>S!$B$2:$B$4</c:f></c:numRef></c:val></c:ser>\
<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>\
<c:valAx><c:axId val=\"2\"/><c:scaling><c:max val=\"500\"/></c:scaling></c:valAx>",
        );
        let spec = parse_chart(&xml).unwrap();
        let gaps = spec.gaps;
        assert!(gaps.contains(&ChartGap::LabelKind), "{gaps:?}");
        assert!(gaps.contains(&ChartGap::Trendline), "{gaps:?}");
        assert!(gaps.contains(&ChartGap::ErrorBars), "{gaps:?}");
        assert!(gaps.contains(&ChartGap::AxisScale), "{gaps:?}");
        // The value label itself *is* expressible, so it is not a gap.
        assert!(spec.series[0].data_labels);
        // Deduplicated and ordered, so the same part always reports the same
        // list however many times a construct occurs.
        let mut sorted = gaps.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(gaps, sorted);
    }

    /// A chart the model has no picture for is reported as such; a 3-D one is
    /// flattened to its two-dimensional picture and reported as degraded.
    #[test]
    fn an_unsupported_and_a_3d_group_are_both_named() {
        let xml = part("<c:radarChart><c:radarStyle val=\"marker\"/></c:radarChart>");
        let spec = parse_chart(&xml).unwrap();
        assert_eq!(spec.kind, Some(ChartKind::Unsupported));

        let xml = part("<c:bar3DChart><c:barDir val=\"col\"/></c:bar3DChart>");
        let spec = parse_chart(&xml).unwrap();
        assert_eq!(spec.kind, Some(ChartKind::Column));
        assert!(spec.gaps.contains(&ChartGap::ThreeD), "{:?}", spec.gaps);
    }

    /// Two groups that stack differently cannot both be drawn — the model
    /// carries one grouping — and that is the stated cost of the flat series
    /// list, so it is named rather than dropped.
    #[test]
    fn two_groups_stacking_differently_is_reported() {
        let xml = part(&format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"stacked\"/>{}\
<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>\
<c:areaChart><c:grouping val=\"standard\"/>{}\
<c:axId val=\"1\"/><c:axId val=\"2\"/></c:areaChart>",
            ser("Rev", "B", ""),
            ser("Cost", "C", "")
        ));
        let spec = parse_chart(&xml).unwrap();
        assert!(
            spec.gaps.contains(&ChartGap::PerGroupGrouping),
            "{:?}",
            spec.gaps
        );
    }
}
