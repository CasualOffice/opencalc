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
        let entry = self
            .entries
            .entry(feature.to_owned())
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
