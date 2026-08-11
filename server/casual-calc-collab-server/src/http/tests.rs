//! HTTP transport tests, against a real origin on a real socket.
//!
//! A stub server rather than a mocked client: the things worth getting wrong
//! here — which verb, which header, which URL, what happens on a redirect —
//! are all things a mock would simply agree with.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post, put};

use super::*;

/// What the stub origin was asked for.
#[derive(Debug, Clone, Default)]
struct Seen {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body_len: usize,
}

type Log = Arc<Mutex<Vec<Seen>>>;

async fn stub(status: StatusCode, body: Vec<u8>) -> (std::net::SocketAddr, Log) {
    let log: Log = Arc::default();
    let state = (log.clone(), status, Arc::new(body));

    async fn record(
        method: &str,
        state: (Log, StatusCode, Arc<Vec<u8>>),
        path: String,
        query: String,
        headers: HeaderMap,
        body: axum::body::Bytes,
    ) -> (StatusCode, Vec<u8>) {
        state.0.lock().unwrap().push(Seen {
            method: method.to_owned(),
            path,
            query,
            headers: headers
                .iter()
                .map(|(k, v)| {
                    (
                        k.as_str().to_lowercase(),
                        v.to_str().unwrap_or_default().to_owned(),
                    )
                })
                .collect(),
            body_len: body.len(),
        });
        (state.1, state.2.as_ref().clone())
    }

    let app = Router::new()
        .route(
            "/doc",
            get(
                |State(s): State<(Log, StatusCode, Arc<Vec<u8>>)>, headers: HeaderMap| async move {
                    record(
                        "GET",
                        s,
                        "/doc".into(),
                        String::new(),
                        headers,
                        Default::default(),
                    )
                    .await
                },
            ),
        )
        .route(
            "/callback",
            post(
                |State(s): State<(Log, StatusCode, Arc<Vec<u8>>)>,
                 headers: HeaderMap,
                 body: axum::body::Bytes| async move {
                    record("POST", s, "/callback".into(), String::new(), headers, body).await
                },
            ),
        )
        .route(
            "/wopi/files/{id}/contents",
            put(
                |State(s): State<(Log, StatusCode, Arc<Vec<u8>>)>,
                 Path(id): Path<String>,
                 Query(q): Query<std::collections::BTreeMap<String, String>>,
                 headers: HeaderMap,
                 body: axum::body::Bytes| async move {
                    let query = q
                        .iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect::<Vec<_>>()
                        .join("&");
                    record(
                        "PUT",
                        s,
                        format!("/wopi/files/{id}/contents"),
                        query,
                        headers,
                        body,
                    )
                    .await
                },
            ),
        )
        .route(
            "/redirect",
            get(|| async {
                (
                    StatusCode::FOUND,
                    [(axum::http::header::LOCATION, "http://127.0.0.1:9/elsewhere")],
                )
            }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, log)
}

fn transport() -> HttpTransport {
    HttpTransport::new(HttpConfig {
        timeout: Duration::from_secs(5),
        max_document_bytes: 4096,
    })
    .unwrap()
}

fn header_of(seen: &Seen, name: &str) -> Option<String> {
    seen.headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.clone())
}

// --- Fetching ---------------------------------------------------------------

#[tokio::test]
async fn a_document_is_fetched() {
    let (addr, _log) = stub(StatusCode::OK, b"a package".to_vec()).await;
    let got = transport()
        .get(format!("http://{addr}/doc"))
        .await
        .expect("fetched");
    assert_eq!(got, b"a package");
}

#[tokio::test]
async fn an_origin_that_refuses_is_reported_with_its_status() {
    let (addr, _log) = stub(StatusCode::FORBIDDEN, Vec::new()).await;
    let err = transport()
        .get(format!("http://{addr}/doc"))
        .await
        .unwrap_err();
    assert!(err.contains("403"), "got {err}");
}

#[tokio::test]
async fn a_document_over_the_ceiling_is_refused_rather_than_allocated() {
    // An origin serving an endless body makes the node allocate until it dies,
    // and it need not be hostile to do it.
    let (addr, _log) = stub(StatusCode::OK, vec![0u8; 8192]).await;
    let err = transport()
        .get(format!("http://{addr}/doc"))
        .await
        .unwrap_err();
    assert!(err.contains("ceiling"), "got {err}");
}

#[tokio::test]
async fn a_redirect_is_not_followed() {
    // Following one would take the request somewhere the token's allow-list
    // never approved, which is the only thing standing between a mis-issued
    // token and every address inside the deployment.
    let (addr, _log) = stub(StatusCode::OK, Vec::new()).await;
    let err = transport()
        .get(format!("http://{addr}/redirect"))
        .await
        .unwrap_err();
    assert!(
        err.contains("302"),
        "a redirect must not be followed: {err}"
    );
}

// --- Delivering -------------------------------------------------------------

#[tokio::test]
async fn an_onlyoffice_callback_is_posted_with_the_package() {
    let (addr, log) = stub(StatusCode::OK, Vec::new()).await;
    transport()
        .put(
            Callback::Url {
                url: format!("http://{addr}/callback"),
            },
            "Budget.xlsx".into(),
            vec![1, 2, 3, 4],
        )
        .await
        .expect("delivered");

    let seen = log.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].body_len, 4);
    assert_eq!(header_of(&seen[0], "content-type").as_deref(), Some(XLSX));
    assert_eq!(
        header_of(&seen[0], "x-opencalc-title").as_deref(),
        Some("Budget.xlsx"),
        "so a host serving many documents knows which arrived"
    );
}

#[tokio::test]
async fn a_wopi_host_gets_a_put_to_contents_with_the_override_header() {
    // Without `X-WOPI-Override: PUT` a WOPI server does not know the request is
    // PutFile, and answers 404 or 501 rather than saving.
    let (addr, log) = stub(StatusCode::OK, Vec::new()).await;
    transport()
        .put(
            Callback::Wopi {
                src: format!("http://{addr}/wopi/files/17"),
                token: "tok-42".into(),
                token_expiry_ms: None,
            },
            "Budget.xlsx".into(),
            vec![9; 10],
        )
        .await
        .expect("delivered");

    let seen = log.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "PUT");
    assert_eq!(seen[0].path, "/wopi/files/17/contents");
    assert_eq!(seen[0].query, "access_token=tok-42");
    assert_eq!(
        header_of(&seen[0], "x-wopi-override").as_deref(),
        Some("PUT")
    );
    assert_eq!(seen[0].body_len, 10);
}

#[tokio::test]
async fn a_trailing_slash_on_the_wopi_source_does_not_double_up() {
    let (addr, log) = stub(StatusCode::OK, Vec::new()).await;
    let _ = transport()
        .put(
            Callback::Wopi {
                src: format!("http://{addr}/wopi/files/17/"),
                token: "t".into(),
                token_expiry_ms: None,
            },
            "x".into(),
            vec![1],
        )
        .await;
    assert_eq!(log.lock().unwrap()[0].path, "/wopi/files/17/contents");
}

#[tokio::test]
async fn a_host_that_refuses_reports_its_status_and_a_bounded_body() {
    let big = vec![b'x'; 100_000];
    let (addr, _log) = stub(StatusCode::INTERNAL_SERVER_ERROR, big).await;
    let err = transport()
        .put(
            Callback::Url {
                url: format!("http://{addr}/callback"),
            },
            "x".into(),
            vec![1],
        )
        .await
        .unwrap_err();
    assert!(err.contains("500"), "got {err}");
    assert!(
        err.len() < 400,
        "a whole HTML error page must not reach a log line: {} chars",
        err.len()
    );
}

#[tokio::test]
async fn a_title_cannot_inject_a_header() {
    // A title comes from the token and is the field most likely to hold
    // whatever a user typed. A newline in it would end the header and start
    // another.
    let (addr, log) = stub(StatusCode::OK, Vec::new()).await;
    transport()
        .put(
            Callback::Url {
                url: format!("http://{addr}/callback"),
            },
            "evil\r\nX-Injected: yes".into(),
            vec![1],
        )
        .await
        .expect("delivered");

    let seen = log.lock().unwrap().clone();
    assert!(
        header_of(&seen[0], "x-injected").is_none(),
        "a header was injected through the title"
    );
    let title = header_of(&seen[0], "x-opencalc-title").unwrap_or_default();
    assert!(
        !title.contains('\r') && !title.contains('\n'),
        "got {title:?}"
    );
}

#[tokio::test]
async fn an_unreachable_host_is_an_error_rather_than_a_hang() {
    // Port 9 is discard: it refuses rather than answering.
    let err = transport()
        .put(
            Callback::Url {
                url: "http://127.0.0.1:9/callback".into(),
            },
            "x".into(),
            vec![1],
        )
        .await
        .unwrap_err();
    assert!(!err.is_empty());
}
