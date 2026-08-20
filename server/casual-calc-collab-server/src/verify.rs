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
use std::sync::{Mutex, RwLock};

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

/// Where the keys came from, so they can be read again.
///
/// A key set that is fetched once and held for the life of the process is a
/// key set that cannot be rotated. [ADR-014](../../../docs/59-COLLABORATION-SERVICE-STACK.md)
/// states the opposite as a decided property — *"they publish a new key, the
/// server picks it up at the next fetch, and no coordinated restart is
/// needed"* — and the code did the first fetch and no others.
///
/// What that cost: an integrator rotating on schedule publishes `k2` beside
/// `k1` and starts signing with `k2`. Every node still holds only `k1`, and
/// `select` refuses an unknown `kid` outright rather than falling back — which
/// is right, and is what makes the missing refresh fatal instead of merely
/// stale. Nobody can join any document, including people rejoining one they
/// were in a minute ago, until an operator restarts every node. The client sees
/// the same `NotAuthorised` a bad token gets, so the cause is invisible from
/// outside. Revocation is the mirror image: pulling a compromised key has no
/// effect on a running node at all.
#[derive(Debug)]
pub struct JwksSource {
    /// The integrator's `jwks_uri`.
    pub url: String,
    /// The algorithms this server will accept a key for.
    pub accepted: Vec<Signing>,
    /// Unix ms of the last attempt, so an unknown `kid` cannot be used to make
    /// this server hammer somebody else's key endpoint.
    last_attempt_ms: Mutex<u64>,
    /// The shortest gap between two on-demand attempts.
    min_interval_ms: u64,
}

impl JwksSource {
    /// Whether an on-demand refresh may run now, recording it if so.
    ///
    /// Deliberately takes the clock as an argument, like everything else in
    /// this crate: a throttle is a timing rule, and timing rules are only
    /// testable when time is passed in.
    pub fn may_attempt(&self, now_ms: u64) -> bool {
        let mut last = self
            .last_attempt_ms
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if now_ms.saturating_sub(*last) < self.min_interval_ms {
            return false;
        }
        *last = now_ms;
        true
    }
}

/// One issuer and the keys that are allowed to speak for it.
///
/// The binding is the point. A deployment serving two tenants holds two
/// trusts, and a token is checked against **the key set of the issuer it
/// names** — never against every key the process happens to hold. Without that
/// binding, `iss` is a label rather than a boundary: a tenant whose signing key
/// is in the one shared set can mint a token naming the other tenant's issuer
/// and it verifies, which is exactly the hole `DEP-10` records.
///
/// The key set is behind a lock because it is **replaceable**: see
/// [`JwksSource`].
#[derive(Debug)]
pub struct Trust {
    /// The `iss` these keys may sign for. Empty trusts any issuer, which is
    /// the single-tenant case and the historical behaviour.
    pub issuer: String,
    /// The keys, and the algorithms they may be used with.
    keys: RwLock<KeySet>,
    /// Where to read them again, when they are fetched rather than configured.
    jwks: Option<JwksSource>,
}

impl Trust {
    /// A trust over a fixed key set — a shared secret, or a test.
    #[must_use]
    pub fn fixed(issuer: impl Into<String>, keys: KeySet) -> Self {
        Self {
            issuer: issuer.into(),
            keys: RwLock::new(keys),
            jwks: None,
        }
    }

    /// A trust whose keys are re-read from `url`.
    #[must_use]
    pub fn refreshing(
        issuer: impl Into<String>,
        keys: KeySet,
        url: String,
        accepted: Vec<Signing>,
        min_interval_ms: u64,
    ) -> Self {
        Self {
            issuer: issuer.into(),
            keys: RwLock::new(keys),
            jwks: Some(JwksSource {
                url,
                accepted,
                last_attempt_ms: Mutex::new(0),
                min_interval_ms,
            }),
        }
    }

    /// Where these keys are re-read from, if anywhere.
    #[must_use]
    pub fn jwks(&self) -> Option<&JwksSource> {
        self.jwks.as_ref()
    }

    /// Replace this issuer's key set.
    ///
    /// Only ever called with a set that parsed. A fetch that failed leaves the
    /// old keys in place on purpose: docs/59 says *"a cached key set keeps
    /// working, since an integrator's key server going down should not evict
    /// everybody"*, and evicting everybody is precisely what installing an
    /// empty set would do.
    pub fn install(&self, keys: KeySet) {
        *self.keys.write().unwrap_or_else(|e| e.into_inner()) = keys;
    }

    /// How many named keys this issuer currently has.
    #[must_use]
    pub fn key_count(&self) -> usize {
        self.keys.read().unwrap_or_else(|e| e.into_inner()).len()
    }
}

/// Checks a token against the keys of the issuer it names, and a policy.
#[derive(Debug)]
pub struct Verifier {
    /// What this server accepts about the claims themselves.
    pub policy: TokenPolicy,
    /// One entry per issuer this server will accept a token from.
    trusts: Vec<Trust>,
}

impl Verifier {
    /// A verifier over a fixed key set, trusting any issuer.
    #[must_use]
    pub fn fixed(policy: TokenPolicy, keys: KeySet) -> Self {
        Self {
            policy,
            trusts: vec![Trust::fixed(String::new(), keys)],
        }
    }

    /// A verifier whose keys are re-read from `url`, trusting any issuer.
    #[must_use]
    pub fn refreshing(
        policy: TokenPolicy,
        keys: KeySet,
        url: String,
        accepted: Vec<Signing>,
        min_interval_ms: u64,
    ) -> Self {
        Self {
            policy,
            trusts: vec![Trust::refreshing(
                String::new(),
                keys,
                url,
                accepted,
                min_interval_ms,
            )],
        }
    }

    /// A verifier over one key set per issuer.
    ///
    /// # Panics
    ///
    /// If `trusts` is empty, or if any two name the same issuer, or if a named
    /// issuer sits beside the trust-anybody entry. Each is a configuration
    /// error that would otherwise make the boundary silently ambiguous, and
    /// this is called once at startup.
    #[must_use]
    pub fn tenanted(policy: TokenPolicy, trusts: Vec<Trust>) -> Self {
        assert!(!trusts.is_empty(), "a verifier needs at least one trust");
        let named = trusts.iter().filter(|t| !t.issuer.is_empty()).count();
        assert!(
            named == trusts.len() || named == 0,
            "a named issuer cannot sit beside a trust that accepts any issuer: \
             the unnamed one would accept the named one's tokens too"
        );
        let mut seen = BTreeMap::new();
        for trust in &trusts {
            assert!(
                seen.insert(trust.issuer.clone(), ()).is_none(),
                "two trusts name the same issuer {:?}",
                trust.issuer
            );
        }
        Self { policy, trusts }
    }

    /// Every issuer this server accepts, and its keys.
    #[must_use]
    pub fn trusts(&self) -> &[Trust] {
        &self.trusts
    }

    /// Whether any issuer's keys are re-readable.
    #[must_use]
    pub fn refreshable(&self) -> bool {
        self.trusts.iter().any(|t| t.jwks().is_some())
    }

    /// The trust whose keys must have signed `token`, by the `iss` it names.
    ///
    /// Shared with [`Verifier::verify`] rather than reimplemented beside it: a
    /// refresh that re-read a *different* issuer's keys than the one being
    /// verified would fetch forever and never fix anything.
    #[must_use]
    pub fn trust_for(&self, token: &str) -> Option<&Trust> {
        match self.trusts.as_slice() {
            // With one trust there is nothing to route: the token is checked
            // against the only keys there are, and if that trust names an
            // issuer the *verified* `iss` is what refuses a token minted for
            // somebody else. Routing on the unverified value here would refuse
            // it earlier and say less about why.
            [only] => Some(only),
            trusts => {
                let named = unverified_issuer(token)?;
                trusts.iter().find(|t| t.issuer == named)
            }
        }
    }

    /// The sole trust, when there is exactly one.
    ///
    /// The single-tenant path stays as direct as it was; multi-tenant callers
    /// iterate [`Verifier::trusts`].
    #[must_use]
    pub fn sole(&self) -> Option<&Trust> {
        match self.trusts.as_slice() {
            [only] => Some(only),
            _ => None,
        }
    }

    /// How many named keys are currently held, across every issuer.
    #[must_use]
    pub fn key_count(&self) -> usize {
        self.trusts.iter().map(Trust::key_count).sum()
    }
}

/// The `iss` a token claims, read **without** checking the signature.
///
/// Used only to decide which trusted key set must have signed it. That is safe
/// and is how every multi-issuer verifier works: naming another tenant's issuer
/// selects that tenant's keys, which the caller cannot sign for. The value is
/// never trusted — [`Verifier::verify`] compares the *verified* `iss` against
/// the trust that accepted it before returning.
fn unverified_issuer(token: &str) -> Option<String> {
    use base64::Engine as _;

    #[derive(serde::Deserialize)]
    struct JustIssuer {
        iss: String,
    }

    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice::<JustIssuer>(&bytes)
        .ok()
        .map(|j| j.iss)
}

/// Read a JWKS document and parse it into a key set.
///
/// # Errors
///
/// A description of what went wrong, for a log. Callers keep their existing
/// keys on an error rather than acting on it.
pub async fn fetch_keys(url: &str, accepted: &[Signing]) -> Result<KeySet, String> {
    let body = reqwest::get(url)
        .await
        .map_err(|e| format!("could not fetch {url}: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("could not read {url}: {e}"))?;
    KeySet::from_jwks(&body, accepted).map_err(|e| format!("{url}: {e}"))
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
    /// The token names an `iss` this server holds no keys for.
    ///
    /// Distinct from [`VerifyError::UnknownKey`]: the key set was never
    /// consulted, because there is no key set for that issuer to consult.
    UnknownIssuer,
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
            VerifyError::UnknownIssuer => {
                f.write_str("the token names an issuer this server holds no keys for")
            }
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

        // Which issuer's keys must have signed this. Read from the unverified
        // payload, because `iss` is inside the signature it selects the key
        // for — there is no other order available. Naming somebody else's
        // issuer therefore selects *their* keys, which is a harder problem for
        // a forger than the one they started with, not an easier one.
        let trust = self.trust_for(token).ok_or(VerifyError::UnknownIssuer)?;

        let keys = trust.keys.read().unwrap_or_else(|e| e.into_inner());

        // The header is checked against configuration and never consulted to
        // decide what to do. This is the whole defence against algorithm
        // confusion, including `alg: none`.
        let signing = keys
            .accepted
            .iter()
            .copied()
            .find(|s| s.algorithm() == header.alg)
            .ok_or(VerifyError::UnacceptableAlgorithm)?;

        let key = keys
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

        // The signature checked out, so `iss` is now trustworthy — and is
        // checked again against the trust that accepted it. The first read was
        // unverified, and a check that relies on an unverified value is not a
        // check. This is what makes the issuer a boundary rather than a label.
        if !trust.issuer.is_empty() && data.claims.iss != trust.issuer {
            return Err(VerifyError::Claims(TokenError::WrongIssuer));
        }

        data.claims
            .validate(document_key, &self.policy, now_secs)
            .map_err(VerifyError::Claims)?;
        Ok(data.claims)
    }
}

#[cfg(test)]
mod tests;
