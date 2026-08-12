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
use casual_calc_transaction::session::{Base, SnapshotPolicy, Submission};
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
        membership: None,
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

/// The next message that is not the `Opening` notice.
///
/// Every join now begins with one — the server says the token was accepted
/// before it goes to the integrator for the document — and a test that cares
/// what the *answer* was should not have to restate that each time. The notice
/// itself is asserted on directly, once, in
/// [`a_join_is_acknowledged_before_the_document_is_fetched`].
async fn hear_past_opening(socket: &mut Socket) -> Option<ServerMessage> {
    match hear(socket).await {
        Some(ServerMessage::Opening { .. }) => hear(socket).await,
        other => other,
    }
}

/// Join, and return the `Welcome`.
async fn join(socket: &mut Socket, claims: &Claims) -> Option<ServerMessage> {
    say(
        socket,
        &ClientMessage::Join {
            protocol: PROTOCOL_VERSION,
            token: token(claims),
            resume: None,
        },
    )
    .await;
    hear_past_opening(socket).await
}

/// Join offering a resume key, and return whatever the server answers.
async fn join_resuming(
    socket: &mut Socket,
    claims: &Claims,
    key: &str,
    revision: u64,
) -> Option<ServerMessage> {
    say(
        socket,
        &ClientMessage::Join {
            protocol: PROTOCOL_VERSION,
            token: token(claims),
            resume: Some(Resume {
                key: key.to_owned(),
                revision,
            }),
        },
    )
    .await;
    hear_past_opening(socket).await
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
        strings: Default::default(),
    }
}

/// An edit to a named row, so two writers can be told apart.
fn cell_at(row: u32, value: f64) -> WireOperation {
    WireOperation {
        op: Operation::SetCell {
            sheet: 0,
            at: CellRef::new(row, 0),
            cell: Some(Cell::value(CellValue::Number(value))),
        },
        formulas: Default::default(),
        styles: Default::default(),
        strings: Default::default(),
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
        strings: Default::default(),
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
            base: Base::Revision(0),
            ops: vec![cell_edit(42.0)],
        }),
    )
    .await;

    // Ada is acked; the ack is what tells her the edit is ordered.
    let Some(ServerMessage::Ack {
        through: seq,
        revision,
    }) = hear(&mut ada).await
    else {
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
            base: Base::Revision(0),
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
            resume: None,
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
            resume: None,
        },
    )
    .await;
    let answer = hear(&mut socket).await;
    // `Stopped`, not `Refused`. A refusal invites a retry, and a client that
    // retries a version mismatch is refused identically for as long as it keeps
    // trying — a permanent reconnect loop against a server that can never
    // accept it.
    let Some(ServerMessage::Stopped {
        reason: Refusal::ProtocolVersion { server, client },
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
            base: Base::Revision(0),
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
            base: Base::Revision(0),
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
            base: Base::Revision(1),
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
        membership: None,
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
            base: Base::Revision(0),
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
            base: Base::Revision(0),
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
            base: Base::Revision(0),
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

// --- Shutdown drains rather than dropping (PROD-06) ------------------------

/// Start a server that can be told to stop, returning its address and handle.
type Serving = tokio::task::JoinHandle<()>;

async fn start_stoppable(deliver: Arc<dyn Deliver>) -> (SocketAddr, Shutdown, Serving) {
    start_stoppable_with(
        deliver,
        Limits {
            tick_ms: 10,
            ..Limits::default()
        },
    )
    .await
}

async fn start_stoppable_with(
    deliver: Arc<dyn Deliver>,
    limits: Limits,
) -> (SocketAddr, Shutdown, Serving) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = Shutdown::new();
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
        // A long quiesce, so nothing saves on the ordinary cadence and the only
        // save that can happen is the one shutdown forces.
        save: SavePolicy {
            quiesce_ms: 600_000,
            ceiling_ms: 600_000,
            ..SavePolicy::default()
        },
        snapshots: SnapshotPolicy::default(),
        fetch: Arc::new(Canned(package())),
        deliver,
        limits,
        membership: None,
    };
    let handle = shutdown.clone();
    let serving = tokio::spawn(async move {
        let _ = serve_on_with_shutdown(listener, config, handle).await;
    });
    (addr, shutdown, serving)
}

/// Wait for the service to finish shutting down — including the drain, which
/// happens *after* the listener closes. Waiting on the port instead measures
/// only that `serve` returned, which it does whatever the drain is doing.
async fn finished(serving: Serving) -> bool {
    tokio::time::timeout(std::time::Duration::from_secs(5), serving)
        .await
        .is_ok()
}

#[tokio::test]
async fn shutting_down_saves_the_work_that_was_outstanding() {
    // The data-loss case. A rolling deploy that drops connections has
    // inconvenienced people; one that drops connections with unsaved edits
    // behind them has lost their work — and the lifecycle's own cadence is no
    // help, because it is waiting for a quiesce that will never come.
    let seen = Collected::default();
    let (addr, shutdown, serving) = start_stoppable(Arc::new(seen.clone())).await;

    let mut ada = connect(addr).await;
    join(&mut ada, &saving_claims("Ada")).await.unwrap();
    say(
        &mut ada,
        &ClientMessage::Submit(Submission {
            client: casual_calc_transaction::session::ClientId(1),
            seq: 1,
            base: Base::Revision(0),
            ops: vec![cell_edit(42.0)],
        }),
    )
    .await;
    hear(&mut ada).await;

    // Nothing has been saved: the cadence is ten minutes away.
    assert!(
        seen.0.lock().unwrap().is_empty(),
        "the ordinary cadence must not have fired, or this proves nothing"
    );

    shutdown.begin();
    drop(ada);
    assert!(
        finished(serving).await,
        "the service never finished shutting down"
    );

    let delivered = seen.0.lock().unwrap().clone();
    assert_eq!(
        delivered.len(),
        1,
        "expected exactly one final save, got {delivered:?} — more than one \
         means the sweeper is still running alongside the drain and the host \
         receives the same document twice"
    );
    assert_eq!(delivered[0].0, "Budget.xlsx");
}

#[tokio::test]
async fn shutting_down_with_nothing_outstanding_saves_nothing() {
    // Draining must not manufacture a save. A host that receives a spurious
    // callback for a document nobody edited has to reason about why.
    let seen = Collected::default();
    let (addr, shutdown, serving) = start_stoppable(Arc::new(seen.clone())).await;
    let mut ada = connect(addr).await;
    join(&mut ada, &saving_claims("Ada")).await.unwrap();

    shutdown.begin();
    drop(ada);
    assert!(finished(serving).await);
    assert!(
        seen.0.lock().unwrap().is_empty(),
        "nothing was edited, so nothing should have been sent: {:?}",
        seen.0.lock().unwrap()
    );
}

#[tokio::test]
async fn a_host_that_hangs_does_not_stop_the_node_exiting() {
    // Bounded best effort: a node that refuses to exit is a worse failure than
    // one that exits having tried, and the host is often being restarted too.
    struct Hangs;
    impl Deliver for Hangs {
        fn put(
            &self,
            _d: Callback,
            _t: String,
            _b: Vec<u8>,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> {
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_secs(3_600)).await;
                Ok(())
            })
        }
    }

    let (addr, shutdown, serving) = start_stoppable_with(
        Arc::new(Hangs),
        Limits {
            tick_ms: 10,
            drain_timeout_ms: 50,
            ..Limits::default()
        },
    )
    .await;
    let mut ada = connect(addr).await;
    join(&mut ada, &saving_claims("Ada")).await.unwrap();
    say(
        &mut ada,
        &ClientMessage::Submit(Submission {
            client: casual_calc_transaction::session::ClientId(1),
            seq: 1,
            base: Base::Revision(0),
            ops: vec![cell_edit(42.0)],
        }),
    )
    .await;
    hear(&mut ada).await;

    // The drain is bounded, so the node finishes despite the host never
    // answering. Asserted on the service *task* finishing rather than the port
    // closing: the listener goes as soon as `serve` returns, which happens
    // before the drain runs and therefore proves nothing about it.
    shutdown.begin();
    drop(ada);
    assert!(
        finished(serving).await,
        "the node never finished: a hanging host held the drain open"
    );
}

// --- TLS is wired, not merely modelled (PROD-06) ---------------------------

/// Write a self-signed certificate and key, and a CA, into a temp directory.
///
/// Generated rather than committed: a checked-in private key is a private key
/// somebody will eventually use somewhere real.
fn certificate_files() -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("opencalc-tls-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    if !cert_path.exists() {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        std::fs::write(&key_path, cert.key_pair.serialize_pem()).unwrap();
    }
    (cert_path, key_path)
}

#[tokio::test]
async fn a_tls_endpoint_produces_a_usable_server_configuration() {
    // Exposure modelled TLS with validation and startup warnings, and `serve`
    // ignored all of it — so the TLS choice was documentation.
    let (cert, key) = certificate_files();
    let endpoint = crate::config::Endpoint::secured("127.0.0.1:0".parse().unwrap(), cert, key);
    assert!(
        tls_config(&endpoint).is_ok(),
        "a well-formed endpoint must produce a configuration"
    );
}

#[tokio::test]
async fn a_plain_endpoint_is_refused_rather_than_silently_downgraded() {
    let endpoint = crate::config::Endpoint::plain("127.0.0.1:0".parse().unwrap());
    assert!(tls_config(&endpoint).is_err());
}

#[tokio::test]
async fn a_missing_or_empty_certificate_is_named_rather_than_guessed_at() {
    let (cert, key) = certificate_files();
    let missing = crate::config::Endpoint::secured(
        "127.0.0.1:0".parse().unwrap(),
        "/nonexistent/cert.pem".into(),
        key.clone(),
    );
    let err = tls_config(&missing).unwrap_err();
    assert!(err.contains("cert.pem"), "the file must be named: {err}");

    // A file that exists and holds nothing useful is the more confusing case.
    let empty = std::env::temp_dir().join("opencalc-empty.pem");
    std::fs::write(&empty, b"not a certificate").unwrap();
    let err = tls_config(&crate::config::Endpoint::secured(
        "127.0.0.1:0".parse().unwrap(),
        empty,
        key,
    ))
    .unwrap_err();
    assert!(err.contains("no certificate"), "got {err}");

    // And a certificate with no key behind it.
    let err = tls_config(&crate::config::Endpoint::secured(
        "127.0.0.1:0".parse().unwrap(),
        cert,
        "/nonexistent/key.pem".into(),
    ))
    .unwrap_err();
    assert!(err.contains("key.pem"), "got {err}");
}

#[tokio::test]
async fn requiring_a_client_certificate_builds_a_verifier() {
    // The half that is easy to configure and forget: TLS proves the traffic is
    // private, and a client CA is what proves the peer is one of yours.
    let (cert, key) = certificate_files();
    let mutual =
        crate::config::Endpoint::secured("127.0.0.1:0".parse().unwrap(), cert.clone(), key)
            .requiring_client_certificate(cert);
    assert!(tls_config(&mutual).is_ok());
}

#[tokio::test]
async fn a_client_ca_that_holds_no_certificate_is_refused() {
    // Otherwise the endpoint comes up demanding client certificates it can
    // never accept, which reads as a client problem and is a configuration one.
    let (cert, key) = certificate_files();
    let empty = std::env::temp_dir().join("opencalc-empty-ca.pem");
    std::fs::write(&empty, b"nothing here").unwrap();
    let endpoint = crate::config::Endpoint::secured("127.0.0.1:0".parse().unwrap(), cert, key)
        .requiring_client_certificate(empty);
    let err = tls_config(&endpoint).unwrap_err();
    assert!(err.contains("no CA certificate"), "got {err}");
}

// --- Client inactivity ------------------------------------------------------

/// A server that gives up on a quiet connection quickly.
async fn start_impatient() -> (SocketAddr, Shutdown, Serving) {
    start_stoppable_with(
        Arc::new(Collected::default()),
        Limits {
            tick_ms: 10,
            presence_ttl_ms: 10_000,
            client_ping_ms: 20,
            client_idle_ms: 100,
            ..Limits::default()
        },
    )
    .await
}

#[tokio::test]
async fn a_connection_nobody_answers_on_is_closed() {
    // The half-open case: a laptop that slept, a network that vanished. The
    // socket looks open to us and there is nobody on the far end, and without
    // this it stays that way forever — holding a slot, a subscription and a
    // place in the participant cap.
    //
    // The client here stops polling its stream, so tungstenite never sends the
    // pong it would otherwise send automatically. That is exactly what a
    // vanished peer looks like from here.
    let (addr, _shutdown, _serving) = start_impatient().await;
    let mut socket = connect(addr).await;
    join(&mut socket, &claims("Ada", Access::Edit))
        .await
        .unwrap();
    assert_eq!(until(addr, |s| s.participants == 1).await.participants, 1);

    // Say nothing and answer nothing.
    let gone = until(addr, |s| s.participants == 0).await;
    assert_eq!(
        gone.participants, 0,
        "a connection nobody is on was held open"
    );
}

#[tokio::test]
async fn a_client_that_answers_pings_is_kept_even_while_it_says_nothing() {
    // The other half, and the one that matters more: closing a connection
    // because its user went to lunch is worse than holding it. A live client
    // answers a WebSocket ping without the page doing anything, so reading the
    // socket is enough to stay.
    let (addr, _shutdown, _serving) = start_impatient().await;
    let mut socket = connect(addr).await;
    join(&mut socket, &claims("Ada", Access::Edit))
        .await
        .unwrap();

    // Keep polling the stream — which is what answers the pings — for well past
    // the idle limit, without sending a single application message.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
    while std::time::Instant::now() < deadline {
        let _ = tokio::time::timeout(std::time::Duration::from_millis(30), socket.next()).await;
    }

    assert_eq!(
        stats_of(addr).await.participants,
        1,
        "a client that answers pings was dropped for being quiet"
    );
}

#[tokio::test]
async fn answering_keeps_a_participant_in_the_roster_rather_than_expiring_their_cursor() {
    // Presence and the connection must agree. Before this they did not: the
    // roster expired a quiet participant while their socket stayed open, so
    // the node held a connection that could still submit edits and belonged to
    // nobody as far as presence was concerned.
    let (addr, _shutdown, _serving) = start_stoppable_with(
        Arc::new(Collected::default()),
        Limits {
            tick_ms: 10,
            // A presence TTL far shorter than the idle limit, so the roster
            // would expire first if nothing refreshed it.
            presence_ttl_ms: 60,
            client_ping_ms: 20,
            client_idle_ms: 10_000,
            ..Limits::default()
        },
    )
    .await;

    let mut socket = connect(addr).await;
    join(&mut socket, &claims("Ada", Access::Edit))
        .await
        .unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
    while std::time::Instant::now() < deadline {
        let _ = tokio::time::timeout(std::time::Duration::from_millis(20), socket.next()).await;
    }

    assert_eq!(
        stats_of(addr).await.participants,
        1,
        "a connected, responsive participant was expired from the roster"
    );
}

// --- Resuming after a disconnect (ADR-015) ----------------------------------

#[tokio::test]
async fn a_reconnecting_participant_is_the_same_participant() {
    // The property everything else here rests on. The server suppresses
    // duplicate submissions by `(client, seq)`; if a reconnect were a new
    // client, a chunk the server had already committed and not yet
    // acknowledged would be committed a second time when the client resent it.
    let addr = start(Arc::new(Canned(package()))).await;
    let ada = claims("Ada", Access::Edit);

    let mut first = connect(addr).await;
    let Some(ServerMessage::Welcome { client: was, .. }) =
        join_resuming(&mut first, &ada, "key-ada", 0).await
    else {
        panic!("a first join gets a welcome, key or no key")
    };
    drop(first);

    let mut again = connect(addr).await;
    let Some(ServerMessage::Resumed { client: now, .. }) =
        join_resuming(&mut again, &ada, "key-ada", 0).await
    else {
        panic!("presenting a key the server issued should resume, not start over")
    };
    assert_eq!(
        now, was,
        "the same participant, so duplicate suppression holds"
    );
}

#[tokio::test]
async fn a_resumed_participant_is_given_what_it_missed_and_not_a_snapshot() {
    let addr = start(Arc::new(Canned(package()))).await;
    let ada = claims("Ada", Access::Edit);
    let bob = claims("Bob", Access::Edit);

    let mut hers = connect(addr).await;
    join_resuming(&mut hers, &ada, "key-ada", 0).await.unwrap();
    drop(hers);

    // While she is away, somebody else edits.
    let mut his = connect(addr).await;
    let Some(ServerMessage::Welcome {
        client, revision, ..
    }) = join(&mut his, &bob).await
    else {
        panic!("joined")
    };
    say(
        &mut his,
        &ClientMessage::Submit(Submission {
            client,
            seq: 1,
            base: Base::Revision(revision),
            ops: vec![cell_edit(42.0)],
        }),
    )
    .await;
    assert!(matches!(
        hear(&mut his).await,
        Some(ServerMessage::Ack { .. })
    ));

    let mut again = connect(addr).await;
    let Some(ServerMessage::Resumed {
        revision: caught_up,
        missed,
        ..
    }) = join_resuming(&mut again, &ada, "key-ada", 0).await
    else {
        panic!("resumed")
    };
    assert_eq!(missed.len(), 1, "exactly what happened while she was away");
    assert_eq!(caught_up, 1, "and where that leaves the document");
}

#[tokio::test]
async fn a_resend_after_reconnecting_is_recognised_rather_than_applied_twice() {
    // The whole point of ADR-015, end to end at the protocol level: a client
    // submits, the socket dies before the acknowledgement reaches it, and it
    // reconnects and sends the same chunk again because it cannot know whether
    // the first one landed.
    let addr = start(Arc::new(Canned(package()))).await;
    let ada = claims("Ada", Access::Edit);

    let mut first = connect(addr).await;
    let Some(ServerMessage::Welcome {
        client, revision, ..
    }) = join_resuming(&mut first, &ada, "key-ada", 0).await
    else {
        panic!("joined")
    };
    let chunk = Submission {
        client,
        seq: 1,
        base: Base::Revision(revision),
        ops: vec![cell_edit(7.0)],
    };
    say(&mut first, &ClientMessage::Submit(chunk.clone())).await;
    let Some(ServerMessage::Ack {
        revision: landed, ..
    }) = hear(&mut first).await
    else {
        panic!("committed")
    };
    // The acknowledgement is thrown away, standing in for a socket that died
    // between the server sending it and the client reading it — the exact
    // window in which a client cannot know what happened.
    drop(first);

    let mut again = connect(addr).await;
    let Some(ServerMessage::Resumed { .. }) = join_resuming(&mut again, &ada, "key-ada", 0).await
    else {
        panic!("resumed")
    };
    say(&mut again, &ClientMessage::Submit(chunk)).await;
    let Some(ServerMessage::Ack { revision: told, .. }) = hear(&mut again).await else {
        panic!("the resend is acknowledged rather than refused")
    };
    assert_eq!(
        told, landed,
        "told where it landed the first time, not committed a second time"
    );

    // And the document moved once, not twice.
    let mut watcher = connect(addr).await;
    let Some(ServerMessage::Welcome { revision: now, .. }) = join(&mut watcher, &ada).await else {
        panic!("joined")
    };
    assert_eq!(now, landed, "one edit, one revision");
}

#[tokio::test]
async fn a_resume_key_is_not_honoured_for_a_different_user() {
    // A key is a disambiguator, not a credential — but it must not be usable as
    // one either. Someone holding a valid token for this document who guessed
    // another participant's key could otherwise adopt their client id and have
    // that participant's submissions suppressed as duplicates of their own.
    let addr = start(Arc::new(Canned(package()))).await;
    let ada = claims("Ada", Access::Edit);
    let bob = claims("Bob", Access::Edit);

    let mut hers = connect(addr).await;
    let Some(ServerMessage::Welcome { client: ada_is, .. }) =
        join_resuming(&mut hers, &ada, "key-ada", 0).await
    else {
        panic!("joined")
    };

    let mut his = connect(addr).await;
    let answer = join_resuming(&mut his, &bob, "key-ada", 0).await;
    let Some(ServerMessage::Welcome { client: bob_is, .. }) = answer else {
        panic!("another user presenting her key starts afresh; got {answer:?}")
    };
    assert_ne!(bob_is, ada_is, "and is emphatically not her");
}

#[tokio::test]
async fn a_client_too_far_behind_to_catch_up_is_told_before_its_document_is_replaced() {
    // The bounded-offline edge of ADR-011. The work is still lost — what
    // changes is that it is lost audibly, so a host can offer to put the unsent
    // cells somewhere before the snapshot lands on top of them.
    let addr = start(Arc::new(Canned(package()))).await;
    let ada = claims("Ada", Access::Edit);

    // First, so the key is one the server recognises: a key it has never seen
    // is a participant joining for the first time, which has lost nothing and
    // must *not* be warned.
    let mut first = connect(addr).await;
    join_resuming(&mut first, &ada, "key-ada", 0).await.unwrap();
    drop(first);

    let mut socket = connect(addr).await;
    // A revision this document's history cannot reach back to — here by being
    // implausibly far ahead, which is the same unanswerable question as being
    // too far behind: there is no run of operations that gets from there to
    // here.
    let answer = join_resuming(&mut socket, &ada, "key-ada", 9_000).await;
    let Some(ServerMessage::Refused {
        reason: Refusal::TooFarBehind { .. },
        ..
    }) = answer
    else {
        panic!("said so first; got {answer:?}")
    };
    // And only then the fresh start.
    assert!(matches!(
        hear(&mut socket).await,
        Some(ServerMessage::Welcome { .. })
    ));
}

#[tokio::test]
async fn a_ping_is_answered_with_its_own_nonce() {
    // The client's only way to tell a live connection from a half-open one. The
    // nonce is the part that matters: without it a late answer to an earlier
    // ping satisfies the current one, and a connection that is failing reads as
    // healthy for as long as it keeps failing.
    let addr = start(Arc::new(Canned(package()))).await;
    let mut socket = connect(addr).await;
    join(&mut socket, &claims("Ada", Access::Edit))
        .await
        .unwrap();

    for nonce in [1u64, 2, 7_000_000] {
        say(&mut socket, &ClientMessage::Ping { nonce }).await;
        assert_eq!(
            hear(&mut socket).await,
            Some(ServerMessage::Pong { nonce }),
            "answered, and with the nonce that was asked"
        );
    }
}

#[tokio::test]
async fn a_viewer_may_still_ping() {
    // Liveness is not an edit. A read-only participant whose connection died
    // needs to find out as much as anyone — arguably more, since nothing else
    // they do would ever draw an answer out of the server.
    let addr = start(Arc::new(Canned(package()))).await;
    let mut socket = connect(addr).await;
    join(&mut socket, &claims("Vic", Access::View))
        .await
        .unwrap();
    say(&mut socket, &ClientMessage::Ping { nonce: 4 }).await;
    assert_eq!(
        hear(&mut socket).await,
        Some(ServerMessage::Pong { nonce: 4 })
    );
}

#[tokio::test]
async fn a_join_is_acknowledged_before_the_document_is_fetched() {
    // Opening a document means asking the integrator's server for it, which can
    // take as long as the configured HTTP timeout. Before this, the client saw
    // nothing during that wait — an open socket and silence, which is precisely
    // what a hung server looks like, and which a user answers by reloading and
    // starting a second wait alongside the first.
    let addr = start(Arc::new(Canned(package()))).await;
    let mut socket = connect(addr).await;
    say(
        &mut socket,
        &ClientMessage::Join {
            protocol: PROTOCOL_VERSION,
            token: token(&claims("Ada", Access::Edit)),
            resume: None,
        },
    )
    .await;

    let first = hear(&mut socket).await;
    let Some(ServerMessage::Opening { title }) = first else {
        panic!("the token being accepted is said first; got {first:?}")
    };
    // Named, so a client can show the wait against the document it is for.
    assert_eq!(title, "Budget.xlsx");
    assert!(matches!(
        hear(&mut socket).await,
        Some(ServerMessage::Welcome { .. })
    ));
}

#[tokio::test]
async fn a_document_is_fetched_once_however_many_arrive_at_the_same_moment() {
    // The start of a meeting: everybody opens the same workbook at once. Each
    // arrival used to fetch it for itself — thirty downloads of one file from
    // the integrator, twenty-nine thrown away, aimed at that server at the
    // moment it is already busiest. The race was settled after the fact, which
    // is fine for two and badly wrong for thirty.
    #[derive(Default)]
    struct Counting {
        bytes: Vec<u8>,
        calls: Arc<Mutex<usize>>,
    }
    impl Fetch for Counting {
        fn get(
            &self,
            _url: String,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send>> {
            *self.calls.lock().unwrap() += 1;
            let bytes = self.bytes.clone();
            Box::pin(async move {
                // Long enough that the others are certainly waiting on it,
                // which is the situation being tested.
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                Ok(bytes)
            })
        }
    }

    let calls = Arc::new(Mutex::new(0));
    let addr = start(Arc::new(Counting {
        bytes: package(),
        calls: Arc::clone(&calls),
    }))
    .await;

    let ada = claims("Ada", Access::Edit);
    let arrivals = (0..8).map(|_| {
        let ada = ada.clone();
        async move {
            let mut socket = connect(addr).await;
            let answer = join(&mut socket, &ada).await;
            assert!(
                matches!(answer, Some(ServerMessage::Welcome { .. })),
                "everybody gets in: {answer:?}"
            );
        }
    });
    futures_util::future::join_all(arrivals).await;

    assert_eq!(
        *calls.lock().unwrap(),
        1,
        "fetched once, shared by all of them"
    );
}

// --- Two nodes, one document (ADR-017) --------------------------------------
//
// The only arrangement in which a relay exists at all. Everything below this
// has been tested with one node, which is precisely the configuration where the
// relay code never runs.

/// Start a node that is part of a cluster, and return where to reach it.
async fn start_clustered(node: &str, namespace: &str) -> Option<SocketAddr> {
    let url = std::env::var("OPENCALC_TEST_REDIS").ok()?;
    let store = crate::cluster::redis::Redis::connect_within(&url, namespace)
        .await
        .ok()?;
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
        fetch: Arc::new(Canned(package())),
        deliver: Arc::new(Collected::default()),
        limits: Limits::default(),
        membership: Some(Membership {
            node: node.to_owned(),
            store: Arc::new(store),
            // Short, so a test that wants a leadership change does not wait on
            // a production-sized lease.
            lease_ms: 1_000,
        }),
    };
    tokio::spawn(async move {
        let _ = serve_on(listener, config).await;
    });
    Some(addr)
}

/// A namespace nothing else uses, so a real Redis can be run against repeatedly.
fn namespace(name: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "opencalc-two-node:{}:{}:{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

async fn connect_to(addr: SocketAddr) -> Socket {
    let url = format!("ws://{addr}/collab?doc={DOC}");
    let (socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    socket
}

/// Wait up to `within` for a message of interest, ignoring presence chatter.
///
/// Returns `None` on silence rather than panicking, unlike [`hear`]: a test
/// that reads until a socket goes quiet needs silence to be an answer.
async fn hear_edit(socket: &mut Socket, within: std::time::Duration) -> Option<ServerMessage> {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        let frame = tokio::time::timeout_at(deadline, socket.next())
            .await
            .ok()??;
        let tokio_tungstenite::tungstenite::Message::Text(text) = frame.ok()? else {
            continue;
        };
        match serde_json::from_str::<ServerMessage>(&text) {
            Ok(ServerMessage::Presence { .. } | ServerMessage::Departed { .. }) | Err(_) => {}
            Ok(other) => return Some(other),
        }
    }
}

#[tokio::test]
async fn an_edit_on_a_relay_is_ordered_by_the_leader_and_reaches_both() {
    let space = namespace("relayed-edit");
    let (Some(one), Some(two)) = (
        start_clustered("node-one", &space).await,
        start_clustered("node-two", &space).await,
    ) else {
        eprintln!("skipped: set OPENCALC_TEST_REDIS to a reachable server to run it");
        return;
    };

    let ada = claims("Ada", Access::Edit);
    let bob = claims("Bob", Access::Edit);
    let mut hers = connect_to(one).await;
    let mut his = connect_to(two).await;
    let Some(ServerMessage::Welcome {
        client: ada_is,
        revision,
        ..
    }) = join(&mut hers, &ada).await
    else {
        panic!("joined")
    };
    let Some(ServerMessage::Welcome { .. }) = join(&mut his, &bob).await else {
        panic!("joined")
    };

    // One of these two nodes leads and the other relays. Which is not knowable
    // from here and must not matter — that is the property.
    say(
        &mut hers,
        &ClientMessage::Submit(Submission {
            client: ada_is,
            seq: 1,
            base: Base::Revision(revision),
            ops: vec![cell_edit(64.0)],
        }),
    )
    .await;

    let acknowledged = hear_edit(&mut hers, std::time::Duration::from_secs(10)).await;
    assert!(
        matches!(acknowledged, Some(ServerMessage::Ack { through: 1, .. })),
        "the acknowledgement comes back however the edit was routed; got {acknowledged:?}"
    );

    let arrived = hear_edit(&mut his, std::time::Duration::from_secs(10)).await;
    let Some(ServerMessage::Apply { ops, .. }) = arrived else {
        panic!("the edit reached a client on the other node; got {arrived:?}")
    };
    assert_eq!(ops.len(), 1, "and it is the edit that was made");
}

#[tokio::test]
async fn both_nodes_can_write_and_neither_loses_the_other_s_edit() {
    let space = namespace("both-write");
    let (Some(one), Some(two)) = (
        start_clustered("node-one", &space).await,
        start_clustered("node-two", &space).await,
    ) else {
        eprintln!("skipped: set OPENCALC_TEST_REDIS to a reachable server to run it");
        return;
    };

    let ada = claims("Ada", Access::Edit);
    let bob = claims("Bob", Access::Edit);
    let mut hers = connect_to(one).await;
    let mut his = connect_to(two).await;
    let Some(ServerMessage::Welcome {
        client: ada_is,
        revision,
        ..
    }) = join(&mut hers, &ada).await
    else {
        panic!("joined")
    };
    let Some(ServerMessage::Welcome { client: bob_is, .. }) = join(&mut his, &bob).await else {
        panic!("joined")
    };

    say(
        &mut hers,
        &ClientMessage::Submit(Submission {
            client: ada_is,
            seq: 1,
            base: Base::Revision(revision),
            ops: vec![cell_at(20, 1.0)],
        }),
    )
    .await;
    say(
        &mut his,
        &ClientMessage::Submit(Submission {
            client: bob_is,
            seq: 1,
            base: Base::Revision(revision),
            ops: vec![cell_at(21, 2.0)],
        }),
    )
    .await;

    // Both are ordered, whichever node leads and whichever way round they land.
    for (who, socket) in [("Ada", &mut hers), ("Bob", &mut his)] {
        let mut acknowledged = false;
        let mut saw_the_other = false;
        while let Some(message) = hear_edit(socket, std::time::Duration::from_secs(5)).await {
            match message {
                ServerMessage::Ack { through: 1, .. } => acknowledged = true,
                ServerMessage::Apply { .. } => saw_the_other = true,
                _ => {}
            }
            if acknowledged && saw_the_other {
                break;
            }
        }
        assert!(acknowledged, "{who}'s own edit was never acknowledged");
        assert!(saw_the_other, "{who} never received the other's edit");
    }
}

#[tokio::test]
async fn a_single_clustered_node_still_serves_a_join() {
    // Isolating the join from the relay: if this fails, nothing about two nodes
    // is worth looking at yet.
    let Some(addr) = start_clustered("only", &namespace("single")).await else {
        eprintln!("skipped: set OPENCALC_TEST_REDIS to a reachable server to run it");
        return;
    };
    let mut socket = connect_to(addr).await;
    let answer = join(&mut socket, &claims("Ada", Access::Edit)).await;
    assert!(
        matches!(answer, Some(ServerMessage::Welcome { .. })),
        "a clustered node joins like any other; got {answer:?}"
    );
}

#[tokio::test]
async fn two_clustered_nodes_both_serve_a_join() {
    let space = namespace("two-joins");
    let (Some(one), Some(two)) = (
        start_clustered("node-one", &space).await,
        start_clustered("node-two", &space).await,
    ) else {
        eprintln!("skipped: set OPENCALC_TEST_REDIS to a reachable server to run it");
        return;
    };
    let mut hers = connect_to(one).await;
    let first = join(&mut hers, &claims("Ada", Access::Edit)).await;
    assert!(
        matches!(first, Some(ServerMessage::Welcome { .. })),
        "the first node; got {first:?}"
    );
    let mut his = connect_to(two).await;
    let second = join(&mut his, &claims("Bob", Access::Edit)).await;
    assert!(
        matches!(second, Some(ServerMessage::Welcome { .. })),
        "the second node; got {second:?}"
    );
}
