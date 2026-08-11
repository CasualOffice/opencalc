//! Turning a signed string into [`Claims`].
//!
//! [ADR-014](../../../docs/59-COLLABORATION-SERVICE-STACK.md): the integrator
//! signs, this server verifies. It never holds a signing key, so a compromised
//! node cannot issue itself access to a document.
//!
//! # Fetching is not verifying
//!
//! A JWKS document arrives over the network; checking a signature against it
//! does not. The split is deliberate — [`KeySet::from_jwks`] parses bytes the
//! caller obtained however it likes, and [`Verifier`] is then a pure function
//! of the token, the keys, the policy and a supplied clock. Every rule worth
//! getting right is on the pure side.
//!
//! # The two attacks this is shaped around
//!
//! **Algorithm confusion.** A token's own header names its algorithm, and a
//! verifier that believes it can be handed `alg: none` and asked to accept an
//! unsigned token, or handed `alg: HS256` and tricked into using an RSA
//! *public* key — which is published — as an HMAC secret. So the accepted
//! algorithms are **configuration, not input**: the header is checked against
//! them and never consulted to decide what to do.
//!
//! **Key confusion.** `kid` selects which key to try. An unknown `kid` is a
//! refusal rather than an invitation to try the others, because "try everything
//! until one works" is how a key retired for being compromised keeps working.

use std::collections::BTreeMap;

use jsonwebtoken::{Algorithm, DecodingKey, Validation};

use crate::token::{Claims, TokenError, TokenPolicy};

/// The signature algorithms this server will accept.
///
/// Asymmetric by default. HS256 is a shared secret, which means the verifier
/// also holds what it takes to *mint* a token — acceptable for one process in
/// development, and the thing asymmetric keys exist to avoid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signing {
    /// RSA, the most widely issued.
    Rs256,
    /// ECDSA P-256: smaller keys and signatures, same guarantee.
    Es256,
    /// HMAC with a shared secret. Development and standalone only.
    Hs256,
}

impl Signing {
    fn algorithm(self) -> Algorithm {
        match self {
            Signing::Rs256 => Algorithm::RS256,
            Signing::Es256 => Algorithm::ES256,
            Signing::Hs256 => Algorithm::HS256,
        }
    }
}

/// The keys a token may be signed with, by `kid`.
pub struct KeySet {
    keys: BTreeMap<String, DecodingKey>,
    /// The key to use when a token names no `kid`, if there is exactly one.
    solitary: Option<DecodingKey>,
    accepted: Vec<Signing>,
}

impl core::fmt::Debug for KeySet {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Never print the keys themselves: an HS256 "key" is a secret, and a
        // debug line is the most casual way for one to reach a log.
        f.debug_struct("KeySet")
            .field("kids", &self.keys.keys().collect::<Vec<_>>())
            .field("accepted", &self.accepted)
            .finish_non_exhaustive()
    }
}

/// Why a key set could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// The JWKS document was not readable as one.
    Malformed(String),
    /// It parsed and contained no key this server can use.
    NoUsableKeys,
}

impl core::fmt::Display for KeyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            KeyError::Malformed(why) => write!(f, "the key set is malformed: {why}"),
            KeyError::NoUsableKeys => f.write_str("the key set contains no usable key"),
        }
    }
}

impl std::error::Error for KeyError {}

impl KeySet {
    /// Parse a JWKS document — the JSON at an integrator's `jwks_uri`.
    ///
    /// Keys this server cannot use are **skipped rather than refused**: a real
    /// JWKS often carries encryption keys and algorithms beside the signing
    /// one, and rejecting the whole set over a key nobody was going to use
    /// would make an ordinary key set unusable.
    pub fn from_jwks(bytes: &[u8], accepted: &[Signing]) -> Result<Self, KeyError> {
        let parsed: jsonwebtoken::jwk::JwkSet =
            serde_json::from_slice(bytes).map_err(|e| KeyError::Malformed(e.to_string()))?;

        let mut keys = BTreeMap::new();
        for jwk in &parsed.keys {
            let Ok(key) = DecodingKey::from_jwk(jwk) else {
                continue;
            };
            let Some(kid) = jwk.common.key_id.clone() else {
                continue;
            };
            keys.insert(kid, key);
        }
        if keys.is_empty() {
            return Err(KeyError::NoUsableKeys);
        }
        let solitary = (keys.len() == 1).then(|| {
            keys.values()
                .next()
                .expect("length checked immediately above")
                .clone()
        });
        Ok(Self {
            keys,
            solitary,
            accepted: accepted.to_vec(),
        })
    }

    /// A single shared secret, for standalone and development.
    ///
    /// Named for what it is. Anything holding this can mint tokens as well as
    /// check them, which is exactly what [`Signing::Rs256`] avoids.
    #[must_use]
    pub fn shared_secret(secret: &[u8]) -> Self {
        Self {
            keys: BTreeMap::new(),
            solitary: Some(DecodingKey::from_secret(secret)),
            accepted: vec![Signing::Hs256],
        }
    }

    /// How many named keys are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether there are no named keys.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    fn select(&self, kid: Option<&str>) -> Option<&DecodingKey> {
        match kid {
            // An unknown `kid` is a refusal, not an invitation to try the
            // others: falling back is how a key retired for being compromised
            // goes on working.
            Some(kid) => self.keys.get(kid),
            None => self.solitary.as_ref(),
        }
    }
}

/// Checks a token against a key set and a policy.
#[derive(Debug)]
pub struct Verifier {
    /// What this server accepts about the claims themselves.
    pub policy: TokenPolicy,
    /// The keys, and the algorithms they may be used with.
    pub keys: KeySet,
}

/// Why a token was rejected before its claims were even considered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// Not a JWT, or not one this server can read.
    Malformed,
    /// The header named an algorithm this server does not accept — including
    /// `none`.
    UnacceptableAlgorithm,
    /// No key matched the token's `kid`.
    UnknownKey,
    /// The signature did not check out.
    BadSignature,
    /// It was signed correctly and says something unacceptable.
    Claims(TokenError),
}

impl core::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VerifyError::Malformed => f.write_str("the token is not a readable JWT"),
            VerifyError::UnacceptableAlgorithm => {
                f.write_str("the token names an algorithm this server does not accept")
            }
            VerifyError::UnknownKey => f.write_str("no key matches the token's kid"),
            VerifyError::BadSignature => f.write_str("the signature did not verify"),
            VerifyError::Claims(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for VerifyError {}

impl VerifyError {
    /// What the client is told, which is deliberately less than this.
    ///
    /// Every failure here is one answer. Telling a caller *which* of malformed,
    /// wrong-key, bad-signature and expired it was hands them an oracle for
    /// probing the difference.
    #[must_use]
    pub fn refusal(&self) -> casual_calc_transaction::protocol::Refusal {
        casual_calc_transaction::protocol::Refusal::NotAuthorised
    }
}

impl Verifier {
    /// Verify `token` for a client asking to join `document_key`.
    ///
    /// `now_secs` is supplied rather than read, for the same reason it is
    /// everywhere else in this crate: expiry bugs live in rare timing, and
    /// rare timing is only testable when time is an argument.
    pub fn verify(
        &self,
        token: &str,
        document_key: &str,
        now_secs: u64,
    ) -> Result<Claims, VerifyError> {
        let header = jsonwebtoken::decode_header(token).map_err(|_| VerifyError::Malformed)?;

        // The header is checked against configuration and never consulted to
        // decide what to do. This is the whole defence against algorithm
        // confusion, including `alg: none`.
        let signing = self
            .keys
            .accepted
            .iter()
            .copied()
            .find(|s| s.algorithm() == header.alg)
            .ok_or(VerifyError::UnacceptableAlgorithm)?;

        let key = self
            .keys
            .select(header.kid.as_deref())
            .ok_or(VerifyError::UnknownKey)?;

        let mut validation = Validation::new(signing.algorithm());
        // Expiry and audience are checked by `Claims::validate` against the
        // supplied clock, not by the library against the machine's. Two checks
        // of the same thing against different clocks is one more than is useful.
        validation.validate_exp = false;
        validation.validate_aud = false;

        let data = jsonwebtoken::decode::<Claims>(token, key, &validation).map_err(|e| match e
            .kind()
        {
            jsonwebtoken::errors::ErrorKind::InvalidSignature => VerifyError::BadSignature,
            _ => VerifyError::Malformed,
        })?;

        data.claims
            .validate(document_key, &self.policy, now_secs)
            .map_err(VerifyError::Claims)?;
        Ok(data.claims)
    }
}

#[cfg(test)]
mod tests;
