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

use casual_calc_model::{PivotTable, Sheet, Workbook};

use crate::xml::escape_attr;

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Content type for a pivot table part.
pub const CT_TABLE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml";
/// Content type for a pivot cache definition part.
pub const CT_CACHE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml";

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
    let mut out = SheetPivots::default();
    for (i, pivot) in authored(sheet).iter().enumerate() {
        let n = first + i;
        let table_path = format!("xl/pivotTables/pivotTable{n}.xml");
        let cache_path = format!("xl/pivotCache/pivotCacheDefinition{n}.xml");
        let fields = cache_fields(workbook, pivot);

        out.parts
            .push((cache_path.clone(), cache_xml(workbook, pivot, &fields)));
        out.parts
            .push((table_path.clone(), table_xml(pivot, &fields, n)));
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

/// `pivotCacheDefinition`: where the records come from, and that they are not
/// in here.
fn cache_xml(workbook: &Workbook, pivot: &PivotTable, fields: &[String]) -> String {
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
        fields.len()
    );
    for name in fields {
        // `<sharedItems/>` empty on purpose: with no records saved there is
        // nothing to share, and the reader rebuilds both on refresh.
        s.push_str(&format!(
            "<cacheField name=\"{}\" numFmtId=\"0\"><sharedItems/></cacheField>",
            escape_attr(name)
        ));
    }
    s.push_str("</cacheFields></pivotCacheDefinition>");
    s
}

/// `pivotTableDefinition`: the layout, in the cache's field indices.
fn table_xml(pivot: &PivotTable, fields: &[String], n: usize) -> String {
    let axis_of = |i: u32| -> Option<&'static str> {
        if pivot.rows.iter().any(|f| f.source_column == i) {
            Some("axisRow")
        } else if pivot.cols.iter().any(|f| f.source_column == i) {
            Some("axisCol")
        } else if pivot.filters.iter().any(|f| f.source_column == i) {
            Some("axisPage")
        } else {
            None
        }
    };

    // The extent the last refresh wrote, when there is one — a reader needs
    // somewhere to put the report before it has refreshed.
    let location = pivot.output.unwrap_or(casual_calc_model::CellRange::new(
        pivot.anchor,
        pivot.anchor,
    ));
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<pivotTableDefinition xmlns=\"{NS_MAIN}\" name=\"{}\" cacheId=\"{}\" \
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
        fields.len()
    );
    for i in 0..fields.len() as u32 {
        let is_value = pivot.values.iter().any(|f| f.source_column == i);
        match (axis_of(i), is_value) {
            (Some(axis), _) => s.push_str(&format!(
                "<pivotField axis=\"{axis}\" showAll=\"0\"><items count=\"1\"><item t=\"default\"/></items></pivotField>"
            )),
            (None, true) => s.push_str("<pivotField dataField=\"1\" showAll=\"0\"/>"),
            (None, false) => s.push_str("<pivotField showAll=\"0\"/>"),
        }
    }
    s.push_str("</pivotFields>");

    let axis_block = |tag: &str, fs: &[casual_calc_model::PivotAxisField]| -> String {
        if fs.is_empty() {
            return String::new();
        }
        let mut b = format!("<{tag}Fields count=\"{}\">", fs.len());
        for f in fs {
            b.push_str(&format!("<field x=\"{}\"/>", f.source_column));
        }
        b.push_str(&format!("</{tag}Fields>"));
        b
    };
    s.push_str(&axis_block("row", &pivot.rows));
    s.push_str(&axis_block("col", &pivot.cols));

    if !pivot.filters.is_empty() {
        s.push_str(&format!("<pageFields count=\"{}\">", pivot.filters.len()));
        for f in &pivot.filters {
            s.push_str(&format!(
                "<pageField fld=\"{}\" hier=\"-1\"/>",
                f.source_column
            ));
        }
        s.push_str("</pageFields>");
    }
    if !pivot.values.is_empty() {
        s.push_str(&format!("<dataFields count=\"{}\">", pivot.values.len()));
        for f in &pivot.values {
            let name = if f.name.is_empty() {
                fields
                    .get(f.source_column as usize)
                    .cloned()
                    .unwrap_or_default()
            } else {
                f.name.clone()
            };
            s.push_str(&format!(
                "<dataField name=\"{}\" fld=\"{}\" subtotal=\"{}\" baseField=\"0\" baseItem=\"0\"/>",
                escape_attr(&name),
                f.source_column,
                subtotal_token(f)
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
    let _ = n;
    s
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
