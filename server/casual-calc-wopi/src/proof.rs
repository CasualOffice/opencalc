//! WOPI proof keys: proving a request came from *this* editor.
//!
//! A WOPI host hands the browser a `WOPISrc` and an access token, and this
//! service then calls back to the host's REST endpoints with that token. The
//! token alone is a bearer credential: anyone who obtains one can make the same
//! calls. Proof keys are the second factor — every request carries a signature
//! over the token, the URL and a timestamp, made with a private key whose
//! public half the host reads out of this service's discovery document. A
//! replayed or forged call fails the signature even with a valid token.
//!
//! # Direction, because it is the thing that is easy to get backwards
//!
//! This service is the WOPI **client**. It *signs*; the host *verifies*. So
//! there is no incoming request here to reject — the observable behaviour is
//! that a host which checks proof keys accepts our calls, and the way to test
//! that without SharePoint is to verify our own signature exactly as the host
//! would, from the key we actually publish. That is what the tests do.
//!
//! # Optional, and off unless configured
//!
//! No host currently requires this, so a service with no key configured sends
//! no proof headers and behaves as before. Signing with a key the discovery
//! document does not advertise would be worse than not signing at all: the host
//! would reject every request.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};

/// The private key this service signs with, plus the public halves a host needs.
pub struct ProofKeys {
    key: RsaKeyPair,
    /// Big-endian modulus, as published and as a verifier consumes it.
    modulus: Vec<u8>,
    /// Big-endian public exponent.
    exponent: Vec<u8>,
    rng: SystemRandom,
}

impl std::fmt::Debug for ProofKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the key itself, and not by accident: a `#[derive(Debug)]` here
        // would put private key material into any log line that formats the
        // configuration.
        f.debug_struct("ProofKeys")
            .field("modulus_bits", &(self.modulus.len() * 8))
            .finish_non_exhaustive()
    }
}

impl ProofKeys {
    /// Load from a PKCS#8 DER private key.
    ///
    /// # Errors
    ///
    /// If the bytes are not a PKCS#8 RSA key ring will accept, or the public
    /// key inside it cannot be read.
    pub fn from_pkcs8(der: &[u8]) -> Result<Self, String> {
        let key = RsaKeyPair::from_pkcs8(der)
            .map_err(|e| format!("not a usable PKCS#8 RSA private key: {e}"))?;
        let (modulus, exponent) = rsa_public_parts(key.public().as_ref())?;
        Ok(Self {
            key,
            modulus,
            exponent,
            rng: SystemRandom::new(),
        })
    }

    /// The modulus, base64, for the discovery document.
    #[must_use]
    pub fn modulus_b64(&self) -> String {
        B64.encode(&self.modulus)
    }

    /// The public exponent, base64, for the discovery document.
    #[must_use]
    pub fn exponent_b64(&self) -> String {
        B64.encode(&self.exponent)
    }

    /// Sign one request. Returns the `X-WOPI-Proof` value.
    ///
    /// # Errors
    ///
    /// If the signing operation fails, which for a key that loaded means the
    /// random source did.
    pub fn sign(&self, token: &str, url: &str, ticks: i64) -> Result<String, String> {
        let message = signed_payload(token, url, ticks);
        let mut signature = vec![0u8; self.key.public().modulus_len()];
        self.key
            .sign(&RSA_PKCS1_SHA256, &self.rng, &message, &mut signature)
            .map_err(|_| "could not sign the WOPI proof".to_owned())?;
        Ok(B64.encode(&signature))
    }
}

/// The bytes a WOPI proof signs, in the order MS-WOPI specifies.
///
/// Length-prefixed rather than concatenated, and that is load-bearing: without
/// the lengths, a token ending in the first characters of a URL would produce
/// the same bytes as a shorter token and a longer URL, so one valid signature
/// would authorise a request nobody signed.
///
/// **The URL is upper-cased.** Hosts compare against the upper-cased form, so
/// signing the original case produces a signature that verifies nowhere.
#[must_use]
pub fn signed_payload(token: &str, url: &str, ticks: i64) -> Vec<u8> {
    let url = url.to_uppercase();
    let mut out = Vec::with_capacity(token.len() + url.len() + 20);
    let mut push = |bytes: &[u8]| {
        // `u32`, big-endian: the wire is .NET's `int32`, not a native-endian
        // `usize`.
        out.extend_from_slice(&u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_be_bytes());
        out.extend_from_slice(bytes);
    };
    push(token.as_bytes());
    push(url.as_bytes());
    push(&ticks.to_be_bytes());
    out
}

/// The current time as .NET ticks — 100-nanosecond intervals since 0001-01-01.
///
/// The unit is not ours to choose: the host re-derives the same payload from
/// the `X-WOPI-TimeStamp` header, so a value in any other unit produces bytes
/// that do not match and a signature that never verifies.
#[must_use]
pub fn ticks_now() -> i64 {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    // Seconds from 0001-01-01 to 1970-01-01, which is where .NET counts from.
    const EPOCH_OFFSET_SECS: u64 = 62_135_596_800;
    let ticks = (since_epoch.as_secs() + EPOCH_OFFSET_SECS) * 10_000_000
        + u64::from(since_epoch.subsec_nanos()) / 100;
    i64::try_from(ticks).unwrap_or(i64::MAX)
}

/// Pull `(modulus, exponent)` out of a DER `RSAPublicKey`.
///
/// `ring` hands back the DER and does not expose the two integers, and the
/// discovery document has to publish them separately. This is the whole of
/// `RSAPublicKey ::= SEQUENCE { modulus INTEGER, publicExponent INTEGER }` and
/// nothing else, so it is a fixed shape rather than a DER parser.
///
/// Not trusted to be right by inspection: the tests verify a real signature
/// using *these* bytes, so a mis-parse cannot pass.
fn rsa_public_parts(der: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut p = Der(der);
    let mut seq = Der(p.tagged(0x30)?);
    let modulus = seq.tagged(0x02)?;
    let exponent = seq.tagged(0x02)?;
    // DER signs its integers, so a modulus with a high top bit carries a
    // leading zero byte. A host expects the unsigned big-endian number, and a
    // stray zero changes the value it reconstructs.
    let trim = |b: &[u8]| b.strip_prefix(&[0u8]).unwrap_or(b).to_vec();
    Ok((trim(modulus), trim(exponent)))
}

/// Just enough DER to walk a two-integer sequence.
struct Der<'a>(&'a [u8]);

impl<'a> Der<'a> {
    /// The contents of the next element, which must carry `tag`.
    fn tagged(&mut self, tag: u8) -> Result<&'a [u8], String> {
        let (&first, rest) = self.0.split_first().ok_or("truncated DER")?;
        if first != tag {
            return Err(format!("expected DER tag {tag:#04x}, found {first:#04x}"));
        }
        let (&len_byte, rest) = rest.split_first().ok_or("truncated DER length")?;
        let (len, rest) = if len_byte & 0x80 == 0 {
            (usize::from(len_byte), rest)
        } else {
            // Long form: the low bits count the length's own bytes.
            let count = usize::from(len_byte & 0x7f);
            if count == 0 || count > 4 {
                return Err("unsupported DER length".to_owned());
            }
            let (bytes, rest) = rest.split_at_checked(count).ok_or("truncated DER length")?;
            (
                bytes
                    .iter()
                    .fold(0usize, |acc, &b| (acc << 8) | usize::from(b)),
                rest,
            )
        };
        let (body, rest) = rest.split_at_checked(len).ok_or("DER runs past its end")?;
        self.0 = rest;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use ring::signature::{RSA_PKCS1_2048_8192_SHA256, RsaPublicKeyComponents};

    use super::*;

    /// A real 2048-bit key, so the sizes and the DER shapes are the ones a host
    /// would actually be handed. Generated once and committed; it signs nothing
    /// outside these tests.
    const TEST_KEY: &[u8] = include_bytes!("../tests/fixtures/proof-test-key.pkcs8.der");

    fn keys() -> ProofKeys {
        ProofKeys::from_pkcs8(TEST_KEY).expect("the fixture is a usable PKCS#8 RSA key")
    }

    /// **A host verifies our signature using the key we publish.**
    ///
    /// This is the whole feature, and the only honest way to test it without
    /// SharePoint: this service is the WOPI *client*, so it signs and never
    /// verifies, and "we produced a signature" proves nothing on its own. The
    /// check has to close the loop — rebuild the payload from the values a host
    /// receives, and verify with the modulus and exponent the *discovery
    /// document advertises* rather than with the key pair in hand.
    ///
    /// It also proves the DER parse: if `rsa_public_parts` returned the wrong
    /// bytes, these components could not verify anything.
    #[test]
    fn a_host_can_verify_our_proof_with_the_published_modulus_and_exponent() {
        let keys = keys();
        let (token, url, ticks) = (
            "a-token",
            "https://host.example/wopi/files/42",
            638_000_000_000_000_000i64,
        );

        let signature = keys.sign(token, url, ticks).unwrap();

        // Exactly what a host has: two base64 strings out of the discovery XML.
        let modulus = B64.decode(keys.modulus_b64()).unwrap();
        let exponent = B64.decode(keys.exponent_b64()).unwrap();
        let public = RsaPublicKeyComponents {
            n: modulus,
            e: exponent,
        };

        public
            .verify(
                &RSA_PKCS1_2048_8192_SHA256,
                &signed_payload(token, url, ticks),
                &B64.decode(&signature).unwrap(),
            )
            .expect("the published key does not verify the signature this service sends");
    }

    /// **The signature is over the request, not merely over something.**
    ///
    /// A signature that verifies regardless of token, URL or timestamp is a
    /// constant, and would authorise any request at all.
    #[test]
    fn a_proof_does_not_verify_for_a_different_request() {
        let keys = keys();
        let (token, url, ticks) = (
            "a-token",
            "https://host.example/wopi/files/42",
            638_000_000_000_000_000i64,
        );
        let signature = B64.decode(keys.sign(token, url, ticks).unwrap()).unwrap();

        let public = RsaPublicKeyComponents {
            n: B64.decode(keys.modulus_b64()).unwrap(),
            e: B64.decode(keys.exponent_b64()).unwrap(),
        };
        let verifies = |t: &str, u: &str, k: i64| {
            public
                .verify(
                    &RSA_PKCS1_2048_8192_SHA256,
                    &signed_payload(t, u, k),
                    &signature,
                )
                .is_ok()
        };

        assert!(verifies(token, url, ticks), "the honest case must verify");
        assert!(
            !verifies("another-token", url, ticks),
            "a stolen signature must not carry a different token"
        );
        assert!(
            !verifies(token, "https://host.example/wopi/files/43", ticks),
            "nor a different file"
        );
        assert!(
            !verifies(token, url, ticks + 1),
            "nor a replay at another time"
        );
    }

    /// **The URL is upper-cased before signing.**
    ///
    /// Hosts build the payload from the upper-cased URL. Signing the original
    /// case produces a signature that is valid arithmetic and verifies nowhere
    /// — the kind of thing that passes every local test and fails only against
    /// a real SharePoint.
    #[test]
    fn the_url_is_upper_cased_in_the_payload() {
        let lower = signed_payload("t", "https://Host.Example/Files/42", 7);
        let upper = signed_payload("t", "HTTPS://HOST.EXAMPLE/FILES/42", 7);
        assert_eq!(
            lower, upper,
            "the payload must not depend on the URL's case"
        );
        let bytes = String::from_utf8_lossy(&lower).to_string();
        assert!(bytes.contains("HTTPS://HOST.EXAMPLE"), "got {bytes:?}");
    }

    /// **Each field is length-prefixed, so the boundaries cannot be moved.**
    ///
    /// Concatenated without lengths, a token ending in the first characters of
    /// a URL yields the same bytes as a shorter token and a longer URL — one
    /// signature authorising a request nobody signed.
    #[test]
    fn field_boundaries_cannot_be_shifted_between_token_and_url() {
        let a = signed_payload("abcHTTP://X", "/Y", 1);
        let b = signed_payload("abc", "HTTP://X/Y", 1);
        assert_ne!(
            a, b,
            "two different (token, url) splits produced identical signed bytes"
        );
    }

    /// **Ticks are .NET's epoch, not Unix's.**
    ///
    /// The host re-derives the payload from the timestamp header, so the unit
    /// and the epoch are part of the contract rather than an internal choice.
    /// A Unix-epoch value is about 62 billion seconds short, and every
    /// signature would simply fail.
    #[test]
    fn ticks_count_hundred_nanoseconds_since_year_one() {
        let ticks = ticks_now();
        // 2020-01-01 and 2100-01-01 in .NET ticks: a wide bracket, which is the
        // point — it catches a wrong *epoch* or a wrong *unit*, and is not a
        // clock assertion that rots.
        assert!(
            (637_100_000_000_000_000..=662_700_000_000_000_000).contains(&ticks),
            "ticks {ticks} is not a plausible .NET tick count — wrong epoch or unit"
        );
    }

    /// A key that is not a key is refused, rather than panicking at the first
    /// request.
    #[test]
    fn rubbish_is_not_accepted_as_a_key() {
        assert!(ProofKeys::from_pkcs8(b"not a key").is_err());
        assert!(ProofKeys::from_pkcs8(&[]).is_err());
    }
}
