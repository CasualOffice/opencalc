//! Writing a pivot table as a **pivot**, not as the cells it produced.
//!
//! A pivot read from a file keeps its part and is written back byte for byte,
//! exactly as a chart is — see [`casual_calc_model::PivotTable::part`]. What
//! this module is for is the other regime: a pivot **created here**, which until
//! now reached the file only as the values its last refresh happened to write.
//! Excel opened those as an ordinary block of cells, so the field list was gone,
//! the layout could not be changed and a refresh was impossible (`PIV-02`).
//!
//! **The cache carries no records.** `saveData="0"` with `refreshOnLoad="1"`
//! says "the data is not in here; read it from the source when you open me".
//! That is the decided route, and it is the honest one: this engine recomputes a
//! pivot from its source anyway, so writing a records part would be publishing a
//! second copy of the truth that nothing here keeps up to date. The cost is that
//! the pivot is empty until the reader refreshes it, which `refreshOnLoad` makes
//! automatic.
//!
//! Field *names* come from the source's header row, because that is what the
//! model's `source` includes and what `source_column` indexes into.
//!
//! # The derived half, and why it is a separate parameter
//!
//! `refreshOnLoad="1"` is exactly what makes this module a hard prerequisite for
//! Show Values As, date grouping and calculated fields (`docs/85` §1.4, §9 row
//! **B**). Excel rebuilds the whole report from the definition on open, so a
//! feature the definition does not state is a feature the reopened file does not
//! have — and it comes back as raw sums under our caption, which is `PIV-05`'s
//! P0 written from the writing end.
//!
//! So this module writes all three: `@showDataAs`/`@baseField`/`@baseItem` on
//! `<dataField>`, `<fieldGroup><rangePr>` on a grouped `<cacheField>`, and
//! `@formula databaseField="0"` on a calculated one. It takes them as a
//! [`PivotDerived`] rather than reading them off [`PivotTable`], because
//! [`PivotTable`] has nowhere to put them yet: the model fields arrive with
//! `docs/85` §9 slices C, D and E, each of which pays a protocol bump this slice
//! does not. [`PivotDerived::of`] is the seam — three field reads, named in its
//! documentation — and until those fields exist it answers "nothing derived",
//! so no `@showDataAs`, no `<fieldGroup>` and no `@formula` reaches a file
//! written today.
//!
//! The one attribute that *did* change for every pivot is `@dataCaption`, which
//! the schema makes required and which was missing — see `table_xml`. That is a
//! `PIV-02` defect fixed in passing, not part of the derived half.
//!
//! # The two index spaces
//!
//! The model addresses a field by `source_column`, "a zero-based offset into
//! `PivotTable::source`", and `PIV-05` was a P0 about keeping that space clean.
//! OOXML has **one** space — the cache field index — and a calculated or group
//! field lives in it *past* the source columns. Translating between the two is
//! this module's job and nowhere else's, which is the point of the refusal in
//! `docs/85` §5.3: the ambiguity exists in the file, so it must not also exist
//! in the model.
//!
//! The cache field list this module writes is, in order:
//!
//! 1. one field per source column, `0..width`;
//! 2. one per [`PivotDerived::calculated`] entry, at `width + i` — a contiguous
//!    block, so a measure's `calculated` index maps by addition and stays valid
//!    however many group levels the pivot grows;
//! 3. one per distinct grouped axis or filter slot.

use std::collections::{BTreeMap, BTreeSet};

use casual_calc_model::{
    PivotBaseItem, PivotCalculatedField, PivotGroup, PivotGroupBy, PivotShowAs, PivotTable, Sheet,
    Workbook,
};

use crate::xml::escape_attr;

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Content type for a pivot table part.
pub const CT_TABLE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml";
/// Content type for a pivot cache definition part.
pub const CT_CACHE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml";

/// Which of a pivot's three field lists a slot belongs to.
///
/// Also the precedence when one cache field is claimed twice: rows first, then
/// columns, then the page filters — the order `<pivotField axis=…>` was already
/// resolved in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PivotAxisKind {
    /// [`PivotTable::rows`].
    Rows,
    /// [`PivotTable::cols`].
    Cols,
    /// [`PivotTable::filters`].
    Filters,
}

impl PivotAxisKind {
    /// The OOXML `<pivotField @axis>` token.
    fn token(self) -> &'static str {
        match self {
            Self::Rows => "axisRow",
            Self::Cols => "axisCol",
            Self::Filters => "axisPage",
        }
    }
}

/// What one measure reports, beyond its aggregate.
///
/// The `docs/85` §5.1 and §5.3 additions to `PivotValueField`, held here until
/// slices C and E put them on the model.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PivotValueDerived {
    /// `<dataField @showDataAs>`. `None` and
    /// [`PivotShowAs::Normal`] are both the schema's default and neither is
    /// written, so an ordinary measure keeps the bytes it always had.
    pub show_as: Option<PivotShowAs>,
    /// `<dataField @baseField>`, as a `source_column`.
    pub base_field: Option<u32>,
    /// `<dataField @baseItem>`.
    pub base_item: Option<PivotBaseItem>,
    /// The calculated field this measure reports, as an index into
    /// [`PivotDerived::calculated`]. When set, the measure's `source_column`
    /// and `aggregate` are not read — Excel applies the formula, so the
    /// aggregate written is the schema's `sum`.
    pub calculated: Option<u32>,
}

/// The half of a pivot's definition the model does not carry yet.
///
/// Every field here is one `docs/85` §5 adds to the model in slice C, D or E.
/// This module is slice **B**: it writes them, so that when the model gains
/// them the file states them too rather than letting Excel's `refreshOnLoad`
/// rebuild a report that has none of them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PivotDerived {
    /// The bucketing on each grouped axis or filter slot, keyed by which list
    /// and which position in it. A slot with no entry groups by the value
    /// itself, as every slot does today.
    pub groups: BTreeMap<(PivotAxisKind, usize), PivotGroup>,
    /// The derivation on each measure, keyed by its position in
    /// [`PivotTable::values`].
    pub values: BTreeMap<usize, PivotValueDerived>,
    /// `PivotTable::calculated`.
    pub calculated: Vec<PivotCalculatedField>,
}

impl PivotDerived {
    /// What `pivot` says it derives.
    ///
    /// **Empty, and deliberately so.** [`PivotTable`] has no `show_as`, no
    /// `group` and no `calculated`; those three fields arrive with `docs/85`
    /// §9 slices C, D and E, each paying its own `PROTOCOL_VERSION` bump. When
    /// they do, this function is the only thing that changes here — the three
    /// reads are:
    ///
    /// - `pivot.rows[i].group` / `.cols` / `.filters` into
    ///   [`Self::groups`] (slice D);
    /// - `pivot.values[i].show_as` / `.base_field` / `.base_item` /
    ///   `.calculated` into [`Self::values`] (slices C and E);
    /// - `pivot.calculated` into [`Self::calculated`] (slice E).
    ///
    /// Everything those three reads feed — the cache field layout, the index
    /// translation, and the XML — is built and tested now, which is what makes
    /// C, D and E safe to land.
    #[must_use]
    pub fn of(pivot: &PivotTable) -> Self {
        let _ = pivot;
        Self::default()
    }
}

/// Everything one sheet's authored pivots contribute to the package.
#[derive(Debug, Default)]
pub struct SheetPivots {
    /// `(path, xml)` for each pivot table and each cache definition.
    pub parts: Vec<(String, String)>,
    /// `(path, xml)` for each pivot table's own `.rels`, naming its cache.
    pub rels: Vec<(String, String)>,
    /// `(rel id, target)` this sheet's `.rels` must carry, one per table.
    pub sheet_rels: Vec<(String, String)>,
    /// `(rel id, target, cache id)` for `workbook.xml`'s `<pivotCaches>`.
    pub caches: Vec<(String, String, u32)>,
}

/// The pivots this module writes: the ones with no retained part behind them.
pub fn authored(sheet: &Sheet) -> Vec<&PivotTable> {
    sheet.pivots.iter().filter(|p| p.part.is_none()).collect()
}

/// Build every part a sheet's authored pivots need.
///
/// `first` is the 1-based number of this sheet's first pivot, so numbering runs
/// across the workbook the way Excel's does. `rel` mints the ids this sheet and
/// the workbook will use.
pub fn build(workbook: &Workbook, sheet: &Sheet, first: usize, first_rel: usize) -> SheetPivots {
    build_with(workbook, sheet, first, first_rel, &[])
}

/// [`build`], with the derived half supplied rather than read from the model.
///
/// `derived` is parallel to [`authored`]; a pivot past its end, or one whose
/// entry is [`PivotDerived::default`], is written exactly as [`build`] writes
/// it. It exists so the writer can be exercised — and handed to a foreign
/// reader — before slices C, D and E give [`PivotTable`] somewhere to hold this.
pub fn build_with(
    workbook: &Workbook,
    sheet: &Sheet,
    first: usize,
    first_rel: usize,
    derived: &[PivotDerived],
) -> SheetPivots {
    let mut out = SheetPivots::default();
    for (i, pivot) in authored(sheet).iter().enumerate() {
        let n = first + i;
        let table_path = format!("xl/pivotTables/pivotTable{n}.xml");
        let cache_path = format!("xl/pivotCache/pivotCacheDefinition{n}.xml");
        let owned;
        let derivation = match derived.get(i) {
            Some(d) => d,
            None => {
                owned = PivotDerived::of(pivot);
                &owned
            }
        };
        let layout = CacheLayout::of(workbook, pivot, derivation);

        out.parts.push((
            cache_path.clone(),
            cache_xml(workbook, pivot, &layout, derivation),
        ));
        out.parts
            .push((table_path.clone(), table_xml(pivot, &layout, derivation)));
        // The table names its cache, which is how a reader gets from one to the
        // other; without it Excel reports the pivot as unreadable.
        out.rels.push((
            format!("xl/pivotTables/_rels/pivotTable{n}.xml.rels"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"{NS_R}/pivotCacheDefinition\" Target=\"../pivotCache/pivotCacheDefinition{n}.xml\"/>\
</Relationships>"
            ),
        ));
        out.sheet_rels.push((
            format!("rIdPvt{}", first_rel + i),
            format!("../pivotTables/pivotTable{n}.xml"),
        ));
        out.caches.push((
            format!("rIdCache{n}"),
            format!("pivotCache/pivotCacheDefinition{n}.xml"),
            // The cache id is the pivot's own, so a file written twice names the
            // same cache both times.
            pivot.id,
        ));
    }
    out
}

/// What one cache field is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum FieldKind {
    /// A column of the source, at its own `source_column` index.
    Source,
    /// A calculated field, as an index into [`PivotDerived::calculated`].
    Calculated(usize),
    /// A group level over the source column `base`.
    Group { base: u32, group: PivotGroup },
}

/// The cache's field list, and the translation from the model's addressing to
/// it.
///
/// See the module documentation for the order and why the calculated block sits
/// where it does.
#[derive(Debug)]
struct CacheLayout {
    names: Vec<String>,
    kinds: Vec<FieldKind>,
    /// The cache field each axis or filter slot resolves to — its source
    /// column, or the group level derived from it.
    slots: BTreeMap<(PivotAxisKind, usize), u32>,
    /// The cache field each measure reads, parallel to `PivotTable::values`.
    values: Vec<u32>,
    /// How many of `names` are source columns.
    width: usize,
}

impl CacheLayout {
    fn of(workbook: &Workbook, pivot: &PivotTable, derived: &PivotDerived) -> Self {
        let names = cache_fields(workbook, pivot);
        let width = names.len();
        let mut taken: BTreeSet<String> = names.iter().cloned().collect();
        let mut kinds = vec![FieldKind::Source; width];
        let mut names = names;

        // The calculated block is contiguous and comes first, so a measure's
        // `calculated` index is `width + i` however the grouping below grows.
        for (i, field) in derived.calculated.iter().enumerate() {
            let candidate = if field.name.trim().is_empty() {
                format!("Calculated{}", i + 1)
            } else {
                field.name.clone()
            };
            names.push(unique_name(&mut taken, &candidate));
            kinds.push(FieldKind::Calculated(i));
        }

        let mut slots = BTreeMap::new();
        let axes = [
            (
                PivotAxisKind::Rows,
                pivot
                    .rows
                    .iter()
                    .map(|f| f.source_column)
                    .collect::<Vec<_>>(),
            ),
            (
                PivotAxisKind::Cols,
                pivot.cols.iter().map(|f| f.source_column).collect(),
            ),
            (
                PivotAxisKind::Filters,
                pivot.filters.iter().map(|f| f.source_column).collect(),
            ),
        ];
        for (axis, columns) in axes {
            for (slot, base) in columns.into_iter().enumerate() {
                let Some(group) = derived.groups.get(&(axis, slot)) else {
                    // Ungrouped: the slot is the source column itself, which is
                    // every slot written before this change.
                    slots.insert((axis, slot), base);
                    continue;
                };
                // **One cache field per grouped slot, and no sharing.** Two
                // slots asking for the same bucketing of the same column look
                // like one field, and folding them into one produces a field
                // named by both `<rowFields>` and `<pageFields>` — a field on
                // two axes, which `<pivotField @axis>` cannot say and no reader
                // can resolve. A second field with the same `@base` and the
                // same `<rangePr>` is redundant; a field on two axes is wrong.
                let candidate = group
                    .by
                    .caption()
                    .map(str::to_owned)
                    // `range` groups a numeric field in place in Excel, so the
                    // level has no caption of its own and the base field's name
                    // is the honest starting point.
                    .unwrap_or_else(|| names.get(base as usize).cloned().unwrap_or_default());
                names.push(unique_name(&mut taken, &candidate));
                kinds.push(FieldKind::Group {
                    base,
                    group: *group,
                });
                slots.insert((axis, slot), names.len() as u32 - 1);
            }
        }

        let values = pivot
            .values
            .iter()
            .enumerate()
            .map(|(i, f)| {
                derived
                    .values
                    .get(&i)
                    .and_then(|v| v.calculated)
                    // A calculated index past the end would put `fld` past the
                    // field count, which is the one way to make Excel offer to
                    // repair the file. Nothing in the model can produce it
                    // today; falling back to the source column keeps that true
                    // if something later can.
                    .filter(|c| (*c as usize) < derived.calculated.len())
                    .map(|c| width as u32 + c)
                    .unwrap_or(f.source_column)
            })
            .collect();

        Self {
            names,
            kinds,
            slots,
            values,
            width,
        }
    }

    /// Which axis claims this cache field, if any. Rows, then columns, then
    /// filters — `BTreeMap` order over [`PivotAxisKind`].
    fn axis_of(&self, field: u32) -> Option<PivotAxisKind> {
        self.slots
            .iter()
            .find(|(_, at)| **at == field)
            .map(|((axis, _), _)| *axis)
    }
}

/// A name no other cache field has, because Excel refuses two fields sharing
/// one. Excel's own answer is the same: a second `Years` level is `Years2`.
fn unique_name(taken: &mut BTreeSet<String>, candidate: &str) -> String {
    let base = if candidate.trim().is_empty() {
        "Field"
    } else {
        candidate
    };
    if taken.insert(base.to_owned()) {
        return base.to_owned();
    }
    for n in 2u32.. {
        let tried = format!("{base}{n}");
        if taken.insert(tried.clone()) {
            return tried;
        }
    }
    unreachable!("u32 exhausted")
}

/// The source's header row, which is what `source_column` indexes into.
///
/// A blank header still has to produce a *name*: Excel refuses a cache field
/// with an empty one, and two fields sharing a name is a different refusal, so
/// the fallback is positional and therefore unique.
fn cache_fields(workbook: &Workbook, pivot: &PivotTable) -> Vec<String> {
    let sheet = workbook.sheets.iter().find(|s| s.id == pivot.source_sheet);
    let row = pivot.source.start.row;
    (pivot.source.start.col..=pivot.source.end.col)
        .map(|col| {
            let text = sheet
                .and_then(|s| s.cells.get(casual_calc_model::CellRef::new(row, col)))
                .map(|cell| header_text(workbook, &cell.value))
                .unwrap_or_default();
            if text.trim().is_empty() {
                format!("Column{}", col - pivot.source.start.col + 1)
            } else {
                text
            }
        })
        .collect()
}

/// A header cell as a field name.
///
/// Only the shapes a header actually takes. A number or a boolean in a header
/// row is unusual but legal, and refusing it would drop a field rather than name
/// it awkwardly; an error there has no name worth writing, so it falls through
/// to the positional fallback.
fn header_text(workbook: &Workbook, value: &casual_calc_model::CellValue) -> String {
    use casual_calc_model::CellValue as V;
    match value {
        V::SharedString(id) | V::InlineString(id) => {
            workbook.strings.get(*id).unwrap_or("").to_owned()
        }
        V::Number(n) => {
            let mut buf = ryu_like(*n);
            buf.truncate(buf.trim_end().len());
            buf
        }
        V::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        V::Empty | V::Error(_) => String::new(),
    }
}

/// A number as a header would read it — no exponent for the ordinary cases.
fn ryu_like(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// `pivotCacheDefinition`: where the records come from, that they are not in
/// here, and what each field is.
fn cache_xml(
    workbook: &Workbook,
    pivot: &PivotTable,
    layout: &CacheLayout,
    derived: &PivotDerived,
) -> String {
    let source_name = workbook
        .sheets
        .iter()
        .find(|s| s.id == pivot.source_sheet)
        .map(|s| s.name.as_str())
        .unwrap_or_default();
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<pivotCacheDefinition xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_R}\" \
saveData=\"0\" refreshOnLoad=\"1\" recordCount=\"0\">\
<cacheSource type=\"worksheet\"><worksheetSource ref=\"{}\" sheet=\"{}\"/></cacheSource>\
<cacheFields count=\"{}\">",
        crate::range_a1(&pivot.source),
        escape_attr(source_name),
        layout.names.len()
    );
    for (name, kind) in layout.names.iter().zip(&layout.kinds) {
        // `<sharedItems/>` empty on purpose: with no records saved there is
        // nothing to share, and the reader rebuilds both on refresh.
        let attrs = match kind {
            FieldKind::Source => String::new(),
            // `databaseField="0"` is what says "not a column of the source" —
            // without it Excel looks for a column named `Bonus` and does not
            // find one.
            FieldKind::Calculated(_) => " databaseField=\"0\"".to_owned(),
            FieldKind::Group { .. } => " databaseField=\"0\"".to_owned(),
        };
        let formula = match kind {
            FieldKind::Calculated(i) => format!(
                " formula=\"{}\"",
                escape_attr(
                    derived
                        .calculated
                        .get(*i)
                        .map_or("", |field| field.formula.as_str())
                )
            ),
            _ => String::new(),
        };
        s.push_str(&format!(
            "<cacheField name=\"{}\" numFmtId=\"0\"{formula}{attrs}><sharedItems/>",
            escape_attr(name)
        ));
        if let FieldKind::Group { base, group } = kind {
            s.push_str(&field_group_xml(*base, group));
        }
        s.push_str("</cacheField>");
    }
    s.push_str("</cacheFields></pivotCacheDefinition>");
    s
}

/// `<fieldGroup>`: what a group level buckets, and what it buckets *of*.
///
/// `@base` names the source cache field the level is derived from. `@par` — the
/// next coarser level — is **not** written: our levels are independent axis
/// slots rather than a declared hierarchy, and Excel rebuilds the hierarchy from
/// the axis order on refresh.
///
/// `<groupItems>` is not written either, for the same reason `<sharedItems/>` is
/// empty: `saveData="0"` means the reader derives the items, and a list of items
/// nothing here keeps up to date is the second copy of the truth this module
/// exists to avoid.
fn field_group_xml(base: u32, group: &PivotGroup) -> String {
    let mut range = String::new();
    // `@startNum`/`@endNum` are numbers; the seven time units want
    // `@startDate`/`@endDate`, which are `xsd:dateTime` and would need a
    // calendar this crate does not have and cannot reach — `serial_to_ymd`
    // lives in `casual-calc-layout`, which is not a dependency and making it
    // one is a DAG change, not an export change. An explicit bound on a *date*
    // group is therefore not carried into the file; `autoStart`/`autoEnd` is
    // what a reader sees. This is named rather than silent, and it costs
    // nothing in the first release, whose three honoured units — years,
    // quarters, months — are all auto-bounded.
    if group.by == PivotGroupBy::Range {
        if let Some(start) = group.start {
            range.push_str(&format!(
                " autoStart=\"0\" startNum=\"{}\"",
                ryu_like(start)
            ));
        }
        if let Some(end) = group.end {
            range.push_str(&format!(" autoEnd=\"0\" endNum=\"{}\"", ryu_like(end)));
        }
    }
    if let Some(interval) = group.interval {
        range.push_str(&format!(" groupInterval=\"{}\"", ryu_like(interval)));
    }
    format!(
        "<fieldGroup base=\"{base}\"><rangePr groupBy=\"{}\"{range}/></fieldGroup>",
        group.by.token()
    )
}

/// `pivotTableDefinition`: the layout, in the cache's field indices.
///
/// `@dataCaption` is `use="required"` (`sml.xsd:1096`) and was missing from
/// every pivot this module has written. It is the caption of the measures
/// pseudo-field in the field list, and `Values` is what Excel writes when the
/// user has not renamed it. Found by validating a written part against the
/// vendored schema; it is a defect of `PIV-02`'s writer rather than of this
/// slice, and it is fixed here because a document that fails to validate is a
/// worse place to put new attributes than one that validates.
fn table_xml(pivot: &PivotTable, layout: &CacheLayout, derived: &PivotDerived) -> String {
    // The extent the last refresh wrote, when there is one — a reader needs
    // somewhere to put the report before it has refreshed.
    let location = pivot.output.unwrap_or(casual_calc_model::CellRange::new(
        pivot.anchor,
        pivot.anchor,
    ));
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<pivotTableDefinition xmlns=\"{NS_MAIN}\" name=\"{}\" cacheId=\"{}\" \
dataCaption=\"Values\" \
dataOnRows=\"0\" applyNumberFormats=\"0\" applyBorderFormats=\"0\" \
applyFontFormats=\"0\" applyPatternFormats=\"0\" applyAlignmentFormats=\"0\" \
applyWidthHeightFormats=\"1\" useAutoFormatting=\"1\" itemPrintTitles=\"1\" \
indent=\"0\" outline=\"1\" outlineData=\"1\" multipleFieldFilters=\"0\" \
rowGrandTotals=\"{}\" colGrandTotals=\"{}\">\
<location ref=\"{}\" firstHeaderRow=\"1\" firstDataRow=\"1\" firstDataCol=\"{}\"/>\
<pivotFields count=\"{}\">",
        escape_attr(&pivot.name),
        pivot.id,
        u8::from(pivot.row_grand_totals),
        u8::from(pivot.col_grand_totals),
        crate::range_a1(&location),
        pivot.rows.len().max(1),
        layout.names.len()
    );
    for i in 0..layout.names.len() as u32 {
        let is_value = layout.values.contains(&i);
        match (layout.axis_of(i), is_value) {
            (Some(axis), _) => s.push_str(&format!(
                "<pivotField axis=\"{}\" showAll=\"0\"><items count=\"1\"><item t=\"default\"/></items></pivotField>",
                axis.token()
            )),
            (None, true) => s.push_str("<pivotField dataField=\"1\" showAll=\"0\"/>"),
            (None, false) => s.push_str("<pivotField showAll=\"0\"/>"),
        }
    }
    s.push_str("</pivotFields>");

    let axis_block = |tag: &str, axis: PivotAxisKind, count: usize| -> String {
        if count == 0 {
            return String::new();
        }
        let mut b = format!("<{tag}Fields count=\"{count}\">");
        for slot in 0..count {
            b.push_str(&format!(
                "<field x=\"{}\"/>",
                layout.slots.get(&(axis, slot)).copied().unwrap_or_default()
            ));
        }
        b.push_str(&format!("</{tag}Fields>"));
        b
    };
    s.push_str(&axis_block("row", PivotAxisKind::Rows, pivot.rows.len()));
    s.push_str(&axis_block("col", PivotAxisKind::Cols, pivot.cols.len()));

    if !pivot.filters.is_empty() {
        s.push_str(&format!("<pageFields count=\"{}\">", pivot.filters.len()));
        for slot in 0..pivot.filters.len() {
            s.push_str(&format!(
                "<pageField fld=\"{}\" hier=\"-1\"/>",
                layout
                    .slots
                    .get(&(PivotAxisKind::Filters, slot))
                    .copied()
                    .unwrap_or_default()
            ));
        }
        s.push_str("</pageFields>");
    }
    if !pivot.values.is_empty() {
        s.push_str(&format!("<dataFields count=\"{}\">", pivot.values.len()));
        for (i, f) in pivot.values.iter().enumerate() {
            let field = layout.values[i];
            let calculated = field as usize >= layout.width;
            let name = if f.name.is_empty() {
                layout
                    .names
                    .get(field as usize)
                    .cloned()
                    .unwrap_or_default()
            } else {
                f.name.clone()
            };
            // Excel applies a calculated field's formula rather than an
            // aggregate, so the model's `aggregate` is not what the file means
            // and `sum` — the schema's default — is what it writes.
            let subtotal = if calculated { "sum" } else { subtotal_token(f) };
            s.push_str(&format!(
                "<dataField name=\"{}\" fld=\"{field}\" subtotal=\"{subtotal}\"{}/>",
                escape_attr(&name),
                show_data_as_attrs(derived.values.get(&i), layout.width)
            ));
        }
        s.push_str("</dataFields>");
    }
    if !pivot.style.is_empty() {
        s.push_str(&format!(
            "<pivotTableStyleInfo name=\"{}\" showRowHeaders=\"1\" showColHeaders=\"1\"/>",
            escape_attr(&pivot.style)
        ));
    }
    s.push_str("</pivotTableDefinition>");
    s
}

/// `@showDataAs`, `@baseField` and `@baseItem` for one measure.
///
/// An undived measure keeps `baseField="0" baseItem="0"`, which is what this
/// module has always written and what a reader ignores while `@showDataAs` is
/// `normal`. A derived one moves `@baseItem` to `1048832` — "no item chosen",
/// the schema's default and what a real `showDataAs="percentOfTotal"` carries
/// — unless the mode names one.
fn show_data_as_attrs(derived: Option<&PivotValueDerived>, width: usize) -> String {
    let derived = derived.copied().unwrap_or_default();
    let show_as = derived.show_as.filter(|m| *m != PivotShowAs::Normal);
    let base_field = derived
        .base_field
        // `@baseField` is a cache field index and `docs/85` §8 makes it a
        // *source* column, which `structural.rs` renumbers with the sheet. One
        // past the source columns would name a calculated or group field, which
        // is not a thing to be relative to.
        .filter(|c| (*c as usize) < width)
        .unwrap_or(0);
    let base_item = match derived.base_item {
        Some(item) => item.token(),
        None if show_as.is_some() => PivotBaseItem::UNSET,
        None => 0,
    };
    match show_as {
        Some(mode) => format!(
            " showDataAs=\"{}\" baseField=\"{base_field}\" baseItem=\"{base_item}\"",
            mode.token()
        ),
        None => format!(" baseField=\"{base_field}\" baseItem=\"{base_item}\""),
    }
}

/// OOXML's name for a value field's aggregate.
fn subtotal_token(field: &casual_calc_model::PivotValueField) -> &'static str {
    use casual_calc_model::PivotAggregate as A;
    match field.aggregate {
        A::Sum => "sum",
        A::Count => "count",
        A::Average => "average",
        A::Max => "max",
        A::Min => "min",
        A::Product => "product",
        A::CountNums => "countNums",
        A::StdDev => "stdDev",
        A::StdDevP => "stdDevp",
        A::Var => "var",
        A::VarP => "varp",
    }
}
