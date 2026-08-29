//! Things anchored to the grid rather than in it: charts, pivots, images,
//! comments, hyperlinks and defined names.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

/// One chart's paint items, for the frame the canvas has already computed.
///
/// **The point of `RND-10`, made small enough to land.** The canvas painted
/// charts from its own JavaScript — `drawPie`, `drawBarChart`, `drawLineChart`,
/// `drawAxes`, `drawLegend` — while the PNG renderer painted the same charts
/// from `casual_calc_layout::chart::push_chart`. Two implementations of one
/// picture, so every fix had to be made twice and a divergence between them was
/// invisible until somebody compared a screen to an export.
///
/// The frame comes *from* the caller rather than being derived here. A chart is
/// anchored in cells, and the canvas already resolves that to pixels every
/// frame — including the scroll offset, the frozen panes and the zoom. Deriving
/// it a second time in here would be a second thing to keep in step, which is
/// the exact fault this removes.
#[wasm_bindgen]
pub fn session_chart_items(
    sheet: usize,
    index: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> Result<String, JsError> {
    with_session(|s| {
        let workbook = s.workbook();
        let Some(chart) = workbook
            .sheets
            .get(sheet)
            .and_then(|sh| sh.charts.get(index))
        else {
            return Ok(String::from(r#"{"items":[]}"#));
        };
        let mut list = casual_calc_layout::DisplayList::new();
        casual_calc_layout::chart::push_chart(
            &mut list,
            workbook,
            sheet,
            chart,
            casual_calc_layout::Rect {
                x: i64::from(x),
                y: i64::from(y),
                w: i64::from(w),
                h: i64::from(h),
            },
        );
        serde_json::to_string(&list).map_err(|why| JsError::new(&format!("chart items: {why}")))
    })
    .unwrap_or_else(|| Ok(String::from(r#"{"items":[]}"#)))
}

/// A chart's definition as the host edits it. Distinct from the payload
/// `session_charts` returns, which carries *resolved values* for drawing.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChartWire {
    #[serde(default)]
    index: usize,
    /// Stable identity, unique within the sheet. `index` is a position and
    /// stops naming the same chart the moment one is inserted before it; this
    /// does not. Read-only from the host's side — the engine allocates it.
    #[serde(default)]
    id: u32,
    /// `bar`, `column`, `line`, `area`, `pie`, `doughnut` or `scatter`.
    kind: String,
    #[serde(default)]
    title: String,
    /// The cells the frame covers, `[r0, c0, r1, c1]`.
    anchor: [u32; 4],
    #[serde(default)]
    series: Vec<SeriesWire>,
    /// `r`, `l`, `t`, `b`, `tr`, or absent for no legend.
    #[serde(default)]
    legend: Option<String>,
    /// The frame's offsets from its anchor cells, in EMU — what lets an edge
    /// sit between gridlines instead of snapping to one.
    #[serde(default)]
    from_offset: [i64; 2],
    #[serde(default)]
    to_offset: [i64; 2],
    #[serde(default)]
    x_title: String,
    #[serde(default)]
    y_title: String,
    /// Whether this still writes back from a retained part. Read-only: it is
    /// cleared by editing, not by asking.
    #[serde(default)]
    imported: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SeriesWire {
    #[serde(default)]
    name: String,
    /// The formula naming the category labels, e.g. `Sheet1!$A$2:$A$9`.
    #[serde(default)]
    categories: Option<String>,
    /// The formula naming the plotted values.
    values: String,
}

pub(crate) fn chart_kind_token(kind: ChartKind) -> &'static str {
    match kind {
        ChartKind::Bar => "bar",
        ChartKind::Column => "column",
        ChartKind::Line => "line",
        ChartKind::Area => "area",
        ChartKind::Pie => "pie",
        ChartKind::Doughnut => "doughnut",
        ChartKind::Scatter => "scatter",
        ChartKind::Unsupported => "unsupported",
    }
}

pub(crate) fn chart_kind_from(token: &str) -> ChartKind {
    match token {
        "bar" => ChartKind::Bar,
        "line" => ChartKind::Line,
        "area" => ChartKind::Area,
        "pie" => ChartKind::Pie,
        "doughnut" => ChartKind::Doughnut,
        "scatter" => ChartKind::Scatter,
        _ => ChartKind::Column,
    }
}

pub(crate) fn chart_to_wire(chart: &ChartView, index: usize) -> ChartWire {
    ChartWire {
        id: chart.id,
        index,
        kind: chart_kind_token(chart.kind).to_owned(),
        title: chart.title.clone(),
        anchor: [
            chart.anchor.start.row,
            chart.anchor.start.col,
            chart.anchor.end.row,
            chart.anchor.end.col,
        ],
        series: chart
            .series
            .iter()
            .map(|s| SeriesWire {
                name: s.name.clone(),
                categories: s.categories.clone(),
                values: s.values.clone(),
            })
            .collect(),
        legend: chart.legend.clone(),
        from_offset: [chart.from_offset.x, chart.from_offset.y],
        to_offset: [chart.to_offset.x, chart.to_offset.y],
        x_title: chart.x_title.clone(),
        y_title: chart.y_title.clone(),
        imported: chart.part.is_some(),
    }
}

/// Append a reply to the thread on a cell. A no-op if the cell has no thread —
/// a reply without an opening remark has nothing to attach to.
#[wasm_bindgen]
pub fn session_reply_comment(
    sheet: usize,
    row: u32,
    col: u32,
    text: &str,
    author: &str,
    created: &str,
) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if let Some(thread) = data
            .comments
            .iter_mut()
            .find(|c| c.at.row == row && c.at.col == col)
        {
            thread.replies.push(CommentReply {
                text: text.to_owned(),
                author: (!author.is_empty()).then(|| author.to_owned()),
                created: (!created.is_empty()).then(|| created.to_owned()),
            });
        }
    })
}

/// Mark a cell's thread resolved or reopened.
#[wasm_bindgen]
pub fn session_resolve_comment(
    sheet: usize,
    row: u32,
    col: u32,
    resolved: bool,
) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        if let Some(thread) = data
            .comments
            .iter_mut()
            .find(|c| c.at.row == row && c.at.col == col)
        {
            thread.resolved = resolved;
        }
    })
}

/// Set (or, with an empty target and location, remove) the hyperlink on a cell.
///
/// `target` is an external URI and `location` an anchor inside this workbook;
/// either may be empty, and a link with both means "open that document at this
/// anchor". Goes through the metadata log, so it is undoable like any edit.
#[wasm_bindgen]
pub fn session_set_hyperlink(
    sheet: usize,
    row: u32,
    col: u32,
    target: &str,
    location: &str,
    tooltip: &str,
    display: &str,
) -> Result<(), JsError> {
    let target = target.trim().to_owned();
    let location = location.trim().to_owned();
    let tooltip = tooltip.trim().to_owned();
    let display = display.trim().to_owned();
    edit_sheet_metadata(sheet, move |_, data| {
        data.hyperlinks
            .retain(|h| !(h.range.start.row == row && h.range.start.col == col));
        // Neither destination means "remove": a link with nowhere to go would
        // render as a live link that does nothing.
        if target.is_empty() && location.is_empty() {
            return;
        }
        data.hyperlinks.push(Hyperlink {
            range: CellRange::new(CellRef::new(row, col), CellRef::new(row, col)),
            target: (!target.is_empty()).then_some(target),
            location: (!location.is_empty()).then_some(location),
            tooltip: (!tooltip.is_empty()).then_some(tooltip),
            display: (!display.is_empty()).then_some(display),
        });
    })
}

/// The hyperlink covering a cell as JSON, or `null`.
#[wasm_bindgen]
pub fn session_hyperlink_at(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let Some(link) = s.workbook().sheets.get(sheet).and_then(|sh| {
            sh.hyperlinks.iter().find(|h| {
                row >= h.range.start.row
                    && row <= h.range.end.row
                    && col >= h.range.start.col
                    && col <= h.range.end.col
            })
        }) else {
            return "null".to_owned();
        };
        let field = |v: &Option<String>| v.as_deref().map_or("null".to_owned(), json_string);
        format!(
            "{{\"target\":{},\"location\":{},\"tooltip\":{},\"display\":{}}}",
            field(&link.target),
            field(&link.location),
            field(&link.tooltip),
            field(&link.display),
        )
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// The linked cells within a range as JSON `[{r,c}, …]`, so the grid can
/// underline them without asking cell by cell.
#[wasm_bindgen]
pub fn session_hyperlink_cells(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let mut items = Vec::new();
        for link in &sh.hyperlinks {
            for r in link.range.start.row..=link.range.end.row {
                for c in link.range.start.col..=link.range.end.col {
                    if r >= r0 && r <= r1 && c >= c0 && c <= c1 {
                        items.push(format!("{{\"r\":{r},\"c\":{c}}}"));
                    }
                }
            }
        }
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// The contiguous block of populated cells around `(row, col)`, as JSON
/// `{r0,c0,r1,c1}`, or `null` when the cell is empty.
///
/// What Ctrl+T uses when the selection is a single cell: asking someone to
/// select the whole table first is work the app can do, and doing it here means
/// the same rule applies wherever a block is needed.
#[wasm_bindgen]
pub fn session_block_bounds(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "null".to_owned();
        };
        let filled = |r: u32, c: u32| {
            sh.cells
                .get(CellRef::new(r, c))
                .is_some_and(|cell| !cell.value.is_empty() || cell.formula.is_some())
        };
        if !filled(row, col) {
            return "null".to_owned();
        }
        // Walk out along the row and column, then square the block off. A
        // ragged region grows to its bounding box, which is what a user means
        // by "this table" even when one corner is blank.
        let (mut r0, mut r1, mut c0, mut c1) = (row, row, col, col);
        while r0 > 0 && (c0..=c1).any(|c| filled(r0 - 1, c)) {
            r0 -= 1;
        }
        while c0 > 0 && (r0..=r1).any(|r| filled(r, c0 - 1)) {
            c0 -= 1;
        }
        // Bounded so a pathological sheet cannot make this walk forever.
        let limit = 1_048_576u32;
        while r1 + 1 < limit && (c0..=c1).any(|c| filled(r1 + 1, c)) {
            r1 += 1;
        }
        while c1 + 1 < 16_384 && (r0..=r1).any(|r| filled(r, c1 + 1)) {
            c1 += 1;
        }
        format!("{{\"r0\":{r0},\"c0\":{c0},\"r1\":{r1},\"c1\":{c1}}}")
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// The topmost chart whose frame covers a cell, or `null`.
///
/// Topmost rather than first: charts overlap, and the one drawn last is the one
/// a click lands on.
#[wasm_bindgen]
pub fn session_chart_at(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "null".to_owned();
        };
        let found = sh.charts.iter().enumerate().rev().find(|(_, c)| {
            row >= c.anchor.start.row
                && row <= c.anchor.end.row
                && col >= c.anchor.start.col
                && col <= c.anchor.end.col
        });
        match found {
            Some((i, c)) => {
                serde_json::to_string(&chart_to_wire(c, i)).unwrap_or_else(|_| "null".to_owned())
            }
            None => "null".to_owned(),
        }
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// A1-quote a sheet name for a chart reference, as Excel does.
///
/// A name holding a space or an apostrophe has to be quoted or the reference
/// does not parse — and an unparseable series reference is a chart that draws
/// nothing, with no message saying why.
pub(crate) fn quote_sheet(name: &str) -> String {
    let plain = name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        && !name.chars().next().is_some_and(|c| c.is_ascii_digit());
    if plain {
        name.to_owned()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

pub(crate) fn abs_ref(sheet_name: &str, r0: u32, c0: u32, r1: u32, c1: u32) -> String {
    format!(
        "{}!${}${}:${}${}",
        quote_sheet(sheet_name),
        casual_calc_formula::column_to_letters(c0),
        r0 + 1,
        casual_calc_formula::column_to_letters(c1),
        r1 + 1
    )
}

/// Create a chart over a data range — Excel's Insert ▸ Chart.
///
/// The range is read the way Excel reads it: the first column is the category
/// labels when it holds text, and each remaining column is a series named by
/// its header. Guessing this is the difference between one click and a dialog
/// asking four questions the data already answers.
///
/// Returns the new chart's index.
#[wasm_bindgen]
pub fn session_create_chart(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    kind: &str,
) -> Result<usize, JsError> {
    let (rr0, cc0, rr1, cc1) = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
    let kind = chart_kind_from(kind);
    let built = with_session(|s| {
        let sh = s.workbook().sheets.get(sheet)?;
        let name = sh.name.clone();
        let text_at = |r: u32, c: u32| -> Option<String> {
            let cell = sh.cells.get(CellRef::new(r, c))?;
            match cell.value {
                CellValue::SharedString(_) | CellValue::InlineString(_) => {
                    Some(value_text(s.workbook(), &cell.value))
                }
                _ => None,
            }
        };
        // A header row is one whose cells are text over columns that are not.
        let has_headers = rr1 > rr0
            && (cc0..=cc1).any(|c| text_at(rr0, c).is_some())
            && (cc0..=cc1).any(|c| text_at(rr0 + 1, c).is_none());
        // A label column is a leading column of text beside numeric ones.
        let label_col = (cc1 > cc0
            && text_at(if has_headers { rr0 + 1 } else { rr0 }, cc0).is_some())
        .then_some(cc0);

        let first_data_row = if has_headers { rr0 + 1 } else { rr0 };
        let categories = label_col
            .map(|c| abs_ref(&name, first_data_row, c, rr1, c))
            // With no label column the categories are the row numbers, which
            // Excel leaves implicit; a chart with no `<c:cat>` numbers them
            // 1, 2, 3 by itself.
            .filter(|_| kind != ChartKind::Scatter || cc1 > cc0);

        let mut series = Vec::new();
        for c in cc0..=cc1 {
            if Some(c) == label_col {
                continue;
            }
            series.push(casual_calc_model::ChartSeries {
                name: if has_headers {
                    text_at(rr0, c).unwrap_or_default()
                } else {
                    String::new()
                },
                categories: categories.clone(),
                values: abs_ref(&name, first_data_row, c, rr1, c),
            });
        }
        Some((series, sh.charts.len()))
    })
    .flatten()
    .ok_or_else(|| JsError::new("no such sheet"))?;
    let (series, index) = built;
    if series.is_empty() {
        return Err(JsError::new("select at least one column of values"));
    }

    // Placed to the right of the data, eight columns by fifteen rows — Excel's
    // own default frame, and clear of the range it plots.
    let anchor = CellRange::new(CellRef::new(rr0, cc1 + 2), CellRef::new(rr0 + 14, cc1 + 9));
    let mut chart = ChartView::new(anchor, kind);
    // A legend only earns its space once there is more than one series to tell
    // apart; Excel adds one at two.
    if series.len() > 1 {
        chart.legend = Some("r".to_owned());
    }
    chart.series = series;
    edit_sheet_metadata(sheet, move |sh, data| {
        // Identity is the sheet's to hand out, not the chart's to invent.
        chart.id = sh.next_chart_id();
        data.charts.push(chart);
    })?;
    Ok(index)
}

/// Replace a chart's definition.
///
/// This is what detaches an imported chart: the retained part described the
/// chart as it was, and keeping it would leave the file disagreeing with
/// itself. Returns the part path dropped, or `""`.
#[wasm_bindgen]
pub fn session_set_chart(sheet: usize, index: usize, json: &str) -> Result<String, JsError> {
    let wire: ChartWire =
        serde_json::from_str(json).map_err(|e| JsError::new(&format!("bad chart: {e}")))?;
    let mut dropped = String::new();
    edit_sheet_metadata(sheet, |_, data| {
        let Some(chart) = data.charts.get_mut(index) else {
            return;
        };
        chart.kind = chart_kind_from(&wire.kind);
        chart.title = wire.title;
        chart.anchor = CellRange::new(
            CellRef::new(wire.anchor[0], wire.anchor[1]),
            CellRef::new(wire.anchor[2], wire.anchor[3]),
        );
        chart.series = wire
            .series
            .into_iter()
            .map(|s| casual_calc_model::ChartSeries {
                name: s.name,
                categories: s.categories.filter(|c| !c.trim().is_empty()),
                values: s.values,
            })
            .collect();
        chart.legend = wire.legend.filter(|p| !p.is_empty());
        chart.from_offset = casual_calc_model::Emu {
            x: wire.from_offset[0],
            y: wire.from_offset[1],
        };
        chart.to_offset = casual_calc_model::Emu {
            x: wire.to_offset[0],
            y: wire.to_offset[1],
        };
        chart.x_title = wire.x_title;
        chart.y_title = wire.y_title;
        dropped = chart.detach().unwrap_or_default();
    })?;
    Ok(dropped)
}

/// Remove a chart, and its retained part if it had one.
#[wasm_bindgen]
pub fn session_delete_chart(sheet: usize, index: usize) -> Result<(), JsError> {
    let mut dropped = String::new();
    edit_sheet_metadata(sheet, |_, data| {
        if index < data.charts.len() {
            dropped = data.charts.remove(index).part.unwrap_or_default();
        }
    })?;
    let _ = dropped;
    Ok(())
}

// ---------------------------------------------------------------------------
// Pivot tables.
//
// The wire shape is deliberately its own type rather than the model's. Fields
// travel as *offsets into the source range* on both sides, which is what the
// model stores; sheets travel as indices, because that is what the host knows.
// Serialising `PivotTable` directly would put a 32-hex-character `SheetId` in
// front of the UI and make every panel translate it back.
// ---------------------------------------------------------------------------

/// A pivot's definition as the host sees it.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PivotWire {
    /// Its position in the sheet's pivot list — the handle every other call
    /// takes. Ignored on the way in.
    #[serde(default)]
    index: usize,
    name: String,
    /// Index of the sheet holding the source records.
    source_sheet: usize,
    /// The source rectangle, header row included.
    source: [u32; 4],
    /// Top-left of the report block.
    anchor: [u32; 2],
    #[serde(default)]
    rows: Vec<AxisWire>,
    #[serde(default)]
    cols: Vec<AxisWire>,
    #[serde(default)]
    filters: Vec<FilterWire>,
    #[serde(default)]
    values: Vec<ValueWire>,
    #[serde(default = "yes")]
    row_grand_totals: bool,
    #[serde(default = "yes")]
    col_grand_totals: bool,
    #[serde(default)]
    style: String,
    /// The block the last refresh wrote, or `null`. Read-only.
    #[serde(default)]
    output: Option<[u32; 4]>,
    /// Whether this came from a file and still writes back from its own bytes.
    /// Read-only: it is cleared by refreshing, not by asking.
    #[serde(default)]
    imported: bool,
    /// The source header names, so a panel can label the fields without a
    /// second call. Read-only.
    #[serde(default)]
    fields: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AxisWire {
    field: u32,
    #[serde(default)]
    sort: String,
    #[serde(default = "yes")]
    subtotal: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FilterWire {
    field: u32,
    #[serde(default)]
    selected: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ValueWire {
    field: u32,
    #[serde(default)]
    aggregate: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    number_format: Option<String>,
}

pub(crate) fn yes() -> bool {
    true
}

pub(crate) fn sort_token(sort: PivotSort) -> &'static str {
    match sort {
        PivotSort::Ascending => "ascending",
        PivotSort::Descending => "descending",
        PivotSort::DataSource => "dataSource",
    }
}

pub(crate) fn sort_from(token: &str) -> PivotSort {
    match token {
        "descending" => PivotSort::Descending,
        "dataSource" => PivotSort::DataSource,
        _ => PivotSort::Ascending,
    }
}

pub(crate) fn pivot_to_wire(workbook: &Workbook, pivot: &PivotTable, index: usize) -> PivotWire {
    let rect = |r: CellRange| [r.start.row, r.start.col, r.end.row, r.end.col];
    PivotWire {
        index,
        name: pivot.name.clone(),
        source_sheet: workbook
            .sheets
            .iter()
            .position(|s| s.id == pivot.source_sheet)
            .unwrap_or(0),
        source: rect(pivot.source),
        anchor: [pivot.anchor.row, pivot.anchor.col],
        rows: pivot
            .rows
            .iter()
            .map(|f| AxisWire {
                field: f.source_column,
                sort: sort_token(f.sort).to_owned(),
                subtotal: f.subtotal,
            })
            .collect(),
        cols: pivot
            .cols
            .iter()
            .map(|f| AxisWire {
                field: f.source_column,
                sort: sort_token(f.sort).to_owned(),
                subtotal: f.subtotal,
            })
            .collect(),
        filters: pivot
            .filters
            .iter()
            .map(|f| FilterWire {
                field: f.source_column,
                selected: f.selected.clone(),
            })
            .collect(),
        values: pivot
            .values
            .iter()
            .map(|v| ValueWire {
                field: v.source_column,
                aggregate: v.aggregate.token().to_owned(),
                name: v.name.clone(),
                number_format: v.number_format.clone(),
            })
            .collect(),
        row_grand_totals: pivot.row_grand_totals,
        col_grand_totals: pivot.col_grand_totals,
        style: pivot.style.clone(),
        output: pivot.output.map(rect),
        imported: pivot.part.is_some(),
        fields: pivot_fields(workbook, pivot),
    }
}

/// Every pivot on a sheet, as JSON.
#[wasm_bindgen]
pub fn session_pivots(sheet: usize) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let items: Vec<PivotWire> = sh
            .pivots
            .iter()
            .enumerate()
            .map(|(i, p)| pivot_to_wire(s.workbook(), p, i))
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_owned())
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// The pivot whose report covers a cell, as JSON, or `null`.
///
/// This is what makes the panel follow the cursor: clicking anywhere in a
/// report opens that pivot rather than making the user find it in a list.
#[wasm_bindgen]
pub fn session_pivot_at(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "null".to_owned();
        };
        let found = sh.pivots.iter().enumerate().find(|(_, p)| {
            p.output.is_some_and(|r| {
                row >= r.start.row && row <= r.end.row && col >= r.start.col && col <= r.end.col
            })
        });
        match found {
            Some((i, p)) => serde_json::to_string(&pivot_to_wire(s.workbook(), p, i))
                .unwrap_or_else(|_| "null".to_owned()),
            None => "null".to_owned(),
        }
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// The distinct values of one source field, in the order the pivot groups them
/// — the checklist a page filter shows.
#[wasm_bindgen]
pub fn session_pivot_items(sheet: usize, index: usize, field: u32) -> String {
    with_session(|s| {
        let Some(p) = s
            .workbook()
            .sheets
            .get(sheet)
            .and_then(|sh| sh.pivots.get(index))
        else {
            return "[]".to_owned();
        };
        serde_json::to_string(&casual_calc_eval::pivot::field_items(
            s.workbook(),
            p,
            field,
        ))
        .unwrap_or_else(|_| "[]".to_owned())
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// The source header names for a range, before any pivot exists over it — what
/// the "create pivot" dialog lists.
#[wasm_bindgen]
pub fn session_pivot_fields_for(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let probe = PivotTable::new(
            0,
            String::new(),
            sh.id,
            CellRange::new(CellRef::new(r0, c0), CellRef::new(r1, c1)),
            CellRef::new(0, 0),
        );
        serde_json::to_string(&pivot_fields(s.workbook(), &probe))
            .unwrap_or_else(|_| "[]".to_owned())
    })
    .unwrap_or_else(|| "[]".to_owned())
}

pub(crate) fn pivot_fields(workbook: &Workbook, pivot: &PivotTable) -> Vec<String> {
    casual_calc_eval::pivot::field_names(workbook, pivot)
}

/// Create a pivot over `source`, anchored at `(row, col)` on `dest`.
///
/// Nothing is on any axis yet, so nothing is written: an empty pivot is a
/// placeholder for the panel to fill in, and writing a report before the user
/// has chosen a measure would put a block of nothing on their sheet.
/// Returns the new pivot's index.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_create_pivot(
    source_sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    dest: usize,
    row: u32,
    col: u32,
    name: &str,
) -> Result<usize, JsError> {
    let (rr0, cc0, rr1, cc1) = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
    if rr1 <= rr0 {
        return Err(JsError::new(
            "a pivot needs a header row and at least one row of data under it",
        ));
    }
    let (source_id, taken, next_id) = with_session(|s| {
        let id = s.workbook().sheets.get(source_sheet).map(|sh| sh.id);
        let names: Vec<String> = s
            .workbook()
            .sheets
            .iter()
            .flat_map(|sh| sh.pivots.iter().map(|p| p.name.to_ascii_lowercase()))
            .collect();
        let next = s
            .workbook()
            .sheets
            .iter()
            .flat_map(|sh| sh.pivots.iter().map(|p| p.id))
            .max()
            .unwrap_or(0)
            + 1;
        (id, names, next)
    })
    .ok_or_else(|| JsError::new("no session"))?;
    let source_id = source_id.ok_or_else(|| JsError::new("no such source sheet"))?;

    // `GETPIVOTDATA` addresses a pivot by name, so two sharing one would make
    // every reference to it ambiguous.
    let base = {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            "PivotTable"
        } else {
            trimmed
        }
    };
    let mut unique = base.to_owned();
    let mut n = 1;
    while taken.contains(&unique.to_ascii_lowercase()) {
        n += 1;
        unique = format!("{base}{n}");
    }

    let pivot = PivotTable::new(
        next_id,
        unique,
        source_id,
        CellRange::new(CellRef::new(rr0, cc0), CellRef::new(rr1, cc1)),
        CellRef::new(row, col),
    );
    let mut index = 0;
    edit_sheet_metadata(dest, |_, data| {
        index = data.pivots.len();
        data.pivots.push(pivot);
    })?;
    Ok(index)
}

/// Replace a pivot's definition and rewrite its report, as one undoable step.
///
/// Definition and figures travel together: taking a field off the row axis and
/// leaving the old rows on screen would show a report that answers a question
/// nobody asked any more.
#[wasm_bindgen]
pub fn session_set_pivot(sheet: usize, index: usize, json: &str) -> Result<String, JsError> {
    let wire: PivotWire =
        serde_json::from_str(json).map_err(|e| JsError::new(&format!("bad pivot: {e}")))?;
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let source_id = session
            .workbook()
            .sheets
            .get(wire.source_sheet)
            .map(|sh| sh.id)
            .ok_or_else(|| JsError::new("no such source sheet"))?;
        let existing = session
            .workbook()
            .sheets
            .get(sheet)
            .and_then(|sh| sh.pivots.get(index))
            .cloned()
            .ok_or_else(|| JsError::new("no such pivot"))?;

        let updated = PivotTable {
            id: existing.id,
            name: if wire.name.trim().is_empty() {
                existing.name.clone()
            } else {
                wire.name.trim().to_owned()
            },
            source_sheet: source_id,
            source: CellRange::new(
                CellRef::new(wire.source[0], wire.source[1]),
                CellRef::new(wire.source[2], wire.source[3]),
            ),
            anchor: CellRef::new(wire.anchor[0], wire.anchor[1]),
            rows: wire
                .rows
                .iter()
                .map(|f| PivotAxisField {
                    source_column: f.field,
                    sort: sort_from(&f.sort),
                    subtotal: f.subtotal,
                })
                .collect(),
            cols: wire
                .cols
                .iter()
                .map(|f| PivotAxisField {
                    source_column: f.field,
                    sort: sort_from(&f.sort),
                    subtotal: f.subtotal,
                })
                .collect(),
            filters: wire
                .filters
                .iter()
                .map(|f| PivotFilterField {
                    source_column: f.field,
                    selected: f.selected.clone(),
                })
                .collect(),
            values: wire
                .values
                .iter()
                .map(|v| PivotValueField {
                    source_column: v.field,
                    aggregate: PivotAggregate::from_token(&v.aggregate),
                    name: v.name.clone(),
                    number_format: v
                        .number_format
                        .clone()
                        .filter(|code| !code.trim().is_empty()),
                })
                .collect(),
            row_grand_totals: wire.row_grand_totals,
            col_grand_totals: wire.col_grand_totals,
            style: wire.style.clone(),
            // Not from the host: the extent is whatever the last refresh
            // actually wrote, and the retained part is dropped by refreshing,
            // not by being asked to.
            output: existing.output,
            part: existing.part.clone(),
        };
        apply_pivot(session, sheet, index, updated)
    })
}

/// Recompute a pivot from its source and rewrite the report.
#[wasm_bindgen]
pub fn session_refresh_pivot(sheet: usize, index: usize) -> Result<String, JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let existing = session
            .workbook()
            .sheets
            .get(sheet)
            .and_then(|sh| sh.pivots.get(index))
            .cloned()
            .ok_or_else(|| JsError::new("no such pivot"))?;
        apply_pivot(session, sheet, index, existing)
    })
}

/// Install a definition and its report as one operation.
///
/// The plan is computed against the workbook as it is, then the definition and
/// every cell it implies go through the transaction layer together. Writing the
/// cells outside it would leave undo reversing the definition while the figures
/// stayed, which reads as corruption rather than as an undo.
pub(crate) fn apply_pivot(
    session: &mut WorkbookSession,
    sheet: usize,
    index: usize,
    updated: PivotTable,
) -> Result<String, JsError> {
    let Some(sh) = session.workbook().sheets.get(sheet).cloned() else {
        return Err(JsError::new("no such sheet"));
    };
    // An empty pivot writes nothing: it is a placeholder the panel is still
    // filling in, and a block of nothing on the sheet is worse than a wait.
    if updated.values.is_empty() {
        let mut data = SheetMetadata::capture(&sh);
        data.pivots[index] = updated;
        session
            .edit(EditOperation::set_sheet_metadata(sheet, data))
            .map_err(js)?;
        return Ok(String::new());
    }

    // The planner reads the definition from the workbook, and this one is not
    // installed yet. It goes in directly, is planned against, and comes straight
    // back out — rather than planning against a copy of the workbook, which at
    // the capacity target means duplicating a million cells on every keystroke
    // in the panel. Nothing between these two lines can fail in a way that
    // leaves the definition installed: `plan` writes no cell, and its error is
    // held until the old one is back.
    let previous = std::mem::replace(
        &mut session.workbook_mut().sheets[sheet].pivots[index],
        updated,
    );
    let planned = casual_calc_eval::pivot::plan(session.workbook_mut(), sheet, index);
    let mut committed = std::mem::replace(
        &mut session.workbook_mut().sheets[sheet].pivots[index],
        previous,
    );
    let plan = match planned {
        Ok(plan) => plan,
        Err(e) => return Err(JsError::new(&e.to_string())),
    };
    // The strings and styles the plan's cells name were interned during it and
    // are already in the workbook's tables; only the extent still has to be
    // recorded, and it travels with the definition so undo takes both back.
    committed.output = Some(plan.range);

    let mut data = SheetMetadata::capture(&sh);
    data.pivots[index] = committed;
    // Column widths ride in the same metadata bundle rather than as their own
    // operations: one Ctrl+Z after a layout change must not give back a column
    // width and leave the report changed.
    for (col, width) in plan.widths {
        data.columns.sizes.insert(col, width);
    }
    let mut ops: Vec<EditOperation> = vec![EditOperation::set_sheet_metadata(sheet, data)];
    for (at, cell) in plan.cells {
        ops.push(EditOperation::SetCell { sheet, at, cell });
    }
    session.edit(EditOperation::Batch(ops)).map_err(js)?;
    // Detaching drops retained parts, which no operation reverses; done last,
    // and only once the report is on the sheet.
    let dropped = casual_calc_eval::pivot::detach(session.workbook_mut(), sheet, index);
    Ok(dropped.join("\n"))
}

/// Recompute every pivot in the workbook — Excel's Refresh All.
///
/// Deliberately a command rather than something an edit triggers, which is also
/// Excel's behaviour: a pivot summarizes a snapshot, and having the report move
/// under the cursor while its source is being typed makes both unreadable.
///
/// Returns the pivots that refused, one `name: reason` per line, so the host can
/// say which and why instead of failing the whole command over one collision.
#[wasm_bindgen]
pub fn session_refresh_all_pivots() -> String {
    let counts: Vec<usize> = with_session(|s| {
        s.workbook()
            .sheets
            .iter()
            .map(|sh| sh.pivots.len())
            .collect()
    })
    .unwrap_or_default();
    let mut problems: Vec<String> = Vec::new();
    for (sheet, count) in counts.iter().enumerate() {
        for index in 0..*count {
            let name = with_session(|s| {
                s.workbook().sheets[sheet]
                    .pivots
                    .get(index)
                    .map(|p| p.name.clone())
            })
            .flatten()
            .unwrap_or_default();
            if session_refresh_pivot(sheet, index).is_err() {
                problems.push(name);
            }
        }
    }
    problems.join("\n")
}

/// Whether a cell is inside a pivot's report, and so must not be typed into.
///
/// Excel refuses the edit rather than letting it stand until the next refresh
/// wipes it, and so does this: a value that survives until an unrelated action
/// erases it is worse than one that was never accepted. Returns the pivot's
/// name, or `""` when the cell is free.
#[wasm_bindgen]
pub fn session_pivot_blocks(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        s.workbook()
            .sheets
            .get(sheet)
            .and_then(|sh| {
                sh.pivots.iter().find(|p| {
                    p.output.is_some_and(|r| {
                        row >= r.start.row
                            && row <= r.end.row
                            && col >= r.start.col
                            && col <= r.end.col
                    })
                })
            })
            .map(|p| p.name.clone())
            .unwrap_or_default()
    })
    .unwrap_or_default()
}

/// Remove a pivot, clearing the block it wrote.
#[wasm_bindgen]
pub fn session_delete_pivot(sheet: usize, index: usize) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(sh) = session.workbook().sheets.get(sheet).cloned() else {
            return Ok(());
        };
        let Some(pivot) = sh.pivots.get(index).cloned() else {
            return Ok(());
        };
        let mut data = SheetMetadata::capture(&sh);
        data.pivots.remove(index);
        let mut ops: Vec<EditOperation> = vec![EditOperation::set_sheet_metadata(sheet, data)];
        // The report goes with the definition. Leaving the figures behind would
        // strand a block that looks live, updates from nothing, and quietly
        // ages.
        if let Some(range) = pivot.output {
            for row in range.start.row..=range.end.row {
                for col in range.start.col..=range.end.col {
                    ops.push(EditOperation::ClearCell {
                        sheet,
                        at: CellRef::new(row, col),
                    });
                }
            }
        }
        session.edit(EditOperation::Batch(ops)).map_err(js)
    })
}

/// A cell's comment text, or `""` if it has none.
#[wasm_bindgen]
pub fn session_comment_at(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        s.workbook()
            .sheets
            .get(sheet)
            .and_then(|sh| {
                sh.comments
                    .iter()
                    .find(|c| c.at.row == row && c.at.col == col)
            })
            .map(|c| c.text.clone())
            .unwrap_or_default()
    })
    .unwrap_or_default()
}

/// A cell's whole thread as JSON, or `null` if it has none:
/// `{"text","author","created","resolved",replies:[{"text","author","created"}]}`.
#[wasm_bindgen]
pub fn session_comment_thread(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let Some(thread) = s.workbook().sheets.get(sheet).and_then(|sh| {
            sh.comments
                .iter()
                .find(|c| c.at.row == row && c.at.col == col)
        }) else {
            return "null".to_owned();
        };
        let entry = |text: &str, author: &Option<String>, created: &Option<String>| {
            format!(
                "{{\"text\":{},\"author\":{},\"created\":{}}}",
                json_string(text),
                author.as_deref().map_or("null".to_owned(), json_string),
                created.as_deref().map_or("null".to_owned(), json_string),
            )
        };
        let replies: Vec<String> = thread
            .replies
            .iter()
            .map(|r| entry(&r.text, &r.author, &r.created))
            .collect();
        format!(
            "{{\"text\":{},\"author\":{},\"created\":{},\"resolved\":{},\"replies\":[{}]}}",
            json_string(&thread.text),
            thread
                .author
                .as_deref()
                .map_or("null".to_owned(), json_string),
            thread
                .created
                .as_deref()
                .map_or("null".to_owned(), json_string),
            thread.resolved,
            replies.join(",")
        )
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// The commented cells within a range as JSON `[{r,c}, …]` — the editor draws a
/// note indicator on each.
#[wasm_bindgen]
pub fn session_comments(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let items: Vec<String> = sh
            .comments
            .iter()
            .filter(|c| c.at.row >= r0 && c.at.row <= r1 && c.at.col >= c0 && c.at.col <= c1)
            .map(|c| format!("{{\"r\":{},\"c\":{}}}", c.at.row, c.at.col))
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Define (or replace) a workbook-scoped named range. `refers_to` is a formula
/// such as `Sheet1!A1:B2` or `A1`. Rejects empty names and names that collide
/// with a cell reference. Recalculates so name-using formulas update.
#[wasm_bindgen]
pub fn session_define_name(name: &str, refers_to: &str) -> Result<(), JsError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(JsError::new("name cannot be empty"));
    }
    if casual_calc_formula::parse_a1(name).is_some() {
        return Err(JsError::new("that name looks like a cell reference"));
    }
    let expr = parse(refers_to.trim().trim_start_matches('='))
        .map_err(|e| JsError::new(&e.to_string()))?;
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let mut names = session.workbook().defined_names.clone();
        names.retain(|d| d.name != name);
        names.push(DefinedName {
            name: name.to_owned(),
            sheet: None,
            formula: expr,
        });
        // Undoable, dirties the doc, and recalculates (a new/changed name can
        // resolve previously-#NAME? formulas or change what they resolve to).
        session
            .edit(EditOperation::SetDefinedNames(names))
            .map_err(js)
    })
}

/// Delete a defined name (undoable); recalculates so dependents become `#NAME?`.
#[wasm_bindgen]
pub fn session_delete_name(name: &str) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let mut names = session.workbook().defined_names.clone();
        names.retain(|d| d.name != name);
        session
            .edit(EditOperation::SetDefinedNames(names))
            .map_err(js)
    })
}

/// All defined names as JSON `[{name, refersTo}, …]`.
#[wasm_bindgen]
pub fn session_names() -> String {
    with_session(|s| {
        let items: Vec<String> = s
            .workbook()
            .defined_names
            .iter()
            .map(|d| {
                format!(
                    "{{\"name\":{},\"refersTo\":{}}}",
                    json_string(&d.name),
                    json_string(&d.formula.to_string())
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// The target range of a defined name as JSON `{r0,c0,r1,c1}`, or `null` if the
/// name is unknown or refers to something other than a cell/range.
#[wasm_bindgen]
pub fn session_name_target(name: &str) -> String {
    with_session(|s| {
        let Some(d) = s.workbook().defined_names.iter().find(|d| d.name == name) else {
            return "null".to_owned();
        };
        match &d.formula {
            Expr::Reference(r) => {
                format!(
                    "{{\"r0\":{},\"c0\":{},\"r1\":{},\"c1\":{}}}",
                    r.row, r.col, r.row, r.col
                )
            }
            Expr::Range(a, b) => format!(
                "{{\"r0\":{},\"c0\":{},\"r1\":{},\"c1\":{}}}",
                a.row.min(b.row),
                a.col.min(b.col),
                a.row.max(b.row),
                a.col.max(b.col)
            ),
            _ => "null".to_owned(),
        }
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// The merged ranges of a sheet as JSON `[{r0,c0,r1,c1}, …]`.
#[wasm_bindgen]
pub fn session_merges(sheet: usize) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let items: Vec<String> = sh
            .merges
            .iter()
            .map(|m| {
                format!(
                    "{{\"r0\":{},\"c0\":{},\"r1\":{},\"c1\":{}}}",
                    m.start.row, m.start.col, m.end.row, m.end.col
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// One picture as a `data:` URL, or `""` when the part is missing.
///
/// The media bytes live in the retained parts — the same bytes that get written
/// back — so a picture is never stored twice and what is drawn is always what
/// the file holds.
#[wasm_bindgen]
pub fn session_image_data(part: &str) -> String {
    with_session(|s| {
        let found = s
            .workbook()
            .retained_parts
            .iter()
            .find(|p| p.path == part)?;
        // The declared content type where the package gave one; otherwise the
        // extension. Guessing wrong makes the browser refuse to decode it.
        let mime = found.content_type.clone().unwrap_or_else(|| {
            match part.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()) {
                Some(e) if e == "png" => "image/png",
                Some(e) if e == "jpg" || e == "jpeg" => "image/jpeg",
                Some(e) if e == "gif" => "image/gif",
                Some(e) if e == "bmp" => "image/bmp",
                Some(e) if e == "svg" => "image/svg+xml",
                Some(e) if e == "webp" => "image/webp",
                _ => "application/octet-stream",
            }
            .to_owned()
        });
        Some(format!(
            "data:{mime};base64,{}",
            base64_encode(&found.bytes)
        ))
    })
    .flatten()
    .unwrap_or_default()
}

/// Standard base64, no line breaks — what a `data:` URL wants.
pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        // Padding stands in for the bytes that were not there.
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

thread_local! {
    /// The last date serial the host handed to [`session_set_clock`].
    ///
    /// The session keeps this for the calc engine but exposes no way to read it
    /// back, and printing needs it for the `&D`/`&T` header codes. Kept here
    /// rather than added to the SDK surface: it is a cache of what the host
    /// already said, not a new thing the engine knows.
    static HOST_CLOCK: std::cell::Cell<Option<f64>> = const { std::cell::Cell::new(None) };
}

/// The host's clock as a date serial, or `None` if it has never set one.
pub(crate) fn host_clock() -> Option<f64> {
    HOST_CLOCK.with(std::cell::Cell::get)
}

/// Tell the engine what "now" is, as a date serial, and reseed the random
/// functions.
///
/// The engine reads no clock of its own — a calc engine that does cannot be
/// tested or replayed — so the host supplies it. Called before each
/// recalculation the user asks for; leaving the seed alone reproduces the
/// previous `RAND` values exactly, which is what makes an undo of a
/// recalculation possible.
#[wasm_bindgen]
pub fn session_set_clock(now_serial: f64, seed: f64) {
    HOST_CLOCK.with(|c| c.set(now_serial.is_finite().then_some(now_serial)));
    SESSION.with(|cell| {
        if let Some(session) = cell.borrow_mut().as_mut() {
            // Through the SDK rather than poking the model: the environment is
            // configuration, and setting it has to settle the volatile
            // functions that read it — a NOW() still showing yesterday beside a
            // clock that has visibly moved is worse than the cost of the pass.
            session.set_environment(casual_calc_sdk::Environment {
                now: now_serial,
                seed: seed as u64,
            });
        }
    });
}

/// The lowercase name the host switches on.
pub(crate) fn chart_kind_name(kind: casual_calc_model::ChartKind) -> &'static str {
    use casual_calc_model::ChartKind as K;
    match kind {
        K::Bar => "bar",
        K::Column => "column",
        K::Line => "line",
        K::Area => "area",
        K::Pie => "pie",
        K::Doughnut => "doughnut",
        K::Scatter => "scatter",
        K::Unsupported => "unsupported",
    }
}

/// A finite number as JSON, or `null`. `NaN` and infinities are not JSON, and
/// emitting them bare produces a payload the host cannot parse at all.
pub(crate) fn format_json_number(n: f64) -> String {
    if n.is_finite() {
        let mut s = n.to_string();
        if s.ends_with(".0") {
            s.truncate(s.len() - 2);
        }
        s
    } else {
        "null".to_owned()
    }
}

/// A length in twips as CSS pixels, at the 96 dpi CSS reference resolution.
///
/// The model measures the grid in twips (1/1440 in) and CSS measures it in
/// `px` (1/96 in), so the conversion is exactly 15 twips per pixel. A default
/// 960-twip column is 64 px, which is what the screen grid draws it as.
fn twips_to_css_px(twips: i64) -> f64 {
    twips as f64 / 15.0
}

/// A number for a stylesheet: at most three decimals, no trailing zeros, and
/// the same bytes for the same input on every platform.
fn css_num(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    // `-0` is a number CSS accepts and a diff nobody wants to explain.
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    let mut s = format!("{rounded:.3}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

/// A workbook string as a quoted CSS string, safe to place inside a `<style>`
/// element.
///
/// Two escapes, for two different parsers. CSS needs `"` and `\` escaped.
/// **HTML needs `<` escaped**, and that one is the security-relevant half: the
/// content of a `<style>` element is raw text that ends at the first
/// `</style`, so a header of `</style><img onerror=…>` would close the
/// stylesheet and run script — in a popup that `document.write` gave the
/// editor's own origin, next to the session token and the collaboration
/// socket. `push_html_escaped` cannot help here, because `&lt;` inside a
/// stylesheet is four literal characters, not a `<`. The CSS hex escape
/// `\3c ` is the form both parsers agree on.
fn css_quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            // `<` and `&` cannot end the style element on their own, but a
            // hex escape for both removes the question rather than reasoning
            // about which sequences do.
            '<' | '>' | '&' => out.push_str(&format!("\\{:x} ", ch as u32)),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\{:x} ", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The CSS `content` value for one header or footer section, or `None` when
/// the section is empty.
fn hf_content(parts: &[crate::view::HfPart]) -> Option<String> {
    use crate::view::HfPart;
    if parts.is_empty() {
        return None;
    }
    let terms: Vec<String> = parts
        .iter()
        .map(|p| match p {
            HfPart::Text(t) => css_quoted(t),
            HfPart::PageNumber => "counter(page)".to_owned(),
            HfPart::PageCount => "counter(pages)".to_owned(),
        })
        .collect();
    Some(terms.join(" "))
}

/// The CSS border style keyword for an OOXML line-style token.
///
/// CSS has four line kinds where OOXML has fourteen tokens, so the mapping is
/// lossy by construction: every dashed variant becomes `dashed` and the widths
/// carry the distinction, which is what [`casual_calc_layout::border_width`]
/// already decides for the screen. Sharing that function is the point — a
/// second table here would be a second answer to "how thick is `medium`", and
/// the disagreement would only be visible by holding a printout against a
/// screenshot.
fn css_border_style(token: &str) -> &'static str {
    match token {
        "dashed" | "mediumDashed" | "dashDot" | "dashDotDot" | "mediumDashDot"
        | "mediumDashDotDot" | "slantDashDot" => "dashed",
        "dotted" | "hair" => "dotted",
        "double" => "double",
        _ => "solid",
    }
}

/// An OOXML colour as a CSS `#rrggbb`, or `None` when it is not one.
///
/// **Validated, not escaped**, for the reason [`crate::clipboard::html_cell_css`]
/// gives at length: a colour comes out of the file verbatim and is dropped into
/// a `style="…"` attribute in a document that runs with the editor's origin.
/// A colour is hex or it is not a colour.
///
/// An eight-digit OOXML colour is `AARRGGBB`; CSS's eight-digit form is
/// `RRGGBBAA`. Passing one through as the other is how an opaque black
/// `FF000000` becomes a fully transparent red, so the alpha is dropped and the
/// three colour bytes are kept.
fn css_hex_color(raw: &str) -> Option<String> {
    let hex = raw.strip_prefix('#').unwrap_or(raw);
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        3 | 6 => Some(format!("#{hex}")),
        8 => Some(format!("#{}", &hex[2..])),
        _ => None,
    }
}

/// The CSS for a cell's four border edges, or an empty string when it has none.
fn print_border_css(borders: &casual_calc_model::Borders) -> String {
    let mut css = String::new();
    let mut edge = |side: &str, e: &Option<casual_calc_model::BorderEdge>| {
        let Some(e) = e else { return };
        if e.style == "none" {
            return;
        }
        let colour = e
            .color
            .as_deref()
            .and_then(css_hex_color)
            .unwrap_or_else(|| "#000".to_owned());
        css.push_str(&format!(
            "border-{side}:{}px {} {colour};",
            casual_calc_layout::border_width(&e.style),
            css_border_style(&e.style),
        ));
    };
    edge("top", &borders.top);
    edge("right", &borders.right);
    edge("bottom", &borders.bottom);
    edge("left", &borders.left);
    css
}

/// What a merge does to one cell of the printed table.
enum Cover {
    /// Not merged, or merged in a way that prints nothing here.
    Plain,
    /// The visible top-left of a merge: `(colspan, rowspan)`.
    Anchor(usize, usize),
    /// Inside a merge whose anchor is already printed; emit no `<td>` at all.
    Hidden,
}

/// A printable HTML document for a sheet, honouring its page setup.
///
/// # Why HTML, and who decides where the pages break
///
/// The host prints this by writing it into a window and calling `print()`, so
/// the **browser** breaks it into pages. That is a choice with a cost, and the
/// cost was measured before it was accepted: a printout whose page breaks
/// disagree with a preview is its own defect.
///
/// The alternative is for the engine to paginate — to decide which rows and
/// which columns land on page 3 and emit one positioned block per page. It was
/// rejected here, on three grounds:
///
/// 1. **There is nothing for the browser to disagree *with*.** `casual-calc-layout`
///    does not paginate. It computes axes, viewports, display lists and charts;
///    `grep -rn paginat crates/` finds one comment. The claim in
///    `docs/12` §6 that "`casual-calc-layout` + `render` paginate" is not true
///    of the code, and this function could not have wired up something that
///    does not exist. There is also no print preview and no page-layout view
///    (`docs/12` §3.17), so the only thing a user compares the paper against is
///    **the screen grid** — and screen fidelity is what column widths, merges
///    and borders buy.
/// 2. **A paginator here would be a second layout engine for the same grid.**
///    That is the exact fault `RND-10` removed when the canvas stopped drawing
///    charts its own way; re-introducing it for printing would mean every
///    layout fix had to be made twice, with the divergence invisible until
///    somebody held a printout against a screenshot.
/// 3. **It would still print through the browser**, so its breaks would have to
///    be forced against the browser's own — solving a disagreement by creating
///    one.
///
/// So the browser paginates, and the engine's job is to hand it the numbers
/// rather than leave it guessing: real column widths and row heights from
/// [`casual_calc_layout::GridGeometry`], the paper and margins as an `@page`
/// rule, and the scale as a `zoom` on the table.
///
/// **The one thing the browser cannot work out is the scale**, because CSS has
/// no fit-to-page primitive: "fit this on one page wide" is arithmetic over the
/// grid's own widths against the printable area, and only the engine has both.
/// [`casual_calc_layout::print::effective_scale`] computes it, and the same
/// function will answer for the PDF writer (`IO-03`).
///
/// `zoom` and not `transform: scale()`: a transform does not affect layout, so
/// the browser would paginate the *unscaled* box and the shrunken content would
/// sit in the corner of full-size pages. `zoom` reflows, which is what makes
/// "fit to one page wide" actually produce one page.
///
/// # What is carried
///
/// Paper size and orientation, the four margins, column widths, row heights,
/// merges as `colspan`/`rowspan`, per-cell borders, bold/italic/colour/fill and
/// number-formatted text, print gridlines and row/column headings, the print
/// area, repeat-rows-at-top, manual row breaks, the three scale settings, and
/// the header/footer field codes — including `&P`, as a CSS page counter.
///
/// Rows and columns the sheet hides are left out, as they are when Excel
/// prints. The range is the used region unless `Print_Area` names one.
///
/// # What is not
///
/// Charts, images and conditional formatting do not print; nor do manual
/// *column* breaks, which need the pagination this deliberately does not do.
/// Header and footer come from `oddHeader`/`oddFooter` only — the
/// `differentFirst` and `differentOddEven` variants are not read.
#[wasm_bindgen]
pub fn session_print_html(sheet: usize) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return String::new();
        };
        let p = &sh.print;
        let attr = |m: &std::collections::BTreeMap<String, String>, k: &str| {
            m.get(k).cloned().unwrap_or_default()
        };
        let on = |m: &std::collections::BTreeMap<String, String>, k: &str| {
            matches!(m.get(k).map(String::as_str), Some("1") | Some("true"))
        };
        let inches = |k: &str, fallback: f64| {
            p.margins
                .get(k)
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(fallback)
        };

        // `paperSize` is an enum of numbered stock sizes; the layout crate owns
        // the table, because a fit-to-page scale needs the paper's *extent* and
        // not only its CSS name.
        let paper = casual_calc_layout::print::paper(&attr(&p.page, "paperSize"));
        let landscape = attr(&p.page, "orientation") == "landscape";
        let grid = on(&p.options, "gridLines");
        let headings = on(&p.options, "headings");
        let centre_h = on(&p.options, "horizontalCentered");

        let (mut last_row, mut last_col) = (0u32, 0u32);
        for (at, _) in sh.cells.iter() {
            last_row = last_row.max(at.row);
            last_col = last_col.max(at.col);
        }
        if sh.cells.iter().next().is_none() {
            return String::new(); // nothing to print
        }
        // A merge reaches past the cells that carry a value — only its top-left
        // holds one — so a merged banner over empty columns would otherwise
        // fall outside the used region and print as a single narrow cell.
        for m in &sh.merges {
            last_row = last_row.max(m.start.row.max(m.end.row));
            last_col = last_col.max(m.start.col.max(m.end.col));
        }
        // `Print_Area` narrows what prints; without one the used region prints.
        let (mut first_row, mut first_col) = (0u32, 0u32);
        let named = |n: &str| {
            wb.defined_names
                .iter()
                .find(|d| d.sheet == Some(sh.id) && d.name == n)
                .map(|d| d.formula.to_string())
        };
        if let Some(area) = named("Print_Area")
            && let Some((a, b)) = area.rsplit('!').next().and_then(|r| r.split_once(':'))
            && let (Some(start), Some(end)) = (
                casual_calc_formula::parse_a1(a.trim()),
                casual_calc_formula::parse_a1(b.trim()),
            )
        {
            first_row = start.row.min(end.row);
            first_col = start.col.min(end.col);
            last_row = start.row.max(end.row);
            last_col = start.col.max(end.col);
        }
        // `Print_Titles` repeats rows at the top of every page. In a printed
        // HTML table that is exactly what `<thead>` does, so the rows go there
        // rather than being duplicated by hand.
        let title_rows: Option<(u32, u32)> = named("Print_Titles").and_then(|t| {
            let rows = t.rsplit('!').next()?.replace('$', "");
            let (a, b) = rows.split_once(':')?;
            Some((
                a.trim().parse::<u32>().ok()?.saturating_sub(1),
                b.trim().parse::<u32>().ok()?.saturating_sub(1),
            ))
        });

        // The same geometry the screen grid is drawn from. Hidden lines are
        // already zero-sized here, so they cost nothing in the fit-to-page sum
        // without being filtered a second time.
        let geometry = casual_calc_layout::GridGeometry::for_sheet(sh);

        // The strip carrying row numbers is a column of the printed table and
        // therefore part of what has to fit on the paper.
        const HEADING_COL_TWIPS: i64 = 600;
        let (mut content_w, content_h) = casual_calc_layout::print::content_extent(
            &geometry,
            (first_row, last_row),
            (first_col, last_col),
        );
        if headings {
            content_w += HEADING_COL_TWIPS;
        }
        let page_box = casual_calc_layout::print::PageBox::new(
            paper,
            landscape,
            [
                inches("top", 0.75),
                inches("right", 0.7),
                inches("bottom", 0.75),
                inches("left", 0.7),
            ],
        );
        let scaling = casual_calc_layout::print::Scaling::from_print(sh);
        let scale =
            casual_calc_layout::print::effective_scale(scaling, (content_w, content_h), page_box);

        let mut out = String::new();
        out.push_str("<!doctype html><meta charset=\"utf-8\"><title>");
        push_html_escaped(&mut out, &sh.name);
        out.push_str("</title><style>");

        // Header and footer live in `@page` margin boxes rather than in a
        // `<div>` at the top of the document. Two reasons, and the first is the
        // one the dialog promises: a page number can only be *counted*, and
        // `counter(page)` inside a margin box is the one place CSS can count
        // it. The second is that a margin box repeats on every page, where the
        // `<div>` this replaces printed once — so a two-page report had a
        // header on page one and nothing on page two.
        //
        // The cost is stated rather than hidden: a print engine that ignores
        // margin boxes prints no header at all. That is the trade for `&P`
        // working, and it is the mechanism the PDF writer (`IO-03`) will use.
        let clock = host_clock();
        let ctx = crate::view::HfContext {
            sheet: &sh.name,
            file: "",
            now: clock,
        };
        let hf = |key: &str| p.header_footer_text.get(key).cloned().unwrap_or_default();
        let mut margin_boxes = String::new();
        for (raw, edge) in [(hf("oddHeader"), "top"), (hf("oddFooter"), "bottom")] {
            if raw.is_empty() {
                continue;
            }
            let sections = crate::view::hf_sections(&raw, &ctx);
            for (parts, side) in sections.iter().zip(["left", "center", "right"]) {
                if let Some(content) = hf_content(parts) {
                    margin_boxes.push_str(&format!(
                        "@{edge}-{side}{{content:{content};font:9pt Calibri,Arial,sans-serif;\
                         color:#444;}}"
                    ));
                }
            }
        }
        out.push_str(&format!(
            "@page{{size:{}{};margin:{}in {}in {}in {}in;{margin_boxes}}}",
            paper.css,
            if landscape { " landscape" } else { "" },
            inches("top", 0.75),
            inches("right", 0.7),
            inches("bottom", 0.75),
            inches("left", 0.7),
        ));
        // A manual row break starts a new printed page at that row.
        let row_breaks: std::collections::BTreeSet<u32> = p
            .row_breaks
            .iter()
            .filter_map(|b| b.get("id")?.parse::<u32>().ok())
            .filter_map(|id| id.checked_sub(1))
            .collect();
        out.push_str(
            "body{margin:0;font:11pt Calibri,Arial,sans-serif;color:#000;background:#fff}\
             table{border-collapse:collapse;table-layout:fixed}\
             td,th{padding:1px 4px;white-space:pre;overflow:hidden;vertical-align:bottom}\
             th{font-weight:600;background:#f0f0f0;text-align:center}",
        );
        if grid || headings {
            out.push_str("td,th{border:1px solid #b0b0b0}");
        }
        if centre_h {
            out.push_str("table{margin-left:auto;margin-right:auto}");
        }
        // The scale the dialog collects, finally applied. Only the table is
        // scaled: Excel does not shrink the header and footer with the sheet,
        // and they are in the page margins here, outside it.
        if (scale - 1.0).abs() > f64::EPSILON {
            out.push_str(&format!("table{{zoom:{}}}", css_num(scale)));
        }
        out.push_str("</style>");

        let vis_cols: Vec<u32> = (first_col..=last_col)
            .filter(|c| !sh.hidden_cols.contains(c))
            .collect();

        // Whether a row prints, and in which of the table's two sections. A
        // repeated title row belongs to `<thead>` and nowhere else; printing it
        // in the body as well would show it twice on the first page.
        let row_prints = |r: u32, thead: bool| {
            !sh.is_row_hidden(r) && title_rows.is_some_and(|(a, b)| r >= a && r <= b) == thead
        };

        // What a merge does to the cell at (r, c).
        //
        // Spans count *printed* lines, not model lines: a merge across a hidden
        // column is one column narrower on paper, and one whose top-left is
        // clipped away by the print area is anchored at the first corner that
        // does print. Both are what Excel puts on the page, and either done the
        // naive way produces a row with the wrong number of cells in it — an
        // HTML table that renders as a staircase.
        //
        // Clipped to the section as well as the print area, because a rowspan
        // cannot reach out of `<thead>` into the body.
        let cover = |r: u32, c: u32, thead: bool| -> Cover {
            let Some(m) = sh.merges.iter().find(|m| {
                let (r0, r1) = (m.start.row.min(m.end.row), m.start.row.max(m.end.row));
                let (c0, c1) = (m.start.col.min(m.end.col), m.start.col.max(m.end.col));
                (r0..=r1).contains(&r) && (c0..=c1).contains(&c)
            }) else {
                return Cover::Plain;
            };
            let (r0, r1) = (m.start.row.min(m.end.row), m.start.row.max(m.end.row));
            let (c0, c1) = (m.start.col.min(m.end.col), m.start.col.max(m.end.col));
            let rows: Vec<u32> = (r0.max(first_row)..=r1.min(last_row))
                .filter(|&x| row_prints(x, thead))
                .collect();
            let cols: Vec<u32> = (c0.max(first_col)..=c1.min(last_col))
                .filter(|x| !sh.hidden_cols.contains(x))
                .collect();
            match (rows.first(), cols.first()) {
                (Some(&ar), Some(&ac)) if (ar, ac) == (r, c) => {
                    Cover::Anchor(cols.len(), rows.len())
                }
                (Some(_), Some(_)) => Cover::Hidden,
                _ => Cover::Plain,
            }
        };

        let push_cell = |out: &mut String, r: u32, c: u32, thead: bool| {
            let spans = match cover(r, c, thead) {
                Cover::Hidden => return,
                Cover::Plain => String::new(),
                Cover::Anchor(cols, rows) => {
                    let mut s = String::new();
                    if cols > 1 {
                        s.push_str(&format!(" colspan=\"{cols}\""));
                    }
                    if rows > 1 {
                        s.push_str(&format!(" rowspan=\"{rows}\""));
                    }
                    s
                }
            };
            let cell = sh.cells.get(CellRef::new(r, c));
            let text = cell.map(|cl| display_text(wb, cl)).unwrap_or_default();
            let style = cell
                .and_then(|cl| cl.style)
                .and_then(|id| wb.styles.get(id));
            let mut css = style.map(html_cell_css).unwrap_or_default();
            if let Some(borders) = style.and_then(|st| st.border.as_ref())
                && !borders.is_empty()
            {
                css.push_str(&print_border_css(borders));
            }
            if css.is_empty() {
                out.push_str(&format!("<td{spans}>"));
            } else {
                out.push_str(&format!("<td{spans} style=\"{css}\">"));
            }
            push_html_escaped(out, &text);
            out.push_str("</td>");
        };

        out.push_str("<table>");
        // The column widths, which are the difference between a printout that
        // looks like the sheet and one that does not. `<colgroup>` is the only
        // place a `table-layout:fixed` table takes them from.
        out.push_str("<colgroup>");
        if headings {
            out.push_str(&format!(
                "<col style=\"width:{}px\">",
                css_num(twips_to_css_px(HEADING_COL_TWIPS))
            ));
        }
        for &c in &vis_cols {
            out.push_str(&format!(
                "<col style=\"width:{}px\">",
                css_num(twips_to_css_px(geometry.columns.size(c)))
            ));
        }
        out.push_str("</colgroup>");

        let row_open = |r: u32, broken: bool| {
            let height = css_num(twips_to_css_px(geometry.rows.size(r)));
            let brk = if broken { "break-before:page;" } else { "" };
            format!("<tr style=\"{brk}height:{height}px\">")
        };

        if headings {
            out.push_str("<tr><th></th>");
            for &c in &vis_cols {
                out.push_str("<th>");
                push_html_escaped(&mut out, &casual_calc_formula::column_to_letters(c));
                out.push_str("</th>");
            }
            out.push_str("</tr>");
        }
        // The repeated rows first, inside <thead>; the browser puts them at the
        // top of every page it breaks onto.
        if let Some((tr0, tr1)) = title_rows {
            out.push_str("<thead>");
            for r in tr0..=tr1.min(last_row) {
                if !row_prints(r, true) {
                    continue;
                }
                out.push_str(&row_open(r, false));
                if headings {
                    out.push_str(&format!("<th>{}</th>", r + 1));
                }
                for &c in &vis_cols {
                    push_cell(&mut out, r, c, true);
                }
                out.push_str("</tr>");
            }
            out.push_str("</thead>");
        }
        for r in first_row..=last_row {
            if !row_prints(r, false) {
                continue;
            }
            out.push_str(&row_open(r, row_breaks.contains(&r)));
            if headings {
                out.push_str(&format!("<th>{}</th>", r + 1));
            }
            for &c in &vis_cols {
                push_cell(&mut out, r, c, false);
            }
            out.push_str("</tr>");
        }
        out.push_str("</table>");
        out
    })
    .unwrap_or_default()
}
