//! Deserializing maps whose keys are integers, from JSON objects.
//!
//! # Why this exists
//!
//! JSON object keys are strings, so a `BTreeMap<u32, V>` serializes to
//! `{"0": …}` and `serde_json` parses it back by parsing the key. That works —
//! *until the map is inside an internally-tagged enum.*
//!
//! [`ClientMessage`](../../casual_calc_transaction/protocol/enum.ClientMessage.html)
//! is `#[serde(tag = "type")]`. Serde deserializes an internally-tagged enum by
//! buffering the whole value into its private `Content` type, reading the tag,
//! and then re-deserializing the variant *from the buffer*. That second pass
//! does not go through `serde_json`, and `Content` has no notion that a string
//! key might be meant as a number — so the field fails with
//!
//! ```text
//! invalid type: string "0", expected u32
//! ```
//!
//! and the whole message is unreadable.
//!
//! # What that cost
//!
//! Every operation carrying an integer-keyed map was silently undeliverable in
//! a collaborative session: **autofilter rules, every column width and row
//! height, and outline levels**. The sender's editor applied the change and
//! believed it had sent it; the server could not parse the message at all, so
//! no peer ever heard about it. The client was told `CannotMerge`, which named
//! the transform — the one part that was working.
//!
//! It survived because every test on both sides *constructed* the message
//! struct instead of round-tripping one through text. `net.rs` already carries
//! a comment about a previous bug found exactly this way; the lesson had not
//! been generalized into a test that parses.
//!
//! # What this does
//!
//! Accepts a key written either way — as a JSON string (what a JSON map
//! produces) or as a bare integer (what the `Content` buffer holds when the
//! value came from a self-describing format that kept the integer). Nothing on
//! the wire changes: the bytes are identical, so there is no protocol bump and
//! old and new clients read each other exactly as before. Only the reader gets
//! less fussy about which of the two shapes it is handed.

use core::fmt;
use core::marker::PhantomData;
use core::str::FromStr;
use std::collections::BTreeMap;

use serde::de::{Deserialize, Deserializer, Error, MapAccess, Visitor};

/// A map key that will accept `"7"` or `7`.
struct IntKey<K>(K);

impl<'de, K> Deserialize<'de> for IntKey<K>
where
    K: FromStr + TryFrom<u64> + TryFrom<i64>,
    <K as FromStr>::Err: fmt::Display,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct KeyVisitor<K>(PhantomData<K>);

        impl<K> Visitor<'_> for KeyVisitor<K>
        where
            K: FromStr + TryFrom<u64> + TryFrom<i64>,
            <K as FromStr>::Err: fmt::Display,
        {
            type Value = IntKey<K>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an integer, or an integer written as a string")
            }

            fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
                value.parse::<K>().map(IntKey).map_err(Error::custom)
            }

            fn visit_u64<E: Error>(self, value: u64) -> Result<Self::Value, E> {
                K::try_from(value)
                    .map(IntKey)
                    .map_err(|_| E::custom(format!("map key {value} is out of range")))
            }

            fn visit_i64<E: Error>(self, value: i64) -> Result<Self::Value, E> {
                K::try_from(value)
                    .map(IntKey)
                    .map_err(|_| E::custom(format!("map key {value} is out of range")))
            }
        }

        // `deserialize_any`, deliberately: the point is to handle whichever of
        // the two shapes arrives, and only a self-describing request can say
        // which one it got. Both `serde_json` and serde's `Content` buffer
        // support it.
        deserializer.deserialize_any(KeyVisitor(PhantomData))
    }
}

/// Deserialize a `BTreeMap` whose keys are integers, accepting string keys.
///
/// Pair with the derived `Serialize`, which already emits what this reads:
///
/// ```ignore
/// #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
/// #[serde(deserialize_with = "crate::int_keys::deserialize")]
/// pub rules: BTreeMap<u32, FilterRule>,
/// ```
///
/// # Errors
///
/// Propagates the deserializer's error, and reports a key that is neither an
/// integer nor a string holding one.
pub fn deserialize<'de, D, K, V>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
where
    D: Deserializer<'de>,
    K: Ord + FromStr + TryFrom<u64> + TryFrom<i64>,
    <K as FromStr>::Err: fmt::Display,
    V: Deserialize<'de>,
{
    struct MapVisitor<K, V>(PhantomData<(K, V)>);

    impl<'de, K, V> Visitor<'de> for MapVisitor<K, V>
    where
        K: Ord + FromStr + TryFrom<u64> + TryFrom<i64>,
        <K as FromStr>::Err: fmt::Display,
        V: Deserialize<'de>,
    {
        type Value = BTreeMap<K, V>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a map keyed by integers")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
            let mut out = BTreeMap::new();
            while let Some(IntKey(key)) = access.next_key::<IntKey<K>>()? {
                out.insert(key, access.next_value()?);
            }
            Ok(out)
        }
    }

    deserializer.deserialize_map(MapVisitor(PhantomData))
}
