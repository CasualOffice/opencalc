use super::*;
use crate::wopi::FileInfo;

fn config() -> Config {
    Config {
        proof_key_path: None,
        bind: "127.0.0.1:0".into(),
        public_url: "https://calc.example".into(),
        internal_url: "http://wopi:8090".into(),
        collab_url: "wss://calc.example/collab".into(),
        editor_url: "/editor/editor.html".into(),
        secret: "0123456789abcdef".into(),
        audience: "opencalc-collab".into(),
        allowed_hosts: ["nc.example".to_owned()].into_iter().collect(),
        allow_plain: false,
        max_sessions: 10,
        session_ttl_ms: 3_600_000,
        max_document_bytes: 1024,
        brand: crate::discovery::Brand::default(),
    }
}

fn session(src: &str, can_write: bool) -> Session {
    Session::from(
        src.to_owned(),
        "host-token".to_owned(),
        &FileInfo {
            base_file_name: "Q3.xlsx".to_owned(),
            user_friendly_name: Some("Ada".to_owned()),
            user_id: Some("u-7".to_owned()),
            user_can_write: can_write,
            supports_locks: true,
            supports_update: true,
        },
        casual_calc_sdk::SessionFormat::Xlsx,
        0,
    )
}

/// **Two people opening the same file join the same session.**
///
/// Each arrives from the host with their own access token and gets their own
/// adapter session id. Keying the collaboration session on that id would put
/// them in separate documents that each save over the other — co-editing
/// silently degraded to last-writer-wins, which is the one failure the whole
/// server exists to prevent.
#[test]
fn the_session_key_is_the_file_not_the_visitor() {
    let config = config();
    let ada = session("https://nc.example/wopi/files/7?access_token=aaa", true);
    let bob = session("https://nc.example/wopi/files/7?access_token=bbb", true);

    let ada = claims_for(&config, &ada, "session-1", 0);
    let bob = claims_for(&config, &bob, "session-2", 0);

    assert_eq!(
        ada.document.key, bob.document.key,
        "same file, same session"
    );
    assert!(!ada.document.key.contains("aaa"), "{}", ada.document.key);

    // A different file is a different session.
    let other = session("https://nc.example/wopi/files/8", true);
    assert_ne!(
        claims_for(&config, &other, "s", 0).document.key,
        ada.document.key
    );
}

/// **Both URLs point back at this service, never at the WOPI host.**
///
/// The host's access token is a credential for somebody else's file store. If
/// it reached the collaboration server it would be in that server's memory, its
/// logs, and — clustered — its shared log, which is three more places for it to
/// leak from than the design needs.
#[test]
fn the_server_is_pointed_at_this_service_and_not_at_the_host() {
    let config = config();
    let claims = claims_for(
        &config,
        &session("https://nc.example/wopi/files/7?access_token=secret", true),
        "sid-1",
        0,
    );

    assert_eq!(claims.document.url, "http://wopi:8090/wopi/content/sid-1");
    assert_eq!(claims.callback.url, "http://wopi:8090/wopi/callback/sid-1");
    assert_eq!(claims.callback.kind, "url");

    let anywhere = format!("{}{}", claims.document.url, claims.callback.url);
    assert!(
        !anywhere.contains("secret"),
        "the host's token leaked: {anywhere}"
    );
    assert!(!anywhere.contains("nc.example"), "{anywhere}");
}

/// **A read-only file mints a read-only token.**
///
/// The refusal has to be the server's, not the browser's: hiding the toolbar
/// leaves an editable document one console call away.
#[test]
fn read_only_is_enforced_in_the_token() {
    let config = config();
    assert_eq!(
        claims_for(&config, &session("https://nc.example/f/7", true), "s", 0)
            .permissions
            .access,
        "edit"
    );
    assert_eq!(
        claims_for(&config, &session("https://nc.example/f/7", false), "s", 0)
            .permissions
            .access,
        "view"
    );
}

/// **The token does not outlive the session that backs it.**
///
/// Its fetch URL is a session id. Once the session has gone, a token still
/// inside its own expiry names an endpoint that 404s, and the editor reports a
/// missing document rather than an expired one.
#[test]
fn the_token_expires_with_the_session() {
    let mut config = config();
    config.session_ttl_ms = 600_000;
    let claims = claims_for(
        &config,
        &session("https://nc.example/f/7", true),
        "s",
        1_000,
    );
    assert_eq!(claims.exp, 1_000 + 600, "ttl in seconds, from now");
    assert_eq!(claims.iat, 1_000);
}

/// **What is minted is a valid HS256 token the collaboration server's key
/// opens.**
#[test]
fn the_token_verifies_against_the_shared_secret() {
    let config = config();
    let jwt = mint(&config, &session("https://nc.example/f/7", true), "sid", 0);

    let mut rules = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    rules.set_audience(&["opencalc-collab"]);
    // These claims are asserted elsewhere; this test is about the signature.
    rules.validate_exp = false;
    let opened = jsonwebtoken::decode::<Claims>(
        &jwt,
        &jsonwebtoken::DecodingKey::from_secret(config.secret.as_bytes()),
        &rules,
    )
    .expect("the collaboration server's key opens it");
    assert_eq!(opened.claims.iss, "opencalc-wopi");
    assert_eq!(opened.claims.user.name, "Ada");

    // And a different key does not.
    assert!(
        jsonwebtoken::decode::<Claims>(
            &jwt,
            &jsonwebtoken::DecodingKey::from_secret(b"another-secret!!"),
            &rules,
        )
        .is_err()
    );
}
