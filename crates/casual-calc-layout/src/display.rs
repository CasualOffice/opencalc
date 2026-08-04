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

/// One paint instruction. Text is carried as a string plus its cell rectangle;
/// glyph shaping happens in the render backend (Phase 1D).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PaintItem {
    /// A cell background rectangle.
    CellBackground {
        /// The cell rectangle.
        rect: Rect,
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
