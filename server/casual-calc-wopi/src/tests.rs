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
/// A token shaped like a real one.
///
/// SharePoint's access tokens are long and base64-ish, so they contain `+`, `/`
/// and `=` — characters that must be percent-encoded in a query string. A test
/// token of plain letters makes the encoding a no-op and hides whether the URL
/// that is *signed* matches the URL that is *sent*.
const REALISTIC_TOKEN: &str = "eyJhbGc+iJI/UzI1N=";

#[derive(Clone, Default)]
struct StubHost {
    /// What the file is called. The **only** thing that says what format it is
    /// in — the bytes of a `.csv` are just text — so the adapter reads it from
    /// here and nowhere else.
    name: Arc<Mutex<String>>,
    /// What the file contains. Starts as a marker so a fetch can be recognised.
    content: Arc<Mutex<Vec<u8>>>,
    /// The `Content-Type` the last `PutFile` announced. A host indexes,
    /// previews and scans on this, so bytes and header have to agree.
    put_type: Arc<Mutex<Option<String>>>,
    /// The lock id currently held, if any.
    lock: Arc<Mutex<Option<String>>>,
    /// Every override the host was asked to perform, in order.
    calls: Arc<Mutex<Vec<String>>>,
    /// Whether `PutFile` should fail.
    refuse_writes: Arc<Mutex<bool>>,
    /// What `CheckFileInfo` claims.
    can_write: Arc<Mutex<bool>>,
    /// The proof headers and request target of the last `GetFile`.
    ///
    /// A host verifies against the URL *it received*, so the target is recorded
    /// alongside the signature rather than reconstructed by the test — that is
    /// the whole thing `WOPI-06` can get subtly wrong.
    proof_seen: Arc<Mutex<Option<(String, String, String)>>>,
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
    fn put_type(&self) -> Option<String> {
        self.put_type.lock().unwrap().clone()
    }
}

/// Start the stub over the usual `.xlsx`.
async fn wopi_host() -> (String, StubHost) {
    wopi_host_holding("Q3.xlsx", b"ORIGINAL").await
}

/// Start the stub, returning the `WOPISrc` of its one file.
async fn wopi_host_holding(name: &str, content: &[u8]) -> (String, StubHost) {
    let state = StubHost::default();
    *state.name.lock().unwrap() = name.to_owned();
    *state.content.lock().unwrap() = content.to_vec();
    *state.can_write.lock().unwrap() = true;

    let app = Router::new()
        .route(
            "/wopi/files/1",
            get(
                |State(s): State<StubHost>, Query(q): Query<std::collections::HashMap<String, String>>| async move {
                    s.calls.lock().unwrap().push("CheckFileInfo".to_owned());
                    // Two accepted values, and the second is the point: it
                    // contains `+`, `/` and `=`, so this check also proves the
                    // token survives being put on a query string and taken off
                    // again. With only `host-token` the encoding is a no-op and
                    // a broken encoder passes.
                    if !matches!(
                        q.get("access_token").map(String::as_str),
                        Some("host-token") | Some(REALISTIC_TOKEN)
                    ) {
                        return (StatusCode::UNAUTHORIZED, String::new()).into_response();
                    }
                    Json(serde_json::json!({
                        "BaseFileName": s.name.lock().unwrap().clone(),
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
            get(
                |State(s): State<StubHost>,
                 headers: HeaderMap,
                 uri: axum::http::Uri| async move {
                    s.calls.lock().unwrap().push("GetFile".to_owned());
                    let header = |name: &str| {
                        headers
                            .get(name)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or_default()
                            .to_owned()
                    };
                    *s.proof_seen.lock().unwrap() = Some((
                        header("X-WOPI-Proof"),
                        header("X-WOPI-TimeStamp"),
                        uri.to_string(),
                    ));
                    s.content.lock().unwrap().clone()
                },
            )
            .post(
                |State(s): State<StubHost>, headers: HeaderMap, body: axum::body::Bytes| async move {
                    s.calls.lock().unwrap().push("PutFile".to_owned());
                    *s.put_type.lock().unwrap() = headers
                        .get(header::CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_owned);
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
    adapter_with_proof(None).await.0
}

/// The same, optionally signing its outgoing calls — and handing back the key
/// so a test can verify exactly what a host would see.
async fn adapter_with_proof(key: Option<&[u8]>) -> (String, Option<Arc<crate::proof::ProofKeys>>) {
    let proof = key.map(|der| Arc::new(crate::proof::ProofKeys::from_pkcs8(der).unwrap()));
    let config = Config {
        proof_key_path: None,
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
        proof: proof.clone(),
        host: Host::new(config.max_document_bytes, proof.clone()),
        sessions: Sessions::new(config.max_sessions, config.session_ttl_ms),
        config,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router(service)).await;
    });
    (format!("http://{addr}"), proof)
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

/// **A host's `.csv` opens, edits, and comes back a `.csv`** (`WOPI-05`).
///
/// The defect this holds shut, and the reason discovery advertised one
/// extension: the save leg emitted an OOXML package whatever it opened, so a
/// host that handed us `Books.csv` got a zip back under that name. The original
/// was gone, every tool downstream saw a corrupt CSV, and nothing anywhere said
/// so.
///
/// Both conversions are asserted, because each is a different failure. The
/// collaboration server reads packages and only packages, so the fetch leg must
/// hand it one; the host holds a `.csv`, so the save leg must hand it text —
/// and the `Content-Type` has to say the same thing the bytes do, because that
/// is what a host indexes, previews and virus-scans on.
#[tokio::test]
async fn a_csv_opens_as_a_package_and_saves_back_as_a_csv() {
    let (src, host) = wopi_host_holding("Books.csv", b"Item,Qty\r\nWidget,3\r\n").await;
    let at = adapter().await;
    let client = reqwest::Client::new();

    let page = client
        .get(format!("{at}/wopi/edit"))
        .query(&[("WOPISrc", src.as_str()), ("access_token", "host-token")])
        .send()
        .await
        .expect("the action URL answered");
    assert!(page.status().is_success(), "{}", page.status());
    let id = session_id(&page.text().await.unwrap());

    // The page is told what will be written back, before any editing happens.
    let info: serde_json::Value = client
        .get(format!("{at}/wopi/session/{id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(info["format"], "csv", "the session forgot what it opened");

    // 1. The collaboration server fetches — and is handed a package, which is
    //    the only thing it can open.
    let package = client
        .get(format!("{at}/wopi/content/{id}"))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap()
        .to_vec();
    assert_eq!(
        &package[..2],
        b"PK",
        "the collaboration server was handed something it cannot open: {}",
        String::from_utf8_lossy(&package[..package.len().min(40)])
    );

    // 2. It edits and posts the finished package back, exactly as it would for
    //    an `.xlsx`. Nothing about the server knows this file is a CSV.
    let mut session = WorkbookSession::open(package).expect("the package we served opens");
    session
        .edit(casual_calc_sdk::EditOperation::SetValue {
            sheet: 0,
            at: casual_calc_sdk::CellRef::new(1, 1),
            value: casual_calc_sdk::CellValue::Number(7.0),
        })
        .expect("edited");
    let finished = session.save().expect("the server writes a package");

    let saved = client
        .post(format!("{at}/wopi/callback/{id}"))
        .body(finished)
        .send()
        .await
        .unwrap();
    assert!(saved.status().is_success(), "{}", saved.status());

    // 3. The host's file is still a CSV, and carries the edit.
    assert_eq!(
        String::from_utf8(host.content()).expect("the host's file is still text"),
        "Item,Qty\r\nWidget,7\r\n",
        "the host's .csv was overwritten with something else"
    );
    assert_eq!(
        host.put_type().as_deref(),
        Some("text/csv;charset=utf-8"),
        "the bytes were CSV and the header said they were a spreadsheet package"
    );
}

/// **A file this service cannot save back in its own format is refused before
/// it is locked.**
///
/// Discovery only advertises what the save leg can write, so a host following
/// it never asks. A hand-written link, a stale host configuration or an `.ods`
/// does — and the old behaviour was to assume `.xlsx`, edit it, and write a
/// package over it under its original name.
#[tokio::test]
async fn a_format_this_service_cannot_write_is_never_opened() {
    let (src, host) = wopi_host_holding("Notes.ods", b"PK\x03\x04...").await;
    let at = adapter().await;

    let refused = reqwest::Client::new()
        .get(format!("{at}/wopi/edit"))
        .query(&[("WOPISrc", src.as_str()), ("access_token", "host-token")])
        .send()
        .await
        .expect("answered");

    assert_eq!(refused.status(), reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(host.locked(), None, "a file we cannot write was locked");
    assert!(
        !host.calls().contains(&"LOCK".to_owned()),
        "{:?}",
        host.calls()
    );
}

/// **A `.csv` save of a workbook the format cannot hold names everything it
/// drops, and drops nothing quietly.**
///
/// The case that decides whether this row is honest rather than merely working:
/// somebody opens `Books.csv`, adds a second sheet and a formula, and saves. A
/// `.csv` holds one sheet of values. The save goes ahead — they are editing a
/// CSV and asked for a CSV — but every part of the document that does not
/// survive is counted and named on the way out.
///
/// Asserted on the pure pair rather than through the socket, because what is
/// being checked is the *report*, and a log line read back out of a subscriber
/// is a test of `tracing`.
#[test]
fn a_csv_save_names_the_sheets_and_formulas_it_cannot_carry() {
    use casual_calc_model::{Id, Sheet, SheetId};

    let mut session = WorkbookSession::open_delimited(b"Item,Qty\r\nWidget,3\r\n".to_vec(), b',')
        .expect("the csv opens");
    let workbook = session.workbook_mut();
    // A formula: its answer reaches the file, the formula itself does not.
    let handle = workbook.store_formula(casual_calc_formula::parse("1+1").unwrap());
    let mut cell = casual_calc_sdk::Cell::value(casual_calc_sdk::CellValue::Number(2.0));
    cell.formula = Some(handle);
    workbook.sheets[0]
        .cells
        .set(casual_calc_sdk::CellRef::new(2, 1), cell);
    // A second sheet, with something on it that must not silently vanish.
    let mut notes = Sheet::new(SheetId(Id::from_parts(0x5345_5300_0000_0000, 2)), "Notes");
    let text = workbook.intern_string("do not lose me");
    notes.cells.set(
        casual_calc_sdk::CellRef::new(0, 0),
        casual_calc_sdk::Cell::value(casual_calc_sdk::CellValue::SharedString(text)),
    );
    workbook.sheets.push(notes);

    let package = session
        .save_as(SessionFormat::Xlsx)
        .expect("the collaboration server's package");
    let (bytes, loss) = save_as(package, SessionFormat::Delimited(b',')).expect("converts");

    let loss = loss.expect("a second sheet and a formula were dropped and nothing said so");
    assert!(
        loss.contains("other sheets (1)"),
        "the sheet that is not written was not named: {loss}"
    );
    assert!(
        loss.contains("formulas (1)"),
        "a formula written as its value was not named: {loss}"
    );

    // And the loss is real, not a warning about something that survived.
    let text = String::from_utf8(bytes).expect("csv is text");
    assert!(
        !text.contains("do not lose me"),
        "the second sheet was written into the csv after all: {text:?}"
    );

    // An `.xlsx` save loses nothing and must not cry wolf: a warning on every
    // file is a warning nobody reads on the one that matters.
    let package = session.save_as(SessionFormat::Xlsx).unwrap();
    assert_eq!(save_as(package, SessionFormat::Xlsx).unwrap().1, None);
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

/// **A host can verify the proof on a request this service actually made.**
///
/// The unit tests in `crate::proof` prove the signature is well-formed. They
/// cannot prove the *service* signs the right thing: the payload covers the URL
/// as sent, token and all, so the one mistake that matters — signing a URL that
/// differs from the one on the wire by an encoding, a separator or a missing
/// query parameter — is only visible end to end. Against a real SharePoint it
/// would appear as every request being rejected, with nothing local to look at.
///
/// So this drives the whole path: the adapter opens a file, the stub host
/// records the proof headers and the request target it received, and the
/// signature is verified against the modulus and exponent read out of the
/// **discovery document** — which is the only key a host is ever given.
#[tokio::test]
async fn a_host_can_verify_the_proof_on_a_request_this_service_made() {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as B64;
    use ring::signature::{RSA_PKCS1_2048_8192_SHA256, RsaPublicKeyComponents};

    const TEST_KEY: &[u8] = include_bytes!("../tests/fixtures/proof-test-key.pkcs8.der");

    let (src, host) = wopi_host_holding("Book.xlsx", b"PK\x03\x04not-a-real-package").await;
    let (at, _) = adapter_with_proof(Some(TEST_KEY)).await;
    let client = reqwest::Client::new();

    // An open, which makes the adapter call `GetFile` on the host.
    let page = client
        .get(format!("{at}/wopi/edit"))
        .query(&[("WOPISrc", src.as_str()), ("access_token", REALISTIC_TOKEN)])
        .send()
        .await
        .expect("the action URL answered");
    let st = page.status();
    let body = page.text().await.unwrap();
    assert!(st.is_success(), "{st}: {body}");
    let id = session_id(&body);

    // The fetch is a separate leg: opening reserves the session, and the
    // collaboration server pulling the content is what actually calls `GetFile`
    // on the host. That call is the one carrying the proof.
    client
        .get(format!("{at}/wopi/content/{id}"))
        .send()
        .await
        .unwrap();

    let (signature, timestamp, target) = host
        .proof_seen
        .lock()
        .unwrap()
        .clone()
        .expect("the host never saw a GetFile");
    assert!(!signature.is_empty(), "no X-WOPI-Proof header was sent");
    assert!(!timestamp.is_empty(), "no X-WOPI-TimeStamp header was sent");

    // The key exactly as a host obtains it: parsed out of the discovery XML.
    let xml = client
        .get(format!("{at}/hosting/discovery"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let attribute = |name: &str| {
        let needle = format!("{name}=\"");
        let start = xml.find(&needle).expect("attribute missing from discovery") + needle.len();
        xml[start..].split('"').next().unwrap().to_owned()
    };
    let public = RsaPublicKeyComponents {
        n: B64.decode(attribute("modulus")).unwrap(),
        e: B64.decode(attribute("exponent")).unwrap(),
    };

    // The URL as the *host* saw it, not as the adapter believes it sent it.
    let base = src.trim_end_matches("/wopi/files/1");
    let url = format!("{base}{target}");
    let ticks: i64 = timestamp.parse().expect("the timestamp is a tick count");

    public
        .verify(
            &RSA_PKCS1_2048_8192_SHA256,
            &crate::proof::signed_payload(REALISTIC_TOKEN, &url, ticks),
            &B64.decode(&signature).unwrap(),
        )
        .expect("a host could not verify the proof this service sent");
}

/// **Without a key configured, nothing is advertised and nothing is signed.**
///
/// The feature is optional and off by default. Advertising a proof key while
/// signing with a different one — or with none — makes a host reject every
/// request, so "off" has to mean off in both places at once.
#[tokio::test]
async fn an_unconfigured_service_advertises_no_proof_key_and_signs_nothing() {
    let (src, host) = wopi_host_holding("Book.xlsx", b"PK\x03\x04not-a-real-package").await;
    let at = adapter().await;
    let client = reqwest::Client::new();

    let xml = client
        .get(format!("{at}/hosting/discovery"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        !xml.contains("proof-key"),
        "a service with no key advertised one: {xml}"
    );

    let page = client
        .get(format!("{at}/wopi/edit"))
        .query(&[("WOPISrc", src.as_str()), ("access_token", REALISTIC_TOKEN)])
        .send()
        .await
        .unwrap();
    let id = session_id(&page.text().await.unwrap());
    client
        .get(format!("{at}/wopi/content/{id}"))
        .send()
        .await
        .unwrap();
    let seen = host.proof_seen.lock().unwrap().clone();
    let (signature, timestamp, _) = seen.expect("the host never saw a GetFile");
    assert!(
        signature.is_empty(),
        "an unconfigured service signed anyway"
    );
    assert!(timestamp.is_empty(), "and sent a timestamp");
}
