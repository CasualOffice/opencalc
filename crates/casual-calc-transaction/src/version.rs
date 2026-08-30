//! Named versions: a document's past, and what bounds it.
//!
//! # Why snapshots
//!
//! Not a preference. The collaboration log is a **resume buffer** and not a
//! history (`SAVE-09`): no timestamps, no per-revision author, four to six
//! hundred operations retained, and nothing persisted past thirty seconds after
//! the last participant leaves. And a log that could be kept still could not be
//! replayed reproducibly — `COL-50` has an insert meeting a delete settle a
//! formula range differently in each order, and
//! [`TransformError::Unsupported`](crate::transform::TransformError::Unsupported)
//! refuses three pairs outright. A snapshot is bytes; putting one back requires
//! no transform and no ordering argument. So snapshots are the only available
//! option here rather than the expensive one, and history is **not** blocked on
//! `COL-50`.
//!
//! # What a version costs
//!
//! One version is one [`Workbook::to_snapshot`] — the same deterministic
//! normalized JSON the collaboration server already exchanges (ADR-010), so
//! nothing new is serialized and nothing new can drift. It is a **whole copy**:
//! there is no delta encoding here, because a delta chain is only as
//! recoverable as its weakest link and the point of this feature is to be the
//! thing that still works.
//!
//! **Measured** (`version_tests::measure_snapshot_cost`, release, 20 columns of
//! mixed numbers, shared strings and styles):
//!
//! | populated cells | snapshot | capture | parse back | versions in 50 MiB |
//! | --- | --- | --- | --- | --- |
//! | 10 000 | 0.57 MiB | 2.6 ms | 3.2 ms | 86 |
//! | 100 000 | 5.86 MiB | 15.5 ms | 20.0 ms | 8 |
//! | 300 000 | 17.82 MiB | 27.4 ms | 46.6 ms | **2** |
//!
//! Two things follow, and both are load-bearing.
//!
//! **The byte bound is the one that binds.** [`RetentionPolicy::max_autosave`]
//! is reached only on a small workbook; on a large one
//! [`RetentionPolicy::max_bytes`] is reached first and by a wide margin, which
//! is why there are two ceilings rather than the count one alone. At 300 000
//! cells a fifty-megabyte budget is *two* versions, and a host that wants more
//! either raises the budget or compresses — the same snapshot gzips 11×
//! (17.82 MiB → 1.61 MiB, 31 versions in the same budget), and compressing is
//! the **host's** job, at its storage layer, where a browser gets it from
//! `CompressionStream` off the main thread and an object store often gets it
//! for free. [`Version::byte_len`] is therefore the uncompressed size, and the
//! budget is conservative rather than wrong.
//!
//! **This is not `session_save()`.** docs/83 §6.3 describes a version as
//! "`session_save()` bytes + metadata", and taking that literally would cost
//! **424 ms** at 300 000 cells (`SAVE-06`) — a serialization to a zipped OOXML
//! package. The model snapshot is the same state at 27 ms, fifteen times
//! cheaper, and it is what the collaboration server already exchanges. A
//! version is the *document*, not a file the document could be written as.
//!
//! # What is discarded first
//!
//! Three tiers, and only the first is evictable:
//!
//! | tier | made by | evictable |
//! | --- | --- | --- |
//! | [`VersionKind::Autosave`] | the autosave cadence | yes, oldest first |
//! | [`VersionKind::Saved`] | an explicit `Ctrl+S` | no |
//! | [`VersionKind::Named`] | the user typed a name | no |
//!
//! A store that cannot fit a new version even after evicting every autosave
//! **refuses it and says so** ([`VersionError::Full`]) rather than dropping a
//! version somebody asked for. Deleting a version is refused outright — a named
//! version can be [hidden](VersionStore::hide), and its bytes stay until the
//! ring reaches them. "Delete this version" is a promise about copies on other
//! machines that a distributed system cannot keep.

use casual_calc_model::{ModelError, Workbook};

/// How a version came to exist.
///
/// The tier decides eviction and nothing else. Only the automatic entries are
/// noise, and they are noise right up to the moment they are the only thing
/// left — which is why they are kept at all and why they are the first to go.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum VersionKind {
    /// Written by the autosave cadence. Evictable.
    Autosave,
    /// Written by an explicit save. Kept until deleted.
    Saved,
    /// The user gave it a name. Kept until deleted, never evicted by the ring.
    Named,
}

impl VersionKind {
    /// Whether the retention ring may discard this tier.
    #[must_use]
    pub const fn is_evictable(self) -> bool {
        matches!(self, Self::Autosave)
    }
}

/// A version's identity, assigned by the store and never reused.
///
/// Monotonic within a store, so "newer" is decidable without consulting a
/// clock — which matters because the clock is the host's and a host's clock can
/// go backwards.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct VersionId(pub u64);

impl core::fmt::Display for VersionId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What is known about a version without loading its bytes.
///
/// This is what a version list is made of, and it is deliberately small: a
/// panel showing fifty entries must not hold fifty workbooks.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    /// Assigned by the store, monotonic, never reused.
    pub id: VersionId,
    /// Which tier, and so whether the ring may take it.
    pub kind: VersionKind,
    /// The name the user gave, for [`VersionKind::Named`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// When it was captured, in milliseconds since the Unix epoch.
    ///
    /// **Supplied by the caller**, never read from a clock here: the host owns
    /// I/O and time (AGENTS.md), a WebAssembly build has no clock this crate
    /// can reach, and a captured time this crate invented could not be tested.
    /// It is also why a store must not sort by it — see [`VersionId`].
    pub captured_at_ms: i64,
    /// The collaboration revision the snapshot was taken at, when there was
    /// one. Zero for a document with no server.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub revision: u64,
    /// How many bytes the snapshot occupies, for the retention arithmetic and
    /// for a host that wants to show it.
    pub byte_len: usize,
    /// Hidden from the list at the user's request. The bytes stay.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub hidden: bool,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// A version and the workbook it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionSnapshot {
    /// What is known about it without parsing the bytes.
    pub version: Version,
    /// The deterministic normalized-JSON encoding of the workbook
    /// ([`Workbook::to_snapshot`]).
    pub bytes: Vec<u8>,
}

impl VersionSnapshot {
    /// Rebuild the workbook this version holds.
    ///
    /// # Errors
    ///
    /// Whatever [`Workbook::from_snapshot`] refuses — the bytes are untrusted
    /// in exactly the way an uploaded package is, since they may have come back
    /// from a host's own store.
    pub fn workbook(&self) -> Result<Workbook, ModelError> {
        Workbook::from_snapshot(&self.bytes)
    }
}

/// What bounds the set of versions.
///
/// Two independent ceilings, because either one alone is wrong: a count with no
/// byte ceiling lets twenty snapshots of a large workbook take a gigabyte, and a
/// byte ceiling with no count lets a small workbook accumulate versions without
/// end. Both are checked, and whichever binds first wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPolicy {
    /// How many [`VersionKind::Autosave`] entries to keep.
    pub max_autosave: usize,
    /// How many bytes every version in the store may occupy **in total**,
    /// counting the tiers the ring cannot evict.
    pub max_bytes: u64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            // Sheets' shape and, more to the point, the number at which the
            // list stops being a list and starts being a log.
            max_autosave: 20,
            // The browser is the binding case: a per-origin IndexedDB quota is
            // typically a percentage of free disk, and a spreadsheet is not the
            // only thing in it. Fifty megabytes is a share a host can ask for
            // without a prompt on every platform this runs on.
            max_bytes: 50 << 20,
        }
    }
}

/// Why a version could not be stored.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum VersionError {
    /// The workbook could not be serialized.
    Snapshot(String),
    /// The snapshot alone is larger than the whole budget, so no amount of
    /// eviction would make room. Refused rather than admitted over budget:
    /// admitting it would put the store past a ceiling a host chose, and a
    /// ceiling that yields under pressure is not one.
    TooLarge {
        /// The policy's ceiling, in bytes.
        limit: u64,
        /// What the snapshot asked for.
        asked: u64,
    },
    /// There is no room even with every evictable version discarded, because
    /// the tiers the ring may not touch already fill the budget.
    ///
    /// **Not** resolved by evicting a named version. A named version is
    /// somebody's decision, and quietly discarding it to make room for an
    /// automatic one would be the silent data loss this project does not do.
    Full {
        /// The policy's ceiling, in bytes.
        limit: u64,
        /// What the versions that cannot be evicted already occupy.
        kept: u64,
        /// What the new snapshot asked for.
        asked: u64,
    },
}

impl core::fmt::Display for VersionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Snapshot(why) => write!(f, "[OC-VER-0001] cannot snapshot the workbook: {why}"),
            Self::TooLarge { limit, asked } => write!(
                f,
                "[OC-VER-0002] a version of {asked} bytes does not fit a {limit}-byte budget"
            ),
            Self::Full { limit, kept, asked } => write!(
                f,
                "[OC-VER-0003] no room for {asked} bytes: {kept} of the {limit}-byte budget is \
                 held by versions the ring may not evict"
            ),
        }
    }
}

impl std::error::Error for VersionError {}

/// What a capture did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    /// The version now holding this state.
    pub id: VersionId,
    /// Whether a new entry was written. `false` when the document had not
    /// changed since the newest version and the capture resolved to that one
    /// instead — see [`VersionStore::capture`].
    pub stored: bool,
    /// Versions the retention ring discarded to make room, oldest first.
    pub evicted: Vec<VersionId>,
}

/// The versions of one document, and the policy that bounds them.
///
/// **In memory, and not persistence.** The engine computes; the host decides
/// where bytes live (AGENTS.md). A browser host puts these in IndexedDB, a
/// server host next to the document, and a host with neither gets a list that
/// dies with the tab and should say so. [`into_parts`](Self::into_parts) and
/// [`from_parts`](Self::from_parts) are the seam: metadata is `serde`, and the
/// bytes are handed over as blobs rather than base64'd into JSON, because a
/// fifty-megabyte budget encoded as a JSON array of integers is not a
/// fifty-megabyte budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionStore {
    entries: Vec<VersionSnapshot>,
    next_id: u64,
    policy: RetentionPolicy,
}

impl Default for VersionStore {
    /// An empty store under the default policy.
    ///
    /// Written out rather than derived: a derived `Default` would start
    /// [`VersionId`] at zero, and the ids in this store are also the thing a
    /// host keys its own storage by. "The first version has id 1" is a promise
    /// worth not having a second answer to.
    fn default() -> Self {
        Self::new()
    }
}

impl VersionStore {
    /// An empty store under the default policy.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            policy: RetentionPolicy::default(),
        }
    }

    /// An empty store under `policy`.
    #[must_use]
    pub fn with_policy(policy: RetentionPolicy) -> Self {
        Self {
            policy,
            ..Self::new()
        }
    }

    /// The policy in force.
    #[must_use]
    pub const fn policy(&self) -> RetentionPolicy {
        self.policy
    }

    /// Change the policy. The ring is **not** applied retroactively: tightening
    /// a budget does not silently destroy versions a user can currently see.
    /// The next [`capture`](Self::capture) enforces it.
    pub const fn set_policy(&mut self, policy: RetentionPolicy) {
        self.policy = policy;
    }

    /// Rebuild a store a host persisted, under `policy`.
    ///
    /// Entries are sorted by id and the next id continues past the highest, so
    /// a store that comes back from storage in some other order — an IndexedDB
    /// cursor makes no promise this crate can rely on — still numbers
    /// monotonically and still lists in the order the versions were made.
    #[must_use]
    pub fn from_parts(policy: RetentionPolicy, mut entries: Vec<VersionSnapshot>) -> Self {
        entries.sort_by_key(|entry| entry.version.id);
        let next_id = entries
            .last()
            .map_or(1, |entry| entry.version.id.0.saturating_add(1));
        Self {
            entries,
            next_id,
            policy,
        }
    }

    /// Hand the versions back to the host, oldest first.
    #[must_use]
    pub fn into_parts(self) -> Vec<VersionSnapshot> {
        self.entries
    }

    /// The versions, oldest first, including hidden ones.
    pub fn versions(&self) -> impl Iterator<Item = &Version> {
        self.entries.iter().map(|entry| &entry.version)
    }

    /// The versions a list should show: newest first, hidden ones left out.
    pub fn visible(&self) -> impl Iterator<Item = &Version> {
        self.entries
            .iter()
            .rev()
            .map(|entry| &entry.version)
            .filter(|version| !version.hidden)
    }

    /// How many versions are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// What every version occupies, in bytes.
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.entries
            .iter()
            .map(|entry| entry.version.byte_len as u64)
            .sum()
    }

    /// One version and its bytes.
    #[must_use]
    pub fn get(&self, id: VersionId) -> Option<&VersionSnapshot> {
        self.entries.iter().find(|entry| entry.version.id == id)
    }

    /// The newest version, whatever its tier.
    #[must_use]
    pub fn newest(&self) -> Option<&VersionSnapshot> {
        self.entries.last()
    }

    /// Hide a version from the list, keeping its bytes.
    ///
    /// The only thing offered in place of deletion, and the reason is in the
    /// module docs. Returns whether the id was found.
    pub fn hide(&mut self, id: VersionId) -> bool {
        self.with_version(id, |version| version.hidden = true)
    }

    /// Put a hidden version back in the list.
    pub fn unhide(&mut self, id: VersionId) -> bool {
        self.with_version(id, |version| version.hidden = false)
    }

    /// Give a version a name, promoting it to [`VersionKind::Named`] so the
    /// ring will no longer take it.
    ///
    /// This is how "keep this one" works, and it is why naming is a store
    /// operation rather than a label a host keeps on the side: the name is what
    /// changes the retention tier.
    pub fn name(&mut self, id: VersionId, name: impl Into<String>) -> bool {
        let name = name.into();
        self.with_version(id, |version| {
            version.name = Some(name);
            version.kind = VersionKind::Named;
        })
    }

    fn with_version(&mut self, id: VersionId, f: impl FnOnce(&mut Version)) -> bool {
        match self.entries.iter_mut().find(|entry| entry.version.id == id) {
            Some(entry) => {
                f(&mut entry.version);
                true
            }
            None => false,
        }
    }

    /// Capture `workbook` as a new version.
    ///
    /// `captured_at_ms` is the host's clock; see [`Version::captured_at_ms`] for
    /// why this crate does not read one. `revision` is the collaboration
    /// revision, or zero.
    ///
    /// **A capture that would duplicate the newest version stores nothing** and
    /// returns that version's id with [`Captured::stored`] false — unless it
    /// carries a name, because naming a state is an intention and not an
    /// observation. Without this an autosave cadence that fires on a quiet
    /// document fills the ring with copies of one state and pushes the
    /// versions that differ out of it.
    ///
    /// # Errors
    ///
    /// [`VersionError::Snapshot`] if the workbook cannot be serialized,
    /// [`VersionError::TooLarge`] if one snapshot exceeds the whole budget, and
    /// [`VersionError::Full`] if the versions the ring may not evict already
    /// fill it.
    pub fn capture(
        &mut self,
        workbook: &Workbook,
        kind: VersionKind,
        name: Option<String>,
        captured_at_ms: i64,
        revision: u64,
    ) -> Result<Captured, VersionError> {
        let bytes = workbook
            .to_snapshot()
            .map_err(|error| VersionError::Snapshot(error.to_string()))?;

        if name.is_none()
            && let Some(newest) = self.entries.last()
            && newest.bytes == bytes
        {
            return Ok(Captured {
                id: newest.version.id,
                stored: false,
                evicted: Vec::new(),
            });
        }

        let asked = bytes.len() as u64;
        if asked > self.policy.max_bytes {
            return Err(VersionError::TooLarge {
                limit: self.policy.max_bytes,
                asked,
            });
        }

        let effective = if name.is_some() {
            VersionKind::Named
        } else {
            kind
        };
        let evicted = self.make_room(asked, effective)?;

        let id = VersionId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.entries.push(VersionSnapshot {
            version: Version {
                id,
                kind: effective,
                name,
                captured_at_ms,
                revision,
                byte_len: bytes.len(),
                hidden: false,
            },
            bytes,
        });
        Ok(Captured {
            id,
            stored: true,
            evicted,
        })
    }

    /// Evict until `asked` more bytes fit and the autosave count leaves room.
    ///
    /// Oldest evictable first, and nothing else ever.
    ///
    /// **Feasibility is decided before anything is discarded.** An eviction is
    /// justified only by the version that replaces it, so a capture that cannot
    /// succeed must not have destroyed anything on its way to failing — which
    /// is what "no silent data loss" means when the loss would be caused by the
    /// error path rather than by the feature.
    fn make_room(
        &mut self,
        asked: u64,
        incoming: VersionKind,
    ) -> Result<Vec<VersionId>, VersionError> {
        let kept: u64 = self
            .entries
            .iter()
            .filter(|entry| !entry.version.kind.is_evictable())
            .map(|entry| entry.version.byte_len as u64)
            .sum();
        if kept.saturating_add(asked) > self.policy.max_bytes {
            return Err(VersionError::Full {
                limit: self.policy.max_bytes,
                kept,
                asked,
            });
        }

        let mut evicted = Vec::new();
        // The count ceiling applies to the automatic tier only, and only when
        // the arriving version joins it.
        if incoming.is_evictable() {
            while self.autosave_count().saturating_add(1) > self.policy.max_autosave {
                match self.evict_oldest() {
                    Some(id) => evicted.push(id),
                    None => break,
                }
            }
        }
        while self.total_bytes().saturating_add(asked) > self.policy.max_bytes {
            match self.evict_oldest() {
                Some(id) => evicted.push(id),
                // Unreachable given the feasibility check above, and left as a
                // break rather than a panic: a store that somehow got here has
                // already refused nothing and lost nothing.
                None => break,
            }
        }
        Ok(evicted)
    }

    fn autosave_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.version.kind.is_evictable())
            .count()
    }

    fn evict_oldest(&mut self) -> Option<VersionId> {
        let at = self
            .entries
            .iter()
            .position(|entry| entry.version.kind.is_evictable())?;
        Some(self.entries.remove(at).version.id)
    }
}
