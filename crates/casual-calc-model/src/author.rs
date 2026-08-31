//! Who last changed a cell (`HIST-02`, [`docs/89`]).
//!
//! Attribution is stored the way strings and styles are: an interned id on the
//! cell and one table on the workbook. A `String` per cell would be a million
//! allocations to say one name a million times, which is the cost `PERF-11`
//! changed how every reference is stored to avoid.
//!
//! **The identity is the host's opaque id, and the name is only for display.**
//! Two people called *Alex* are one author under a name and two under an id; a
//! person who renames themselves mid-session is one author under an id and two
//! under a name. A host that supplies no id falls back to the name being the
//! identity, which is exactly what a session without one has today.
//!
//! The reason the id is the host's rather than the JWT `sub` the server already
//! verifies — which would have been free — is that putting an identity
//! provider's subject into the workbook model puts it into version snapshots
//! and anything that serialises them. That is a stronger commitment than a
//! spreadsheet has previously made, and it belongs to the integrator who holds
//! the identity rather than to this crate.
//!
//! [`docs/89`]: https://github.com/CasualOffice/opencalc/blob/main/docs/89-CHANGE-ATTRIBUTION-AND-TRACKING.md

use core::num::NonZeroU32;

/// A cell's author, interned.
///
/// `NonZeroU32` so that `Option<AuthorId>` is four bytes rather than eight —
/// the field sits on [`Cell`](crate::Cell), which is the hottest struct in the
/// model, and eight bytes there is eight megabytes at a million cells.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct AuthorId(pub NonZeroU32);

/// One participant, as the host described them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Author {
    /// The host's opaque identifier. Empty when the host supplied none, in
    /// which case [`Self::name`] is the identity.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// What to show. Never compared — see the module note.
    pub name: String,
}

/// The workbook's authors, interned.
///
/// Ten collaborators cost ten entries, which is why the table is a `Vec` and
/// the lookup is a scan: a linear search over ten is faster than a hash, and a
/// document with enough authors for that to stop being true is not a document
/// this feature was designed for.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct AuthorTable {
    authors: Vec<Author>,
}

impl AuthorTable {
    /// An empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            authors: Vec::new(),
        }
    }

    /// Whether the table holds nothing, so a workbook with no collaboration
    /// serialises without it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.authors.is_empty()
    }

    /// How many authors are known.
    #[must_use]
    pub fn len(&self) -> usize {
        self.authors.len()
    }

    /// The author for an id.
    #[must_use]
    pub fn get(&self, id: AuthorId) -> Option<&Author> {
        self.authors.get(id.0.get() as usize - 1)
    }

    /// Intern an author, returning the id for it.
    ///
    /// **Matched on the id when there is one, and on the name only when there
    /// is not.** That is the whole difference the host's identifier buys: a
    /// participant who renames themselves keeps one entry, and two people
    /// sharing a display name keep two. Interning by name in both cases would
    /// throw the distinction away at the moment it is stored, where no later
    /// code could recover it.
    ///
    /// A rename updates the stored name in place rather than adding an entry,
    /// so the cells already attributed to that person show the new name.
    pub fn intern(&mut self, author: Author) -> AuthorId {
        let found = if author.id.is_empty() {
            self.authors
                .iter()
                .position(|a| a.id.is_empty() && a.name == author.name)
        } else {
            self.authors.iter().position(|a| a.id == author.id)
        };
        if let Some(at) = found {
            // The name may have changed; the identity has not.
            if self.authors[at].name != author.name {
                self.authors[at].name = author.name;
            }
            // `at` is an index this method just found, so the increment cannot
            // wrap and the value cannot be zero.
            return AuthorId(NonZeroU32::new(at as u32 + 1).expect("index + 1 is never zero"));
        }
        self.authors.push(author);
        AuthorId(NonZeroU32::new(self.authors.len() as u32).expect("a pushed table is never empty"))
    }

    /// Every author, in id order.
    pub fn iter(&self) -> impl Iterator<Item = (AuthorId, &Author)> {
        self.authors.iter().enumerate().map(|(i, a)| {
            (
                AuthorId(NonZeroU32::new(i as u32 + 1).expect("index + 1 is never zero")),
                a,
            )
        })
    }
}
