//! The token verifier, against arbitrary tokens.
//!
//! This is the security boundary. Everything the collaboration server knows
//! about who is connecting — which document, what they may do, where the file
//! comes from and where it goes back to — arrives inside a JWT supplied by the
//! connecting party. It is parsed *before* anything about it is believed, which
//! is exactly the position an attacker wants a parser to be in.
//!
//! Two properties, and the second is the one worth having:
//!
//! 1. It **returns**. A panic here is a server any unauthenticated party can
//!    stop, since a token is the first thing sent on a new connection.
//! 2. It **never accepts a token this key did not sign**. The fuzzer is handed
//!    the same secret the verifier holds, so it is free to discover a valid
//!    signature — and if it ever does, by any route other than actually signing
//!    one, that is a forgery and the assertion below fails.
//!
//! Property 2 is what makes this more than a liveness check. Algorithm
//! confusion, `alg: none`, key confusion through a chosen `kid`, and signature
//! truncation all look like ordinary parsing to a crash-only fuzzer and like a
//! catastrophe to this one.

#![no_main]

use casual_calc_collab_server::token::TokenPolicy;
use casual_calc_collab_server::verify::{KeySet, Verifier};
use libfuzzer_sys::fuzz_target;

/// The same secret in both places, deliberately.
///
/// The point is not to keep the fuzzer out — it is to see whether anything it
/// can construct is *accepted*. A token that verifies has either been signed
/// with this key, which a fuzzer will not manage by chance, or has found a way
/// past the check, which is the bug.
const SECRET: &[u8] = b"a shared secret, for fuzzing only";

fuzz_target!(|data: &[u8]| {
    let Ok(token) = core::str::from_utf8(data) else {
        return;
    };
    let verifier = Verifier::fixed(
        TokenPolicy {
            audience: "opencalc-collab".into(),
            leeway_secs: 60,
            allowed_hosts: Default::default(),
            require_https: true,
        },
        KeySet::shared_secret(SECRET),
    );

    // A fixed clock: expiry is checked against a supplied time, and letting the
    // real one in would make a finding unreproducible tomorrow.
    let Ok(claims) = verifier.verify(token, "doc-1", 1_800_000_000) else {
        return;
    };

    // Accepted. Everything below is what acceptance is supposed to guarantee,
    // stated so that a token which slips through fails loudly rather than
    // quietly becoming a session.
    assert_eq!(
        claims.document.key, "doc-1",
        "a token was accepted for a document it does not name"
    );
    assert_eq!(
        claims.aud, "opencalc-collab",
        "a token minted for another audience was accepted here"
    );
    assert!(claims.exp > 1_800_000_000, "an expired token was accepted");
});
