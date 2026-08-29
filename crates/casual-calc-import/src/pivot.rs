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
    /// How many `<dataField>`s carry a `@showDataAs` other than `normal`.
    ///
    /// "Show Values As" turns a measure into a **derivation** of the measure —
    /// a percentage of the grand total, a running total, a rank — so a field
    /// carrying it is not summarized the way its `@subtotal` alone says. Not
    /// modelled, and counted here so it can be reported rather than left to
    /// come out as a plain sum under a caption reading `% of total`.
    pub shown_as: u64,
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
    /// Which cache fields are **calculated** — a formula over the aggregated
    /// values rather than a column of the source.
    ///
    /// `<cacheField formula="Amount*0.1" databaseField="0"/>`. Nothing in the
    /// source range corresponds to one, so its index cannot become a
    /// `source_column`; see [`FieldMap`].
    pub calculated: Vec<bool>,
}

/// The two index spaces a pivot lives in, and the map between them.
///
/// A `pivotTableDefinition` names every field by its **cache-field index**.
/// The model's `source_column` is something else entirely — the doc comment on
/// [`casual_calc_model::PivotAxisField::source_column`] says it: "a zero-based
/// offset into [`PivotTable::source`]". The two coincide only while every cache
/// field is a column of the source, which stops being true the moment a pivot
/// carries a calculated field or a grouped one: Excel appends a cache field for
/// each, so `<dataField fld="3"/>` over a three-column source names nothing the
/// range can address.
///
/// Writing the cache index straight into `source_column` therefore aimed
/// measures and axis fields off the end of the source, where
/// `casual_calc_eval::pivot` reads them as blank without complaint and the
/// writer emits them as out-of-range indices. This maps the one onto the other
/// and says when it cannot.
///
/// [`PivotTable::source`]: casual_calc_model::PivotTable::source
pub struct FieldMap {
    /// Per cache-field index: the source-column offset, or why there is none.
    slots: Vec<Slot>,
    /// How many columns the source rectangle has.
    width: u32,
}

/// What one cache-field index resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// A column of the source, at this offset from its left edge.
    Column(u32),
    /// A calculated field: `@formula` over the aggregates, no column behind it.
    Calculated,
    /// A field with no column behind it that is not calculated either — which
    /// in practice is a **group** field, the extra cache field Excel appends
    /// when a column is grouped into months, quarters or years.
    Derived,
}

impl FieldMap {
    /// Build the map from the cache and the resolved source rectangle.
    ///
    /// Database fields are counted rather than assumed to be a prefix: OOXML
    /// lists them in source-column order, but nothing forbids a derived field
    /// between two of them, and an identity map would then shift every field
    /// after it onto its neighbour's column — a pivot quietly summarizing a
    /// different measure, which is worse than one that refuses.
    #[must_use]
    pub fn build(cache: &CacheSpec, source: CellRange) -> Self {
        let width = source.end.col.saturating_sub(source.start.col) + 1;
        let mut next = 0u32;
        let slots = (0..cache.fields.len())
            .map(|i| {
                if cache.calculated.get(i).copied().unwrap_or(false) {
                    return Slot::Calculated;
                }
                // Past the source's own width there is no column to name, so
                // this is a field the cache added: a group field, or a cache
                // that disagrees with the range it points at. Either way the
                // offset would be unaddressable.
                if next >= width {
                    return Slot::Derived;
                }
                let at = next;
                next += 1;
                Slot::Column(at)
            })
            .collect();
        Self { slots, width }
    }

    /// Resolve one cache-field index.
    ///
    /// Past the end of `<cacheFields>` the map falls back to the index itself,
    /// bounded by the source's width. A cache that declares fewer fields than
    /// its own range has columns is a file this reader has always accepted, and
    /// tightening *that* is not what this is for: the defect was indices the
    /// source cannot address, and the fallback cannot produce one.
    #[must_use]
    pub fn slot(&self, cache_field: u32) -> Slot {
        if let Some(slot) = self.slots.get(cache_field as usize) {
            return *slot;
        }
        if cache_field < self.width {
            Slot::Column(cache_field)
        } else {
            Slot::Derived
        }
    }
}

/// What a pivot's definition said that the model could not take.
///
/// Counted per pivot so [`crate::CompatibilityReport`] can name each kind. All
/// three are *retained* — the pivot's part is written back byte for byte — so
/// the file keeps them; what they are missing from is the model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PivotLosses {
    /// Axis, filter or value fields dropped because they named a calculated
    /// cache field.
    pub calculated_fields: u64,
    /// Ditto for a cache field with no source column behind it — a group
    /// field, which is what date grouping produces.
    pub group_fields: u64,
    /// Value fields kept, but whose `@showDataAs` is not honoured.
    pub shown_as: u64,
}

impl PivotLosses {
    /// Whether anything at all was lost.
    #[must_use]
    pub fn any(self) -> bool {
        self.calculated_fields > 0 || self.group_fields > 0 || self.shown_as > 0
    }
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
                    // `@showDataAs` defaults to `normal`, which means "show the
                    // aggregate itself" and is the only setting this engine
                    // computes. Anything else redefines the figure.
                    if !matches!(
                        read_attr(e, b"showDataAs")?.as_deref(),
                        None | Some("normal")
                    ) {
                        spec.shown_as += 1;
                    }
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
                    // Two spellings for the same thing, and files carry either:
                    // the formula itself, and `@databaseField="0"` saying this
                    // field did not come from the records. Excel writes both
                    // together; taking only one would miss a file that does not.
                    let calculated = read_attr(e, b"formula")?.is_some()
                        || matches!(
                            read_attr(e, b"databaseField")?.as_deref(),
                            Some("0" | "false")
                        );
                    spec.calculated.push(calculated);
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
///
/// Every index in `spec` is a **cache-field** index and every index in the
/// result is a **source-column** offset; [`FieldMap`] is the translation, and a
/// field it cannot translate is left out rather than written through. Left out
/// is the honest outcome: the part is retained whole, so the file still carries
/// the field, while a cache index written into `source_column` would name a
/// column outside the source — which reads as blank on refresh and writes as an
/// out-of-range `<field x>` on save. What was left out comes back in the
/// [`PivotLosses`] so the caller can report it.
pub fn to_model(
    spec: &PivotSpec,
    cache: &CacheSpec,
    id: u32,
    source_sheet: casual_calc_model::SheetId,
    source: CellRange,
    part: String,
) -> (casual_calc_model::PivotTable, PivotLosses) {
    let map = FieldMap::build(cache, source);
    let mut losses = PivotLosses {
        shown_as: spec.shown_as,
        ..PivotLosses::default()
    };
    let mut count = |slot: Slot| match slot {
        Slot::Column(at) => Some(at),
        Slot::Calculated => {
            losses.calculated_fields += 1;
            None
        }
        Slot::Derived => {
            losses.group_fields += 1;
            None
        }
    };

    let axis = |fields: &[PivotAxisField], count: &mut dyn FnMut(Slot) -> Option<u32>| {
        fields
            .iter()
            .filter_map(|f| {
                count(map.slot(f.source_column)).map(|at| PivotAxisField {
                    source_column: at,
                    ..f.clone()
                })
            })
            .collect::<Vec<_>>()
    };
    let rows = axis(&spec.rows, &mut count);
    let cols = axis(&spec.cols, &mut count);

    let filters = spec
        .filters
        .iter()
        .filter_map(|(fld, item)| {
            let at = count(map.slot(*fld))?;
            Some(PivotFilterField {
                source_column: at,
                // The item index is still resolved against the *cache* field,
                // because that is where the shared items are listed. Only the
                // column offset is translated.
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
        })
        .collect();

    let values = spec
        .values
        .iter()
        .filter_map(|v| {
            count(map.slot(v.source_column)).map(|at| PivotValueField {
                source_column: at,
                ..v.clone()
            })
        })
        .collect();

    let anchor = spec
        .location
        .map_or(casual_calc_model::CellRef::new(0, 0), |range| range.start);
    let pivot = casual_calc_model::PivotTable {
        id,
        name: spec.name.clone(),
        source_sheet,
        source,
        anchor,
        rows,
        cols,
        filters,
        values,
        row_grand_totals: spec.row_grand_totals,
        col_grand_totals: spec.col_grand_totals,
        style: spec.style.clone(),
        // The block Excel already wrote. Set so that the first refresh clears
        // exactly what is on screen rather than leaving half of Excel's report
        // under ours.
        output: spec.location,
        part: Some(part),
    };
    (pivot, losses)
}
