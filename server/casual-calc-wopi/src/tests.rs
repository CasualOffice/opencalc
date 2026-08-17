//! The handshake, end to end, against a stub that behaves like a WOPI host.
//!
//! These drive the real router over a real socket, because what is being
//! asserted is the sequence of requests a host sees — which is the only part a
//! Nextcloud administrator can observe, and the only part that decides whether
//! this works at all.

use super::*;
use axum::http::HeaderMap;
use std::sync::Mutex;

/// A WOPI host: it holds one file, and it locks.
#[derive(Clone, Default)]
struct StubHost {
    /// What the file contains. Starts as a marker so a fetch can be recognised.
    content: Arc<Mutex<Vec<u8>>>,
    /// The lock id currently held, if any.
    lock: Arc<Mutex<Option<String>>>,
    /// Every override the host was asked to perform, in order.
    calls: Arc<Mutex<Vec<String>>>,
    /// Whether `PutFile` should fail.
    refuse_writes: Arc<Mutex<bool>>,
    /// What `CheckFileInfo` claims.
    can_write: Arc<Mutex<bool>>,
}

impl StubHost {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
    fn content(&self) -> Vec<u8> {
        self.content.lock().unwrap().clone()
    }
    fn locked(&self) -> Option<String> {
        self.lock.lock().unwrap().clone()
    }
}

/// Start the stub, returning the `WOPISrc` of its one file.
async fn wopi_host() -> (String, StubHost) {
    let state = StubHost::default();
    *state.content.lock().unwrap() = b"ORIGINAL".to_vec();
    *state.can_write.lock().unwrap() = true;

    let app = Router::new()
        .route(
            "/wopi/files/1",
            get(
                |State(s): State<StubHost>, Query(q): Query<std::collections::HashMap<String, String>>| async move {
                    s.calls.lock().unwrap().push("CheckFileInfo".to_owned());
                    if q.get("access_token").map(String::as_str) != Some("host-token") {
                        return (StatusCode::UNAUTHORIZED, String::new()).into_response();
                    }
                    Json(serde_json::json!({
                        "BaseFileName": "Q3.xlsx",
                        "Size": s.content.lock().unwrap().len(),
                        "UserCanWrite": *s.can_write.lock().unwrap(),
                        "SupportsLocks": true,
                        "SupportsUpdate": true,
                        "UserFriendlyName": "Ada",
                        "UserId": "u-7",
                    }))
                    .into_response()
                },
            )
            .post(
                |State(s): State<StubHost>, headers: HeaderMap| async move {
                    let over = headers
                        .get("X-WOPI-Override")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_owned();
                    let asked = headers
                        .get("X-WOPI-Lock")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_owned();
                    s.calls.lock().unwrap().push(over.clone());
                    let mut held = s.lock.lock().unwrap();
                    match over.as_str() {
                        "LOCK" => match held.clone() {
                            Some(existing) if existing != asked => (
                                StatusCode::CONFLICT,
                                [("X-WOPI-Lock", existing)],
                            )
                                .into_response(),
                            _ => {
                                *held = Some(asked);
                                StatusCode::OK.into_response()
                            }
                        },
                        "REFRESH_LOCK" | "UNLOCK" => {
                            if held.as_deref() != Some(asked.as_str()) {
                                return (
                                    StatusCode::CONFLICT,
                                    [("X-WOPI-Lock", held.clone().unwrap_or_default())],
                                )
                                    .into_response();
                            }
                            if over == "UNLOCK" {
                                *held = None;
                            }
                            StatusCode::OK.into_response()
                        }
                        _ => StatusCode::NOT_IMPLEMENTED.into_response(),
                    }
                },
            ),
        )
        .route(
            "/wopi/files/1/contents",
            get(|State(s): State<StubHost>| async move {
                s.calls.lock().unwrap().push("GetFile".to_owned());
                s.content.lock().unwrap().clone()
            })
            .post(
                |State(s): State<StubHost>, headers: HeaderMap, body: axum::body::Bytes| async move {
                    s.calls.lock().unwrap().push("PutFile".to_owned());
                    if *s.refuse_writes.lock().unwrap() {
                        return StatusCode::INTERNAL_SERVER_ERROR;
                    }
                    let asked = headers
                        .get("X-WOPI-Lock")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default();
                    // A real host refuses a write that does not carry its lock.
                    if s.lock.lock().unwrap().as_deref() != Some(asked) {
                        return StatusCode::CONFLICT;
                    }
                    *s.content.lock().unwrap() = body.to_vec();
                    StatusCode::OK
                },
            ),
        )
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}/wopi/files/1"), state)
}

/// Start the adapter, returning its base URL.
async fn adapter() -> String {
    let config = Config {
        bind: "127.0.0.1:0".into(),
        public_url: "http://calc.example".into(),
        internal_url: "http://wopi:8090".into(),
        collab_url: "ws://calc.example/collab".into(),
        editor_url: "/editor/editor.html".into(),
        secret: "0123456789abcdef".into(),
        audience: "opencalc-collab".into(),
        allowed_hosts: ["127.0.0.1".to_owned()].into_iter().collect(),
        // The stub is plain HTTP on loopback, which is exactly the case the
        // setting documents and is not defaulted on.
        allow_plain: true,
        max_sessions: 8,
        session_ttl_ms: 3_600_000,
        max_document_bytes: 1 << 20,
        brand: discovery::Brand::default(),
    };
    let service = Arc::new(Service {
        host: Host::new(config.max_document_bytes),
        sessions: Sessions::new(config.max_sessions, config.session_ttl_ms),
        config,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router(service)).await;
    });
    format!("http://{addr}")
}

/// Pull the session id out of the page the action URL served.
fn session_id(page: &str) -> String {
    let start = page.find("const SESSION = \"").expect("a session id") + 17;
    let end = page[start..].find('"').expect("a closing quote") + start;
    page[start..end].to_owned()
}

/// **A host opens a file, the editor saves it, and the bytes arrive back.**
///
/// The whole point, in one test: discovery is not enough, and neither is any
/// single request. What makes OpenCalc installable is that this exact sequence
/// works — `CheckFileInfo`, `LOCK`, `GetFile`, `PutFile` — and every one of
/// them carries something the previous one produced.
#[tokio::test]
async fn a_file_opens_edits_and_saves_back_to_the_host() {
    let (src, host) = wopi_host().await;
    let at = adapter().await;
    let client = reqwest::Client::new();

    // 1. The host sends a browser to the action URL.
    let page = client
        .get(format!("{at}/wopi/edit"))
        .query(&[("WOPISrc", src.as_str()), ("access_token", "host-token")])
        .send()
        .await
        .expect("the action URL answered");
    assert!(page.status().is_success(), "{}", page.status());
    let id = session_id(&page.text().await.unwrap());

    // Opening took the lock, so nothing else can write while this is open.
    assert!(host.locked().is_some(), "the file was not locked");

    // 2. The page asks what to start.
    let info: serde_json::Value = client
        .get(format!("{at}/wopi/session/{id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(info["title"], "Q3.xlsx", "the name came from CheckFileInfo");
    assert_eq!(info["editable"], true);
    assert!(!info["token"].as_str().unwrap().is_empty());

    // 3. The collaboration server fetches the document from us.
    let fetched = client
        .get(format!("{at}/wopi/content/{id}"))
        .send()
        .await
        .unwrap();
    assert!(fetched.status().is_success());
    assert_eq!(fetched.bytes().await.unwrap().as_ref(), b"ORIGINAL");

    // 4. And posts the finished package back to us.
    let saved = client
        .post(format!("{at}/wopi/callback/{id}"))
        .body(b"EDITED".to_vec())
        .send()
        .await
        .unwrap();
    assert!(saved.status().is_success(), "{}", saved.status());
    assert_eq!(host.content(), b"EDITED", "the host has the new bytes");

    // 5. Closing releases the lock rather than leaving it to expire.
    client
        .post(format!("{at}/wopi/close/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(host.locked(), None, "the lock outlived the session");

    assert_eq!(
        host.calls(),
        vec!["CheckFileInfo", "LOCK", "GetFile", "PutFile", "UNLOCK"],
        "the sequence a host sees"
    );
}

/// **A `WOPISrc` that is not on the allow-list is refused before any request is
/// made.**
///
/// Refusing after the fetch is not refusing: the request has already been made
/// from inside the perimeter, which is the whole of the attack.
#[tokio::test]
async fn an_unlisted_host_is_never_contacted() {
    let (src, host) = wopi_host().await;
    let at = adapter().await;
    // Same file, addressed by a name that is not on the list.
    let disguised = src.replace("127.0.0.1", "localhost");

    let refused = reqwest::Client::new()
        .get(format!("{at}/wopi/edit"))
        .query(&[
            ("WOPISrc", disguised.as_str()),
            ("access_token", "host-token"),
        ])
        .send()
        .await
        .expect("answered");

    assert_eq!(refused.status(), reqwest::StatusCode::BAD_REQUEST);
    assert!(
        host.calls().is_empty(),
        "the host was contacted anyway: {:?}",
        host.calls()
    );
}

/// **A token the host rejects gets a 401, not a session.**
#[tokio::test]
async fn a_token_the_host_rejects_opens_nothing() {
    let (src, _) = wopi_host().await;
    let at = adapter().await;

    let refused = reqwest::Client::new()
        .get(format!("{at}/wopi/edit"))
        .query(&[("WOPISrc", src.as_str()), ("access_token", "stale")])
        .send()
        .await
        .expect("answered");
    assert_eq!(refused.status(), reqwest::StatusCode::UNAUTHORIZED);
}

/// **A read-only session cannot write, and does not hold a lock.**
///
/// `/wopi/view` is read-only whatever the host says the user may do. Enforcing
/// it here rather than in the browser matters because the callback endpoint is
/// reachable by anything that knows the session id.
#[tokio::test]
async fn a_view_session_is_refused_a_save() {
    let (src, host) = wopi_host().await;
    let at = adapter().await;
    let client = reqwest::Client::new();

    let page = client
        .get(format!("{at}/wopi/view"))
        .query(&[("WOPISrc", src.as_str()), ("access_token", "host-token")])
        .send()
        .await
        .unwrap();
    let id = session_id(&page.text().await.unwrap());
    assert_eq!(host.locked(), None, "a reader took a lock");

    let refused = client
        .post(format!("{at}/wopi/callback/{id}"))
        .body(b"EDITED".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), reqwest::StatusCode::FORBIDDEN);
    assert_eq!(host.content(), b"ORIGINAL", "a reader wrote to the file");
    assert!(!host.calls().contains(&"PutFile".to_owned()));
}

/// **A save the host refuses is reported as a failure.**
///
/// The collaboration server keeps a document whose callback failed and retries;
/// answering `200` to a `PutFile` that did not happen is how an afternoon's
/// work disappears with nothing in any log.
#[tokio::test]
async fn a_refused_save_is_not_reported_as_success() {
    let (src, host) = wopi_host().await;
    let at = adapter().await;
    let client = reqwest::Client::new();

    let page = client
        .get(format!("{at}/wopi/edit"))
        .query(&[("WOPISrc", src.as_str()), ("access_token", "host-token")])
        .send()
        .await
        .unwrap();
    let id = session_id(&page.text().await.unwrap());

    *host.refuse_writes.lock().unwrap() = true;
    let answered = client
        .post(format!("{at}/wopi/callback/{id}"))
        .body(b"EDITED".to_vec())
        .send()
        .await
        .unwrap();

    assert!(
        !answered.status().is_success(),
        "a failed PutFile was reported as saved"
    );
    assert_eq!(host.content(), b"ORIGINAL");
}

/// **A file somebody else is editing opens read-only rather than failing.**
///
/// The bytes are readable and the person asked to look at them. An error page
/// is a worse answer than a read-only document, and refusing the lock is the
/// normal case in an organisation, not an exceptional one.
#[tokio::test]
async fn a_file_locked_elsewhere_still_opens() {
    let (src, host) = wopi_host().await;
    *host.lock.lock().unwrap() = Some("held-by-word".to_owned());
    let at = adapter().await;
    let client = reqwest::Client::new();

    let page = client
        .get(format!("{at}/wopi/edit"))
        .query(&[("WOPISrc", src.as_str()), ("access_token", "host-token")])
        .send()
        .await
        .unwrap();
    assert!(page.status().is_success(), "{}", page.status());
    let id = session_id(&page.text().await.unwrap());

    let info: serde_json::Value = client
        .get(format!("{at}/wopi/session/{id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        info["editable"], false,
        "opened writable over someone's lock"
    );
    assert_eq!(
        host.locked().as_deref(),
        Some("held-by-word"),
        "the other lock was stolen"
    );
}

/// **The discovery document is served, and names this deployment.**
#[tokio::test]
async fn discovery_is_served() {
    let at = adapter().await;
    let xml = reqwest::get(format!("{at}/hosting/discovery"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(xml.contains("<wopi-discovery>"), "{xml}");
    assert!(
        xml.contains(r#"urlsrc="http://calc.example/wopi/edit?""#),
        "{xml}"
    );
}

/// **The brand reaches the editor without breaking the URL it travels on.**
///
/// The editor bundle is static and served elsewhere, so its address is the only
/// thing this service can configure about it. Two ways that goes wrong: a
/// second `?` when the URL already has a query — the editor's own `?fonts=` is
/// the usual case, and it makes the whole query one parameter name — and an
/// unencoded `&` in a name, which silently starts a parameter of its own.
#[test]
fn the_brand_is_appended_to_the_editor_url_safely() {
    let plain = discovery::Brand::default();
    assert_eq!(
        branded("/editor/editor.html", &plain),
        "/editor/editor.html",
        "an unbranded deployment gets an unchanged URL"
    );

    let theirs = discovery::Brand {
        name: "Ada & Co".to_owned(),
        accent: "#ff0055".to_owned(),
        favicon: String::new(),
    };
    assert_eq!(
        branded("/editor/editor.html", &theirs),
        "/editor/editor.html?brand=Ada%20%26%20Co&accent=%23ff0055"
    );
    // An existing query is joined, not restarted.
    assert_eq!(
        branded("/editor/editor.html?fonts=/api/fonts", &theirs),
        "/editor/editor.html?fonts=/api/fonts&brand=Ada%20%26%20Co&accent=%23ff0055"
    );
}
