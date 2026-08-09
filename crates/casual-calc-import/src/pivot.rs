//! Reading a pivot table's definition back out of a package.
//!
//! Two parts again, and again neither is optional. `xl/pivotTables/pivotTableN.xml`
//! says which fields sit on which axis and how the measures are summarized;
//! `xl/pivotCache/pivotCacheDefinitionN.xml` says where the records came from
//! and what each field's items are. The pivot part refers to fields only by
//! *index*, so without the cache there is no way to know that field 3 is
//! `Amount` — or which range to re-read when the pivot is refreshed.
//!
//! Both parts stay retained and are written back byte for byte. What is read
//! here makes an imported pivot **live** — listed in the field panel,
//! reconfigurable, refreshable — rather than merely preserved. Until the user
//! actually changes one, nothing here reaches the writer, so a file that is
//! opened and saved is unchanged.
//!
//! Refreshing an imported pivot is therefore never automatic. Our report is
//! laid out in tabular form and Excel's default is compact, so a refresh on
//! load would silently reformat every pivot in every file anyone opens. It
//! happens when the user asks, and asking is what detaches the retained part —
//! see [`casual_calc_model::PivotTable::part`].

use casual_calc_model::{
    CellRange, PivotAggregate, PivotAxisField, PivotFilterField, PivotSort, PivotValueField,
};
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::ImportError;
use crate::read::{read_attr, xml_err};

/// What a `pivotTableDefinition` says, with fields still identified by index.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PivotSpec {
    /// `@name`, the caption `GETPIVOTDATA` addresses it by.
    pub name: String,
    /// `<location ref>`: the block the pivot occupies today.
    pub location: Option<CellRange>,
    /// `@cacheId`, which is what ties this to a `<pivotCache>` in the workbook.
    pub cache_id: Option<u32>,
    /// Row-axis fields, outermost first.
    pub rows: Vec<PivotAxisField>,
    /// Column-axis fields, outermost first.
    pub cols: Vec<PivotAxisField>,
    /// Page fields, with the item index each has selected.
    pub filters: Vec<(u32, Option<u32>)>,
    /// The measures.
    pub values: Vec<PivotValueField>,
    /// `@rowGrandTotals`, defaulting to on as the schema does.
    pub row_grand_totals: bool,
    /// `@colGrandTotals`.
    pub col_grand_totals: bool,
    /// `<pivotTableStyleInfo name>`.
    pub style: String,
}

/// What a `pivotCacheDefinition` says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CacheSpec {
    /// `<worksheetSource sheet>` — the sheet name, which is how the source is
    /// named in the file even though the model resolves it to a `SheetId`.
    pub sheet: Option<String>,
    /// `<worksheetSource ref>`.
    pub range: Option<CellRange>,
    /// `<worksheetSource name>` — set instead of `ref` when the source is a
    /// table or a defined name.
    pub name: Option<String>,
    /// Each cache field's name, in index order.
    pub fields: Vec<String>,
    /// Each cache field's shared items, as display text, so a `<pageField item>`
    /// index can be resolved to the value it selects.
    pub items: Vec<Vec<String>>,
}

/// `@axis` -> which list a field belongs on.
enum Axis {
    Row,
    Col,
    Page,
    None,
}

fn axis_of(value: Option<&str>) -> Axis {
    match value {
        Some("axisRow") => Axis::Row,
        Some("axisCol") => Axis::Col,
        Some("axisPage") => Axis::Page,
        _ => Axis::None,
    }
}

/// A boolean attribute, honouring OOXML's `1`/`0`/`true`/`false` spellings.
fn flag(value: Option<String>, default: bool) -> bool {
    match value.as_deref() {
        Some("1" | "true") => true,
        Some("0" | "false") => false,
        _ => default,
    }
}

/// Parse a `pivotTableDefinition` part.
///
/// Field order matters twice over: `<pivotFields>` is positional (entry *n*
/// describes cache field *n*), and `<rowFields>`/`<colFields>` give the nesting
/// order, which is not the same thing. Reading the axis from `<pivotField>`
/// alone would lose which field is outermost.
pub fn parse_pivot_table(xml: &[u8]) -> Result<PivotSpec, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut spec = PivotSpec {
        row_grand_totals: true,
        col_grand_totals: true,
        ..PivotSpec::default()
    };

    // Per pivotField index: its axis and whether it keeps a default subtotal.
    let mut axes: Vec<(Axis, bool, PivotSort)> = Vec::new();
    // Which list is currently open, since `<field x>` is the same element in
    // both `<rowFields>` and `<colFields>`.
    let mut in_rows = false;
    let mut in_cols = false;

    loop {
        let event = reader.read_event_into(&mut buf).map_err(xml_err)?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => match e.local_name().as_ref() {
                b"pivotTableDefinition" => {
                    spec.name = read_attr(e, b"name")?.unwrap_or_default();
                    spec.cache_id = read_attr(e, b"cacheId")?.and_then(|v| v.parse().ok());
                    spec.row_grand_totals = flag(read_attr(e, b"rowGrandTotals")?, true);
                    spec.col_grand_totals = flag(read_attr(e, b"colGrandTotals")?, true);
                }
                b"location" => {
                    spec.location = read_attr(e, b"ref")?
                        .as_deref()
                        .and_then(crate::a1::parse_range);
                }
                b"pivotField" => {
                    let axis = axis_of(read_attr(e, b"axis")?.as_deref());
                    let subtotal = flag(read_attr(e, b"defaultSubtotal")?, true);
                    let sort = match read_attr(e, b"sortType")?.as_deref() {
                        Some("descending") => PivotSort::Descending,
                        // `manual` is an explicit item order we do not model;
                        // source order is closer to it than an alphabetical
                        // re-sort would be, and it is what Excel falls back to.
                        Some("manual") => PivotSort::DataSource,
                        _ => PivotSort::Ascending,
                    };
                    axes.push((axis, subtotal, sort));
                }
                b"rowFields" => in_rows = true,
                b"colFields" => in_cols = true,
                b"field" => {
                    let Some(x) = read_attr(e, b"x")?.and_then(|v| v.parse::<i32>().ok()) else {
                        continue;
                    };
                    // `x="-2"` is the placeholder marking where the measures
                    // sit on the axis, not a field. Our layout always puts them
                    // innermost on the column axis, so the marker is dropped.
                    let Ok(index) = u32::try_from(x) else {
                        continue;
                    };
                    let (subtotal, sort) = axes
                        .get(index as usize)
                        .map_or((true, PivotSort::Ascending), |(_, s, o)| (*s, *o));
                    let field = PivotAxisField {
                        source_column: index,
                        sort,
                        subtotal,
                    };
                    if in_rows {
                        spec.rows.push(field);
                    } else if in_cols {
                        spec.cols.push(field);
                    }
                }
                b"pageField" => {
                    let Some(fld) = read_attr(e, b"fld")?.and_then(|v| v.parse::<u32>().ok())
                    else {
                        continue;
                    };
                    let item = read_attr(e, b"item")?.and_then(|v| v.parse::<u32>().ok());
                    spec.filters.push((fld, item));
                }
                b"dataField" => {
                    let Some(fld) = read_attr(e, b"fld")?.and_then(|v| v.parse::<u32>().ok())
                    else {
                        continue;
                    };
                    spec.values.push(PivotValueField {
                        source_column: fld,
                        aggregate: read_attr(e, b"subtotal")?
                            .as_deref()
                            .map_or(PivotAggregate::Sum, PivotAggregate::from_token),
                        name: read_attr(e, b"name")?.unwrap_or_default(),
                        // `@numFmtId` indexes styles.xml, which is a part away.
                        // Left unset rather than guessed: an unformatted figure
                        // is legible, and a wrong format is a wrong number.
                        number_format: None,
                    });
                }
                b"pivotTableStyleInfo" => {
                    spec.style = read_attr(e, b"name")?.unwrap_or_default();
                }
                _ => {}
            },
            Event::End(ref e) => match e.local_name().as_ref() {
                b"rowFields" => in_rows = false,
                b"colFields" => in_cols = false,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(spec)
}

/// Parse a `pivotCacheDefinition` part.
pub fn parse_pivot_cache(xml: &[u8]) -> Result<CacheSpec, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut spec = CacheSpec::default();
    let mut in_shared = false;

    loop {
        let event = reader.read_event_into(&mut buf).map_err(xml_err)?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => match e.local_name().as_ref() {
                b"worksheetSource" => {
                    spec.sheet = read_attr(e, b"sheet")?;
                    spec.name = read_attr(e, b"name")?;
                    spec.range = read_attr(e, b"ref")?
                        .as_deref()
                        .and_then(crate::a1::parse_range);
                }
                b"cacheField" => {
                    spec.fields.push(read_attr(e, b"name")?.unwrap_or_default());
                    spec.items.push(Vec::new());
                }
                b"sharedItems" => in_shared = true,
                // The item elements, which appear only inside `<sharedItems>`
                // — `<s>` also names a series elsewhere in OOXML, so the guard
                // is not decoration.
                b"s" | b"n" | b"b" | b"d" | b"e" | b"m" if in_shared => {
                    let Some(items) = spec.items.last_mut() else {
                        continue;
                    };
                    let text = match e.local_name().as_ref() {
                        b"m" => "(blank)".to_owned(),
                        b"b" => {
                            let raw = read_attr(e, b"v")?.unwrap_or_default();
                            if raw == "1" || raw == "true" {
                                "TRUE".to_owned()
                            } else {
                                "FALSE".to_owned()
                            }
                        }
                        b"n" => {
                            // Rendered the way the engine renders a number key,
                            // so a page filter's stored selection matches what
                            // grouping produces from the same cell.
                            let raw = read_attr(e, b"v")?.unwrap_or_default();
                            raw.parse::<f64>().map_or(raw, casual_calc_layout_general)
                        }
                        _ => read_attr(e, b"v")?.unwrap_or_default(),
                    };
                    items.push(text);
                }
                _ => {}
            },
            Event::End(ref e) => {
                if e.local_name().as_ref() == b"sharedItems" {
                    in_shared = false;
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(spec)
}

/// The `General` rendering of a number.
///
/// Duplicated from the layout crate's `format_general` rather than depended on:
/// the import crate sits below layout in the DAG, and one number-to-text rule
/// is not worth inverting an edge over. The two are pinned together by
/// `a_numeric_shared_item_reads_as_the_engine_would_group_it`.
fn casual_calc_layout_general(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// Turn the two specs into the model's pivot, resolving field indices against
/// the cache's field list and the page-field item indices against its items.
pub fn to_model(
    spec: &PivotSpec,
    cache: &CacheSpec,
    id: u32,
    source_sheet: casual_calc_model::SheetId,
    source: CellRange,
    part: String,
) -> casual_calc_model::PivotTable {
    let anchor = spec
        .location
        .map_or(casual_calc_model::CellRef::new(0, 0), |range| range.start);
    casual_calc_model::PivotTable {
        id,
        name: spec.name.clone(),
        source_sheet,
        source,
        anchor,
        rows: spec.rows.clone(),
        cols: spec.cols.clone(),
        filters: spec
            .filters
            .iter()
            .map(|(fld, item)| PivotFilterField {
                source_column: *fld,
                selected: item
                    .and_then(|i| {
                        cache
                            .items
                            .get(*fld as usize)
                            .and_then(|items| items.get(i as usize))
                            .cloned()
                    })
                    .map(|v| vec![v])
                    .unwrap_or_default(),
            })
            .collect(),
        values: spec.values.clone(),
        row_grand_totals: spec.row_grand_totals,
        col_grand_totals: spec.col_grand_totals,
        style: spec.style.clone(),
        // The block Excel already wrote. Set so that the first refresh clears
        // exactly what is on screen rather than leaving half of Excel's report
        // under ours.
        output: spec.location,
        part: Some(part),
    }
}
