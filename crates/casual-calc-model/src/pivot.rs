//! Pivot tables: the definition, not the result.
//!
//! A pivot table is a *query* — take a rectangle of records, group them by some
//! fields down the side and some across the top, and aggregate a measure at
//! each intersection. What this module holds is that query. The answer is
//! ordinary cells written into the sheet, which is also how Excel stores it:
//! open any `.xlsx` with a pivot in it and the numbers are right there in
//! `sheetData`. That is why an imported pivot renders correctly in a viewer
//! that has never heard of pivot tables, and why refreshing one here produces a
//! file every other reader can open.
//!
//! The layout written is **tabular**: one column per row field, subtotals at
//! the foot of each group, grand totals last. Excel's default is "compact",
//! which stacks every row field into a single indented column. Compact is a
//! presentation trick — the indent carries the field structure — and it makes
//! the output unreferenceable, because `A5` might be a region or a product
//! depending on how far it is indented. Tabular puts each field in its own
//! column, which is what a formula, a chart, or a second pivot can read.
//!
//! See `docs/54-PIVOT-TABLES.md`.

use serde::{Deserialize, Serialize};

use crate::ids::SheetId;
use crate::store::{CellRange, CellRef};

/// How a measure is summarized at each intersection.
///
/// These are OOXML's `dataField/@subtotal` values, and the names are Excel's
/// rather than the formula language's: `Count` is `COUNTA` (every non-empty
/// value) and `CountNums` is `COUNT` (numbers only). Excel picks `Count` when
/// the field holds text and `Sum` when it holds numbers, which is why both are
/// here and neither is "the" default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PivotAggregate {
    /// Total of the numbers.
    Sum,
    /// How many records have any value at all (`COUNTA`).
    Count,
    /// How many records have a numeric value (`COUNT`).
    CountNums,
    /// Arithmetic mean of the numbers.
    Average,
    /// Largest number.
    Max,
    /// Smallest number.
    Min,
    /// Product of the numbers.
    Product,
    /// Sample standard deviation.
    StdDev,
    /// Population standard deviation.
    StdDevP,
    /// Sample variance.
    Var,
    /// Population variance.
    VarP,
}

impl PivotAggregate {
    /// The OOXML `@subtotal` token, which is also the wire name.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Sum => "sum",
            Self::Count => "count",
            Self::CountNums => "countNums",
            Self::Average => "average",
            Self::Max => "max",
            Self::Min => "min",
            Self::Product => "product",
            Self::StdDev => "stdDev",
            Self::StdDevP => "stdDevp",
            Self::Var => "var",
            Self::VarP => "varp",
        }
    }

    /// Parse an OOXML `@subtotal` token. Unknown tokens fall back to `Sum`,
    /// which is the schema's own default.
    #[must_use]
    pub fn from_token(token: &str) -> Self {
        match token {
            "count" => Self::Count,
            "countNums" => Self::CountNums,
            "average" => Self::Average,
            "max" => Self::Max,
            "min" => Self::Min,
            "product" => Self::Product,
            "stdDev" => Self::StdDev,
            "stdDevp" => Self::StdDevP,
            "var" => Self::Var,
            "varp" => Self::VarP,
            _ => Self::Sum,
        }
    }

    /// The caption Excel writes when the user has not renamed the field, as in
    /// `Sum of Amount`.
    #[must_use]
    pub fn caption_prefix(self) -> &'static str {
        match self {
            Self::Sum => "Sum of",
            Self::Count => "Count of",
            Self::CountNums => "Count of",
            Self::Average => "Average of",
            Self::Max => "Max of",
            Self::Min => "Min of",
            Self::Product => "Product of",
            Self::StdDev => "StdDev of",
            Self::StdDevP => "StdDevp of",
            Self::Var => "Var of",
            Self::VarP => "Varp of",
        }
    }
}

/// The order a field's distinct values appear in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PivotSort {
    /// Numbers ascending, then text A–Z, then booleans — Excel's own ordering,
    /// and the reason a column of mixed types does not interleave.
    #[default]
    Ascending,
    /// The reverse.
    Descending,
    /// First appearance in the source, which is what a user who has already
    /// sorted the source data expects to see preserved.
    DataSource,
}

/// How a measure is reported: as the aggregate itself, or as a derivation of
/// it.
///
/// These are OOXML's `dataField/@showDataAs` values (`ST_ShowDataAs`,
/// `schemas/ooxml/sml.xsd:1515`), and all nine are here deliberately even
/// though the first release honours five. A new externally-tagged variant is a
/// break an old peer cannot read at all (`COL-54`), so completing the set once
/// — in the release that has working behaviour behind it — makes every later
/// mode a *behaviour* change rather than a second protocol change. See
/// `docs/85` §5.1.
///
/// **Nothing on [`PivotValueField`] carries this yet.** The field that does
/// arrives with `docs/85` §9 slice C, which is what pays the protocol bump;
/// the vocabulary is here because slice B — the exporter — cannot write
/// `@showDataAs` without a type that names the tokens, and naming them
/// anywhere else would mean naming them twice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PivotShowAs {
    /// The aggregate, undivided. The schema's default.
    #[default]
    Normal,
    /// The aggregate minus the base item's.
    Difference,
    /// The aggregate over the base item's.
    Percent,
    /// The difference from the base item, over the base item.
    PercentDiff,
    /// A running total along the base field.
    RunTotal,
    /// The aggregate over its row's total.
    PercentOfRow,
    /// The aggregate over its column's total.
    PercentOfCol,
    /// The aggregate over the grand total.
    PercentOfTotal,
    /// `v · grand / (row · col)` — how far the cell departs from what the
    /// margins alone would predict.
    Index,
}

impl PivotShowAs {
    /// The OOXML `@showDataAs` token, which is also the wire name.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Difference => "difference",
            Self::Percent => "percent",
            Self::PercentDiff => "percentDiff",
            Self::RunTotal => "runTotal",
            Self::PercentOfRow => "percentOfRow",
            Self::PercentOfCol => "percentOfCol",
            Self::PercentOfTotal => "percentOfTotal",
            Self::Index => "index",
        }
    }

    /// Parse an OOXML `@showDataAs` token. Unknown tokens fall back to
    /// [`Self::Normal`], which is the schema's own default.
    #[must_use]
    pub fn from_token(token: &str) -> Self {
        match token {
            "difference" => Self::Difference,
            "percent" => Self::Percent,
            "percentDiff" => Self::PercentDiff,
            "runTotal" => Self::RunTotal,
            "percentOfRow" => Self::PercentOfRow,
            "percentOfCol" => Self::PercentOfCol,
            "percentOfTotal" => Self::PercentOfTotal,
            "index" => Self::Index,
            _ => Self::Normal,
        }
    }

    /// Whether this mode reads a base field and a base item at all.
    ///
    /// The five that do not are the ones whose base is a truncation of the
    /// cell's own key, and they are the five the first release honours
    /// (`docs/85` §5.1).
    #[must_use]
    pub fn needs_base(self) -> bool {
        matches!(
            self,
            Self::Difference | Self::Percent | Self::PercentDiff | Self::RunTotal
        )
    }
}

/// Which item of the base field a base-relative [`PivotShowAs`] measures
/// against.
///
/// `@baseItem` is an item index with two reserved values above any real one,
/// so it is a small addressing space rather than a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PivotBaseItem {
    /// Excel's *(previous)*: the item before this one, in the base field's own
    /// order.
    Previous,
    /// Excel's *(next)*.
    Next,
    /// A specific item of the base field, by index.
    Item(u32),
}

impl PivotBaseItem {
    /// `@baseItem`'s value when nothing is chosen.
    ///
    /// The schema's default (`sml.xsd:1279`), and what a
    /// `<dataField showDataAs="percentOfTotal">` carries — see the fixture at
    /// `crates/casual-calc-import/src/tests.rs:1679`.
    pub const UNSET: u32 = 1_048_832;
    /// `@baseItem`'s reserved value for *(previous)*.
    ///
    /// **Not verified against a file Excel wrote.** [`Self::UNSET`] is pinned
    /// by the schema and by a fixture; these two reserved encodings are pinned
    /// by neither. They are unreachable in the first release, because the four
    /// modes that need a base item are refused (`docs/85` §7), and they must be
    /// checked against a real file before any of those modes lands.
    pub const PREVIOUS: u32 = 1_048_828;
    /// `@baseItem`'s reserved value for *(next)*. See [`Self::PREVIOUS`] for
    /// how far this is checked.
    pub const NEXT: u32 = 1_048_829;

    /// The number `@baseItem` carries for this choice.
    #[must_use]
    pub fn token(self) -> u32 {
        match self {
            Self::Previous => Self::PREVIOUS,
            Self::Next => Self::NEXT,
            Self::Item(index) => index,
        }
    }
}

/// The unit a [`PivotGroup`] buckets by.
///
/// OOXML's `ST_GroupBy` (`schemas/ooxml/sml.xsd:805`). All eight are here for
/// the reason [`PivotShowAs`] carries all nine; the first release honours
/// `Years`, `Quarters` and `Months`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PivotGroupBy {
    /// Numeric buckets of a fixed width — Excel's *starting at / ending at /
    /// by*. The schema's default.
    #[default]
    Range,
    /// Seconds within the minute.
    Seconds,
    /// Minutes within the hour.
    Minutes,
    /// Hours within the day.
    Hours,
    /// Days within the year.
    Days,
    /// Months, pooled across years.
    Months,
    /// Quarters, pooled across years.
    Quarters,
    /// Years.
    Years,
}

impl PivotGroupBy {
    /// The OOXML `@groupBy` token, which is also the wire name.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Range => "range",
            Self::Seconds => "seconds",
            Self::Minutes => "minutes",
            Self::Hours => "hours",
            Self::Days => "days",
            Self::Months => "months",
            Self::Quarters => "quarters",
            Self::Years => "years",
        }
    }

    /// Parse an OOXML `@groupBy` token. Unknown tokens fall back to
    /// [`Self::Range`], which is the schema's own default.
    #[must_use]
    pub fn from_token(token: &str) -> Self {
        match token {
            "seconds" => Self::Seconds,
            "minutes" => Self::Minutes,
            "hours" => Self::Hours,
            "days" => Self::Days,
            "months" => Self::Months,
            "quarters" => Self::Quarters,
            "years" => Self::Years,
            _ => Self::Range,
        }
    }

    /// The name Excel gives the cache field a group level creates, as it
    /// appears in the field list.
    ///
    /// `Range` has none: Excel groups a numeric field in place and the field
    /// keeps its own name.
    #[must_use]
    pub fn caption(self) -> Option<&'static str> {
        match self {
            Self::Range => None,
            Self::Seconds => Some("Seconds"),
            Self::Minutes => Some("Minutes"),
            Self::Hours => Some("Hours"),
            Self::Days => Some("Days"),
            Self::Months => Some("Months"),
            Self::Quarters => Some("Quarters"),
            Self::Years => Some("Years"),
        }
    }
}

/// Bucketing applied to a field's values before they are grouped by.
///
/// `<cacheField><fieldGroup><rangePr>`. Grouping changes an item's *key*, not
/// its label: a month's key is the ordinal `1..12`, so ascending order is
/// January-first rather than alphabetical. See `docs/85` §5.2.
///
/// As with [`PivotShowAs`], no field on [`PivotAxisField`] or
/// [`PivotFilterField`] carries this yet; that arrives with `docs/85` §9 slice
/// D.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PivotGroup {
    /// `@groupBy`.
    pub by: PivotGroupBy,
    /// `@groupInterval`. Unset is the schema's `1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<f64>,
    /// `@startNum` / `@startDate`, as a serial. Unset is `@autoStart`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<f64>,
    /// `@endNum` / `@endDate`, as a serial. Unset is `@autoEnd`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<f64>,
}

/// A field computed from the aggregated values rather than read from the
/// source.
///
/// `<cacheField @formula databaseField="0">`. Excel binds each field name in
/// the formula to the **sum** of that field over the group and applies the
/// formula once, so `Units*Price` reports `SUM(Units) × SUM(Price)` and the
/// column does not add up. That is what this file's `@formula` means to Excel
/// when it is reopened, which is why it is the binding here — any other one
/// would give a single file two answers. See `docs/85` §3.3 and §12 Q1.
///
/// No field on [`PivotTable`] carries these yet; that arrives with `docs/85`
/// §9 slice E.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PivotCalculatedField {
    /// The field's name, which is what the field list and the report caption
    /// show.
    pub name: String,
    /// The formula text, in Excel's pivot dialect: field names and operators,
    /// no cell references, no ranges, no defined names.
    pub formula: String,
    /// A number-format code applied to the results, e.g. `0.00%`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_format: Option<String>,
}

/// A field placed on the row or column axis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PivotAxisField {
    /// Which source column it is, as a zero-based offset into
    /// [`PivotTable::source`] — not an absolute column, so moving the pivot or
    /// its source does not have to renumber anything.
    pub source_column: u32,
    /// The order its values appear in.
    #[serde(default, skip_serializing_if = "is_default_sort")]
    pub sort: PivotSort,
    /// Whether a subtotal row (or column) closes each of this field's groups.
    ///
    /// Only meaningful on an outer field: the innermost field's "subtotal"
    /// would repeat the single line above it, so it is never emitted.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub subtotal: bool,
}

/// A field used as a page filter — the dropdowns above a pivot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PivotFilterField {
    /// Zero-based offset into [`PivotTable::source`].
    pub source_column: u32,
    /// The values kept, as the text the pivot displays for them.
    ///
    /// Empty means every value — the `(All)` state — rather than none. A filter
    /// that excludes everything is expressed by selecting a value that does not
    /// occur, not by an empty list, because "I have not chosen yet" and "I chose
    /// nothing" arrive at the same empty vector and only one of them should
    /// blank the report.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected: Vec<String>,
}

/// A measure: what is aggregated at each intersection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PivotValueField {
    /// Zero-based offset into [`PivotTable::source`].
    pub source_column: u32,
    /// How it is summarized.
    pub aggregate: PivotAggregate,
    /// The caption shown for it. Empty means derive one, as Excel does.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// A number-format code applied to the results, e.g. `#,##0.00`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_format: Option<String>,
}

/// A pivot table's definition and the extent it last wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PivotTable {
    /// Unique within the workbook.
    pub id: u32,
    /// The name shown in the field list and used by `GETPIVOTDATA`.
    pub name: String,
    /// The sheet the source records live on. A pivot is very often on a
    /// different sheet from its data, so this is not implied by the sheet the
    /// pivot itself sits on.
    pub source_sheet: SheetId,
    /// The source rectangle **including its header row**, which names the
    /// fields.
    pub source: CellRange,
    /// Top-left cell of the output block, on the sheet holding this pivot.
    pub anchor: CellRef,
    /// Fields down the side, outermost first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<PivotAxisField>,
    /// Fields across the top, outermost first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cols: Vec<PivotAxisField>,
    /// Page filters, shown stacked above the report.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<PivotFilterField>,
    /// The measures, left to right.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<PivotValueField>,
    /// Whether a grand-total row closes the report.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub row_grand_totals: bool,
    /// Whether a grand-total column closes the report.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub col_grand_totals: bool,
    /// The table style name used to band the output, resolved the same way a
    /// [`crate::Table`]'s is.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub style: String,
    /// The rectangle the last refresh wrote.
    ///
    /// Kept so the next refresh can clear exactly what the previous one filled.
    /// Recomputing the extent from the definition would work only while the
    /// source has not changed — which is the one case a refresh is not for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<CellRange>,
    /// The package path of the `pivotTable` part this was read from, when it
    /// came from a file.
    ///
    /// While this is set the part is written back byte for byte and nothing
    /// here reaches the writer, exactly as a chart's does. It is cleared by
    /// [`PivotTable::detach`] the moment the definition is edited, because a
    /// retained part that no longer describes the pivot on screen is worse than
    /// no part at all: the file would disagree with itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
}

fn yes() -> bool {
    true
}

fn is_yes(value: &bool) -> bool {
    *value
}

fn is_default_sort(sort: &PivotSort) -> bool {
    *sort == PivotSort::Ascending
}

impl PivotTable {
    /// A pivot over `source`, anchored at `anchor`, with nothing on any axis.
    #[must_use]
    pub fn new(
        id: u32,
        name: String,
        source_sheet: SheetId,
        source: CellRange,
        anchor: CellRef,
    ) -> Self {
        Self {
            id,
            name,
            source_sheet,
            source,
            anchor,
            rows: Vec::new(),
            cols: Vec::new(),
            filters: Vec::new(),
            values: Vec::new(),
            row_grand_totals: true,
            col_grand_totals: true,
            style: String::new(),
            output: None,
            part: None,
        }
    }

    /// Note that the definition has been edited, so the retained part no longer
    /// describes it. Returns the part path that has to be dropped, if any.
    pub fn detach(&mut self) -> Option<String> {
        self.part.take()
    }

    /// Whether the field at `source_column` is used anywhere.
    #[must_use]
    pub fn uses(&self, source_column: u32) -> bool {
        self.rows.iter().any(|f| f.source_column == source_column)
            || self.cols.iter().any(|f| f.source_column == source_column)
            || self
                .filters
                .iter()
                .any(|f| f.source_column == source_column)
            || self.values.iter().any(|f| f.source_column == source_column)
    }
}
