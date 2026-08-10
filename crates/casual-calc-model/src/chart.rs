//! Charts: what one plots, and where it sits.
//!
//! A chart here is in one of two regimes, and [`ChartView::part`] says which.
//!
//! **Read from a file** (`part` is set). The chart part is retained byte for
//! byte and written back from those bytes; what is modelled is only what a
//! renderer needs to put something on screen. Nothing here reaches the writer,
//! so a field this does not know about costs a picture, never a file. A chart
//! part carries hundreds of formatting elements, and modelling it half-way then
//! writing from the model would lose every one — the exact silent edit that
//! retention exists to prevent.
//!
//! **Made here** (`part` is `None`). There are no bytes to preserve, so this
//! type *is* the chart and the writer builds the part from it. Editing an
//! imported chart moves it into this regime, which is why the two halves never
//! have to merge: a chart is described by its retained bytes or by this, never
//! by both. That is [`ChartView::detach`], and it is the same rule a pivot
//! table follows — a retained part that no longer describes what is on screen
//! is worse than no part, because a reader believes the part.
//!
//! What that costs is stated plainly rather than hidden: editing an imported
//! chart drops the formatting this type does not model. The alternative is a
//! file whose chart part and whose chart disagree.

use serde::{Deserialize, Serialize};

use crate::store::CellRange;

/// What kind of picture a chart draws.
///
/// `Bar` and `Column` are one element in OOXML (`<c:barChart>`) distinguished
/// by `<c:barDir>`; they are separate here because they are separate pictures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChartKind {
    /// Horizontal bars.
    Bar,
    /// Vertical columns.
    Column,
    /// A line per series.
    Line,
    /// A filled line per series.
    Area,
    /// One ring of slices.
    Pie,
    /// A pie with a hole.
    Doughnut,
    /// Points at (x, y).
    Scatter,
    /// A chart type this does not draw. Retained and written back like any
    /// other; simply not rendered, which is visibly incomplete rather than
    /// silently wrong.
    Unsupported,
}

/// One series within a chart.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartSeries {
    /// The series name as displayed. Either literal text or the resolved
    /// contents of the cell `<c:tx>` points at; the reference itself is not
    /// kept, because a renderer needs the label, not where it came from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The formula naming the category (x) values, e.g. `Sheet1!$A$2:$A$9`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<String>,
    /// The formula naming the plotted (y) values.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub values: String,
}

/// A frame's offset into the cell it is anchored to, in EMUs.
///
/// English Metric Units: 914,400 to the inch, 9,525 to a pixel at 96 dpi.
/// OOXML's own unit, kept rather than converted because a chart's edge lands
/// wherever it was dragged, not on a cell boundary. Without this a frame can
/// only start and end on gridlines: dragging an edge does nothing until it
/// crosses one, then jumps a whole column, and the chart never sits where it
/// was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Emu {
    /// Horizontal offset.
    pub x: i64,
    /// Vertical offset.
    pub y: i64,
}

impl Emu {
    /// EMUs in one pixel at 96 dpi.
    pub const PER_PIXEL: i64 = 9_525;

    /// Whether both axes are zero — the common case, so it is not serialized.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.x == 0 && self.y == 0
    }
}

/// Whether a [`ChartView::id`] has never been allocated.
///
/// Skipping it on write is what keeps a snapshot taken before this field
/// existed byte-identical when it is read and written again (ADR-010).
fn is_unassigned(id: &u32) -> bool {
    *id == 0
}

/// A chart anchored on a sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartView {
    /// Identity, stable for the chart's lifetime and unique within its sheet.
    ///
    /// Every other collection on a sheet is identified by where it points:
    /// a comment by its cell, a hyperlink and a validation by their range, a
    /// conditional format by its OOXML priority. A chart is the exception —
    /// two of them may sit on the same cells, so the anchor does not name one.
    /// Without this it can only be referred to by its index in the sheet's
    /// list, which stops being the same chart the moment anything is inserted
    /// before it, and which two concurrent editors would both claim
    /// ([ADR-011](../../../docs/56-COLLABORATION-CONCURRENCY-DESIGN.md)).
    ///
    /// Zero means unassigned — a chart from a snapshot written before this
    /// field existed. Assigned on import in document order, so the same file
    /// always yields the same ids.
    #[serde(default, skip_serializing_if = "is_unassigned")]
    pub id: u32,
    /// The cells the chart's frame covers, from the drawing's anchor.
    ///
    /// Inclusive, like every other range in the model. OOXML's `<xdr:to>` is
    /// exclusive, so the last covered cell is the one before it; the remainder
    /// travels in [`Self::to_offset`].
    pub anchor: CellRange,
    /// How far into [`Self::anchor`]'s first cell the frame's top-left sits.
    #[serde(default, skip_serializing_if = "Emu::is_zero")]
    pub from_offset: Emu,
    /// How far past the last cell's far edge the frame's bottom-right sits.
    ///
    /// Zero means the frame ends exactly on the gridline. This is `<xdr:to>`'s
    /// own offset unchanged: the `to` cell is one past the last covered one, so
    /// an offset measured into it is the same number as one measured past the
    /// cell before.
    #[serde(default, skip_serializing_if = "Emu::is_zero")]
    pub to_offset: Emu,
    /// What it draws.
    pub kind: ChartKind,
    /// The title, empty when the chart has none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Its series, in plot order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub series: Vec<ChartSeries>,
    /// Where the legend sits — `r`, `l`, `t`, `b`, or `tr`. `None` is no
    /// legend, which is what a single-series chart usually wants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legend: Option<String>,
    /// The category (horizontal) axis title, empty when it has none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub x_title: String,
    /// The value (vertical) axis title.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub y_title: String,
    /// The package path of the chart part this was read from.
    ///
    /// Set means the part is authoritative and is written back byte for byte;
    /// `None` means this type is the chart and the writer builds the part from
    /// it. See the module docs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
}

impl ChartView {
    /// A chart with no series yet, anchored over `anchor`.
    #[must_use]
    pub fn new(anchor: CellRange, kind: ChartKind) -> Self {
        Self {
            // Unassigned: the caller adding this to a sheet allocates, because
            // uniqueness is a property of the sheet and not of the chart.
            id: 0,
            anchor,
            from_offset: Emu::default(),
            to_offset: Emu::default(),
            kind,
            title: String::new(),
            series: Vec::new(),
            legend: None,
            x_title: String::new(),
            y_title: String::new(),
            part: None,
        }
    }

    /// Stop writing this chart back from its own bytes, because it has been
    /// edited and they no longer describe it. Returns the part path to drop.
    pub fn detach(&mut self) -> Option<String> {
        self.part.take()
    }
}

impl ChartKind {
    /// The kind an OOXML chart-group element name denotes.
    ///
    /// `bar_dir` is `<c:barDir val>`, which is the only thing separating a bar
    /// chart from a column chart — and its schema default is `col`, so a
    /// missing element means columns rather than bars.
    #[must_use]
    pub fn from_element(name: &str, bar_dir: Option<&str>) -> Self {
        match name {
            "barChart" | "bar3DChart" => {
                if bar_dir == Some("bar") {
                    Self::Bar
                } else {
                    Self::Column
                }
            }
            "lineChart" | "line3DChart" => Self::Line,
            "areaChart" | "area3DChart" => Self::Area,
            "pieChart" | "pie3DChart" | "ofPieChart" => Self::Pie,
            "doughnutChart" => Self::Doughnut,
            "scatterChart" | "bubbleChart" => Self::Scatter,
            _ => Self::Unsupported,
        }
    }
}

/// A picture anchored on a sheet.
///
/// The bytes are **not** here: they stay in [`crate::Workbook::retained_parts`]
/// under `part`, so an image is stored once and written back from the same
/// bytes it arrived in. Copying a multi-megabyte PNG into the sheet would
/// double a workbook's memory to gain nothing — the renderer needs to know
/// *which* part to draw, not to own it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageView {
    /// The cells the picture's frame covers, inclusive.
    pub anchor: CellRange,
    /// How far into the first cell the frame's top-left sits.
    #[serde(default, skip_serializing_if = "Emu::is_zero")]
    pub from_offset: Emu,
    /// How far past the last cell's far edge its bottom-right sits.
    #[serde(default, skip_serializing_if = "Emu::is_zero")]
    pub to_offset: Emu,
    /// The package path of its media part, e.g. `xl/media/image1.png`.
    pub part: String,
}
