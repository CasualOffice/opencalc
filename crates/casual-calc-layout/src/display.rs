//! The backend-neutral display list — the single contract between layout and any
//! renderer (ADR-008). Serializable so it can be golden-tested.

use serde::{Deserialize, Serialize};

/// A rectangle in twips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rect {
    /// Left edge.
    pub x: i64,
    /// Top edge.
    pub y: i64,
    /// Width.
    pub w: i64,
    /// Height.
    pub h: i64,
}

/// Horizontal text alignment within a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Align {
    /// Left-aligned (default for text).
    Left,
    /// Right-aligned (default for numbers).
    Right,
}

/// One border edge resolved for painting: a pixel `width` (derived from the
/// OOXML line-style token, e.g. `thin`→1, `medium`→2, `thick`/`double`→3) plus
/// an optional `RRGGBB` color. Carried resolved so the renderer stays dumb and
/// the display list is deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BorderLine {
    /// The line width in pixels (≥ 1).
    pub width: u32,
    /// The line color as `RRGGBB` hex, if specified (defaults to a dark line).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// One paint instruction. Text is carried as a string plus its cell rectangle;
/// glyph shaping happens in the render backend (Phase 1D).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PaintItem {
    /// A cell background rectangle, optionally carrying a solid fill color.
    CellBackground {
        /// The cell rectangle.
        rect: Rect,
        /// The solid fill color as `RRGGBB` hex, if the cell has a fill.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fill: Option<String>,
    },
    /// A gridline segment (a thin rectangle is used by the renderer).
    GridLine {
        /// From/to as a zero-height or zero-width rectangle.
        rect: Rect,
    },
    /// A merged range: one cell occupying `rect`.
    ///
    /// A backend draws it as a single cell — `fill` if the anchor has one and
    /// the ground otherwise, across the whole rectangle, then one outline
    /// around the range. Filling the whole rectangle is what **erases the
    /// gridlines between the cells it covers**, since those are no longer cell
    /// boundaries; outlining it afterwards is what keeps the range from reading
    /// as a hole in the grid. The order is not incidental — outlining first and
    /// filling second paints the outline away, which is how two adjacent merged
    /// headers first came out looking like one.
    ///
    /// This carries the fill rather than leaving it to a following
    /// [`CellBackground`](Self::CellBackground) precisely so the two cannot be
    /// ordered wrongly. The anchor's text and border still follow as their own
    /// items, on top.
    MergedRegion {
        /// The union rectangle of the merged range.
        rect: Rect,
        /// The anchor's solid fill as `RRGGBB`, if it has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fill: Option<String>,
    },
    /// A conditional-formatting **data bar**: a horizontal bar filling
    /// `fraction` of the cell at `rect`, painted behind the cell's own text.
    ///
    /// A backend insets the bar a little from the cell's edges, takes
    /// `fraction` of the width that is left, starting at the inset left edge,
    /// and fills it in `color` — **partly transparent, because the number is
    /// still shown through it**. A data bar annotates a value rather than
    /// replacing it; an opaque bar across half a cell hides half the digits.
    /// A `fraction` of zero paints nothing.
    ///
    /// The **cell's** rectangle travels here rather than the bar's own, with
    /// the fraction beside it, for two reasons. The fraction *is* the datum —
    /// where the value sits between its rule's range minimum and maximum — and
    /// a display list that carried only a pre-multiplied width would make a
    /// wrong fraction indistinguishable from an inset measured differently, so
    /// no golden test could tell which had broken. And the inset is the
    /// backend's business: it is a device-pixel quantity, and layout works in
    /// twips at no particular resolution.
    ///
    /// This variant exists because the resolved bar
    /// ([`CellEffect::data_bar`](crate::conditional::CellEffect::data_bar))
    /// reached the browser canvas and nothing else — the display list had no
    /// primitive for a partial-width rectangle inside a cell, so every headless
    /// PNG (thumbnail, preview, server-side export) showed the range as plain
    /// numbers (`RND-07`).
    DataBar {
        /// The **cell** rectangle the bar is drawn inside — not the bar's own,
        /// which the backend derives from this and `fraction`.
        rect: Rect,
        /// How full the bar is, from zero (empty) to one (the full cell width):
        /// where the value sits between its rule's range minimum and maximum.
        fraction: f64,
        /// The bar colour as `RRGGBB` hex. Empty, or anything that is not valid
        /// hex, means the backend's own default bar colour.
        color: String,
    },
    /// Cell text to be shaped and painted, clipped to `rect`.
    Text {
        /// The cell rectangle the text is clipped to.
        rect: Rect,
        /// The display string (number-format applied by layout).
        content: String,
        /// Horizontal alignment.
        align: Align,
        /// The font color as `RRGGBB` hex, if specified (defaults to black).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        /// Whether the text is bold.
        #[serde(default, skip_serializing_if = "core::ops::Not::not")]
        bold: bool,
        /// Whether the text is italic.
        #[serde(default, skip_serializing_if = "core::ops::Not::not")]
        italic: bool,
        /// Requested font family (the cell's own, else the workbook default).
        /// `None` means the renderer's default family. A font resolver maps this
        /// to a concrete bundled face at paint time.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_name: Option<String>,
        /// Font size in points (the cell's own, else the workbook default).
        /// `None` means the renderer's default size.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_pt: Option<f32>,
    },
    /// A picture anchored on the sheet, drawn into `rect` on top of the cells.
    ///
    /// **The bytes are not here, and deliberately.** What travels is the
    /// package path of the media part —
    /// [`ImageView::part`](casual_calc_model::ImageView::part) — because a
    /// display list is rebuilt for every viewport and every frame, and a
    /// display list that owned its pictures would copy every megabyte of them
    /// each time. The model makes the same choice for the same reason: a
    /// picture is stored once, under its part path, and everything else refers
    /// to it. A backend resolves the path against whatever holds the media.
    ///
    /// That is the one place this list is not self-contained, so it is worth
    /// being exact about what the consequence is: a backend handed no media
    /// cannot draw the picture, and **must say so** rather than skip it. The
    /// CPU backend returns the ones it could not draw, named and counted.
    ///
    /// `rect` is the frame in sheet twips, resolved from the anchor's cells
    /// **and** its EMU offsets — a picture's edge sits wherever it was dragged,
    /// which is almost never on a gridline.
    Image {
        /// The frame rectangle, in sheet twips.
        rect: Rect,
        /// The package path of the media part, e.g. `xl/media/image1.png`.
        part: String,
    },
    /// The border edges of a cell, painted on top of fills and text.
    CellBorder {
        /// The cell rectangle whose edges are stroked.
        rect: Rect,
        /// Left edge, if present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        left: Option<BorderLine>,
        /// Right edge, if present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        right: Option<BorderLine>,
        /// Top edge, if present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        top: Option<BorderLine>,
        /// Bottom edge, if present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bottom: Option<BorderLine>,
    },
}

/// A window of paint instructions, in deterministic painter's order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayList {
    /// The paint items, back-to-front.
    pub items: Vec<PaintItem>,
}

impl DisplayList {
    /// An empty display list.
    pub fn new() -> Self {
        Self::default()
    }
}
