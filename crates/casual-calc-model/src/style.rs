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

/// A cell's formatting. Extensible; equal styles are deduplicated in the table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Style {
    /// The number-format code (e.g. `0.00`, `mm-dd-yy`), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_format: Option<String>,
}

impl Style {
    /// Whether this style carries no formatting.
    pub fn is_default(&self) -> bool {
        self.number_format.is_none()
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
