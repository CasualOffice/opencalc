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
