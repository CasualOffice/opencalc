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
}

impl ModelError {
    /// The stable diagnostic code for this error (`docs/20`).
    pub fn code(&self) -> &'static str {
        match self {
            ModelError::Invariant(_) => "OC-MDL-0001",
            ModelError::Snapshot(_) => "OC-MDL-0004",
        }
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] ", self.code())?;
        match self {
            ModelError::Invariant(msg) => write!(f, "model invariant violated: {msg}"),
            ModelError::Snapshot(err) => write!(f, "snapshot (de)serialization failed: {err}"),
        }
    }
}

impl std::error::Error for ModelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ModelError::Snapshot(err) => Some(err),
            ModelError::Invariant(_) => None,
        }
    }
}

impl From<serde_json::Error> for ModelError {
    fn from(err: serde_json::Error) -> Self {
        ModelError::Snapshot(err)
    }
}
