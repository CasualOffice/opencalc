//! Interned cell styles. Cells reference a [`StyleId`]; a million cells sharing a
//! format cost one style. See `docs/22-NORMALIZED-SCHEMA.md`.
//!
//! This is a compact starting shape: a style currently carries its number-format
//! code. Font, fill, and border are added in a later increment; they extend
//! `Style` without changing the table's API.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::ids::{Id, StyleId};

/// Namespace for style ids (high 64 bits of the `Id`).
const STYLE_NAMESPACE: u64 = 0x5354_5900_0000_0000; // "STY\0"

/// One edge of a cell border: an OOXML line-style token (e.g. `thin`, `medium`,
/// `dashed`, `double`) plus an optional `RRGGBB` color. The token is stored raw
/// so any style — even ones the renderer doesn't specialize — round-trips.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BorderEdge {
    /// The OOXML line-style token (never empty; `none` edges are `None` instead).
    pub style: String,
    /// The line color as `RRGGBB` hex, if specified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// A cell's four borders. An absent edge means "no line".
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Borders {
    /// Left edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<BorderEdge>,
    /// Right edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<BorderEdge>,
    /// Top edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<BorderEdge>,
    /// Bottom edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<BorderEdge>,
}

impl Borders {
    /// Whether no edge carries a line.
    pub fn is_empty(&self) -> bool {
        self.left.is_none() && self.right.is_none() && self.top.is_none() && self.bottom.is_none()
    }
}

/// Horizontal text alignment within a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HAlign {
    /// Left-aligned.
    Left,
    /// Centered.
    Center,
    /// Right-aligned.
    Right,
}

impl HAlign {
    /// The OOXML `horizontal` attribute token.
    pub fn ooxml(self) -> &'static str {
        match self {
            HAlign::Left => "left",
            HAlign::Center => "center",
            HAlign::Right => "right",
        }
    }

    /// Parse an OOXML `horizontal` token.
    pub fn from_ooxml(token: &str) -> Option<Self> {
        match token {
            "left" => Some(HAlign::Left),
            "center" | "centerContinuous" => Some(HAlign::Center),
            "right" => Some(HAlign::Right),
            _ => None,
        }
    }
}

/// Vertical text alignment within a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VAlign {
    /// Top-aligned.
    Top,
    /// Vertically centered.
    Middle,
    /// Bottom-aligned.
    Bottom,
}

impl VAlign {
    /// The OOXML `vertical` attribute token.
    pub fn ooxml(self) -> &'static str {
        match self {
            VAlign::Top => "top",
            VAlign::Middle => "center",
            VAlign::Bottom => "bottom",
        }
    }

    /// Parse an OOXML `vertical` token.
    pub fn from_ooxml(token: &str) -> Option<Self> {
        match token {
            "top" => Some(VAlign::Top),
            "center" | "distributed" => Some(VAlign::Middle),
            "bottom" | "justify" => Some(VAlign::Bottom),
            _ => None,
        }
    }
}

/// A cell's formatting. Extensible; equal styles are deduplicated in the table.
/// Colors are `"RRGGBB"` hex strings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Style {
    /// The number-format code (e.g. `0.00`, `mm-dd-yy`), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_format: Option<String>,
    /// Bold text.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    /// Italic text.
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    /// Underlined text.
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    /// Strikethrough text.
    #[serde(default, skip_serializing_if = "is_false")]
    pub strike: bool,
    /// Font family name (e.g. `Calibri`, `Arial`), if specified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_name: Option<String>,
    /// Font size in **half-points**, so it stays `Hash + Eq` (a float cannot).
    /// 11pt is stored as `22`; divide by 2 for points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size_hp: Option<u32>,
    /// Font color as `RRGGBB` hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_color: Option<String>,
    /// Solid fill (background) color as `RRGGBB` hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<String>,
    /// Horizontal alignment (defaults per value type when unset).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<HAlign>,
    /// Vertical alignment (defaults to bottom when unset, per OOXML).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valign: Option<VAlign>,
    /// Wrap text within the cell (instead of overflowing/clipping).
    #[serde(default, skip_serializing_if = "is_false")]
    pub wrap: bool,
    /// Text rotation, in OOXML's `textRotation` encoding: `0`–`90` is that many
    /// degrees counter-clockwise, `91`–`180` is `value - 90` degrees *clockwise*,
    /// and `255` means the letters are stacked vertically without rotating.
    ///
    /// Stored in the spec's own encoding rather than as a signed angle so the
    /// round-trip is lossless and there is no conversion to get backwards.
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub rotation: u16,
    /// Clip overflowing text at the cell edge instead of letting it spill into
    /// empty neighbours. Only meaningful when [`Style::wrap`] is off — the three
    /// states a user picks between are overflow (the default), wrap, and clip.
    ///
    /// SpreadsheetML has no attribute for this: Excel always spills into empty
    /// neighbours. It is therefore a view choice this engine keeps in its own
    /// model and **does not write to `.xlsx`**; see the fidelity ledger.
    #[serde(default, skip_serializing_if = "is_false")]
    pub clip: bool,
    /// Indent level (in indent units, ~3 space-widths each) from the alignment's
    /// leading edge. `0` (the default) writes no `indent` attribute.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub indent: u8,
    /// Cell borders, if any edge is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<Borders>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero_u16(value: &u16) -> bool {
    *value == 0
}

fn is_zero_u8(value: &u8) -> bool {
    *value == 0
}

impl Style {
    /// Whether this style carries no formatting.
    pub fn is_default(&self) -> bool {
        self.number_format.is_none()
            && !self.bold
            && !self.italic
            && !self.underline
            && !self.strike
            && self.font_name.is_none()
            && self.font_size_hp.is_none()
            && self.font_color.is_none()
            && self.fill_color.is_none()
            && self.align.is_none()
            && self.valign.is_none()
            && !self.wrap
            && !self.clip
            && self.rotation == 0
            && self.indent == 0
            && self.border.is_none()
    }
}

/// A deduplicated table of styles, serialized as an ordered list. A style's id
/// encodes its insertion index, so identical interning yields identical ids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Vec<Style>", into = "Vec<Style>")]
pub struct StyleTable {
    entries: Vec<Style>,
    index: HashMap<Style, u32>,
}

impl StyleTable {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the table holds no styles.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The number of interned styles.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Intern `style`, returning its (possibly pre-existing) id.
    pub fn intern(&mut self, style: Style) -> StyleId {
        if let Some(&index) = self.index.get(&style) {
            return Self::id_for(index);
        }
        let index = self.entries.len() as u32;
        self.entries.push(style.clone());
        self.index.insert(style, index);
        Self::id_for(index)
    }

    /// Resolve an id to its style, or `None` if it is not from this table.
    pub fn get(&self, id: StyleId) -> Option<&Style> {
        let index = self.index_of(id)? as usize;
        self.entries.get(index)
    }

    /// The zero-based index encoded by `id`, if it is from this table.
    pub fn index_of(&self, id: StyleId) -> Option<u32> {
        let raw = id.0.get();
        if (raw >> 64) as u64 != STYLE_NAMESPACE {
            return None;
        }
        u32::try_from((raw as u64).checked_sub(1)?).ok()
    }

    /// Iterate the interned styles in index order.
    pub fn iter(&self) -> impl Iterator<Item = &Style> {
        self.entries.iter()
    }

    /// Whether `id` resolves within this table.
    pub fn contains(&self, id: StyleId) -> bool {
        self.get(id).is_some()
    }

    fn id_for(index: u32) -> StyleId {
        StyleId(Id::from_parts(STYLE_NAMESPACE, index as u64 + 1))
    }
}

impl From<Vec<Style>> for StyleTable {
    fn from(entries: Vec<Style>) -> Self {
        let mut index = HashMap::with_capacity(entries.len());
        for (i, style) in entries.iter().enumerate() {
            index.entry(style.clone()).or_insert(i as u32);
        }
        Self { entries, index }
    }
}

impl From<StyleTable> for Vec<Style> {
    fn from(table: StyleTable) -> Self {
        table.entries
    }
}
