//! The interned string table. Cells reference strings by [`StringId`] so a
//! million cells sharing text cost one string. See `docs/22-NORMALIZED-SCHEMA.md`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::ids::{Id, StringId};

/// Namespace for string ids (high 64 bits of the `Id`).
const STRING_NAMESPACE: u64 = 0x5354_5200_0000_0000; // "STR\0"

/// A deduplicated table of strings, serialized as an ordered list. Interning is
/// deterministic: a string's id encodes its insertion index, so the same
/// sequence of interns always yields the same ids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Vec<String>", into = "Vec<String>")]
pub struct StringTable {
    entries: Vec<String>,
    index: HashMap<String, u32>,
}

impl StringTable {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the table holds no strings.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The number of interned strings.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Intern `value`, returning its (possibly pre-existing) id.
    pub fn intern(&mut self, value: &str) -> StringId {
        if let Some(&index) = self.index.get(value) {
            return Self::id_for(index);
        }
        let index = self.entries.len() as u32;
        self.entries.push(value.to_owned());
        self.index.insert(value.to_owned(), index);
        Self::id_for(index)
    }

    /// Resolve an id to its string, or `None` if it is not from this table.
    pub fn get(&self, id: StringId) -> Option<&str> {
        let raw = id.0.get();
        if (raw >> 64) as u64 != STRING_NAMESPACE {
            return None;
        }
        let index = (raw as u64).checked_sub(1)? as usize;
        self.entries.get(index).map(String::as_str)
    }

    /// Whether `id` resolves within this table.
    pub fn contains(&self, id: StringId) -> bool {
        self.get(id).is_some()
    }

    fn id_for(index: u32) -> StringId {
        StringId(Id::from_parts(STRING_NAMESPACE, index as u64 + 1))
    }
}

impl From<Vec<String>> for StringTable {
    fn from(entries: Vec<String>) -> Self {
        let mut index = HashMap::with_capacity(entries.len());
        for (i, value) in entries.iter().enumerate() {
            index.entry(value.clone()).or_insert(i as u32);
        }
        Self { entries, index }
    }
}

impl From<StringTable> for Vec<String> {
    fn from(table: StringTable) -> Self {
        table.entries
    }
}
