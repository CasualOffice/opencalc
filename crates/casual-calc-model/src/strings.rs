//! The interned string table. Cells reference strings by [`StringId`] so a
//! million cells sharing text cost one string. See `docs/22-NORMALIZED-SCHEMA.md`.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::ids::StringId;
use crate::style::TextRun;

/// A deduplicated table of strings, serialized as an ordered list. Interning is
/// deterministic: a string's id encodes its insertion index, so the same
/// sequence of interns always yields the same ids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "StringTableRepr", into = "StringTableRepr")]
pub struct StringTable {
    entries: Vec<String>,
    /// Run formatting for the entries that have any, by index.
    ///
    /// Sparse and kept beside the text rather than replacing it: nearly every
    /// string in a workbook is unformatted, and every caller that wants the
    /// characters — rendering, search, export to CSV — should not have to
    /// reassemble them from runs.
    runs: BTreeMap<u32, Vec<TextRun>>,
    /// Interning is keyed on text **and** runs. Two cells reading "Total" are
    /// the same string only if they are formatted the same way; keying on text
    /// alone would give the second one the first one's formatting.
    index: HashMap<(String, Vec<TextRun>), u32>,
}

/// The serialized shape. Split out so a plain workbook's snapshot is still a
/// flat list of strings, with the run map absent entirely.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StringTableRepr {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    entries: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "crate::int_keys::deserialize")]
    runs: BTreeMap<u32, Vec<TextRun>>,
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

    /// Intern plain `value`, returning its (possibly pre-existing) id.
    pub fn intern(&mut self, value: &str) -> StringId {
        self.intern_runs(value, Vec::new())
    }

    /// Intern rich text: the flattened characters plus the runs that formatted
    /// them. A single run with no formatting is stored as a plain string, so
    /// reading a file that happens to wrap unformatted text in one `<r>` does
    /// not create a needlessly rich entry.
    pub fn intern_rich(&mut self, runs: Vec<TextRun>) -> StringId {
        let text: String = runs.iter().map(|r| r.text.as_str()).collect();
        let plain = runs
            .iter()
            .all(|r| r.font.as_ref().is_none_or(|f| f.is_empty()));
        self.intern_runs(&text, if plain { Vec::new() } else { runs })
    }

    fn intern_runs(&mut self, value: &str, runs: Vec<TextRun>) -> StringId {
        let key = (value.to_owned(), runs.clone());
        if let Some(&index) = self.index.get(&key) {
            return Self::id_for(index);
        }
        let index = self.entries.len() as u32;
        self.entries.push(value.to_owned());
        if !runs.is_empty() {
            self.runs.insert(index, runs);
        }
        self.index.insert(key, index);
        Self::id_for(index)
    }

    /// The formatted runs behind `id`, or `None` when the string is plain.
    pub fn runs(&self, id: StringId) -> Option<&[TextRun]> {
        let index = self.index_of(id)?;
        self.runs.get(&index).map(Vec::as_slice)
    }

    /// Resolve an id to its string, or `None` if it is not from this table.
    pub fn get(&self, id: StringId) -> Option<&str> {
        let index = self.index_of(id)? as usize;
        self.entries.get(index).map(String::as_str)
    }

    /// The zero-based index encoded by `id`, if it is from this table.
    pub fn index_of(&self, id: StringId) -> Option<u32> {
        // Fallible still, because the caller's question is "does this resolve
        // *here*" and an id from another workbook's table answers no. The
        // namespace tag that used to be checked here could not answer that
        // either — it distinguished a style from a string, which the type
        // system already does — and cost twelve bytes in every cell to do it
        // (docs/58).
        let index = id.index();
        ((index as usize) < self.entries.len()).then_some(index)
    }

    /// Iterate the interned strings in index order.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(String::as_str)
    }

    /// The id of the entry at `index`, for callers walking the table in order
    /// (the writer, which needs each entry's runs alongside its text).
    pub fn id_at(&self, index: usize) -> Option<StringId> {
        (index < self.entries.len()).then(|| Self::id_for(index as u32))
    }

    /// Whether `id` resolves within this table.
    pub fn contains(&self, id: StringId) -> bool {
        self.get(id).is_some()
    }

    fn id_for(index: u32) -> StringId {
        StringId::at(index)
    }
}

impl From<StringTableRepr> for StringTable {
    fn from(repr: StringTableRepr) -> Self {
        let StringTableRepr { entries, runs } = repr;
        let mut index = HashMap::with_capacity(entries.len());
        for (i, value) in entries.iter().enumerate() {
            let key = (
                value.clone(),
                runs.get(&(i as u32)).cloned().unwrap_or_default(),
            );
            index.entry(key).or_insert(i as u32);
        }
        Self {
            entries,
            runs,
            index,
        }
    }
}

impl From<StringTable> for StringTableRepr {
    fn from(table: StringTable) -> Self {
        Self {
            entries: table.entries,
            runs: table.runs,
        }
    }
}
