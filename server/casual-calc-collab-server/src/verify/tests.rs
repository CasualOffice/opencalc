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
    Verifier {
        policy: policy(),
        keys: KeySet::shared_secret(SECRET),
    }
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
        Verifier {
            policy: policy(),
            keys: KeySet {
                keys: std::collections::BTreeMap::new(),
                solitary: Some(DecodingKey::from_secret(SECRET)),
                accepted: vec![],
            },
        }
        .verify(&signed(&claims("doc-1")), "doc-1", 1_500),
        Err(VerifyError::UnacceptableAlgorithm)
    );
}

#[test]
fn a_token_naming_an_algorithm_the_server_does_not_accept_is_refused() {
    // The other half of algorithm confusion: a verifier configured for RS256
    // must not accept an HS256 token, or the *public* key — which is published
    // — becomes usable as an HMAC secret by anyone who has it.
    let rs_only = Verifier {
        policy: policy(),
        keys: KeySet {
            keys: std::collections::BTreeMap::new(),
            solitary: Some(DecodingKey::from_secret(SECRET)),
            accepted: vec![Signing::Rs256],
        },
    };
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
    let verifier = Verifier {
        policy: policy(),
        keys,
    };

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
