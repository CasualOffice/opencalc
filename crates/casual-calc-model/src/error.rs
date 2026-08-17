//! Model and snapshot errors. See `docs/20-ERROR-CODE-REGISTRY.md`.

use core::fmt;

/// An error from validating the model or reading/writing a snapshot.
#[derive(Debug)]
#[non_exhaustive]
pub enum ModelError {
    /// A model invariant was violated (e.g. a duplicate sheet id).
    Invariant(&'static str),
    /// A snapshot could not be (de)serialized.
    Snapshot(serde_json::Error),
    /// A snapshot asked for more than one snapshot may have.
    ///
    /// Distinct from [`ModelError::Snapshot`] because it is not a malformed
    /// document: the bytes may be perfectly well formed and simply too many.
    /// An operator seeing this needs to know it is a limit, not corruption.
    SnapshotTooLarge {
        /// What ran out: `"bytes"` or `"populated cells"`.
        what: &'static str,
        /// The ceiling it passed.
        limit: u64,
        /// What was actually asked for.
        asked: u64,
    },
}

impl ModelError {
    /// The stable diagnostic code for this error (`docs/20`).
    pub fn code(&self) -> &'static str {
        match self {
            ModelError::Invariant(_) => "OC-MDL-0001",
            ModelError::Snapshot(_) => "OC-MDL-0004",
            ModelError::SnapshotTooLarge { .. } => "OC-MDL-0005",
        }
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] ", self.code())?;
        match self {
            ModelError::Invariant(msg) => write!(f, "model invariant violated: {msg}"),
            ModelError::Snapshot(err) => write!(f, "snapshot (de)serialization failed: {err}"),
            ModelError::SnapshotTooLarge { what, limit, asked } => write!(
                f,
                "this snapshot has {asked} {what}, over the {limit} one snapshot may have"
            ),
        }
    }
}

impl std::error::Error for ModelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ModelError::Snapshot(err) => Some(err),
            ModelError::Invariant(_) | ModelError::SnapshotTooLarge { .. } => None,
        }
    }
}

impl From<serde_json::Error> for ModelError {
    fn from(err: serde_json::Error) -> Self {
        ModelError::Snapshot(err)
    }
}
