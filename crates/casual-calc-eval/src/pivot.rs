//! Computing a pivot table and writing its report into the sheet.
//!
//! The definition lives in [`casual_calc_model::PivotTable`]; this is the part
//! that answers it. Three stages, deliberately separated:
//!
//! 1. **Read** the source rectangle into records, keeping only the columns the
//!    pivot actually uses.
//! 2. **Aggregate** into every (row-prefix, column-prefix) pair at once, which
//!    is what makes subtotals and grand totals fall out of the same pass rather
//!    than needing a second traversal per level.
//! 3. **Lay out** the report and write it as ordinary cells.
//!
//! Stage 3 writing plain cells is the whole reason a workbook we produce opens
//! anywhere. Excel does the same — a pivot's numbers are in `sheetData`, not
//! computed on load — so a reader that has never heard of pivot tables still
//! shows the right report.
//!
//! Refreshing **refuses rather than overwrites**. If the new report would land
//! on a cell that is not part of the previous one and is not empty, nothing is
//! written and the caller is told where the collision is. Silently replacing
//! something the user typed is the one behaviour a spreadsheet must not have,
//! and it is the same rule spilling follows.

use std::collections::{BTreeMap, BTreeSet};

use casual_calc_model::{
    Cell, CellRange, CellRef, CellValue, ErrorValue, PivotAggregate, PivotSort, PivotTable,
    PivotValueField, Sheet, Style, Workbook,
};

use crate::value::{Value, value_from_cell};

/// The text shown for a page filter that is not narrowing anything.
pub const ALL_ITEMS: &str = "(All)";
/// The caption on the grand-total row and column.
pub const GRAND_TOTAL: &str = "Grand Total";
/// What an empty key displays as, matching Excel's own placeholder.
pub const BLANK_ITEM: &str = "(blank)";

/// Why a refresh could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PivotError {
    /// The source rectangle has no data rows under its header.
    EmptySource,
    /// The pivot names a source sheet that is no longer in the workbook.
    MissingSource,
    /// Nothing is on the values axis, so there is nothing to report.
    NoValues,
    /// The report would land on a cell holding something else, at this address.
    Collision(CellRef),
}

impl core::fmt::Display for PivotError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptySource => f.write_str("the source range has no rows under its header"),
            Self::MissingSource => f.write_str("the source sheet is gone"),
            Self::NoValues => f.write_str("add at least one field to Values"),
            Self::Collision(at) => write!(
                f,
                "the report would overwrite data at row {}, column {}",
                at.row + 1,
                at.col + 1
            ),
        }
    }
}

/// A distinct value of a field, as the pivot groups by it.
///
/// Ordering is Excel's: numbers, then text, then booleans, then errors, and
/// blanks last. A column of mixed types therefore comes out in blocks rather
/// than interleaved, which is what makes a mistyped number visible instead of
/// hidden among the real ones.
#[derive(Debug, Clone, PartialEq)]
enum PKey {
    Number(f64),
    Text(String),
    Bool(bool),
    Error(ErrorValue),
    Blank,
}

impl PKey {
    fn rank(&self) -> u8 {
        match self {
            Self::Number(_) => 0,
            Self::Text(_) => 1,
            Self::Bool(_) => 2,
            Self::Error(_) => 3,
            Self::Blank => 4,
        }
    }
}

impl Eq for PKey {}

impl Ord for PKey {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        match (self, other) {
            // `total_cmp` rather than `partial_cmp`: a NaN in the data must not
            // make the sort order depend on comparison order, or two refreshes
            // of the same workbook could disagree.
            (Self::Number(a), Self::Number(b)) => a.total_cmp(b),
            // Case-insensitive, like Excel's own item sort, with a case-
            // sensitive tiebreak so `a` and `A` still order deterministically.
            (Self::Text(a), Self::Text(b)) => a
                .to_lowercase()
                .cmp(&b.to_lowercase())
                .then_with(|| a.cmp(b)),
            (Self::Bool(a), Self::Bool(b)) => a.cmp(b),
            (Self::Error(a), Self::Error(b)) => a.to_string().cmp(&b.to_string()),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

impl PartialOrd for PKey {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Running totals for one measure over one group.
///
/// Every aggregate the model offers is derivable from these six numbers, so a
/// record is visited once no matter how many summaries are asked for, and
/// adding `Average` beside `Sum` costs nothing.
#[derive(Debug, Clone, Default)]
struct Acc {
    /// Records with any value at all (`COUNTA`).
    count: f64,
    /// Records with a numeric value (`COUNT`).
    count_nums: f64,
    sum: f64,
    /// Sum of squares, for the variance family.
    sum_sq: f64,
    product: f64,
    min: f64,
    max: f64,
}

impl Acc {
    fn add(&mut self, num: Option<f64>, nonempty: bool) {
        if nonempty {
            self.count += 1.0;
        }
        let Some(n) = num else { return };
        if self.count_nums == 0.0 {
            self.min = n;
            self.max = n;
            self.product = 1.0;
        }
        self.count_nums += 1.0;
        self.sum += n;
        self.sum_sq += n * n;
        self.product *= n;
        self.min = self.min.min(n);
        self.max = self.max.max(n);
    }

    fn finish(&self, aggregate: PivotAggregate) -> Value {
        let n = self.count_nums;
        // An aggregate over no numbers is nothing at all, not zero: writing 0
        // where a group has no numeric data claims a measurement that was never
        // taken. Excel leaves the cell empty too.
        let need_nums = |v: f64| {
            if n == 0.0 {
                Value::Empty
            } else {
                Value::Number(v)
            }
        };
        match aggregate {
            PivotAggregate::Sum => need_nums(self.sum),
            PivotAggregate::Count => Value::Number(self.count),
            PivotAggregate::CountNums => Value::Number(self.count_nums),
            PivotAggregate::Average => {
                if n == 0.0 {
                    Value::Error(ErrorValue::Div0)
                } else {
                    Value::Number(self.sum / n)
                }
            }
            PivotAggregate::Max => need_nums(self.max),
            PivotAggregate::Min => need_nums(self.min),
            PivotAggregate::Product => need_nums(self.product),
            PivotAggregate::StdDev | PivotAggregate::Var => {
                if n < 2.0 {
                    Value::Error(ErrorValue::Div0)
                } else {
                    let var = (self.sum_sq - self.sum * self.sum / n) / (n - 1.0);
                    let var = var.max(0.0);
                    Value::Number(if aggregate == PivotAggregate::Var {
                        var
                    } else {
                        var.sqrt()
                    })
                }
            }
            PivotAggregate::StdDevP | PivotAggregate::VarP => {
                if n == 0.0 {
                    Value::Error(ErrorValue::Div0)
                } else {
                    let var = (self.sum_sq - self.sum * self.sum / n) / n;
                    let var = var.max(0.0);
                    Value::Number(if aggregate == PivotAggregate::VarP {
                        var
                    } else {
                        var.sqrt()
                    })
                }
            }
        }
    }
}

/// A tuple of item indices identifying a group, or a prefix of one.
type Prefix = Vec<u32>;

/// One rendered cell of a report, before it is written.
#[derive(Debug, Clone, PartialEq)]
pub struct PivotCell {
    /// Where it goes, absolute on the pivot's own sheet.
    pub at: CellRef,
    /// What it holds.
    pub value: Value,
    /// How it is painted.
    pub kind: PivotCellKind,
    /// The number-format code, when the measure carries one.
    pub number_format: Option<String>,
}

/// The role a report cell plays, which is what decides how it is painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PivotCellKind {
    /// A page-filter name or its current selection.
    Filter,
    /// Any part of the header block.
    Header,
    /// An item label down the side.
    RowLabel,
    /// A subtotal label or figure.
    Subtotal,
    /// A grand-total label or figure.
    GrandTotal,
    /// An aggregated measure.
    Value,
}

/// A computed report: where it goes and what is in it.
#[derive(Debug, Clone, PartialEq)]
pub struct PivotReport {
    /// The rectangle the report occupies.
    pub range: CellRange,
    /// Its cells. Blank positions are simply absent.
    pub cells: Vec<PivotCell>,
    /// The first row of the data area, used to band alternate rows.
    pub first_data_row: u32,
}

/// The source header names, left to right — the field list a UI offers.
#[must_use]
pub fn field_names(workbook: &Workbook, pivot: &PivotTable) -> Vec<String> {
    let Some(sheet) = source_sheet(workbook, pivot) else {
        return Vec::new();
    };
    let header = pivot.source.start.row;
    (pivot.source.start.col..=pivot.source.end.col)
        .enumerate()
        .map(|(i, col)| {
            let text = cell_text(workbook, sheet, CellRef::new(header, col));
            if text.is_empty() {
                format!("Column{}", i + 1)
            } else {
                text
            }
        })
        .collect()
}

/// The distinct values of one source column, in display order — what a page
/// filter's dropdown lists.
#[must_use]
pub fn field_items(workbook: &Workbook, pivot: &PivotTable, source_column: u32) -> Vec<String> {
    let Some(sheet) = source_sheet(workbook, pivot) else {
        return Vec::new();
    };
    let col = pivot.source.start.col + source_column;
    if col > pivot.source.end.col {
        return Vec::new();
    }
    let mut seen: BTreeSet<PKey> = BTreeSet::new();
    for row in (pivot.source.start.row + 1)..=pivot.source.end.row {
        seen.insert(key_at(workbook, sheet, CellRef::new(row, col)));
    }
    seen.into_iter().map(|k| key_text(&k)).collect()
}

fn source_sheet<'a>(workbook: &'a Workbook, pivot: &PivotTable) -> Option<&'a Sheet> {
    workbook.sheets.iter().find(|s| s.id == pivot.source_sheet)
}

fn cell_text(workbook: &Workbook, sheet: &Sheet, at: CellRef) -> String {
    sheet.cells.get(at).map_or_else(String::new, |cell| {
        value_from_cell(&cell.value, &workbook.strings)
            .as_text()
            .unwrap_or_default()
    })
}

fn key_at(workbook: &Workbook, sheet: &Sheet, at: CellRef) -> PKey {
    match sheet.cells.get(at).map(|c| &c.value) {
        None | Some(CellValue::Empty) => PKey::Blank,
        Some(CellValue::Number(n)) => PKey::Number(*n),
        Some(CellValue::Bool(b)) => PKey::Bool(*b),
        Some(CellValue::Error(e)) => PKey::Error(*e),
        Some(CellValue::SharedString(id) | CellValue::InlineString(id)) => {
            let text = workbook.strings.get(*id).unwrap_or_default();
            if text.is_empty() {
                PKey::Blank
            } else {
                PKey::Text(text.to_owned())
            }
        }
    }
}

/// How an item reads on screen — and, because a page filter stores its
/// selection as text, the identity a filter compares against.
fn key_text(key: &PKey) -> String {
    match key {
        PKey::Number(n) => casual_calc_layout::format_general(*n),
        PKey::Text(s) => s.clone(),
        PKey::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        PKey::Error(e) => e.to_string(),
        PKey::Blank => BLANK_ITEM.to_owned(),
    }
}

/// One source row, reduced to what this pivot needs of it.
struct Record {
    /// Item index per row field.
    rows: Vec<u32>,
    /// Item index per column field.
    cols: Vec<u32>,
    /// `(numeric value, is non-empty)` per measure.
    values: Vec<(Option<f64>, bool)>,
}

/// The distinct items of one axis field, in display order.
struct Axis {
    items: Vec<PKey>,
    /// Item -> its index in `items`, so a record is placed in one lookup.
    index: BTreeMap<PKey, u32>,
}

impl Axis {
    fn build(mut items: Vec<PKey>, sort: PivotSort, first_seen: &[PKey]) -> Self {
        match sort {
            PivotSort::Ascending => items.sort(),
            PivotSort::Descending => {
                items.sort();
                items.reverse();
            }
            PivotSort::DataSource => {
                items = first_seen.to_vec();
            }
        }
        let index = items
            .iter()
            .enumerate()
            .map(|(i, k)| (k.clone(), i as u32))
            .collect();
        Self { items, index }
    }
}

/// The source, reduced to what the report needs: the surviving records and the
/// ordered items of every field they are grouped by.
struct Source {
    records: Vec<Record>,
    row_axes: Vec<Axis>,
    col_axes: Vec<Axis>,
}

/// Read, filter, and index the source records.
fn read_records(workbook: &Workbook, pivot: &PivotTable) -> Result<Source, PivotError> {
    let sheet = source_sheet(workbook, pivot).ok_or(PivotError::MissingSource)?;
    if pivot.source.end.row <= pivot.source.start.row {
        return Err(PivotError::EmptySource);
    }
    let base = pivot.source.start.col;
    let last = pivot.source.end.col;
    let at = |column: u32| -> Option<u32> {
        let col = base + column;
        (col <= last).then_some(col)
    };

    // Pass one: the surviving rows and every field's distinct items, both in
    // source order so `DataSource` sorting has something to preserve.
    let mut kept: Vec<Vec<PKey>> = Vec::new();
    let mut nums: Vec<Vec<(Option<f64>, bool)>> = Vec::new();
    let axis_fields: Vec<u32> = pivot
        .rows
        .iter()
        .chain(pivot.cols.iter())
        .map(|f| f.source_column)
        .collect();

    for row in (pivot.source.start.row + 1)..=pivot.source.end.row {
        // A page filter with no selection is the `(All)` state and narrows
        // nothing; only a non-empty list excludes anything.
        let excluded = pivot.filters.iter().any(|f| {
            if f.selected.is_empty() {
                return false;
            }
            let Some(col) = at(f.source_column) else {
                return false;
            };
            !f.selected
                .contains(&key_text(&key_at(workbook, sheet, CellRef::new(row, col))))
        });
        if excluded {
            continue;
        }
        kept.push(
            axis_fields
                .iter()
                .map(|c| {
                    at(*c).map_or(PKey::Blank, |col| {
                        key_at(workbook, sheet, CellRef::new(row, col))
                    })
                })
                .collect(),
        );
        nums.push(
            pivot
                .values
                .iter()
                .map(|v| {
                    let Some(col) = at(v.source_column) else {
                        return (None, false);
                    };
                    match sheet.cells.get(CellRef::new(row, col)).map(|c| &c.value) {
                        None | Some(CellValue::Empty) => (None, false),
                        Some(CellValue::Number(n)) => (Some(*n), true),
                        // A boolean counts as a value and as the number Excel
                        // coerces it to; a text or error cell counts only
                        // towards COUNTA, which is why `Count` and `Sum` can
                        // disagree about how much data a group holds.
                        Some(CellValue::Bool(b)) => (Some(if *b { 1.0 } else { 0.0 }), true),
                        Some(_) => (None, true),
                    }
                })
                .collect(),
        );
    }
    if kept.is_empty() {
        return Err(PivotError::EmptySource);
    }

    // Pass two: turn each field's items into an ordered axis, then place every
    // record on it.
    let mut axes: Vec<Axis> = Vec::new();
    for (slot, field) in pivot.rows.iter().chain(pivot.cols.iter()).enumerate() {
        let mut first_seen: Vec<PKey> = Vec::new();
        let mut distinct: BTreeSet<PKey> = BTreeSet::new();
        for record in &kept {
            let key = record[slot].clone();
            if distinct.insert(key.clone()) {
                first_seen.push(key);
            }
        }
        axes.push(Axis::build(
            distinct.into_iter().collect(),
            field.sort,
            &first_seen,
        ));
    }

    let row_count = pivot.rows.len();
    let records = kept
        .into_iter()
        .zip(nums)
        .map(|(keys, values)| Record {
            rows: (0..row_count)
                .map(|i| axes[i].index[&keys[i]])
                .collect::<Vec<_>>(),
            cols: (row_count..keys.len())
                .map(|i| axes[i].index[&keys[i]])
                .collect::<Vec<_>>(),
            values,
        })
        .collect();

    let col_axes = axes.split_off(row_count);
    Ok(Source {
        records,
        row_axes: axes,
        col_axes,
    })
}

/// The caption a measure is addressed by — its own name, or the one derived
/// from its aggregate and field as Excel derives it.
#[must_use]
pub fn value_caption(workbook: &Workbook, pivot: &PivotTable, value: &PivotValueField) -> String {
    if !value.name.is_empty() {
        return value.name.clone();
    }
    let field = field_names(workbook, pivot)
        .get(value.source_column as usize)
        .cloned()
        .unwrap_or_default();
    format!("{} {field}", value.aggregate.caption_prefix())
}

/// Answer one figure from a pivot without going near its layout — what
/// `GETPIVOTDATA` asks for.
///
/// `measure` names a value field, by its caption (`Sum of Amount`) or by the
/// bare source field (`Amount`). Each `(field, item)` narrows to one item of
/// one grouping field; with none, the answer is the grand total, which is
/// exactly the same query with every group left open.
///
/// Deliberately re-aggregated rather than read out of the written report.
/// Locating a cell means reproducing the layout's rules — which labels are
/// written, where a subtotal sits — in a second place, and the two would drift.
/// This asks the source the same question the report asked.
///
/// A field that is not on any axis is refused: Excel refuses it too, and
/// answering would report a figure the report does not show.
pub fn lookup(
    workbook: &Workbook,
    pivot: &PivotTable,
    measure: &str,
    criteria: &[(String, String)],
) -> Result<Value, ErrorValue> {
    let index = pivot
        .values
        .iter()
        .position(|v| {
            value_caption(workbook, pivot, v).eq_ignore_ascii_case(measure)
                || field_names(workbook, pivot)
                    .get(v.source_column as usize)
                    .is_some_and(|n| n.eq_ignore_ascii_case(measure))
        })
        .ok_or(ErrorValue::Ref)?;
    let value = &pivot.values[index];

    let names = field_names(workbook, pivot);
    let Source {
        records,
        row_axes,
        col_axes,
    } = read_records(workbook, pivot).map_err(|_| ErrorValue::Ref)?;

    // Criteria are matched against the grouping fields by name, then against
    // the record's own key text — the same rendering the report labels use, so
    // what a formula names is what a reader sees.
    let axis_fields: Vec<u32> = pivot
        .rows
        .iter()
        .chain(pivot.cols.iter())
        .map(|f| f.source_column)
        .collect();
    let mut wanted: Vec<(usize, &str)> = Vec::new();
    for (field, item) in criteria {
        let column = names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(field))
            .ok_or(ErrorValue::Ref)?;
        let slot = axis_fields
            .iter()
            .position(|c| *c as usize == column)
            .ok_or(ErrorValue::Ref)?;
        wanted.push((slot, item.as_str()));
    }

    // The axes turn an item's text back into the index the records carry.
    let axes: Vec<&Axis> = row_axes.iter().chain(col_axes.iter()).collect();
    let mut targets: Vec<(usize, u32)> = Vec::new();
    for (slot, item) in wanted {
        let axis = axes.get(slot).ok_or(ErrorValue::Ref)?;
        let found = axis
            .items
            .iter()
            .position(|k| key_text(k).eq_ignore_ascii_case(item))
            .ok_or(ErrorValue::Ref)?;
        targets.push((slot, found as u32));
    }

    let row_count = pivot.rows.len();
    let mut acc = Acc::default();
    let mut matched = false;
    for record in &records {
        let hit = targets.iter().all(|(slot, want)| {
            let got = if *slot < row_count {
                record.rows[*slot]
            } else {
                record.cols[*slot - row_count]
            };
            got == *want
        });
        if hit {
            matched = true;
            let (num, nonempty) = record.values[index];
            acc.add(num, nonempty);
        }
    }
    // No record at all is `#REF!`, not an empty aggregate: the report has no
    // such intersection to point at, and a blank would read as "zero here".
    if !matched {
        return Err(ErrorValue::Ref);
    }
    Ok(acc.finish(value.aggregate))
}

/// Accumulate every record into every (row-prefix, column-prefix) pair.
///
/// The prefixes are what subtotals and grand totals *are*: the grand total is
/// the empty prefix on both axes, a row subtotal is a short row prefix against
/// the empty column prefix, and a leaf figure is both prefixes at full length.
/// Computing them together costs `(R+1)·(C+1)` accumulator updates per record
/// and removes any chance of a subtotal disagreeing with the rows above it.
fn accumulate(records: &[Record], values: usize) -> BTreeMap<(Prefix, Prefix, u32), Acc> {
    let mut acc: BTreeMap<(Prefix, Prefix, u32), Acc> = BTreeMap::new();
    for record in records {
        for r in 0..=record.rows.len() {
            for c in 0..=record.cols.len() {
                for v in 0..values {
                    let key = (
                        record.rows[..r].to_vec(),
                        record.cols[..c].to_vec(),
                        v as u32,
                    );
                    let (num, nonempty) = record.values[v];
                    acc.entry(key).or_default().add(num, nonempty);
                }
            }
        }
    }
    acc
}

/// A line of the report's row area: a leaf, a subtotal, or the grand total.
#[derive(Debug, Clone, PartialEq)]
enum Line {
    Leaf(Prefix),
    Subtotal(Prefix),
    Grand,
}

/// Expand the occurring tuples into display order, inserting subtotals where a
/// group closes and the grand total at the end.
///
/// Subtotals go innermost-first, so a change at level 1 emits the level-2
/// subtotal before the level-1 one, which is the nesting a reader expects.
fn lines(tuples: &BTreeSet<Prefix>, subtotal_at: &[bool], grand: bool) -> Vec<Line> {
    let depth = subtotal_at.len();
    let mut out: Vec<Line> = Vec::new();
    let mut previous: Option<&Prefix> = None;
    for tuple in tuples {
        if let Some(prev) = previous {
            for level in (1..depth).rev() {
                if prev[..level] != tuple[..level] && subtotal_at[level - 1] {
                    out.push(Line::Subtotal(prev[..level].to_vec()));
                }
            }
        }
        out.push(Line::Leaf(tuple.clone()));
        previous = Some(tuple);
    }
    if let Some(prev) = previous {
        for level in (1..depth).rev() {
            if subtotal_at[level - 1] {
                out.push(Line::Subtotal(prev[..level].to_vec()));
            }
        }
    }
    if grand && depth > 0 {
        out.push(Line::Grand);
    }
    out
}

/// The prefix a line aggregates over.
fn line_prefix(line: &Line) -> Prefix {
    match line {
        Line::Leaf(p) | Line::Subtotal(p) => p.clone(),
        Line::Grand => Vec::new(),
    }
}

/// Compute the report without writing it.
///
/// Separated from the write so `GETPIVOTDATA` and a preview can ask for the
/// numbers without touching the sheet.
pub fn compute(workbook: &Workbook, pivot: &PivotTable) -> Result<PivotReport, PivotError> {
    if pivot.values.is_empty() {
        return Err(PivotError::NoValues);
    }
    let Source {
        records,
        row_axes,
        col_axes,
    } = read_records(workbook, pivot)?;
    let acc = accumulate(&records, pivot.values.len());

    let row_tuples: BTreeSet<Prefix> = records.iter().map(|r| r.rows.clone()).collect();
    let col_tuples: BTreeSet<Prefix> = records.iter().map(|r| r.cols.clone()).collect();
    let row_subs: Vec<bool> = pivot.rows.iter().map(|f| f.subtotal).collect();
    let col_subs: Vec<bool> = pivot.cols.iter().map(|f| f.subtotal).collect();
    let row_lines = lines(&row_tuples, &row_subs, pivot.row_grand_totals);
    let col_lines = lines(&col_tuples, &col_subs, pivot.col_grand_totals);

    let names = field_names(workbook, pivot);
    let caption = |v: &casual_calc_model::PivotValueField| -> String {
        if !v.name.is_empty() {
            return v.name.clone();
        }
        let field = names
            .get(v.source_column as usize)
            .cloned()
            .unwrap_or_default();
        format!("{} {field}", v.aggregate.caption_prefix())
    };

    let r0 = pivot.anchor.row;
    let c0 = pivot.anchor.col;
    let row_fields = pivot.rows.len() as u32;
    let value_count = pivot.values.len() as u32;
    let mut cells: Vec<PivotCell> = Vec::new();

    // Page filters stack above the report, one per row, with a blank row under
    // them so the header block reads as separate.
    let mut top = r0;
    for filter in &pivot.filters {
        let name = names
            .get(filter.source_column as usize)
            .cloned()
            .unwrap_or_default();
        let shown = if filter.selected.is_empty() {
            ALL_ITEMS.to_owned()
        } else if filter.selected.len() == 1 {
            filter.selected[0].clone()
        } else {
            format!("({} items)", filter.selected.len())
        };
        cells.push(PivotCell {
            at: CellRef::new(top, c0),
            value: Value::Text(name),
            kind: PivotCellKind::Filter,
            number_format: None,
        });
        cells.push(PivotCell {
            at: CellRef::new(top, c0 + 1),
            value: Value::Text(shown),
            kind: PivotCellKind::Filter,
            number_format: None,
        });
        top += 1;
    }
    if !pivot.filters.is_empty() {
        top += 1;
    }

    // The data area is one column per (column line × measure). With no column
    // field there is one line — the empty prefix — so a pivot with only row
    // fields still gets its measures side by side.
    let col_slots: Vec<Prefix> = if pivot.cols.is_empty() {
        vec![Vec::new()]
    } else {
        col_lines.iter().map(line_prefix).collect()
    };
    let data_c0 = c0 + row_fields;

    // Header block. With no column field it is a single row of captions; with
    // one it is the outer field's name, then a row per column field, then a
    // measure row when there is more than one measure to distinguish.
    let head_rows: u32 = if pivot.cols.is_empty() {
        1
    } else {
        1 + pivot.cols.len() as u32 + u32::from(value_count > 1)
    };
    let last_head = top + head_rows - 1;

    if pivot.cols.is_empty() {
        for (i, v) in pivot.values.iter().enumerate() {
            cells.push(PivotCell {
                at: CellRef::new(last_head, data_c0 + i as u32),
                value: Value::Text(caption(v)),
                kind: PivotCellKind::Header,
                number_format: None,
            });
        }
    } else {
        // Top-left corner: the measure when there is only one, else the word
        // Excel uses when several share the axis.
        cells.push(PivotCell {
            at: CellRef::new(top, c0),
            value: Value::Text(if value_count == 1 {
                caption(&pivot.values[0])
            } else {
                "Values".to_owned()
            }),
            kind: PivotCellKind::Header,
            number_format: None,
        });
        let outer = names
            .get(pivot.cols[0].source_column as usize)
            .cloned()
            .unwrap_or_default();
        cells.push(PivotCell {
            at: CellRef::new(top, data_c0),
            value: Value::Text(outer),
            kind: PivotCellKind::Header,
            number_format: None,
        });
        // One row per column field, each showing that field's item. A label is
        // written only where it changes, so a value spanning four columns is
        // written once — which is what makes the grouping visible without
        // merging cells that a formula would then have to see through.
        for (depth, axis) in col_axes.iter().enumerate() {
            let row = top + 1 + depth as u32;
            let mut previous: Option<&Prefix> = None;
            for (slot, prefix) in col_slots.iter().enumerate() {
                let column = data_c0 + slot as u32 * value_count;
                let (text, kind) = if prefix.len() > depth {
                    let changed = previous
                        .is_none_or(|p| p.len() <= depth || p[..=depth] != prefix[..=depth]);
                    if !changed {
                        previous = Some(prefix);
                        continue;
                    }
                    (
                        key_text(&axis.items[prefix[depth] as usize]),
                        PivotCellKind::Header,
                    )
                } else if depth == prefix.len() {
                    // A short prefix here is a subtotal or the grand total: the
                    // line stops before this field, and the label says so.
                    let label = if prefix.is_empty() {
                        GRAND_TOTAL.to_owned()
                    } else {
                        format!(
                            "{} Total",
                            key_text(
                                &col_axes[prefix.len() - 1].items
                                    [prefix[prefix.len() - 1] as usize]
                            )
                        )
                    };
                    (
                        label,
                        if prefix.is_empty() {
                            PivotCellKind::GrandTotal
                        } else {
                            PivotCellKind::Subtotal
                        },
                    )
                } else {
                    previous = Some(prefix);
                    continue;
                };
                cells.push(PivotCell {
                    at: CellRef::new(row, column),
                    value: Value::Text(text),
                    kind,
                    number_format: None,
                });
                previous = Some(prefix);
            }
        }
        if value_count > 1 {
            for (slot, _) in col_slots.iter().enumerate() {
                for (i, v) in pivot.values.iter().enumerate() {
                    cells.push(PivotCell {
                        at: CellRef::new(last_head, data_c0 + slot as u32 * value_count + i as u32),
                        value: Value::Text(caption(v)),
                        kind: PivotCellKind::Header,
                        number_format: None,
                    });
                }
            }
        }
    }
    // The row-field names sit on the last header row, directly over the labels
    // they describe.
    for (i, field) in pivot.rows.iter().enumerate() {
        cells.push(PivotCell {
            at: CellRef::new(last_head, c0 + i as u32),
            value: Value::Text(
                names
                    .get(field.source_column as usize)
                    .cloned()
                    .unwrap_or_default(),
            ),
            kind: PivotCellKind::Header,
            number_format: None,
        });
    }

    // The body. With no row field there is a single line, so a pivot that only
    // has column fields still reports one row of figures.
    let row_slots: Vec<Line> = if pivot.rows.is_empty() {
        vec![Line::Leaf(Vec::new())]
    } else {
        row_lines
    };
    let first_data_row = last_head + 1;
    for (line_index, line) in row_slots.iter().enumerate() {
        let row = first_data_row + line_index as u32;
        let prefix = line_prefix(line);
        match line {
            Line::Leaf(p) => {
                // Only the labels that changed since the line above, for the
                // same reason as the column headers.
                let previous = line_index
                    .checked_sub(1)
                    .and_then(|i| row_slots.get(i))
                    .map(line_prefix);
                for (depth, axis) in row_axes.iter().enumerate() {
                    let same = previous
                        .as_ref()
                        .is_some_and(|q| q.len() > depth && q[..=depth] == p[..=depth]);
                    if same {
                        continue;
                    }
                    cells.push(PivotCell {
                        at: CellRef::new(row, c0 + depth as u32),
                        value: Value::Text(key_text(&axis.items[p[depth] as usize])),
                        kind: PivotCellKind::RowLabel,
                        number_format: None,
                    });
                }
            }
            Line::Subtotal(p) => {
                let depth = p.len() - 1;
                cells.push(PivotCell {
                    at: CellRef::new(row, c0 + depth as u32),
                    value: Value::Text(format!(
                        "{} Total",
                        key_text(&row_axes[depth].items[p[depth] as usize])
                    )),
                    kind: PivotCellKind::Subtotal,
                    number_format: None,
                });
            }
            Line::Grand => {
                cells.push(PivotCell {
                    at: CellRef::new(row, c0),
                    value: Value::Text(GRAND_TOTAL.to_owned()),
                    kind: PivotCellKind::GrandTotal,
                    number_format: None,
                });
            }
        }
        let row_kind = match line {
            Line::Leaf(_) => PivotCellKind::Value,
            Line::Subtotal(_) => PivotCellKind::Subtotal,
            Line::Grand => PivotCellKind::GrandTotal,
        };
        for (slot, col_prefix) in col_slots.iter().enumerate() {
            for (i, v) in pivot.values.iter().enumerate() {
                let key = (prefix.clone(), col_prefix.clone(), i as u32);
                let value = acc
                    .get(&key)
                    .map_or(Value::Empty, |a| a.finish(v.aggregate));
                let kind =
                    if col_prefix.len() < pivot.cols.len() && row_kind == PivotCellKind::Value {
                        // A short column prefix on an ordinary row is a column
                        // subtotal or the column grand total.
                        if col_prefix.is_empty() {
                            PivotCellKind::GrandTotal
                        } else {
                            PivotCellKind::Subtotal
                        }
                    } else {
                        row_kind
                    };
                cells.push(PivotCell {
                    at: CellRef::new(row, data_c0 + slot as u32 * value_count + i as u32),
                    value,
                    kind,
                    number_format: v.number_format.clone(),
                });
            }
        }
    }

    let last_row = first_data_row + row_slots.len() as u32 - 1;
    let last_col = data_c0 + col_slots.len() as u32 * value_count - 1;
    Ok(PivotReport {
        range: CellRange::new(pivot.anchor, CellRef::new(last_row, last_col.max(c0))),
        cells,
        first_data_row,
    })
}

/// Everything a refresh would do to the sheet, before any of it is done.
///
/// A plan rather than a direct write so the host can put the whole refresh
/// through its own transaction layer as one undoable step. Applying it here
/// with [`refresh`] and applying it as a batch of cell operations produce the
/// same grid — there is one layout routine, not two.
#[derive(Debug, Clone, PartialEq)]
pub struct PivotPlan {
    /// The rectangle the new report occupies.
    pub range: CellRange,
    /// Every cell the refresh touches: the new contents, or `None` where the
    /// previous report reached and this one does not.
    pub cells: Vec<(CellRef, Option<Cell>)>,
    /// Column widths (twips) wide enough for the report, as `(column, width)`.
    ///
    /// Excel widens a pivot's columns on every update, and without it
    /// `Average of Amount` reads as `Average`: a header is only clipped
    /// because the cell beside it is occupied, which in a report is always.
    /// Part of the plan rather than a follow-up call so it lands in the same
    /// undoable step — one `Ctrl+Z` after a layout change should not give back
    /// a column width.
    pub widths: Vec<(u32, i64)>,
}

/// Twips wide enough for `chars` characters at the default font.
///
/// The inverse of the writer's `twips_to_col_chars`: OOXML measures a column
/// in characters, so counting them is the format's own unit rather than an
/// approximation of pixels. Real glyph metrics would be closer still, but they
/// live in the host and this is only ever used to make a column *wider* than
/// the default that would otherwise clip.
fn width_for_chars(chars: usize) -> i64 {
    (((chars.min(MAX_AUTOFIT_CHARS) as f64) * 7.0 + 5.0) * 15.0).round() as i64
}

/// The widest a report column is grown to. A stray long label should not push
/// the rest of the report off the screen.
const MAX_AUTOFIT_CHARS: usize = 40;

/// Work out what a refresh would write, interning the strings and styles it
/// needs but touching no cell.
///
/// On [`PivotError::Collision`] the plan is abandoned whole. A refresh that
/// wrote the cells that happened to fit and stopped at the first obstruction
/// would leave a report that is half one answer and half another.
pub fn plan(
    workbook: &mut Workbook,
    sheet_index: usize,
    pivot_index: usize,
) -> Result<PivotPlan, PivotError> {
    let pivot = workbook.sheets[sheet_index].pivots[pivot_index].clone();
    let report = compute(workbook, &pivot)?;

    // A cell inside the previous report is ours to reuse; anything else must
    // already be empty. Checked before anything is planned, so a refusal costs
    // nothing.
    let previous = pivot.output;
    let inside = |range: Option<CellRange>, at: CellRef| {
        range.is_some_and(|r| {
            at.row >= r.start.row
                && at.row <= r.end.row
                && at.col >= r.start.col
                && at.col <= r.end.col
        })
    };
    for row in report.range.start.row..=report.range.end.row {
        for col in report.range.start.col..=report.range.end.col {
            let at = CellRef::new(row, col);
            if inside(previous, at) {
                continue;
            }
            if workbook.sheets[sheet_index]
                .cells
                .get(at)
                .is_some_and(|c| !c.is_blank())
            {
                return Err(PivotError::Collision(at));
            }
        }
    }

    let mut cells: Vec<(CellRef, Option<Cell>)> = Vec::new();
    // Give back everything the previous report covered that this one does not.
    // Recomputing the old extent from the definition would only work while the
    // source had not changed, which is the one case a refresh is not for.
    if let Some(range) = previous {
        for row in range.start.row..=range.end.row {
            for col in range.start.col..=range.end.col {
                let at = CellRef::new(row, col);
                if !inside(Some(report.range), at) {
                    cells.push((at, None));
                }
            }
        }
    }

    let colors = casual_calc_layout::table_style::table_style_colors(
        workbook,
        if pivot.style.is_empty() {
            DEFAULT_STYLE
        } else {
            &pivot.style
        },
    );
    // Positions the report leaves blank inside its own rectangle — the gaps
    // between repeated labels — still have to be cleared, or the last refresh's
    // text shows through them.
    let mut filled: BTreeSet<CellRef> = BTreeSet::new();
    for cell in &report.cells {
        filled.insert(cell.at);
        let value = to_cell_value(workbook, &cell.value);
        let banded =
            cell.kind == PivotCellKind::Value && (cell.at.row - report.first_data_row) % 2 == 1;
        let style = Style {
            number_format: cell.number_format.clone(),
            bold: matches!(
                cell.kind,
                PivotCellKind::Header | PivotCellKind::Subtotal | PivotCellKind::GrandTotal
            ),
            fill_color: Some(match cell.kind {
                PivotCellKind::Header => colors.header_fill.clone(),
                PivotCellKind::Subtotal | PivotCellKind::GrandTotal => colors.band_fill.clone(),
                PivotCellKind::Filter => colors.body_fill.clone(),
                PivotCellKind::RowLabel | PivotCellKind::Value => {
                    if banded {
                        colors.band_fill.clone()
                    } else {
                        colors.body_fill.clone()
                    }
                }
            }),
            font_color: Some(match cell.kind {
                PivotCellKind::Header => colors.header_text.clone(),
                _ => colors.body_text.clone(),
            }),
            ..Style::default()
        };
        let style = workbook.intern_style(style);
        cells.push((
            cell.at,
            Some(Cell {
                value,
                style: Some(style),
                formula: None,
                flags: casual_calc_model::CellFlags::new(),
            }),
        ));
    }
    for row in report.range.start.row..=report.range.end.row {
        for col in report.range.start.col..=report.range.end.col {
            let at = CellRef::new(row, col);
            if !filled.contains(&at) {
                cells.push((at, None));
            }
        }
    }
    cells.sort_by_key(|(at, _)| *at);

    // Column widths, from the longest text each report column holds. Only ever
    // wider than what is there: a column the user widened by hand keeps its
    // width, and one they narrowed is theirs to have narrowed.
    let default_width = workbook.sheets[sheet_index]
        .columns
        .default
        .unwrap_or(casual_calc_layout::DEFAULT_COL_WIDTH);
    let mut longest: BTreeMap<u32, usize> = BTreeMap::new();
    for cell in &report.cells {
        let text = match &cell.value {
            Value::Text(s) => s.chars().count(),
            Value::Number(n) => match cell.number_format.as_deref() {
                Some(code) => casual_calc_layout::format_number(*n, code).chars().count(),
                None => casual_calc_layout::format_general(*n).chars().count(),
            },
            _ => 0,
        };
        let slot = longest.entry(cell.at.col).or_default();
        *slot = (*slot).max(text);
    }
    let widths = longest
        .into_iter()
        // Two characters of room. One is not enough: every header and total in
        // a report is bold, which is wider than the character unit assumes, and
        // a caption that ends exactly on the column edge reads as clipped even
        // when it is not.
        .map(|(col, chars)| (col, width_for_chars(chars + 2)))
        .filter(|(col, width)| {
            *width
                > workbook.sheets[sheet_index]
                    .columns
                    .size(*col, default_width)
        })
        .collect();

    Ok(PivotPlan {
        range: report.range,
        cells,
        widths,
    })
}

/// The table style a pivot uses when it names none.
pub const DEFAULT_STYLE: &str = "TableStyleMedium2";

/// Record that a plan has been applied: the new extent, and the retained parts
/// that no longer describe what is on the sheet.
pub fn commit(workbook: &mut Workbook, sheet_index: usize, pivot_index: usize, range: CellRange) {
    workbook.sheets[sheet_index].pivots[pivot_index].output = Some(range);
    detach(workbook, sheet_index, pivot_index);
}

/// Recompute one pivot and write its report into the sheet.
///
/// Returns the rectangle written. On [`PivotError::Collision`] nothing is
/// changed at all — not even the cells that would have fitted — so a refused
/// refresh leaves the previous report intact and readable.
pub fn refresh(
    workbook: &mut Workbook,
    sheet_index: usize,
    pivot_index: usize,
) -> Result<CellRange, PivotError> {
    let plan = plan(workbook, sheet_index, pivot_index)?;
    for (at, cell) in &plan.cells {
        match cell {
            Some(cell) => workbook.sheets[sheet_index].cells.set(*at, cell.clone()),
            None => {
                workbook.sheets[sheet_index].cells.clear(*at);
            }
        }
    }
    for (col, width) in &plan.widths {
        workbook.sheets[sheet_index]
            .columns
            .sizes
            .insert(*col, *width);
    }
    // Only now, once the report is actually on the sheet: a refused refresh
    // must leave an imported pivot exactly as it arrived, retained part and
    // all.
    commit(workbook, sheet_index, pivot_index, plan.range);
    Ok(plan.range)
}

/// Refresh every pivot the editor owns, collecting the ones that refused.
///
/// Called after a recalculation, because a pivot may summarize formula results
/// and those are only correct once the pass has run.
///
/// **Imported pivots are skipped.** They are refreshed only when the user asks,
/// because refreshing rewrites the block in our tabular layout while Excel's is
/// compact — doing it automatically would silently reformat every pivot in
/// every file anyone opens, and a save would then be an edit nobody made.
pub fn refresh_all(workbook: &mut Workbook) -> Vec<(String, PivotError)> {
    let mut failures = Vec::new();
    for sheet_index in 0..workbook.sheets.len() {
        for pivot_index in 0..workbook.sheets[sheet_index].pivots.len() {
            if workbook.sheets[sheet_index].pivots[pivot_index]
                .part
                .is_some()
            {
                continue;
            }
            if let Err(error) = refresh(workbook, sheet_index, pivot_index) {
                let name = workbook.sheets[sheet_index].pivots[pivot_index]
                    .name
                    .clone();
                failures.push((name, error));
            }
        }
    }
    failures
}

/// Resolve a relationship target against the part that declared it.
///
/// A `.rels` target is relative to its source's folder, so
/// `xl/pivotTables/pivotTable1.xml` reaching `../pivotCache/x.xml` lands in
/// `xl/pivotCache/x.xml`. Comparing the raw strings instead would fail to match
/// the very relationship that has to be removed.
fn resolve_part(source: &str, target: &str) -> String {
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

/// Stop writing an imported pivot back from its own bytes.
///
/// Called the moment the definition is edited or refreshed, because our report
/// is laid out in tabular form and the retained part describes Excel's compact
/// one. Keeping both would leave a file whose pivot part and whose cells
/// disagree — worse than having no part, because a reader believes the part.
///
/// Removes the pivot part, the relationship reaching it, and — when no other
/// pivot still uses it — the cache it shares, the workbook relationship to the
/// cache, and the `<pivotCache>` element declaring it. A cache left behind with
/// nothing pointing at it is what Excel reports as a file needing repair.
///
/// Returns the package paths dropped.
pub fn detach(workbook: &mut Workbook, sheet_index: usize, pivot_index: usize) -> Vec<String> {
    let Some(part) = workbook.sheets[sheet_index].pivots[pivot_index].detach() else {
        return Vec::new();
    };
    let mut dropped = vec![part.clone()];

    // The cache this pivot reached, before its own relationships go.
    let cache: Option<String> = workbook
        .retained_rels
        .iter()
        .find(|r| r.source == part && r.rel_type.ends_with("/pivotCacheDefinition"))
        .map(|r| resolve_part(&r.source, &r.target));

    workbook
        .retained_rels
        .retain(|r| r.source != part && resolve_part(&r.source, &r.target) != part);
    workbook.retained_parts.retain(|p| p.path != part);

    let Some(cache) = cache else { return dropped };
    // Another pivot may share the cache; Excel writes one per source range, not
    // one per pivot.
    let still_used = workbook
        .sheets
        .iter()
        .flat_map(|s| s.pivots.iter())
        .filter_map(|p| p.part.as_deref())
        .any(|other| {
            workbook
                .retained_rels
                .iter()
                .any(|r| r.source == other && resolve_part(&r.source, &r.target) == cache)
        });
    if still_used {
        return dropped;
    }

    // The `<pivotCache r:id>` in workbook.xml names the relationship, not the
    // part, so the element has to be found through the id it carries.
    let rel_ids: Vec<String> = workbook
        .retained_rels
        .iter()
        .filter(|r| resolve_part(&r.source, &r.target) == cache)
        .map(|r| r.id.clone())
        .collect();
    workbook.retained_refs.retain(|(name, attrs)| {
        name != "pivotCache" || !attrs.get("id").is_some_and(|id| rel_ids.contains(id))
    });
    // The records part hangs off the cache definition by its own relationship,
    // so it is found the same way rather than by guessing at the file name.
    let records: Vec<String> = workbook
        .retained_rels
        .iter()
        .filter(|r| r.source == cache)
        .map(|r| resolve_part(&r.source, &r.target))
        .collect();
    workbook
        .retained_rels
        .retain(|r| r.source != cache && resolve_part(&r.source, &r.target) != cache);
    workbook
        .retained_parts
        .retain(|p| p.path != cache && !records.contains(&p.path));
    dropped.push(cache);
    dropped.extend(records);
    dropped
}

fn to_cell_value(workbook: &mut Workbook, value: &Value) -> CellValue {
    match value {
        Value::Empty | Value::Lambda(_) | Value::Array { .. } => CellValue::Empty,
        Value::Number(n) => CellValue::Number(*n),
        Value::Bool(b) => CellValue::Bool(*b),
        Value::Error(e) => CellValue::Error(*e),
        Value::Text(s) => CellValue::SharedString(workbook.intern_string(s)),
    }
}
