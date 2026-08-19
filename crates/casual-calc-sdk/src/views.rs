//! Personal views: rows a participant hides for themselves alone (`COL-32`).
//!
//! Filtering a shared document hides rows for *everyone*, which is Excel's
//! model and is what the file format can express — but it is also the complaint
//! that made Google build filter views: sorting or filtering a sheet somebody
//! else is reading yanks the floor out from under them.
//!
//! [docs/71](../../../docs/71-FILTER-SHARING-AND-VIEWS.md) settles the policy.
//! This module holds the half of it that is *not* document state.
//!
//! # Why this is not on `Sheet`
//!
//! The whole design turns on one constraint:
//!
//! > **A personal view must not change a single cell value.**
//!
//! Cell values are what `recalculate` writes, what `save` serializes, and what
//! every participant is required to agree about. `SUBTOTAL`'s 101–111 codes and
//! `AGGREGATE` skip hidden rows by asking `Sheet::is_row_hidden`, so anything
//! reachable from a `Sheet` is by definition shared: put a personal hidden set
//! there and the same cell reads 4 on one screen and 6 on another, and the
//! convergence property the collaboration design rests on becomes false.
//!
//! So the personal set lives here, on the session, beside the other things a
//! session owns and a document does not — and there are two questions that look
//! identical and are not:
//!
//! | Question | Asked by | Answer |
//! | --- | --- | --- |
//! | "do I draw this row?" | the layout | shared **∪** personal |
//! | "does `SUBTOTAL` skip this row?" | the evaluator | shared only |
//!
//! The evaluator reaches the sheet and never reaches this type. That is not a
//! convention to be remembered; it is the reason the state is stored somewhere
//! the evaluator has no path to.
//!
//! # What it must never do
//!
//! Never on the wire, never in the undo history, never in the saved file. A
//! personal view survives nothing — reload and it is gone, which is what "not
//! part of the document" means.

use std::collections::{BTreeMap, BTreeSet};

/// Rows hidden for this participant alone, per sheet.
///
/// Empty by default and cheap when unused: a session with no personal view
/// holds an empty map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersonalViews {
    /// Keyed by sheet index. A sheet with no personal view has no entry, so
    /// "has a view" and "has a view that hides nothing" stay distinguishable —
    /// the second is a filter whose predicate currently matches everything, and
    /// clearing it is a different act from never having had one.
    hidden: BTreeMap<usize, BTreeSet<u32>>,
}

impl PersonalViews {
    /// No views anywhere.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Hide `rows` on `sheet`, for this participant only. Replaces any view
    /// already on that sheet, because a filter is a statement of what is shown
    /// rather than an addition to what is hidden.
    pub fn set(&mut self, sheet: usize, rows: BTreeSet<u32>) {
        self.hidden.insert(sheet, rows);
    }

    /// Drop the view on one sheet. Its rows come back.
    pub fn clear(&mut self, sheet: usize) {
        self.hidden.remove(&sheet);
    }

    /// Drop every view. One obvious click, because undo will not do it: a
    /// personal view is not a document edit, so pressing undo after applying
    /// one undoes whatever you last did *to the document* instead.
    pub fn clear_all(&mut self) {
        self.hidden.clear();
    }

    /// Whether `sheet` has a personal view at all.
    #[must_use]
    pub fn has_view(&self, sheet: usize) -> bool {
        self.hidden.contains_key(&sheet)
    }

    /// The rows this participant hides on `sheet`, if any.
    #[must_use]
    pub fn hidden_rows(&self, sheet: usize) -> Option<&BTreeSet<u32>> {
        self.hidden.get(&sheet)
    }

    /// Whether this participant's own view hides `row`.
    ///
    /// **Only the personal half.** Callers wanting "should this row be drawn"
    /// must union this with the sheet's own hidden set; the split is the point,
    /// so this deliberately cannot answer that on its own.
    #[must_use]
    pub fn hides(&self, sheet: usize, row: u32) -> bool {
        self.hidden
            .get(&sheet)
            .is_some_and(|rows| rows.contains(&row))
    }

    /// Whether any sheet has a view, for chrome that offers to clear them.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hidden.is_empty()
    }

    /// Move views with the sheets when one is inserted, removed or reordered.
    ///
    /// Without this a personal view silently transfers to whichever sheet
    /// inherits the index — hiding rows on a sheet the participant never
    /// filtered, with no operation on the wire to explain it and nothing in the
    /// history to undo. `remap` returns the sheet's new index, or `None` if it
    /// is gone.
    pub fn resequence(&mut self, remap: impl Fn(usize) -> Option<usize>) {
        self.hidden = std::mem::take(&mut self.hidden)
            .into_iter()
            .filter_map(|(sheet, rows)| remap(sheet).map(|now| (now, rows)))
            .collect();
    }
}

#[cfg(test)]
mod tests;
