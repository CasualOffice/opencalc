//! Verifier tests, aimed at the two attacks a JWT verifier exists to survive:
//! being told which algorithm to use, and being told which key.

use jsonwebtoken::{EncodingKey, Header};

use super::*;
use crate::token::{Access, Document, Permissions, User};

const SECRET: &[u8] = b"a shared secret, for development only";

fn claims(document_key: &str) -> Claims {
    Claims {
        iss: "https://host.example".into(),
        aud: "opencalc-collab".into(),
        exp: 2_000,
        iat: Some(1_000),
        nbf: None,
        jti: None,
        user: User {
            id: "u-17".into(),
            name: "Ada".into(),
            email: None,
            avatar_url: None,
            group: None,
            color: None,
        },
        document: Document {
            key: document_key.into(),
            id: "file-1".into(),
            title: "Budget.xlsx".into(),
            version: None,
            owner_id: None,
            url: "https://host.example/files/1".into(),
        },
        permissions: Permissions {
            access: Access::Edit,
            download: true,
            print: true,
            copy: true,
        },
        owner: false,
        callback: None,
    }
}

fn policy() -> TokenPolicy {
    TokenPolicy {
        audience: "opencalc-collab".into(),
        leeway_secs: 30,
        allowed_hosts: std::collections::BTreeSet::new(),
        require_https: true,
    }
}

fn verifier() -> Verifier {
    Verifier::fixed(policy(), KeySet::shared_secret(SECRET))
}

/// Sign with the shared secret, as a well-behaved host would.
fn signed(claims: &Claims) -> String {
    jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(SECRET),
    )
    .unwrap()
}

#[test]
fn a_correctly_signed_token_yields_its_claims() {
    let token = signed(&claims("doc-1"));
    let out = verifier().verify(&token, "doc-1", 1_500).unwrap();
    assert_eq!(out.user.id, "u-17");
    assert_eq!(out.permissions.access, Access::Edit);
}

#[test]
fn a_token_signed_with_another_secret_is_refused() {
    let token = jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        &claims("doc-1"),
        &EncodingKey::from_secret(b"not the secret"),
    )
    .unwrap();
    assert_eq!(
        verifier().verify(&token, "doc-1", 1_500),
        Err(VerifyError::BadSignature)
    );
}

#[test]
fn an_unsigned_token_is_refused_however_confidently_it_asks() {
    // `alg: none` is the oldest JWT attack there is: a token that declares it
    // needs no signature, verified by a server that believes the declaration.
    // The accepted algorithms are configuration, so the header cannot choose.
    let unsigned = format!(
        "{}.{}.",
        base64url(br#"{"alg":"none","typ":"JWT"}"#),
        base64url(serde_json::to_string(&claims("doc-1")).unwrap().as_bytes())
    );
    // Refused twice over, which is the point: `decode_header` will not even
    // parse `none` into an algorithm, so it is rejected as malformed before
    // reaching the accepted-algorithms check that would also have refused it.
    // The assertion is on the property rather than on which layer caught it —
    // pinning the layer would make a defence-in-depth change look like a
    // regression.
    let outcome = verifier().verify(&unsigned, "doc-1", 1_500);
    assert!(
        outcome.is_err(),
        "an unsigned token was accepted: {outcome:?}"
    );

    // And the check that would have caught it is genuinely there: a token
    // signed with an algorithm the server does not accept is refused for that
    // reason and not by accident.
    assert_eq!(
        Verifier::fixed(
            policy(),
            KeySet {
                keys: std::collections::BTreeMap::new(),
                solitary: Some(DecodingKey::from_secret(SECRET)),
                accepted: vec![],
            }
        )
        .verify(&signed(&claims("doc-1")), "doc-1", 1_500),
        Err(VerifyError::UnacceptableAlgorithm)
    );
}

#[test]
fn a_token_naming_an_algorithm_the_server_does_not_accept_is_refused() {
    // The other half of algorithm confusion: a verifier configured for RS256
    // must not accept an HS256 token, or the *public* key — which is published
    // — becomes usable as an HMAC secret by anyone who has it.
    let rs_only = Verifier::fixed(
        policy(),
        KeySet {
            keys: std::collections::BTreeMap::new(),
            solitary: Some(DecodingKey::from_secret(SECRET)),
            accepted: vec![Signing::Rs256],
        },
    );
    let token = signed(&claims("doc-1"));
    assert_eq!(
        rs_only.verify(&token, "doc-1", 1_500),
        Err(VerifyError::UnacceptableAlgorithm)
    );
}

#[test]
fn an_unknown_kid_is_refused_rather_than_falling_back_to_another_key() {
    // "Try everything until one works" is how a key retired for being
    // compromised goes on working.
    let keys = KeySet {
        keys: [("current".to_owned(), DecodingKey::from_secret(SECRET))]
            .into_iter()
            .collect(),
        solitary: None,
        accepted: vec![Signing::Hs256],
    };
    let verifier = Verifier::fixed(policy(), keys);

    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some("retired".into());
    let token =
        jsonwebtoken::encode(&header, &claims("doc-1"), &EncodingKey::from_secret(SECRET)).unwrap();
    assert_eq!(
        verifier.verify(&token, "doc-1", 1_500),
        Err(VerifyError::UnknownKey)
    );

    // And the right kid still works, so the refusal is about the key and not
    // about kids in general.
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some("current".into());
    let token =
        jsonwebtoken::encode(&header, &claims("doc-1"), &EncodingKey::from_secret(SECRET)).unwrap();
    assert!(verifier.verify(&token, "doc-1", 1_500).is_ok());
}

#[test]
fn the_claims_are_checked_after_the_signature_and_against_the_supplied_clock() {
    let token = signed(&claims("doc-1"));
    let v = verifier();

    // Correctly signed and expired.
    assert_eq!(
        v.verify(&token, "doc-1", 9_000),
        Err(VerifyError::Claims(crate::token::TokenError::Expired))
    );
    // Correctly signed and for another document.
    assert_eq!(
        v.verify(&token, "doc-2", 1_500),
        Err(VerifyError::Claims(crate::token::TokenError::WrongDocument))
    );
}

#[test]
fn rubbish_is_refused_without_panicking() {
    let v = verifier();
    for token in ["", "not a jwt", "a.b.c", "....", "eyJ", "a.b"] {
        assert!(v.verify(token, "doc-1", 1_500).is_err(), "for {token:?}");
    }
}

#[test]
fn every_failure_tells_the_client_the_same_thing() {
    // A caller who can tell malformed from wrong-key from bad-signature from
    // expired has an oracle for probing the difference.
    use casual_calc_transaction::protocol::Refusal;
    for e in [
        VerifyError::Malformed,
        VerifyError::UnacceptableAlgorithm,
        VerifyError::UnknownKey,
        VerifyError::BadSignature,
        VerifyError::Claims(crate::token::TokenError::Expired),
    ] {
        assert_eq!(e.refusal(), Refusal::NotAuthorised);
        assert!(!e.to_string().is_empty(), "the operator still gets detail");
    }
}

// --- JWKS parsing ----------------------------------------------------------

/// A minimal RSA JWKS, as an integrator's `jwks_uri` would serve.
const JWKS: &str = r#"{
  "keys": [
    {
      "kty": "RSA", "use": "sig", "alg": "RS256", "kid": "key-1",
      "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
      "e": "AQAB"
    }
  ]
}"#;

#[test]
fn a_jwks_document_becomes_a_key_set_indexed_by_kid() {
    let keys = KeySet::from_jwks(JWKS.as_bytes(), &[Signing::Rs256]).unwrap();
    assert_eq!(keys.len(), 1);
    assert!(keys.select(Some("key-1")).is_some());
    assert!(keys.select(Some("key-2")).is_none());
    // A single key is used when a token names no kid, which is the common
    // shape for a host that has never rotated.
    assert!(keys.select(None).is_some());
}

#[test]
fn keys_this_server_cannot_use_are_skipped_rather_than_refused() {
    // A real JWKS carries encryption keys and algorithms beside the signing
    // one. Refusing the whole set over a key nobody was going to use would make
    // an ordinary key set unusable.
    let mixed = JWKS.replace(
        "\"keys\": [",
        "\"keys\": [ {\"kty\":\"oct\",\"kid\":\"symmetric\",\"k\":\"c2VjcmV0\"},",
    );
    let keys = KeySet::from_jwks(mixed.as_bytes(), &[Signing::Rs256]).unwrap();
    assert!(
        keys.select(Some("key-1")).is_some(),
        "the usable one survives"
    );
}

#[test]
fn a_key_set_with_nothing_usable_is_an_error_rather_than_an_empty_verifier() {
    // Silently ending up with no keys would refuse every token with
    // `UnknownKey`, which reads as a client problem and is a configuration one.
    assert!(matches!(
        KeySet::from_jwks(br#"{"keys":[]}"#, &[Signing::Rs256]),
        Err(KeyError::NoUsableKeys)
    ));
    assert!(matches!(
        KeySet::from_jwks(b"not json", &[Signing::Rs256]),
        Err(KeyError::Malformed(_))
    ));
}

#[test]
fn a_key_set_never_prints_its_keys() {
    // An HS256 "key" is a secret, and a debug line is the most casual way for
    // one to reach a log.
    let rendered = format!("{:?}", KeySet::shared_secret(SECRET));
    assert!(!rendered.contains("secret"), "in: {rendered}");
    assert!(rendered.contains("KeySet"));
}

fn base64url(bytes: &[u8]) -> String {
    // Only the test needs this; the library does its own decoding.
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..chunk.len() + 1 {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 0x3f) as usize] as char);
        }
    }
    out
}

/// **A key set can be replaced while the server runs, because keys rotate.**
///
/// `read_verifier` fetched the JWKS once and moved it into the config for the
/// life of the process, while ADR-014 and docs/59 state the opposite as a
/// decided property: *"they publish a new key, the server picks it up at the
/// next fetch, and no coordinated restart is needed."*
///
/// The consequence was total rather than partial. `select` refuses an unknown
/// `kid` outright rather than falling back — which is correct, and is what
/// makes a stale set fatal: an integrator rotating on schedule locks **every**
/// user out of **every** document until an operator restarts every node, and
/// the client sees the same `NotAuthorised` a bad token gets.
#[test]
fn installing_a_new_key_set_lets_a_rotated_key_in() {
    const NEXT: &[u8] = b"the key published in this morning's rotation";

    let only_current = KeySet {
        keys: [("k1".to_owned(), DecodingKey::from_secret(SECRET))]
            .into_iter()
            .collect(),
        solitary: None,
        accepted: vec![Signing::Hs256],
    };
    let verifier = Verifier::fixed(policy(), only_current);

    let signed_with_next = |kid: &str| {
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some(kid.to_owned());
        jsonwebtoken::encode(&header, &claims("doc-1"), &EncodingKey::from_secret(NEXT)).unwrap()
    };
    let token = signed_with_next("k2");

    assert_eq!(
        verifier.verify(&token, "doc-1", 1_500),
        Err(VerifyError::UnknownKey),
        "the new key is not held yet, so this is refused — correctly"
    );

    // The integrator publishes k2 beside k1; the server re-reads.
    verifier.sole().expect("one tenant").install(KeySet {
        keys: [
            ("k1".to_owned(), DecodingKey::from_secret(SECRET)),
            ("k2".to_owned(), DecodingKey::from_secret(NEXT)),
        ]
        .into_iter()
        .collect(),
        solitary: None,
        accepted: vec![Signing::Hs256],
    });

    assert!(
        verifier.verify(&token, "doc-1", 1_500).is_ok(),
        "after the refresh the rotated key must be accepted"
    );
    assert_eq!(verifier.key_count(), 2);

    // Revocation is the mirror image, and is why a clock is needed as well as
    // the on-demand path: nothing presents a token for a key being withdrawn.
    verifier.sole().expect("one tenant").install(KeySet {
        keys: [("k2".to_owned(), DecodingKey::from_secret(NEXT))]
            .into_iter()
            .collect(),
        solitary: None,
        accepted: vec![Signing::Hs256],
    });
    let old = {
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some("k1".to_owned());
        jsonwebtoken::encode(&header, &claims("doc-1"), &EncodingKey::from_secret(SECRET)).unwrap()
    };
    assert_eq!(
        verifier.verify(&old, "doc-1", 1_500),
        Err(VerifyError::UnknownKey),
        "a withdrawn key stops working without a restart"
    );
}

/// **The on-demand refresh is throttled, because its trigger is attacker-reachable.**
///
/// An unknown `kid` is what a newly published key looks like — and also what
/// anyone can produce by inventing one. Without a throttle, every connection
/// attempt becomes a request to the integrator's key endpoint, and this server
/// becomes the thing hammering it.
#[test]
fn an_unknown_kid_cannot_be_used_to_hammer_the_key_endpoint() {
    let verifier = Verifier::refreshing(
        policy(),
        KeySet::shared_secret(SECRET),
        "https://example.invalid/jwks.json".to_owned(),
        vec![Signing::Rs256],
        10_000,
    );
    let source = verifier
        .sole()
        .and_then(Trust::jwks)
        .expect("this one refreshes");

    assert!(source.may_attempt(50_000), "the first attempt runs");
    assert!(!source.may_attempt(50_001), "an immediate second does not");
    assert!(
        !source.may_attempt(59_999),
        "nor does one just inside the interval"
    );
    assert!(source.may_attempt(60_000), "and one after it does");

    // A verifier over a fixed key set has nothing to re-read, and must not
    // pretend otherwise.
    assert!(
        Verifier::fixed(policy(), KeySet::shared_secret(SECRET))
            .sole()
            .and_then(Trust::jwks)
            .is_none()
    );
}

// ---------------------------------------------------------------------------
// Tenant isolation (DEP-10).
//
// One `Verifier` held one key set and never looked at `iss`, so every issuer a
// deployment trusted could mint for every other issuer's documents. Checking
// `iss` against a policy would not have fixed it: with one shared key set the
// claim is a label the minter fills in. The boundary has to be the *binding* —
// a token is checked against the keys of the issuer it names, and no others.
// ---------------------------------------------------------------------------

const TENANT_A: &[u8] = b"tenant a's signing secret, 32+ bytes";
const TENANT_B: &[u8] = b"tenant b's signing secret, 32+ bytes";

/// Two tenants, each with its own key set.
fn tenanted() -> Verifier {
    Verifier::tenanted(
        policy(),
        vec![
            Trust::fixed("https://a.example", KeySet::shared_secret(TENANT_A)),
            Trust::fixed("https://b.example", KeySet::shared_secret(TENANT_B)),
        ],
    )
}

fn claims_from(issuer: &str, document_key: &str) -> Claims {
    Claims {
        iss: issuer.into(),
        ..claims(document_key)
    }
}

fn signed_with(secret: &[u8], claims: &Claims) -> String {
    jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(secret),
    )
    .unwrap()
}

/// **A tenant cannot mint for another tenant's issuer.**
///
/// The whole of `DEP-10` in one assertion. Tenant A holds a real signing key
/// this server trusts, and names tenant B — so every check that looks only at
/// "is this signed by a key we trust" passes. It is refused because naming B
/// selects *B's* keys, which A cannot sign for.
#[test]
fn one_tenant_cannot_mint_for_another() {
    let forged = signed_with(TENANT_A, &claims_from("https://b.example", "doc-1"));
    assert_eq!(
        tenanted().verify(&forged, "doc-1", 1_500),
        Err(VerifyError::BadSignature),
        "a key this server trusts minted a token for a document it does not own"
    );
}

/// And each tenant still works, so the test above cannot pass by refusing
/// everything.
#[test]
fn each_tenant_verifies_against_its_own_keys() {
    let verifier = tenanted();
    for (issuer, secret) in [
        ("https://a.example", TENANT_A),
        ("https://b.example", TENANT_B),
    ] {
        let token = signed_with(secret, &claims_from(issuer, "doc-1"));
        let out = verifier
            .verify(&token, "doc-1", 1_500)
            .unwrap_or_else(|e| panic!("{issuer} could not verify its own token: {e}"));
        assert_eq!(out.iss, issuer);
    }
}

/// An issuer the deployment has never heard of is refused before any key is
/// consulted — rather than being tried against the first tenant's keys.
#[test]
fn an_unknown_issuer_is_refused() {
    let token = signed_with(TENANT_A, &claims_from("https://elsewhere.example", "doc-1"));
    assert_eq!(
        tenanted().verify(&token, "doc-1", 1_500),
        Err(VerifyError::UnknownIssuer)
    );
}

/// A single-tenant deployment can still pin its issuer. One key set, so the
/// signature proves nothing about *who* the token was minted for; the verified
/// `iss` is what refuses it.
#[test]
fn a_pinned_issuer_refuses_a_token_minted_for_someone_else() {
    let verifier = Verifier::tenanted(
        policy(),
        vec![Trust::fixed(
            "https://host.example",
            KeySet::shared_secret(SECRET),
        )],
    );

    let ours = signed(&claims_from("https://host.example", "doc-1"));
    assert!(
        verifier.verify(&ours, "doc-1", 1_500).is_ok(),
        "the pinned issuer's own token was refused"
    );

    // Same key, same signature check, different `iss`.
    let theirs = signed(&claims_from("https://other.example", "doc-1"));
    assert_eq!(
        verifier.verify(&theirs, "doc-1", 1_500),
        Err(VerifyError::Claims(TokenError::WrongIssuer))
    );
}

/// Unpinned stays unpinned: a deployment that names no issuer accepts any, as
/// it always has. Guards against the fix being a silent breaking change for
/// every existing single-tenant install.
#[test]
fn an_unnamed_issuer_still_accepts_any() {
    let token = signed(&claims_from("https://anything.example", "doc-1"));
    assert!(verifier().verify(&token, "doc-1", 1_500).is_ok());
}

/// The refresh path and the verify path must pick the same tenant, or an
/// unknown `kid` re-reads keys that were never going to help.
#[test]
fn the_refresh_path_picks_the_same_tenant_as_verification() {
    let verifier = tenanted();
    let token = signed_with(TENANT_B, &claims_from("https://b.example", "doc-1"));
    assert_eq!(
        verifier.trust_for(&token).map(|t| t.issuer.as_str()),
        Some("https://b.example")
    );
}
