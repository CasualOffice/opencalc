//! End-to-end tests: a real listener, real WebSockets, two participants.
//!
//! Everything below this module was already gated as a state machine. What
//! only a running server can show is that the shell wires them together — that
//! a token is checked before a document is touched, that a refused permission
//! is refused at the socket, and that what one participant does reaches
//! another.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};
use casual_calc_transaction::protocol::{ClientMessage, PROTOCOL_VERSION, Refusal, ServerMessage};
use casual_calc_transaction::session::{SnapshotPolicy, Submission};
use casual_calc_transaction::wire::WireOperation;
use casual_calc_transaction::{Operation, SheetFields, SheetMetadata};
use futures_util::{SinkExt, StreamExt};
use jsonwebtoken::{EncodingKey, Header};

use super::*;
use crate::token::{Access, Document, Permissions, TokenPolicy, User};
use crate::verify::KeySet;

const SECRET: &[u8] = b"a shared secret, for development only";
const DOC: &str = "doc-1";

/// A document to serve, as bytes, so the fetcher has something real to answer
/// with and the session has something real to open.
fn package() -> Vec<u8> {
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    sheet
        .cells
        .set(CellRef::new(0, 0), Cell::value(CellValue::Number(1.0)));
    wb.sheets.push(sheet);
    casual_calc_export::write_workbook(&wb).unwrap()
}

struct Canned(Vec<u8>);

impl Fetch for Canned {
    fn get(&self, _url: String) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send>> {
        let bytes = self.0.clone();
        Box::pin(async move { Ok(bytes) })
    }
}

/// Records what was delivered, so a test can watch the server save.
#[derive(Clone, Default)]
struct Collected(Arc<Mutex<Vec<(String, usize)>>>);

impl Deliver for Collected {
    fn put(
        &self,
        _destination: Callback,
        title: String,
        bytes: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> {
        let seen = Arc::clone(&self.0);
        Box::pin(async move {
            seen.lock().unwrap().push((title, bytes.len()));
            Ok(())
        })
    }
}

/// A host that refuses everything, for the not-saving path.
struct Refusing;

impl Deliver for Refusing {
    fn put(
        &self,
        _destination: Callback,
        _title: String,
        _bytes: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> {
        Box::pin(async move { Err("the host said no".to_owned()) })
    }
}

struct Unreachable;

impl Fetch for Unreachable {
    fn get(&self, _url: String) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send>> {
        Box::pin(async move { Err("the origin is down".to_owned()) })
    }
}

fn claims(name: &str, access: Access) -> Claims {
    Claims {
        iss: "https://host.example".into(),
        aud: "opencalc-collab".into(),
        // Far enough ahead that a real clock is inside it.
        exp: 4_000_000_000,
        iat: None,
        nbf: None,
        jti: None,
        user: User {
            id: format!("u-{name}"),
            name: name.into(),
            email: None,
            avatar_url: None,
            group: None,
            color: None,
        },
        document: Document {
            key: DOC.into(),
            id: "file-1".into(),
            title: "Budget.xlsx".into(),
            version: None,
            owner_id: None,
            url: "https://host.example/files/1".into(),
        },
        permissions: Permissions {
            access,
            download: true,
            print: true,
            copy: true,
        },
        callback: None,
    }
}

fn token(claims: &Claims) -> String {
    jsonwebtoken::encode(
        &Header::new(jsonwebtoken::Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(SECRET),
    )
    .unwrap()
}

/// Start a server on an ephemeral port and return its address.
async fn start(fetch: Arc<dyn Fetch>) -> SocketAddr {
    start_with(fetch, Arc::new(Collected::default()), Limits::default()).await
}

/// Start a server, choosing how it delivers and what it will hold.
async fn start_with(
    fetch: Arc<dyn Fetch>,
    deliver: Arc<dyn Deliver>,
    limits: Limits,
) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let config = ServiceConfig {
        bind: addr,
        verifier: Verifier {
            policy: TokenPolicy {
                audience: "opencalc-collab".into(),
                leeway_secs: 60,
                allowed_hosts: BTreeSet::new(),
                require_https: true,
            },
            keys: KeySet::shared_secret(SECRET),
        },
        save: SavePolicy::default(),
        snapshots: SnapshotPolicy::default(),
        fetch,
        deliver,
        limits,
    };
    tokio::spawn(async move {
        let _ = serve_on(listener, config).await;
    });
    addr
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(addr: SocketAddr) -> Socket {
    let url = format!("ws://{addr}/collab?doc={DOC}");
    let (socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    socket
}

async fn say(socket: &mut Socket, message: &ClientMessage) {
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::to_string(message).unwrap(),
        ))
        .await
        .unwrap();
}

/// The next server message, or `None` if the connection closed first.
async fn hear(socket: &mut Socket) -> Option<ServerMessage> {
    loop {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
            .await
            .expect("the server answered within five seconds")?;
        match frame.ok()? {
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                return serde_json::from_str(&text).ok();
            }
            tokio_tungstenite::tungstenite::Message::Close(_) => return None,
            _ => continue,
        }
    }
}

/// Join, and return the `Welcome`.
async fn join(socket: &mut Socket, claims: &Claims) -> Option<ServerMessage> {
    say(
        socket,
        &ClientMessage::Join {
            protocol: PROTOCOL_VERSION,
            token: token(claims),
        },
    )
    .await;
    hear(socket).await
}

fn cell_edit(value: f64) -> WireOperation {
    WireOperation {
        op: Operation::SetCell {
            sheet: 0,
            at: CellRef::new(5, 5),
            cell: Some(Cell::value(CellValue::Number(value))),
        },
        formulas: Default::default(),
        styles: Default::default(),
    }
}

fn comment_change() -> WireOperation {
    WireOperation {
        op: Operation::SetSheetMetadata {
            sheet: 0,
            data: Box::new(SheetMetadata::default()),
            changed: SheetFields::COMMENTS,
        },
        formulas: Default::default(),
        styles: Default::default(),
    }
}

// --- The happy path --------------------------------------------------------

#[tokio::test]
async fn an_editor_joins_and_is_given_the_document() {
    let addr = start(Arc::new(Canned(package()))).await;
    let mut socket = connect(addr).await;

    let welcome = join(&mut socket, &claims("Ada", Access::Edit)).await;
    let Some(ServerMessage::Welcome {
        protocol,
        revision,
        snapshot,
        editable,
        ..
    }) = welcome
    else {
        panic!("expected a welcome, got {welcome:?}");
    };
    assert_eq!(protocol, PROTOCOL_VERSION);
    assert_eq!(revision, 0, "a fresh session starts at zero");
    assert!(!snapshot.is_empty(), "and hands over the document");
    assert!(editable);
}

#[tokio::test]
async fn what_one_participant_edits_reaches_the_other() {
    // The whole point of the server, end to end.
    let addr = start(Arc::new(Canned(package()))).await;

    let mut ada = connect(addr).await;
    join(&mut ada, &claims("Ada", Access::Edit)).await.unwrap();
    let mut grace = connect(addr).await;
    join(&mut grace, &claims("Grace", Access::Edit))
        .await
        .unwrap();

    say(
        &mut ada,
        &ClientMessage::Submit(Submission {
            client: casual_calc_transaction::session::ClientId(1),
            seq: 1,
            base: 0,
            ops: vec![cell_edit(42.0)],
        }),
    )
    .await;

    // Ada is acked; the ack is what tells her the edit is ordered.
    let Some(ServerMessage::Ack { seq, revision }) = hear(&mut ada).await else {
        panic!("expected an ack");
    };
    assert_eq!(seq, 1);
    assert_eq!(revision, 1);

    // Grace is told what landed.
    let heard = hear(&mut grace).await;
    let Some(ServerMessage::Apply { revision, ops }) = heard else {
        panic!("expected an apply, got {heard:?}");
    };
    assert_eq!(revision, 1);
    assert_eq!(ops.len(), 1);
}

#[tokio::test]
async fn two_participants_join_the_same_session_rather_than_two_of_them() {
    // A second joiner must land in the session already running, not open a
    // second one over the same document — which is the one outcome that must
    // never happen.
    let addr = start(Arc::new(Canned(package()))).await;

    let mut ada = connect(addr).await;
    join(&mut ada, &claims("Ada", Access::Edit)).await.unwrap();
    say(
        &mut ada,
        &ClientMessage::Submit(Submission {
            client: casual_calc_transaction::session::ClientId(1),
            seq: 1,
            base: 0,
            ops: vec![cell_edit(42.0)],
        }),
    )
    .await;
    hear(&mut ada).await;

    // Grace arrives after the edit and must start from revision 1, not 0.
    let mut grace = connect(addr).await;
    let Some(ServerMessage::Welcome { revision, .. }) =
        join(&mut grace, &claims("Grace", Access::Edit)).await
    else {
        panic!("expected a welcome");
    };
    assert_eq!(
        revision, 1,
        "everyone in a session starts from the same revision"
    );
}

// --- Authorisation ---------------------------------------------------------

#[tokio::test]
async fn a_connection_that_does_not_join_first_gets_nowhere() {
    let addr = start(Arc::new(Canned(package()))).await;
    let mut socket = connect(addr).await;
    say(&mut socket, &ClientMessage::Heartbeat).await;
    assert!(
        hear(&mut socket).await.is_none(),
        "a connection that opens with anything else is not speaking this protocol"
    );
}

#[tokio::test]
async fn a_token_for_another_document_is_refused() {
    // The client asked for `doc-1` in the URL; the token says otherwise.
    let addr = start(Arc::new(Canned(package()))).await;
    let mut socket = connect(addr).await;

    let mut wrong = claims("Ada", Access::Edit);
    wrong.document.key = "some-other-document".into();
    let answer = join(&mut socket, &wrong).await;
    assert!(
        matches!(
            answer,
            Some(ServerMessage::Stopped {
                reason: Refusal::NotAuthorised
            })
        ),
        "got {answer:?}"
    );
}

#[tokio::test]
async fn a_badly_signed_token_is_refused() {
    let addr = start(Arc::new(Canned(package()))).await;
    let mut socket = connect(addr).await;

    let forged = jsonwebtoken::encode(
        &Header::new(jsonwebtoken::Algorithm::HS256),
        &claims("Mallory", Access::Edit),
        &EncodingKey::from_secret(b"not the secret"),
    )
    .unwrap();
    say(
        &mut socket,
        &ClientMessage::Join {
            protocol: PROTOCOL_VERSION,
            token: forged,
        },
    )
    .await;
    assert!(matches!(
        hear(&mut socket).await,
        Some(ServerMessage::Stopped {
            reason: Refusal::NotAuthorised
        })
    ));
}

#[tokio::test]
async fn a_mismatched_protocol_is_told_so_at_once() {
    let addr = start(Arc::new(Canned(package()))).await;
    let mut socket = connect(addr).await;
    say(
        &mut socket,
        &ClientMessage::Join {
            protocol: PROTOCOL_VERSION + 99,
            token: token(&claims("Ada", Access::Edit)),
        },
    )
    .await;
    let answer = hear(&mut socket).await;
    let Some(ServerMessage::Refused {
        reason: Refusal::ProtocolVersion { server, client },
        ..
    }) = answer
    else {
        panic!(
            "a mismatched peer should stop here rather than proceed until a \
             missing field produces something more confusing; got {answer:?}"
        );
    };
    assert_eq!(server, PROTOCOL_VERSION);
    assert_eq!(
        client,
        PROTOCOL_VERSION + 99,
        "and the message reports what the client said, not what this server \
         speaks — otherwise it claims the two agree while refusing them"
    );
}

// --- Permissions are enforced at the socket --------------------------------

#[tokio::test]
async fn a_viewer_is_refused_an_edit_by_the_server() {
    // Not by hiding a toolbar. A client that sends one anyway is refused.
    let addr = start(Arc::new(Canned(package()))).await;
    let mut socket = connect(addr).await;

    let Some(ServerMessage::Welcome { editable, .. }) =
        join(&mut socket, &claims("Vic", Access::View)).await
    else {
        panic!("expected a welcome");
    };
    assert!(!editable, "and is told so up front");

    say(
        &mut socket,
        &ClientMessage::Submit(Submission {
            client: casual_calc_transaction::session::ClientId(1),
            seq: 1,
            base: 0,
            ops: vec![cell_edit(42.0)],
        }),
    )
    .await;
    let answer = hear(&mut socket).await;
    assert!(
        matches!(
            answer,
            Some(ServerMessage::Refused {
                reason: Refusal::ReadOnlyAccess,
                ..
            })
        ),
        "got {answer:?}"
    );
}

#[tokio::test]
async fn a_commenter_may_comment_and_may_not_edit() {
    let addr = start(Arc::new(Canned(package()))).await;
    let mut socket = connect(addr).await;
    join(&mut socket, &claims("Cam", Access::Comment))
        .await
        .unwrap();

    say(
        &mut socket,
        &ClientMessage::Submit(Submission {
            client: casual_calc_transaction::session::ClientId(1),
            seq: 1,
            base: 0,
            ops: vec![comment_change()],
        }),
    )
    .await;
    assert!(
        matches!(hear(&mut socket).await, Some(ServerMessage::Ack { .. })),
        "a comment is allowed"
    );

    say(
        &mut socket,
        &ClientMessage::Submit(Submission {
            client: casual_calc_transaction::session::ClientId(1),
            seq: 2,
            base: 1,
            ops: vec![cell_edit(42.0)],
        }),
    )
    .await;
    assert!(
        matches!(
            hear(&mut socket).await,
            Some(ServerMessage::Refused {
                reason: Refusal::ReadOnlyAccess,
                ..
            })
        ),
        "and a cell edit is not"
    );
}

// --- Presence --------------------------------------------------------------

#[tokio::test]
async fn presence_carries_the_name_from_the_token_and_not_from_the_client() {
    // The client cannot state a name — `ClientMessage::Presence` has no field
    // for one — so what other participants see comes from what the host signed.
    let addr = start(Arc::new(Canned(package()))).await;

    let mut ada = connect(addr).await;
    join(&mut ada, &claims("Ada", Access::Edit)).await.unwrap();
    let mut grace = connect(addr).await;
    join(&mut grace, &claims("Grace", Access::Edit))
        .await
        .unwrap();

    say(
        &mut grace,
        &ClientMessage::Presence {
            sheet: 0,
            selection: [1, 2, 3, 4],
        },
    )
    .await;

    let heard = hear(&mut ada).await;
    let Some(ServerMessage::Presence {
        name,
        selection,
        color,
        ..
    }) = heard
    else {
        panic!("expected presence, got {heard:?}");
    };
    assert_eq!(name, "Grace", "from the token");
    assert_eq!(selection, [1, 2, 3, 4]);
    assert!(!color.is_empty(), "and a colour to draw the cursor with");
}

// --- Failure -----------------------------------------------------------------

#[tokio::test]
async fn a_document_that_cannot_be_fetched_is_reported_rather_than_hung() {
    let addr = start(Arc::new(Unreachable)).await;
    let mut socket = connect(addr).await;
    let answer = join(&mut socket, &claims("Ada", Access::Edit)).await;
    assert!(
        matches!(answer, Some(ServerMessage::Stopped { .. })),
        "got {answer:?}"
    );
}

#[tokio::test]
async fn the_health_endpoint_says_nothing_about_documents() {
    // A node with no documents is healthy. Reporting otherwise takes a working
    // node out of rotation and makes the outage worse.
    let addr = start(Arc::new(Canned(package()))).await;
    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let (mut sender, connection) = hyper_like_handshake(stream).await;
    tokio::spawn(connection);
    let response = sender.get("/healthz").await;
    assert!(response.contains("200"), "got {response}");
}

/// A deliberately tiny HTTP/1.1 client, so the health check does not pull in a
/// whole client stack for one request.
async fn hyper_like_handshake(
    mut stream: tokio::net::TcpStream,
) -> (Tiny, impl Future<Output = ()>) {
    use tokio::io::AsyncWriteExt;
    let _ = stream.flush().await;
    (Tiny { stream }, async {})
}

struct Tiny {
    stream: tokio::net::TcpStream,
}

impl Tiny {
    async fn get(&mut self, path: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        self.stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes(),
            )
            .await
            .unwrap();
        let mut out = Vec::new();
        let _ = self.stream.read_to_end(&mut out).await;
        String::from_utf8_lossy(&out).into_owned()
    }
}

// --- The service actually saves (PROD-03) ----------------------------------

/// A save policy that fires almost immediately, so a test does not wait five
/// seconds for the quiesce timer.
fn prompt_saves() -> SavePolicy {
    SavePolicy {
        quiesce_ms: 10,
        ceiling_ms: 100,
        ..SavePolicy::default()
    }
}

async fn start_saving(deliver: Arc<dyn Deliver>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let config = ServiceConfig {
        bind: addr,
        verifier: Verifier {
            policy: TokenPolicy {
                audience: "opencalc-collab".into(),
                leeway_secs: 60,
                allowed_hosts: BTreeSet::new(),
                require_https: true,
            },
            keys: KeySet::shared_secret(SECRET),
        },
        save: prompt_saves(),
        snapshots: SnapshotPolicy::default(),
        fetch: Arc::new(Canned(package())),
        deliver,
        limits: Limits {
            tick_ms: 10,
            idle_eviction_ms: 50,
            presence_ttl_ms: 60,
            ..Limits::default()
        },
    };
    tokio::spawn(async move {
        let _ = serve_on(listener, config).await;
    });
    addr
}

/// A token whose callback the server will deliver to.
fn saving_claims(name: &str) -> Claims {
    let mut c = claims(name, Access::Edit);
    c.callback = Some(Callback::Url {
        url: "https://host.example/callback".into(),
    });
    c
}

#[tokio::test]
async fn an_edit_is_eventually_delivered_to_the_host() {
    // The finding that made the audit worth doing: the save lifecycle was
    // fully built, fully tested, and driven by nothing, so the service ordered
    // edits correctly and held the only copy in memory.
    let seen = Collected::default();
    let addr = start_saving(Arc::new(seen.clone())).await;

    let mut ada = connect(addr).await;
    join(&mut ada, &saving_claims("Ada")).await.unwrap();
    say(
        &mut ada,
        &ClientMessage::Submit(Submission {
            client: casual_calc_transaction::session::ClientId(1),
            seq: 1,
            base: 0,
            ops: vec![cell_edit(42.0)],
        }),
    )
    .await;
    hear(&mut ada).await; // the ack

    // The quiesce timer is ten milliseconds; give it room without making the
    // test wait on a real cadence.
    for _ in 0..100 {
        if !seen.0.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let delivered = seen.0.lock().unwrap().clone();
    assert!(
        !delivered.is_empty(),
        "the document was never sent to the host"
    );
    let (title, bytes) = &delivered[0];
    assert_eq!(title, "Budget.xlsx", "named as the token named it");
    assert!(*bytes > 1000, "and it is a real package, not an empty one");
}

#[tokio::test]
async fn a_host_that_refuses_gets_the_participants_warned() {
    // On the first failure, not the last: a warning is only useful while there
    // is still time to copy the work out.
    let addr = start_saving(Arc::new(Refusing)).await;
    let mut ada = connect(addr).await;
    join(&mut ada, &saving_claims("Ada")).await.unwrap();
    say(
        &mut ada,
        &ClientMessage::Submit(Submission {
            client: casual_calc_transaction::session::ClientId(1),
            seq: 1,
            base: 0,
            ops: vec![cell_edit(42.0)],
        }),
    )
    .await;

    let mut warned = false;
    for _ in 0..40 {
        match tokio::time::timeout(std::time::Duration::from_millis(200), hear(&mut ada)).await {
            Ok(Some(ServerMessage::Refused {
                reason: Refusal::NotSaving,
                ..
            }))
            | Ok(Some(ServerMessage::Stopped {
                reason: Refusal::NotSaving,
            })) => {
                warned = true;
                break;
            }
            Ok(Some(_)) => continue,
            _ => break,
        }
    }
    assert!(
        warned,
        "participants were never told their work is not being saved"
    );
}

/// Read `/stats` — the node's own view of what it is holding.
async fn stats_of(addr: SocketAddr) -> Stats {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET /stats HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut out = Vec::new();
    let _ = stream.read_to_end(&mut out).await;
    let text = String::from_utf8_lossy(&out).into_owned();
    let body = text.rsplit("\r\n\r\n").next().unwrap_or_default();
    serde_json::from_str(body).unwrap_or_else(|e| panic!("stats body {body:?}: {e}"))
}

/// Poll until `f` holds, or give up. Time-based state needs a window rather
/// than a sleep, or the test is a race on a slow machine.
async fn until(addr: SocketAddr, f: impl Fn(Stats) -> bool) -> Stats {
    let mut last = stats_of(addr).await;
    for _ in 0..100 {
        if f(last) {
            return last;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        last = stats_of(addr).await;
    }
    last
}

#[tokio::test]
async fn a_document_nobody_is_in_is_eventually_let_go_of() {
    // Nothing removed a document from the registry, so a node held every
    // workbook it had ever opened until the OOM killer arrived. Asserted
    // against the node's own count: an earlier version of this test checked
    // that rejoining worked, which it does whether or not anything was
    // evicted — a mutation that deleted the eviction call passed it.
    let addr = start_saving(Arc::new(Collected::default())).await;
    {
        let mut ada = connect(addr).await;
        join(&mut ada, &saving_claims("Ada")).await.unwrap();
        assert_eq!(until(addr, |s| s.documents == 1).await.documents, 1);
        say(&mut ada, &ClientMessage::Leave).await;
    }
    let after = until(addr, |s| s.documents == 0).await;
    assert_eq!(after.documents, 0, "the document was never let go of");

    // And it reopens cleanly afterwards.
    let mut grace = connect(addr).await;
    assert!(matches!(
        join(&mut grace, &saving_claims("Grace")).await,
        Some(ServerMessage::Welcome { .. })
    ));
}

#[tokio::test]
async fn a_participant_who_stops_talking_is_forgotten() {
    // `Roster::expire` was written, tested and never called, so cursors
    // accumulated for people who had left.
    let addr = start_saving(Arc::new(Collected::default())).await;
    let mut ada = connect(addr).await;
    join(&mut ada, &saving_claims("Ada")).await.unwrap();
    assert_eq!(until(addr, |s| s.participants == 1).await.participants, 1);

    // Say nothing for longer than the TTL. The connection stays open, which is
    // the case that matters: a dropped socket is noticed anyway.
    let after = until(addr, |s| s.participants == 0).await;
    assert_eq!(
        after.participants, 0,
        "a silent participant was never expired"
    );
}

#[tokio::test]
async fn a_document_with_unsaved_work_is_not_evicted() {
    // Letting go of a document with work outstanding loses exactly the thing
    // the lifecycle exists to deliver.
    let addr = start_saving(Arc::new(Refusing)).await;
    let mut ada = connect(addr).await;
    join(&mut ada, &saving_claims("Ada")).await.unwrap();
    say(
        &mut ada,
        &ClientMessage::Submit(Submission {
            client: casual_calc_transaction::session::ClientId(1),
            seq: 1,
            base: 0,
            ops: vec![cell_edit(42.0)],
        }),
    )
    .await;
    hear(&mut ada).await;
    say(&mut ada, &ClientMessage::Leave).await;

    // The host refuses every save, so the work stays outstanding. Well past the
    // eviction window, the document must still be here.
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert_eq!(
        stats_of(addr).await.documents,
        1,
        "a document with unsaved work was evicted, losing it"
    );
}

#[tokio::test]
async fn a_full_node_turns_a_new_document_away_rather_than_degrading() {
    let addr = start_with(
        Arc::new(Canned(package())),
        Arc::new(Collected::default()),
        Limits {
            max_documents: 0,
            ..Limits::default()
        },
    )
    .await;
    let mut socket = connect(addr).await;
    let answer = join(&mut socket, &claims("Ada", Access::Edit)).await;
    assert!(
        matches!(answer, Some(ServerMessage::Stopped { .. })),
        "got {answer:?}"
    );
}

#[tokio::test]
async fn a_full_document_turns_the_next_arrival_away() {
    let addr = start_with(
        Arc::new(Canned(package())),
        Arc::new(Collected::default()),
        Limits {
            max_participants: 1,
            ..Limits::default()
        },
    )
    .await;
    let mut ada = connect(addr).await;
    assert!(matches!(
        join(&mut ada, &claims("Ada", Access::Edit)).await,
        Some(ServerMessage::Welcome { .. })
    ));

    let mut grace = connect(addr).await;
    let answer = join(&mut grace, &claims("Grace", Access::Edit)).await;
    assert!(
        matches!(answer, Some(ServerMessage::Stopped { .. })),
        "the second arrival is refused rather than everyone degrading: {answer:?}"
    );
}
