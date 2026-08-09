//! Charts, as much of one as is needed to draw it.
//!
//! **This is a display projection, not the source of truth.** A chart part is
//! retained byte for byte and written back from those bytes; what is modelled
//! here is only what a renderer needs to put something on screen. Nothing
//! writes a chart from this type, so a field missing from it costs a picture,
//! never a file.
//!
//! That division is deliberate. A chart part carries hundreds of formatting
//! elements, and modelling it half-way then writing from the model would lose
//! every one this does not know about — the exact silent edit retention exists
//! to prevent. When charts become editable the two halves have to merge, and
//! that is a design decision to take then, with the writer in hand.

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

/// A chart anchored on a sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartView {
    /// The cells the chart's frame covers, from the drawing's anchor.
    pub anchor: CellRange,
    /// What it draws.
    pub kind: ChartKind,
    /// The title, empty when the chart has none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Its series, in plot order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub series: Vec<ChartSeries>,
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
    /// The cells the picture's frame covers.
    pub anchor: CellRange,
    /// The package path of its media part, e.g. `xl/media/image1.png`.
    pub part: String,
}
