//! Identifiers. `Id` is a non-zero 128-bit value serialized as 32 hex chars;
//! typed newtypes wrap it. See `docs/22-NORMALIZED-SCHEMA.md`.

use core::fmt;

use serde::de::{self, Deserialize, Deserializer, Visitor};
use serde::ser::{Serialize, Serializer};

/// A non-zero, deterministic 128-bit identifier.
///
/// Serialized as a 32-character lowercase hex string so snapshots are stable and
/// human-diffable. Construct via [`Id::from_parts`] or an [`IdGenerator`]; the
/// zero value is invalid and rejected on deserialize.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(u128);

impl Id {
    /// Wrap a raw value, returning `None` if it is zero.
    pub fn new(value: u128) -> Option<Id> {
        (value != 0).then_some(Id(value))
    }

    /// Compose an id from a namespace (high 64 bits) and counter (low 64 bits).
    /// A non-zero counter guarantees a non-zero id.
    pub fn from_parts(namespace: u64, counter: u64) -> Id {
        Id(((namespace as u128) << 64) | (counter as u128))
    }

    /// The raw 128-bit value.
    pub fn get(self) -> u128 {
        self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({self})")
    }
}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("{:032x}", self.0))
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Id, D::Error> {
        struct IdVisitor;
        impl Visitor<'_> for IdVisitor {
            type Value = Id;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a 32-character hex string naming a non-zero id")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Id, E> {
                let raw = u128::from_str_radix(value, 16)
                    .map_err(|_| E::custom("id is not valid hexadecimal"))?;
                Id::new(raw).ok_or_else(|| E::custom("id must be non-zero"))
            }
        }
        deserializer.deserialize_str(IdVisitor)
    }
}

/// Deterministic id source for one namespace. Counters start at 1, so every
/// produced id is non-zero and unique within the namespace.
#[derive(Debug, Clone)]
pub struct IdGenerator {
    namespace: u64,
    counter: u64,
}

impl IdGenerator {
    /// A generator for `namespace`, starting its counter at 0 (first id is 1).
    pub fn new(namespace: u64) -> Self {
        Self {
            namespace,
            counter: 0,
        }
    }

    /// Produce the next id in sequence.
    pub fn next_id(&mut self) -> Id {
        self.counter += 1;
        Id::from_parts(self.namespace, self.counter)
    }
}

/// Declare a typed newtype over [`Id`] that serializes transparently.
macro_rules! id_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Id);

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_newtype!(
    /// Identifies a worksheet.
    SheetId
);
/// An index into a per-workbook interned table, numbered from one.
///
/// Deliberately **not** an [`Id`]. These sit inside every populated cell, so
/// their width is the 1M-cell budget: a `u128` costs 16 bytes and, having no
/// spare bit pattern, makes `Option` cost 32. `NonZeroU32` costs four and its
/// option costs four, which is why the numbering starts at one
/// ([58](../../../docs/58-INTERNED-ID-WIDTH.md)).
macro_rules! index_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Debug,
            serde::Serialize,
            serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub core::num::NonZeroU32);

        impl $name {
            /// The id for a zero-based table position.
            ///
            /// # Panics
            ///
            /// If `index` is `u32::MAX`, which would need a table of four
            /// billion entries to reach.
            #[must_use]
            pub fn at(index: u32) -> Self {
                Self(
                    core::num::NonZeroU32::new(index + 1)
                        .expect("index + 1 is never zero below u32::MAX"),
                )
            }

            /// The zero-based table position this id names.
            #[must_use]
            pub fn index(self) -> u32 {
                self.0.get() - 1
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                self.0.get().fmt(f)
            }
        }
    };
}

index_newtype!(
    /// Identifies an interned style record.
    StyleId
);
index_newtype!(
    /// Identifies an interned string.
    StringId
);
id_newtype!(
    /// Identifies a number-format record.
    NumberFormatId
);
id_newtype!(
    /// Identifies a defined name.
    DefinedNameId
);

/// A handle into the per-workbook formula AST arena (a reserved calc seam; the
/// arena itself is populated by the calc engine in Phase 2).
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct FormulaHandle(pub u32);
