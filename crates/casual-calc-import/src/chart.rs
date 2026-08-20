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

use casual_calc_model::{CellRange, CellRef, ChartKind, ChartSeries, Emu};
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

/// What a chart part plots.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChartSpec {
    /// The chart kind, or `Unsupported`.
    pub kind: Option<ChartKind>,
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

    let mut bar_dir: Option<String> = None;
    let mut group: Option<String> = None;
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
                    b"barDir" => bar_dir = read_attr(e, b"val")?,
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
                            // The first group decides the kind. A combination
                            // chart has several; drawing the first is wrong in
                            // a way that is visible, which beats drawing
                            // nothing at all.
                            group.get_or_insert(n);
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
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    spec.title = title_text;
    spec.kind = group.map(|g| ChartKind::from_element(&g, bar_dir.as_deref()));
    Ok(spec)
}
