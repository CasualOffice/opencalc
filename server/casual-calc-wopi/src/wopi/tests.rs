//! The WOPI client, against a stub host that records what it was asked.
//!
//! A stub rather than a mock library: what these need to assert is the exact
//! shape of the request on the wire — which override header, which lock id,
//! which path — and a recording server is the only thing that sees it the way a
//! real host does.

use super::*;
use std::sync::{Arc, Mutex};

/// Everything the stub host saw.
#[derive(Debug, Default)]
struct Seen {
    path: String,
    query: String,
    over: String,
    lock: String,
    body: Vec<u8>,
}

/// A status, some headers, and a body.
type Reply = (u16, Vec<(String, String)>, Vec<u8>);

#[derive(Clone, Default)]
struct Stub {
    seen: Arc<Mutex<Vec<Seen>>>,
    /// What to answer with: status, headers, body.
    reply: Arc<Mutex<Reply>>,
}

impl Stub {
    fn last(&self) -> Seen {
        self.seen.lock().unwrap().pop().expect("a request")
    }
}

/// Start a host that records and answers whatever it was told to.
async fn stub(status: u16, headers: &[(&str, &str)], body: &[u8]) -> (String, Stub) {
    let state = Stub::default();
    *state.reply.lock().unwrap() = (
        status,
        headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        body.to_vec(),
    );
    let app = axum::Router::new()
        .fallback(
            |axum::extract::State(state): axum::extract::State<Stub>,
             request: axum::extract::Request| async move {
                let uri = request.uri().clone();
                let head = request.headers().clone();
                let body = axum::body::to_bytes(request.into_body(), 64 * 1024 * 1024)
                    .await
                    .unwrap_or_default();
                state.seen.lock().unwrap().push(Seen {
                    path: uri.path().to_owned(),
                    query: uri.query().unwrap_or_default().to_owned(),
                    over: header(&head, "X-WOPI-Override"),
                    lock: header(&head, "X-WOPI-Lock"),
                    body: body.to_vec(),
                });
                let (code, headers, body) = state.reply.lock().unwrap().clone();
                let mut response = axum::response::Response::builder()
                    .status(axum::http::StatusCode::from_u16(code).unwrap());
                for (k, v) in headers {
                    response = response.header(k, v);
                }
                response.body(axum::body::Body::from(body)).unwrap()
            },
        )
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), state)
}

fn header(map: &axum::http::HeaderMap, name: &str) -> String {
    map.get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

/// **A `WOPISrc` that carries a query string still addresses `/contents`.**
///
/// Appending `/contents` to the whole src gives `…?a=b/contents`, which a host
/// answers with a 404 that reads exactly like a missing file. Hosts whose src
/// is path-only work either way, so this is invisible until the first
/// integration with one that is not.
#[test]
fn the_contents_url_is_built_around_an_existing_query() {
    assert_eq!(
        contents_of("https://nc.example/wopi/files/7"),
        "https://nc.example/wopi/files/7/contents"
    );
    assert_eq!(
        contents_of("https://sp.example/_vti_bin/wopi.ashx/files/7?a=b"),
        "https://sp.example/_vti_bin/wopi.ashx/files/7/contents?a=b"
    );
    // A trailing slash is not a path segment.
    assert_eq!(
        contents_of("https://nc.example/wopi/files/7/"),
        "https://nc.example/wopi/files/7/contents"
    );
}

/// **A host that does not say whether the user may write is read-only.**
///
/// The permissive default is the dangerous one: it is silent, and it lands on
/// somebody else's file.
#[tokio::test]
async fn write_permission_is_not_assumed() {
    let (src, _) = stub(200, &[], br#"{"BaseFileName":"Q3.xlsx"}"#).await;
    let info = Host::new(64 << 20)
        .check_file_info(&src, "t")
        .await
        .expect("info");
    assert!(!info.user_can_write, "absent means no");
    assert!(!info.supports_locks);
    assert!(!info.supports_update);
}

/// **`CheckFileInfo` is what validates the access token.**
///
/// We hold no key that could check someone else's credential, so the check is
/// to use it and read the answer.
#[tokio::test]
async fn a_rejected_token_is_reported_as_such() {
    let (src, _) = stub(401, &[], b"").await;
    let problem = Host::new(64 << 20)
        .check_file_info(&src, "stale")
        .await
        .expect_err("rejected");
    assert!(matches!(problem, Problem::Unauthorised), "{problem:?}");
}

/// **A 409 carries the lock actually held, and it is kept.**
///
/// It is the only way to tell "you lost your lock" from "somebody else has the
/// file", and those have different recoveries.
#[tokio::test]
async fn a_lock_conflict_keeps_the_id_that_won() {
    let (src, _) = stub(409, &[("X-WOPI-Lock", "held-by-word")], b"").await;
    let problem = Host::new(64 << 20)
        .put_file(&src, "t", Some("ours"), b"x".to_vec())
        .await
        .expect_err("conflict");
    match problem {
        Problem::LockMismatch(held) => assert_eq!(held, "held-by-word"),
        other => panic!("expected a lock mismatch, got {other:?}"),
    }
}

/// **`PutFile` is a `POST` to `/contents` with the override and the lock.**
///
/// Every one of those is load-bearing: without the override a host answers 404
/// or 501, and without the lock a host that locked on open answers 409.
#[tokio::test]
async fn a_save_is_addressed_the_way_wopi_specifies() {
    let (src, stub) = stub(200, &[], b"").await;
    Host::new(64 << 20)
        .put_file(&src, "tok", Some("lock-9"), b"package".to_vec())
        .await
        .expect("saved");
    let seen = stub.last();
    assert!(seen.path.ends_with("/contents"), "{}", seen.path);
    assert_eq!(seen.over, "PUT");
    assert_eq!(seen.lock, "lock-9");
    assert_eq!(seen.body, b"package");
    assert!(seen.query.contains("access_token=tok"));
}

/// **The three lock calls differ only by their override header.**
#[tokio::test]
async fn the_lock_operations_are_named_on_the_wire() {
    for (operation, expected) in [
        ("lock", "LOCK"),
        ("refresh", "REFRESH_LOCK"),
        ("unlock", "UNLOCK"),
    ] {
        let (src, stub) = stub(200, &[], b"").await;
        let host = Host::new(64 << 20);
        match operation {
            "lock" => host.lock(&src, "t", "id-1").await,
            "refresh" => host.refresh_lock(&src, "t", "id-1").await,
            _ => host.unlock(&src, "t", "id-1").await,
        }
        .expect("accepted");
        let seen = stub.last();
        assert_eq!(seen.over, expected);
        assert_eq!(seen.lock, "id-1");
        // A lock is taken on the file, not on its contents.
        assert!(!seen.path.ends_with("/contents"), "{}", seen.path);
    }
}

/// **A file larger than this service will hold is refused, not buffered.**
#[tokio::test]
async fn an_oversized_file_is_refused() {
    let (src, _) = stub(200, &[], &vec![0u8; 4096]).await;
    let problem = Host::new(1024)
        .get_file(&src, "t")
        .await
        .expect_err("refused");
    assert!(
        matches!(&problem, Problem::Failed(why) if why.contains("over the 1024")),
        "{problem:?}"
    );
}

/// **No failure message contains the access token.**
///
/// WOPI puts a bearer credential for somebody else's file store in the query
/// string, so every error built from a URL leaks one. `reqwest`'s own `Display`
/// prints the URL it failed on, which is how this happens without anybody
/// writing a line of logging.
#[tokio::test]
async fn a_failure_never_repeats_the_access_token() {
    const SECRET: &str = "zzsupersecrettokenzz";

    // A host that answers unhelpfully.
    let (src, _) = stub(500, &[], b"upstream exploded").await;
    let host = Host::new(64 << 20);
    let mut messages = vec![
        host.check_file_info(&src, SECRET)
            .await
            .unwrap_err()
            .to_string(),
        host.get_file(&src, SECRET).await.unwrap_err().to_string(),
        host.lock(&src, SECRET, "l").await.unwrap_err().to_string(),
        host.put_file(&src, SECRET, Some("l"), vec![])
            .await
            .unwrap_err()
            .to_string(),
    ];

    // And a host that is not there at all, which is the transport path — the
    // one where the URL comes back inside the error rather than being built.
    let dead = "http://127.0.0.1:9/wopi/files/1";
    messages.push(
        host.check_file_info(dead, SECRET)
            .await
            .unwrap_err()
            .to_string(),
    );
    messages.push(host.get_file(dead, SECRET).await.unwrap_err().to_string());

    for message in messages {
        assert!(
            !message.contains(SECRET),
            "an access token reached an error message: {message}"
        );
    }
}
