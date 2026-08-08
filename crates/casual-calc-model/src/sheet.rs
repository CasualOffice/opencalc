//! Worksheets. Definition tables (styles, numbering, defined names, notes,
//! theme) live on the workbook; the sheet holds its grid, merges, and view.
//! See `docs/22-NORMALIZED-SCHEMA.md`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::ids::SheetId;
use crate::store::{CellRange, CellRef, CellStore};

/// Per-axis sizing (column widths or row heights), in twips: an optional default
/// plus per-line overrides. Empty means "use the engine default".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AxisSizing {
    /// Default line size (twips) for this axis, if the sheet sets one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<i64>,
    /// Explicit per-line sizes (twips), keyed by zero-based index.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sizes: BTreeMap<u32, i64>,
}

impl AxisSizing {
    /// Whether nothing is set (no default, no overrides).
    pub fn is_empty(&self) -> bool {
        self.default.is_none() && self.sizes.is_empty()
    }

    /// The size (twips) of `index`, falling back to `default` then `fallback`.
    pub fn size(&self, index: u32, fallback: i64) -> i64 {
        self.sizes
            .get(&index)
            .copied()
            .or(self.default)
            .unwrap_or(fallback)
    }
}

/// A sheet's view state: the frozen (pinned) row/column bands and zoom level.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SheetView {
    /// Number of rows frozen at the top.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub frozen_rows: u32,
    /// Number of columns frozen at the left.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub frozen_cols: u32,
    /// Zoom magnification as a whole percentage (`100` = normal). `0` means the
    /// view uses the application default, so no explicit `zoomScale` is written.
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub zoom: u16,
    /// Whether the grid lines are hidden. OOXML shows grid lines by default
    /// (`showGridLines="1"` implied), so this is `false` for a normal sheet and
    /// only `true` when the sheet carries `showGridLines="0"`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hide_gridlines: bool,
    /// Whether the row and column headers are hidden — OOXML's
    /// `showRowColHeaders="0"`. Shown by default, so this is `false` for a
    /// normal sheet.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hide_headers: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

fn is_zero_u16(value: &u16) -> bool {
    *value == 0
}

impl SheetView {
    /// Whether the view is at its default (nothing frozen, default zoom).
    pub fn is_default(&self) -> bool {
        self.frozen_rows == 0 && self.frozen_cols == 0 && self.zoom == 0 && !self.hide_gridlines
    }
}

/// Outline (row/column grouping) properties from `<sheetPr><outlinePr/>`: where
/// group summary rows/columns sit relative to their detail. Both flags default
/// to `true` (summary below a row group, right of a column group), matching the
/// OOXML defaults, so an untouched sheet writes no `<outlinePr>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutlinePr {
    /// Whether a group's summary row sits below its detail rows.
    pub summary_below: bool,
    /// Whether a group's summary column sits to the right of its detail columns.
    pub summary_right: bool,
}

impl Default for OutlinePr {
    fn default() -> Self {
        Self {
            summary_below: true,
            summary_right: true,
        }
    }
}

impl OutlinePr {
    /// Whether both flags are at their OOXML defaults (summary below/right).
    pub fn is_default(&self) -> bool {
        self.summary_below && self.summary_right
    }
}

/// One worksheet: an identity, a name, its sparse cell grid, merged ranges, and
/// view state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Sheet {
    /// Stable sheet identity.
    pub id: SheetId,
    /// Display name (tab label).
    pub name: String,
    /// The populated cells.
    #[serde(default, skip_serializing_if = "CellStore::is_empty")]
    pub cells: CellStore,
    /// Merged cell ranges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merges: Vec<CellRange>,
    /// View state (frozen panes).
    #[serde(default, skip_serializing_if = "SheetView::is_default")]
    pub view: SheetView,
    /// Column widths (twips).
    #[serde(default, skip_serializing_if = "AxisSizing::is_empty")]
    pub columns: AxisSizing,
    /// Row heights (twips).
    #[serde(default, skip_serializing_if = "AxisSizing::is_empty")]
    pub rows: AxisSizing,
    /// Hidden rows, by zero-based index.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub hidden_rows: BTreeSet<u32>,
    /// Hidden columns, by zero-based index.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub hidden_cols: BTreeSet<u32>,
    /// Outline (grouping) nesting level per row, by zero-based index. Sparse:
    /// only rows with a non-zero level appear.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub row_outline_levels: BTreeMap<u32, u8>,
    /// Outline (grouping) nesting level per column, by zero-based index. Sparse:
    /// only columns with a non-zero level appear.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub col_outline_levels: BTreeMap<u32, u8>,
    /// Rows whose outline group is collapsed, by zero-based index.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub collapsed_rows: BTreeSet<u32>,
    /// Columns whose outline group is collapsed, by zero-based index.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub collapsed_cols: BTreeSet<u32>,
    /// Outline summary-position properties (`<outlinePr>`).
    #[serde(default, skip_serializing_if = "OutlinePr::is_default")]
    pub outline: OutlinePr,
    /// Tab color as an `RRGGBB` hex string (no `#`), if the tab is colored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_color: Option<String>,
    /// Data-validation rules (currently in-cell dropdown lists).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validations: Vec<DataValidation>,
    /// Conditional-formatting rules (highlight-cells with a fill color).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditional_formats: Vec<ConditionalFormat>,
    /// Cell comments / notes, keyed by cell address.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<CellComment>,
    /// The autofilter over a header range, if one is turned on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_filter: Option<AutoFilter>,
    /// Rows the autofilter is currently hiding, by zero-based index.
    ///
    /// Deliberately *not* folded into [`Sheet::hidden_rows`]: a filter must be
    /// able to release exactly the rows it hid without disturbing rows the user
    /// hid by hand. Both sets hide a row — see [`Sheet::is_row_hidden`] — but
    /// only this one is cleared when the filter changes.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub filter_hidden: BTreeSet<u32>,
}

/// An autofilter: a header range plus a rule per filtered column.
///
/// Mirrors OOXML `<autoFilter>`. The range covers the header row *and* the body
/// rows beneath it, which is what Excel writes and what the filter buttons are
/// drawn from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutoFilter {
    /// Header row + body, inclusive.
    pub range: CellRange,
    /// Active rules, keyed by column *offset* from `range.start.col` — the same
    /// `colId` basis OOXML uses, so the mapping survives a round-trip
    /// unchanged. Columns absent from the map are unfiltered.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rules: BTreeMap<u32, FilterRule>,
}

/// How one column of an autofilter selects rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterRule {
    /// Keep rows whose displayed text is one of these. OOXML `<filters>`.
    ///
    /// The empty string stands for a blank cell, which OOXML encodes out of
    /// band as `<filters blank="1">`.
    Values(Vec<String>),
    /// Keep rows matching one or two comparisons. OOXML `<customFilters>`.
    Custom {
        /// The first (always present) comparison.
        first: CustomFilter,
        /// An optional second comparison.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        second: Option<CustomFilter>,
        /// Join the two with AND rather than OR. OOXML `customFilters/@and`.
        #[serde(default)]
        and: bool,
    },
}

/// One comparison inside a [`FilterRule::Custom`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomFilter {
    /// The comparison to apply.
    pub op: FilterOp,
    /// The operand, as text. For [`FilterOp::Equal`] and [`FilterOp::NotEqual`]
    /// it may contain the OOXML wildcards `*` and `?` — which is how Excel
    /// stores "contains" (`*foo*`), "begins with" (`foo*`) and "ends with"
    /// (`*foo`). There are no separate operators for those.
    pub value: String,
}

/// The comparison operators OOXML `<customFilter>` allows. Nothing outside this
/// set is representable in a `.xlsx`, so nothing outside it is modelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterOp {
    /// `equal` — supports wildcards.
    Equal,
    /// `notEqual` — supports wildcards.
    NotEqual,
    /// `greaterThan`.
    GreaterThan,
    /// `greaterThanOrEqual`.
    GreaterThanOrEqual,
    /// `lessThan`.
    LessThan,
    /// `lessThanOrEqual`.
    LessThanOrEqual,
}

impl FilterOp {
    /// The OOXML `operator` attribute value.
    pub fn as_ooxml(&self) -> &'static str {
        match self {
            FilterOp::Equal => "equal",
            FilterOp::NotEqual => "notEqual",
            FilterOp::GreaterThan => "greaterThan",
            FilterOp::GreaterThanOrEqual => "greaterThanOrEqual",
            FilterOp::LessThan => "lessThan",
            FilterOp::LessThanOrEqual => "lessThanOrEqual",
        }
    }

    /// Parse an OOXML `operator` attribute. Unknown values — and the absent
    /// attribute, which OOXML defines as `equal` — fall back to `Equal`.
    pub fn from_ooxml(s: &str) -> Self {
        match s {
            "notEqual" => FilterOp::NotEqual,
            "greaterThan" => FilterOp::GreaterThan,
            "greaterThanOrEqual" => FilterOp::GreaterThanOrEqual,
            "lessThan" => FilterOp::LessThan,
            "lessThanOrEqual" => FilterOp::LessThanOrEqual,
            _ => FilterOp::Equal,
        }
    }
}

/// Match `text` against an OOXML filter pattern, where `*` is any run of
/// characters and `?` is exactly one. Comparison is case-insensitive, as it is
/// in Excel.
///
/// Iterative with a backtrack point rather than recursive, so a pathological
/// pattern like `*a*a*a*…` against a long string cannot blow the stack or go
/// exponential — it stays O(text × pattern) in the worst case.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    // Where to resume if the current `*` guess turns out too short.
    let (mut star, mut retry) = (usize::MAX, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = pi;
            retry = ti;
            pi += 1;
        } else if star != usize::MAX {
            // Let the last `*` swallow one more character and try again.
            retry += 1;
            ti = retry;
            pi = star + 1;
        } else {
            return false;
        }
    }
    // Trailing `*`s can still match the empty remainder.
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

impl CustomFilter {
    /// Whether a cell passes this comparison.
    ///
    /// `text` is the cell as the user sees it and `num` its numeric value when
    /// it has one. Ordering comparisons use `num` when both sides are numeric
    /// and fall back to case-insensitive text ordering otherwise, so a filter
    /// on a text column still behaves sensibly.
    pub fn matches(&self, text: &str, num: Option<f64>) -> bool {
        match self.op {
            FilterOp::Equal => wildcard_match(&self.value, text),
            FilterOp::NotEqual => !wildcard_match(&self.value, text),
            _ => {
                let ord = match (num, self.value.trim().parse::<f64>()) {
                    (Some(a), Ok(b)) => a.partial_cmp(&b),
                    _ => Some(text.to_lowercase().cmp(&self.value.to_lowercase())),
                };
                let Some(ord) = ord else {
                    return false; // NaN compares false against everything
                };
                match self.op {
                    FilterOp::GreaterThan => ord.is_gt(),
                    FilterOp::GreaterThanOrEqual => ord.is_ge(),
                    FilterOp::LessThan => ord.is_lt(),
                    FilterOp::LessThanOrEqual => ord.is_le(),
                    _ => unreachable!("equality handled above"),
                }
            }
        }
    }
}

impl FilterRule {
    /// Whether a cell passes this rule, and so keeps its row visible.
    pub fn matches(&self, text: &str, num: Option<f64>) -> bool {
        match self {
            // Case-insensitive, matching the checklist the values came from.
            FilterRule::Values(vals) => vals.iter().any(|v| v.eq_ignore_ascii_case(text)),
            FilterRule::Custom { first, second, and } => {
                let a = first.matches(text, num);
                match second {
                    Some(b) => {
                        let b = b.matches(text, num);
                        if *and { a && b } else { a || b }
                    }
                    None => a,
                }
            }
        }
    }
}

impl AutoFilter {
    /// A filter over `range` with no column rules yet — buttons shown, nothing
    /// filtered out.
    pub fn new(range: CellRange) -> Self {
        Self {
            range,
            rules: BTreeMap::new(),
        }
    }

    /// The first row of data, i.e. the row below the header.
    pub fn body_start(&self) -> u32 {
        self.range.start.row.saturating_add(1)
    }

    /// Whether any column currently narrows the rows.
    pub fn is_active(&self) -> bool {
        !self.rules.is_empty()
    }
}

/// A note attached to a cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CellComment {
    /// The cell the note is anchored to.
    pub at: CellRef,
    /// The note text.
    pub text: String,
    /// The author, if recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

/// A conditional-formatting rule: cells in `range` whose value satisfies `rule`
/// are painted with `fill` (an `RRGGBB` hex). First matching rule wins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConditionalFormat {
    /// The range the rule applies to.
    pub range: CellRange,
    /// The predicate on a cell's value.
    pub rule: CfRule,
    /// Fill color (`RRGGBB`, no `#`) applied when the rule matches.
    pub fill: String,
}

/// A conditional-format predicate. Numeric comparisons act on a cell's numeric
/// value; `TextContains` acts on its display text (case-insensitive).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CfRule {
    /// Value strictly greater than the operand.
    GreaterThan(f64),
    /// Value strictly less than the operand.
    LessThan(f64),
    /// Value equal to the operand.
    EqualTo(f64),
    /// Value within `[low, high]` inclusive.
    Between(f64, f64),
    /// Display text contains the substring (case-insensitive).
    TextContains(String),
    /// A two- or three-stop colour scale across the range's numeric span. The
    /// colours are `RRGGBB`, ordered low → high, and the cell's own colour is
    /// interpolated from where its value falls between them.
    ///
    /// Unlike the predicates above this is not a per-cell test: it needs the
    /// range's minimum and maximum, so it is evaluated with range statistics
    /// rather than by [`CfRule::matches_number`]. The sibling
    /// [`ConditionalFormat::fill`] is unused for this kind.
    ColorScale(Vec<String>),
    /// A proportional bar drawn behind the value, in this `RRGGBB` colour, its
    /// length being the value's position in the range's span. Also
    /// range-relative, and likewise ignores `fill`.
    DataBar(String),
}

/// A data-validation rule over a range. Only the explicit-list dropdown kind is
/// modeled today (`<dataValidation type="list">` with an inline value list).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DataValidation {
    /// The range the rule applies to.
    pub range: CellRange,
    /// The allowed values shown in the dropdown.
    pub values: Vec<String>,
}

impl DataValidation {
    /// Whether this rule's range covers `(row, col)`.
    pub fn covers(&self, row: u32, col: u32) -> bool {
        row >= self.range.start.row
            && row <= self.range.end.row
            && col >= self.range.start.col
            && col <= self.range.end.col
    }
}

impl Sheet {
    /// A new empty sheet.
    pub fn new(id: SheetId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            cells: CellStore::new(),
            merges: Vec::new(),
            view: SheetView::default(),
            columns: AxisSizing::default(),
            rows: AxisSizing::default(),
            hidden_rows: BTreeSet::new(),
            hidden_cols: BTreeSet::new(),
            row_outline_levels: BTreeMap::new(),
            col_outline_levels: BTreeMap::new(),
            collapsed_rows: BTreeSet::new(),
            collapsed_cols: BTreeSet::new(),
            outline: OutlinePr::default(),
            tab_color: None,
            validations: Vec::new(),
            conditional_formats: Vec::new(),
            comments: Vec::new(),
            auto_filter: None,
            filter_hidden: BTreeSet::new(),
        }
    }

    /// Whether a row is hidden, for any reason — hidden by hand or filtered out.
    ///
    /// Every reader that asks "should this row be drawn / measured / exported as
    /// hidden" must go through here rather than reading [`Sheet::hidden_rows`]
    /// directly, or filtered rows leak back into view.
    pub fn is_row_hidden(&self, row: u32) -> bool {
        self.hidden_rows.contains(&row) || self.filter_hidden.contains(&row)
    }
}

impl ConditionalFormat {
    /// Whether the rule's range covers `(row, col)`.
    pub fn covers(&self, row: u32, col: u32) -> bool {
        row >= self.range.start.row
            && row <= self.range.end.row
            && col >= self.range.start.col
            && col <= self.range.end.col
    }
}

impl CfRule {
    /// Whether this rule matches a numeric value.
    pub fn matches_number(&self, n: f64) -> bool {
        match self {
            CfRule::GreaterThan(x) => n > *x,
            CfRule::LessThan(x) => n < *x,
            CfRule::EqualTo(x) => (n - *x).abs() < 1e-9,
            CfRule::Between(lo, hi) => n >= *lo && n <= *hi,
            // Range-relative kinds are not per-cell predicates: they need the
            // range's own statistics, so a caller evaluates them with
            // `is_range_relative` and its own min/max rather than here.
            CfRule::TextContains(_) | CfRule::ColorScale(_) | CfRule::DataBar(_) => false,
        }
    }

    /// Whether this rule is evaluated against the range's statistics (minimum
    /// and maximum) rather than by a per-cell predicate.
    #[must_use]
    pub fn is_range_relative(&self) -> bool {
        matches!(self, CfRule::ColorScale(_) | CfRule::DataBar(_))
    }
    /// Whether this rule matches display text (only `TextContains` does).
    pub fn matches_text(&self, text: &str) -> bool {
        match self {
            CfRule::TextContains(s) => text.to_lowercase().contains(&s.to_lowercase()),
            _ => false,
        }
    }
}
