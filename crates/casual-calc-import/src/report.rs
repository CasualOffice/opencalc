//! The compatibility report and its dual-axis disposition taxonomy.
//! See `docs/34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md`, `docs/35`-style taxonomy.

use std::collections::BTreeMap;

/// How well the semantic model captured a construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelOutcome {
    /// Fully represented in the model.
    Mapped,
    /// Partially represented; some nuance lost.
    Degraded,
    /// Not represented in the model at all.
    Omitted,
}

/// Whether a construct's original bytes are kept for write-back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionOutcome {
    /// Original bytes retained and re-emittable.
    Preserved,
    /// Not kept (semantic-mode drop).
    NotRetained,
    /// Nothing to retain (fully mapped, regenerated on write).
    NotApplicable,
}

/// One aggregated report entry for a feature (element/type name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityEntry {
    /// The feature key (element local-name or cell type).
    pub feature: String,
    /// The worst model outcome observed for this feature.
    pub model: ModelOutcome,
    /// The retention outcome observed for this feature.
    pub retention: RetentionOutcome,
    /// How many times it was seen.
    pub count: u64,
}

/// The aggregated dispositions produced by import. Deterministically ordered by
/// feature; nothing is dropped without a recorded disposition.
#[derive(Debug, Clone, Default)]
pub struct CompatibilityReport {
    entries: BTreeMap<String, (ModelOutcome, RetentionOutcome, u64)>,
}

/// How many distinct features one report will name before folding the rest into
/// `(overflow)`.
///
/// **A security bound, not a tidiness one.** Feature keys are taken from the
/// file: a cell's `t=` attribute reaches [`CompatibilityReport::record`]
/// verbatim, so a workbook carrying a million cells with a million distinct
/// type strings inserted a million `String` keys into a map with no ceiling.
/// The reader's other admission limits do not help — each individual cell is
/// perfectly legal, and the growth is in the *report about* them.
///
/// Generous next to any real file. A workbook that legitimately degrades in
/// more than this many distinct ways has told the host what it needs to know
/// long before the cap.
pub const MAX_REPORT_FEATURES: usize = 512;

/// The key every feature past [`MAX_REPORT_FEATURES`] is counted under.
///
/// Counted rather than dropped, because [34](../../../docs/34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)
/// is explicit that nothing leaves the system silently: a host that sees
/// `(overflow)` knows its report is incomplete, where a truncated list would
/// simply look like a file with fewer problems than it has.
pub const OVERFLOW_FEATURE: &str = "(overflow)";

fn severity(outcome: ModelOutcome) -> u8 {
    match outcome {
        ModelOutcome::Mapped => 0,
        ModelOutcome::Degraded => 1,
        ModelOutcome::Omitted => 2,
    }
}

impl CompatibilityReport {
    /// Record one observation of `feature`, keeping the worst model outcome.
    pub fn record(&mut self, feature: &str, model: ModelOutcome, retention: RetentionOutcome) {
        // The cap lives here rather than at the call sites. There are two dozen
        // of those and exactly one of them remembered to bound anything, which
        // is the argument: a limit every caller must opt into is a limit that
        // holds until somebody adds the twenty-fourth call.
        //
        // A feature already named stays named however many times it recurs —
        // the count is what makes the report useful, and it costs no memory.
        let key =
            if self.entries.len() >= MAX_REPORT_FEATURES && !self.entries.contains_key(feature) {
                OVERFLOW_FEATURE
            } else {
                feature
            };
        let entry = self
            .entries
            .entry(key.to_owned())
            .or_insert((model, retention, 0));
        if severity(model) > severity(entry.0) {
            entry.0 = model;
            entry.1 = retention;
        }
        entry.2 += 1;
    }

    /// The entries, sorted by feature.
    pub fn entries(&self) -> Vec<CompatibilityEntry> {
        self.entries
            .iter()
            .map(|(feature, &(model, retention, count))| CompatibilityEntry {
                feature: feature.clone(),
                model,
                retention,
                count,
            })
            .collect()
    }

    /// Whether every recorded feature was fully `Mapped`.
    pub fn is_clean(&self) -> bool {
        self.entries
            .values()
            .all(|&(model, _, _)| model == ModelOutcome::Mapped)
    }

    /// Whether any feature was recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod bound_tests {
    use super::*;

    /// A file cannot make the report grow without limit.
    ///
    /// Feature keys come from the document — a cell's `t=` attribute reaches
    /// `record` verbatim — so before the cap a workbook with a million distinct
    /// type strings inserted a million `String` keys into an unbounded map,
    /// while docs/34 stated the report was "bounded … with an `(overflow)`
    /// bucket". The reader's other limits do not help: each cell is legal and
    /// the growth is in the report *about* them.
    #[test]
    fn a_flood_of_distinct_features_is_folded_into_one_overflow_entry() {
        let mut report = CompatibilityReport::default();
        for i in 0..MAX_REPORT_FEATURES * 4 {
            report.record(
                &format!("t{i}"),
                ModelOutcome::Omitted,
                RetentionOutcome::NotRetained,
            );
        }

        let entries = report.entries();
        assert!(
            entries.len() <= MAX_REPORT_FEATURES + 1,
            "the report grew to {} entries",
            entries.len()
        );
        let overflow = entries
            .iter()
            .find(|e| e.feature == OVERFLOW_FEATURE)
            .expect("the excess is counted, not dropped");
        // Everything past the cap, and nothing else.
        assert_eq!(overflow.count, (MAX_REPORT_FEATURES * 3) as u64);
    }

    /// A feature already named keeps its own row however often it recurs.
    ///
    /// Folding recurrences into `(overflow)` would make the count — the part a
    /// host acts on — wrong for the features that matter most.
    #[test]
    fn a_known_feature_still_counts_after_the_cap_is_reached() {
        let mut report = CompatibilityReport::default();
        report.record("f", ModelOutcome::Omitted, RetentionOutcome::NotRetained);
        for i in 0..MAX_REPORT_FEATURES * 2 {
            report.record(
                &format!("t{i}"),
                ModelOutcome::Omitted,
                RetentionOutcome::NotRetained,
            );
        }
        report.record("f", ModelOutcome::Omitted, RetentionOutcome::NotRetained);

        let f = report
            .entries()
            .into_iter()
            .find(|e| e.feature == "f")
            .expect("a named feature keeps its row");
        assert_eq!(f.count, 2, "and keeps counting");
    }
}
