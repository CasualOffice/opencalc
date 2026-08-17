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
use casual_calc_transaction::protocol::{
    ClientMessage, Draft, PROTOCOL_VERSION, Refusal, ServerMessage,
};
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
/// A host that never answers, so the drain has to give up on its own.
#[derive(Clone, Default)]
struct Hanging(Arc<Mutex<usize>>);

impl Deliver for Hanging {
    fn put(
        &self,
        _destination: Callback,
        _title: String,
        _bytes: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> {
        *self.0.lock().unwrap() += 1;
        // Longer than any deadline a test will set. An integrator restarting at
        // the same moment as this node is exactly this: connections that open
        // and then say nothing.
        Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
            Ok(())
        })
    }
}

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

/// A service configuration with nothing unusual about it.
///
/// Extracted so the TLS test can take one and change a single field: a test
/// that hand-builds its own config drifts from this one, and then proves
/// something about a server nobody runs.
fn plain_config(addr: SocketAddr) -> ServiceConfig {
    ServiceConfig {
        bind: addr,
        verifier: Verifier::fixed(
            TokenPolicy {
                audience: "opencalc-collab".into(),
                leeway_secs: 60,
                allowed_hosts: BTreeSet::new(),
                require_https: true,
            },
            KeySet::shared_secret(SECRET),
        ),
        save: SavePolicy::default(),
        snapshots: SnapshotPolicy::default(),
        fetch: Arc::new(Canned(Vec::new())) as Arc<dyn Fetch>,
        deliver: Arc::new(Collected::default()) as Arc<dyn Deliver>,
        limits: Limits::default(),
        membership: None,
        tls: None,
    }
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
        fetch,
        deliver,
        limits,
        ..plain_config(addr)
    };
    tokio::spawn(async move {
        let _ = serve_on(listener, config).await;
    });
    addr
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Connect for a named document rather than the shared default.
///
/// The drain is about *several* documents, and `connect` pins one key — with it
/// they would all be the same session and the test would measure one save.
async fn connect_to_document(addr: SocketAddr, doc: &str) -> Socket {
    let url = format!("ws://{addr}/collab?doc={doc}");
    let (socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    socket
}

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
                // Presence is unsolicited: a joiner is told who is already in
                // the document, and anybody may move at any moment. A test
                // asking for "the reply to what I just did" wants the reply to
                // what it just did, so these are skipped rather than counted.
                // The tests that are *about* presence use `hear_presence`.
                match serde_json::from_str(&text).ok()? {
                    ServerMessage::Presence { .. } | ServerMessage::Departed { .. } => continue,
                    other => return Some(other),
                }
            }
            tokio_tungstenite::tungstenite::Message::Close(_) => return None,
            _ => continue,
        }
    }
}

/// The next presence-family message, for the tests that are about presence.
///
/// The counterpart to `hear` skipping them: something has to be able to read
/// them, and a test about presence should say so at its call site rather than
/// relying on presence happening to be next in the queue.
async fn hear_presence(socket: &mut Socket) -> Option<ServerMessage> {
    loop {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
            .await
            .expect("the server answered within five seconds")?;
        match frame.ok()? {
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                match serde_json::from_str(&text).ok()? {
                    message @ (ServerMessage::Presence { .. } | ServerMessage::Departed { .. }) => {
                        return Some(message);
                    }
                    _ => continue,
                }
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

/// **Somebody who joins is told who is already in the document.**
///
/// Presence is broadcast only when a participant *moves*, so without this a
/// joiner sees an empty room until one of the people already in it happens to
/// click something. Two people reading the same document saw no evidence of one
/// another at all — and the roster already knew, it was simply never asked on
/// the way in.
#[tokio::test]
async fn a_joiner_is_told_who_is_already_here() {
    let addr = start(Arc::new(Canned(package()))).await;

    let mut ada = connect(addr).await;
    join(&mut ada, &claims("Ada", Access::Edit)).await.unwrap();
    say(
        &mut ada,
        &ClientMessage::Presence {
            sheet: 2,
            selection: [4, 5, 6, 7],
            editing: None,
        },
    )
    .await;

    // Grace arrives to a document where Ada is sitting still.
    let mut grace = connect(addr).await;
    join(&mut grace, &claims("Grace", Access::Edit))
        .await
        .unwrap();

    let heard = hear_presence(&mut grace).await;
    let Some(ServerMessage::Presence {
        name,
        sheet,
        selection,
        ..
    }) = heard
    else {
        panic!("the joiner was told about nobody, got {heard:?}");
    };
    assert_eq!(name, "Ada");
    // Where Ada actually is, not where she was when she joined — the roster's
    // current entry, so a replay cannot show a stale cursor.
    assert_eq!(sheet, 2);
    assert_eq!(selection, [4, 5, 6, 7]);
}

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
            editing: None,
        },
    )
    .await;

    let heard = hear_presence(&mut ada).await;
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

// --- Drafts (COL-35) --------------------------------------------------------

/// The next presence-family message, or `None` if none arrives in `within`.
///
/// For the half of a relay claim that is about something *not* happening. The
/// blocking `hear_presence` cannot express "and nobody else was told", because
/// waiting for a message that must never come is a five-second timeout followed
/// by a panic.
async fn hear_presence_within(
    socket: &mut Socket,
    within: std::time::Duration,
) -> Option<ServerMessage> {
    tokio::time::timeout(within, hear_presence(socket))
        .await
        .ok()
        .flatten()
}

/// **What somebody is typing reaches the others, and does not come back to
/// them.**
///
/// The gap COL-35 names: a participant saw another's work only once it was
/// committed, so two people could fill the same cell and neither knew until one
/// of them lost. The draft rides presence (ADR-011) and so is relayed, not
/// transformed.
///
/// The no-echo half is not a nicety. An author whose own draft came back would
/// paint a second cursor on their own cell, one keystroke behind the one they
/// are actually typing into.
#[tokio::test]
async fn a_draft_reaches_the_others_and_is_not_echoed_to_whoever_typed_it() {
    let addr = start(Arc::new(Canned(package()))).await;

    let mut grace = connect(addr).await;
    join(&mut grace, &claims("Grace", Access::Edit))
        .await
        .unwrap();

    let mut ada = connect(addr).await;
    let welcome = join(&mut ada, &claims("Ada", Access::Edit)).await.unwrap();
    let ServerMessage::Welcome { client: ada_id, .. } = welcome else {
        panic!("expected a welcome, got {welcome:?}");
    };

    say(
        &mut ada,
        &ClientMessage::Presence {
            sheet: 0,
            selection: [3, 1, 3, 1],
            editing: Some(Draft::new(3, 1, "=SUM(A1:A")),
        },
    )
    .await;

    let heard = hear_presence(&mut grace).await;
    let Some(ServerMessage::Presence {
        client,
        name,
        editing,
        ..
    }) = heard
    else {
        panic!("the other participant was never told, got {heard:?}");
    };
    assert_eq!(client, ada_id);
    assert_eq!(name, "Ada", "still the name from the token");
    let draft = editing.expect("the draft itself, not merely that she is busy");
    assert_eq!(
        draft.text, "=SUM(A1:A",
        "the text is what was asked for: peers watch the value appear"
    );
    assert_eq!(draft.at, [3, 1]);

    // And Ada is told nothing about herself. Whatever she may still have queued
    // from joining — she was told who was already here — none of it is a draft.
    while let Some(message) =
        hear_presence_within(&mut ada, std::time::Duration::from_millis(400)).await
    {
        if let ServerMessage::Presence {
            client, editing, ..
        } = message
        {
            assert_ne!(
                client, ada_id,
                "a participant was sent their own draft back"
            );
            assert!(editing.is_none(), "and it was a draft at that");
        }
    }
}

/// **A draft is bounded by the server, whatever the client says.**
///
/// It arrives once per keystroke from a party the server has no reason to
/// trust, is held in memory for the presence TTL, and is drawn into a cell on
/// everybody else's grid. A client is asked to bound it; the server is what
/// makes the bound true.
#[tokio::test]
async fn a_draft_longer_than_the_bound_is_cut_back_before_anybody_sees_it() {
    let addr = start(Arc::new(Canned(package()))).await;

    let mut watcher = connect(addr).await;
    join(&mut watcher, &claims("Watcher", Access::Edit))
        .await
        .unwrap();
    let mut shouty = connect(addr).await;
    join(&mut shouty, &claims("Shouty", Access::Edit))
        .await
        .unwrap();

    // Constructed past the bound deliberately: `Draft::new` truncates, so the
    // message is built by hand the way another implementation would send it.
    let huge = serde_json::json!({
        "type": "presence",
        "sheet": 0,
        "selection": [0, 0, 0, 0],
        "editing": { "at": [0, 0], "text": "x".repeat(100_000) },
    });
    shouty
        .send(tokio_tungstenite::tungstenite::Message::Text(
            huge.to_string(),
        ))
        .await
        .unwrap();

    let heard = hear_presence(&mut watcher).await;
    let Some(ServerMessage::Presence {
        editing: Some(draft),
        ..
    }) = heard
    else {
        panic!("expected a draft, got {heard:?}");
    };
    assert_eq!(
        draft.text.chars().count(),
        Draft::MAX_TEXT,
        "the server relayed as much text as it was handed"
    );
}

/// **A participant who leaves takes their draft with them.**
///
/// An abandoned edit must cost nothing to clean up. The strong form of that is
/// not "the others were told" — they are, by `Departed` — but that the server
/// no longer holds it: somebody arriving afterwards must not be told about a
/// half-typed cell belonging to a person who is not here.
#[tokio::test]
async fn a_participant_who_leaves_takes_their_draft_out_of_the_roster() {
    let addr = start(Arc::new(Canned(package()))).await;

    let mut ada = connect(addr).await;
    let welcome = join(&mut ada, &claims("Ada", Access::Edit)).await.unwrap();
    let ServerMessage::Welcome { client: ada_id, .. } = welcome else {
        panic!("expected a welcome, got {welcome:?}");
    };
    say(
        &mut ada,
        &ClientMessage::Presence {
            sheet: 0,
            selection: [3, 1, 3, 1],
            editing: Some(Draft::new(3, 1, "abandoned")),
        },
    )
    .await;

    // Somebody arriving while she is typing is told about it — which is what
    // makes the assertion after her departure mean something.
    let mut grace = connect(addr).await;
    join(&mut grace, &claims("Grace", Access::Edit))
        .await
        .unwrap();
    let heard = hear_presence(&mut grace).await;
    let Some(ServerMessage::Presence {
        editing: Some(draft),
        ..
    }) = heard
    else {
        panic!("a joiner was not told what was being typed, got {heard:?}");
    };
    assert_eq!(draft.text, "abandoned");

    say(&mut ada, &ClientMessage::Leave).await;
    let heard = hear_presence(&mut grace).await;
    assert!(
        matches!(heard, Some(ServerMessage::Departed { client }) if client == ada_id),
        "her departure was never announced, got {heard:?}"
    );

    // And the roster no longer holds it: the next arrival is told about Grace,
    // who is not typing, and about nobody else.
    let mut cleo = connect(addr).await;
    join(&mut cleo, &claims("Cleo", Access::Edit))
        .await
        .unwrap();
    while let Some(message) =
        hear_presence_within(&mut cleo, std::time::Duration::from_millis(400)).await
    {
        if let ServerMessage::Presence {
            client, editing, ..
        } = message
        {
            assert_ne!(
                client, ada_id,
                "a departed participant was still in the roster"
            );
            assert!(editing.is_none(), "a ghost draft outlived its author");
        }
    }
}

/// **A participant who cannot edit cannot broadcast one.**
///
/// A draft is the preview of an edit, and a viewer has no edit to preview: the
/// server refuses their submissions at the operation (COL-17), so a draft from
/// one could never become anything. Relaying it would make presence the one
/// channel by which a read-only participant puts arbitrary text of their
/// choosing into everybody else's grid — which is a worse thing to have built
/// than the feature is to have.
///
/// Their cursor still travels. Being present is not editing.
#[tokio::test]
async fn a_viewer_may_be_seen_but_may_not_put_text_in_anybody_else_s_grid() {
    let addr = start(Arc::new(Canned(package()))).await;

    let mut editor = connect(addr).await;
    join(&mut editor, &claims("Editor", Access::Edit))
        .await
        .unwrap();
    let mut viewer = connect(addr).await;
    join(&mut viewer, &claims("Viewer", Access::View))
        .await
        .unwrap();

    say(
        &mut viewer,
        &ClientMessage::Presence {
            sheet: 0,
            selection: [9, 9, 9, 9],
            editing: Some(Draft::new(9, 9, "not mine to type")),
        },
    )
    .await;

    let heard = hear_presence(&mut editor).await;
    let Some(ServerMessage::Presence {
        selection, editing, ..
    }) = heard
    else {
        panic!("expected presence, got {heard:?}");
    };
    assert_eq!(selection, [9, 9, 9, 9], "a viewer is still visible");
    assert!(
        editing.is_none(),
        "a participant who may not edit had their text relayed anyway"
    );
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
        verifier: Verifier::fixed(
            TokenPolicy {
                audience: "opencalc-collab".into(),
                leeway_secs: 60,
                allowed_hosts: BTreeSet::new(),
                require_https: true,
            },
            KeySet::shared_secret(SECRET),
        ),
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
        tls: None,
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
        verifier: Verifier::fixed(
            TokenPolicy {
                audience: "opencalc-collab".into(),
                leeway_secs: 60,
                allowed_hosts: BTreeSet::new(),
                require_https: true,
            },
            KeySet::shared_secret(SECRET),
        ),
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
        tls: None,
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

/// DEP-05. The drain has to fit the grace period it is given.
///
/// It used to be sequential, each document getting its own `drain_timeout_ms`,
/// so thirty documents with unsaved work needed up to five minutes against
/// Docker's ten-second default stop grace and Kubernetes' thirty. Whatever had
/// not finished was SIGKILLed mid-drain — losing exactly the work the drain
/// exists to save.
///
/// Several documents against a host that never answers, which is what an
/// integrator restarting alongside this node looks like. The claim is that the
/// process still stops, and stops inside its budget, rather than being held open
/// by a callback that will never return.
#[tokio::test]
async fn the_drain_stops_inside_its_deadline_when_the_host_hangs() {
    let hanging = Hanging::default();
    let (addr, shutdown, serving) = start_stoppable_with(
        Arc::new(hanging.clone()),
        Limits {
            tick_ms: 10,
            // Short, so the test measures the mechanism rather than waiting on
            // production numbers. Sequentially this would still be six seconds
            // for six documents; the deadline is for all of them together.
            drain_deadline_ms: 1_000,
            drain_timeout_ms: 1_000,
            ..Limits::default()
        },
    )
    .await;

    // Six documents, each with an edit nobody has saved.
    let mut sockets = Vec::new();
    for n in 0..6u32 {
        let mut socket = connect_to_document(addr, &format!("drain-{n}")).await;
        let mut claims = saving_claims("Ada");
        claims.document.key = format!("drain-{n}");
        let Some(ServerMessage::Welcome {
            client, revision, ..
        }) = join(&mut socket, &claims).await
        else {
            panic!("joined drain-{n}")
        };
        say(
            &mut socket,
            &ClientMessage::Submit(Submission {
                client,
                seq: 1,
                base: Base::Revision(revision),
                ops: vec![cell_edit(n as f64)],
            }),
        )
        .await;
        let _ = hear(&mut socket).await;
        sockets.push(socket);
    }

    let began = std::time::Instant::now();
    shutdown.begin();
    assert!(
        finished(serving).await,
        "the process never stopped: a hanging host held the drain open"
    );
    let took = began.elapsed();

    // Generous against the 1s deadline — a loaded runner is slow — and far
    // under the six seconds a sequential drain would need, which is the
    // difference the change makes.
    assert!(
        took < std::time::Duration::from_secs(4),
        "the drain took {took:?}, which is the sequential shape rather than the deadline"
    );
    // And it genuinely tried, rather than passing by doing nothing: every
    // document was handed to the host before the deadline cut them off.
    let attempted = *hanging.0.lock().unwrap();
    assert!(
        attempted >= 2,
        "only {attempted} saves were attempted, so the deadline was not what stopped it"
    );
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

/// DEP-01. The listener must actually speak TLS when a certificate is
/// configured — the property the test below deliberately does not establish.
///
/// `tls_config` returning `Ok` proves a certificate parses, and for a long time
/// that was the entire coverage: nothing called it outside these tests, `serve`
/// bound a plain socket, and an operator who supplied a certificate got
/// plaintext WebSockets carrying document contents and bearer tokens. Both
/// things that should have revealed it agreed with the mistake — startup logged
/// `tls = true` from the *configuration*, and `--healthcheck` downgraded to a
/// bare TCP connect whenever a certificate was set, so neither could tell
/// plaintext from TLS.
///
/// So this asserts from the outside, in both directions. A plaintext request
/// must **fail**, because that is the symptom the defect produced and the only
/// assertion that distinguishes a fixed server from a broken one; and a TLS
/// request must succeed, so a server that merely refuses everything cannot pass.
#[tokio::test]
async fn a_configured_certificate_means_the_socket_speaks_tls() {
    let (cert, key) = certificate_files();
    let endpoint = crate::config::Endpoint::secured("127.0.0.1:0".parse().unwrap(), cert, key);
    let tls = std::sync::Arc::new(tls_config(&endpoint).unwrap());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut config = plain_config(addr);
    config.tls = Some(tls);
    let shutdown = crate::net::Shutdown::new();
    let serving = tokio::spawn(crate::net::serve_on_with_shutdown(
        listener,
        config,
        shutdown.clone(),
    ));
    // The listener is open before `serve_on_with_shutdown` is called, so there
    // is no readiness race to wait out — but the accept loop is spawned, so give
    // it a moment to be the thing accepting.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let plain = reqwest::Client::new()
        .get(format!("http://{addr}/healthz"))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await;
    assert!(
        plain.is_err(),
        "the endpoint answered a plaintext request: {plain:?}"
    );

    let secure = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
        .get(format!("https://{addr}/healthz"))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .expect("a TLS request must be answered");
    assert!(secure.status().is_success());
    assert_eq!(secure.text().await.unwrap(), "ok");

    shutdown.begin();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), serving).await;
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

/// `/metrics` reports what happened, not a set of zeros.
///
/// Every number it carries was already being computed and thrown into a log
/// line; `/stats` returned two integers, so "are saves failing?" could only be
/// answered by tailing logs — which no alert can do (DEP-06). The assertion
/// that matters is therefore not that the endpoint parses, but that a save
/// actually moves a counter.
#[tokio::test]
async fn metrics_count_a_save_that_really_happened() {
    let seen = Collected::default();
    let addr = start_saving(Arc::new(seen.clone())).await;

    let mut socket = connect(addr).await;
    let claims = saving_claims("Ada");
    let Some(ServerMessage::Welcome {
        client, revision, ..
    }) = join(&mut socket, &claims).await
    else {
        panic!("joined")
    };

    // The delta is the claim, not the absolute value: this node saves on its
    // own cadence, so a fixed "before" number would be a race. A counter stuck
    // at any constant fails an increase assertion; one that never moves fails
    // it too.
    let before = counter(&scrape(addr).await, "opencalc_saves_accepted_total");

    say(
        &mut socket,
        &ClientMessage::Submit(Submission {
            client,
            seq: 1,
            base: Base::Revision(revision),
            ops: vec![cell_edit(7.0)],
        }),
    )
    .await;
    let _ = hear(&mut socket).await;

    // The save is driven by the sweeper on its own clock, so wait for the
    // delivery rather than for a duration.
    for _ in 0..80 {
        if !seen.0.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(!seen.0.lock().unwrap().is_empty(), "the document was saved");

    let body = scrape(addr).await;
    let after = counter(&body, "opencalc_saves_accepted_total");
    assert!(
        after > before,
        "the save moved the counter ({before} -> {after}): {body}"
    );
    assert_eq!(
        counter(&body, "opencalc_saves_failed_total"),
        0,
        "and was not counted as a failure: {body}"
    );
    // The gauges are read from the registry, so their *value* depends on
    // whether this document has been evicted yet — which this test does not
    // control and is not about. That they are exposed and parseable is the
    // claim; `evict_if_idle` has its own tests.
    let _ = counter(&body, "opencalc_documents");
    let _ = counter(&body, "opencalc_participants");
    // Prometheus needs the type lines, not only the numbers: a body without
    // them scrapes as nothing at all.
    assert!(body.contains("# TYPE opencalc_saves_accepted_total counter"));
    assert!(body.contains("# TYPE opencalc_documents gauge"));
}

/// One counter's value out of a Prometheus exposition.
fn counter(body: &str, name: &str) -> u64 {
    body.lines()
        .find_map(|l| l.strip_prefix(name)?.trim().parse().ok())
        .unwrap_or_else(|| panic!("{name} is not in:\n{body}"))
}

/// Standalone has no coordinator to be cut off from, so it is ready as soon as
/// it is listening.
///
/// What this pins is that readiness exists at all, and that a node with no
/// cluster is not reported unready for merely lacking one. The case that
/// actually moves the probe — a coordinator that goes away — is
/// `a_node_cut_off_from_the_coordinator_reports_itself_unready`, below.
#[tokio::test]
async fn a_standalone_node_is_ready_as_soon_as_it_listens() {
    let addr = start(Arc::new(Canned(package()))).await;
    let response = reqwest::get(format!("http://{addr}/readyz"))
        .await
        .expect("readyz answered");
    assert!(response.status().is_success());
    let body = response.text().await.unwrap();
    assert!(body.contains("standalone"), "{body}");

    // And liveness stays separate: `/healthz` is deliberately unconditional, so
    // a probe failure means "do not send traffic" rather than "restart this".
    let health = reqwest::get(format!("http://{addr}/healthz"))
        .await
        .expect("healthz answered");
    assert!(health.status().is_success());
}

/// **A clustered node that has lost the coordinator reports itself unready,
/// while staying alive.**
///
/// This is the distinction the two probes exist for, and the standalone test
/// above cannot show it. A node that cannot reach the coordinator cannot order
/// an edit, so every submission it accepts is one it will refuse — it should be
/// taken out of the load balancer. It should *not* be restarted: the fault is
/// not in this process, and cycling it loses every session it is holding for no
/// gain. So `/readyz` must move and `/healthz` must not.
///
/// The coordinator is taken away by connecting this node through a proxy the
/// test can cut, rather than by stopping the Redis every other test shares.
#[tokio::test]
async fn a_node_cut_off_from_the_coordinator_reports_itself_unready() {
    let Ok(url) = std::env::var("OPENCALC_TEST_REDIS") else {
        return;
    };
    let (through, cut) = proxy_to(&url).await;
    let space = namespace("cut-off");
    let Some(addr) = start_clustered_with(
        &through,
        "node-one",
        &space,
        SavePolicy::default(),
        Collected::default(),
    )
    .await
    else {
        return;
    };

    let before = reqwest::get(format!("http://{addr}/readyz"))
        .await
        .expect("readyz answered");
    assert!(
        before.status().is_success(),
        "a connected node is ready: {}",
        before.status()
    );

    // Cut every forwarded socket. redis-rs's multiplexed connection does not
    // reconnect on its own, which is exactly the production shape: the node
    // stays up, holding sessions, quietly unable to order anything.
    drop(cut);

    let after = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let r = reqwest::get(format!("http://{addr}/readyz"))
                .await
                .expect("readyz answered");
            if r.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                return r;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the node noticed the coordinator had gone");

    let body = after.text().await.unwrap();
    assert!(
        body.contains("coordinator"),
        "the probe says why, so an operator is not left guessing: {body}"
    );

    // Liveness is unchanged: this node must not be restarted for it.
    let health = reqwest::get(format!("http://{addr}/healthz"))
        .await
        .expect("healthz answered");
    assert!(
        health.status().is_success(),
        "a cut-off node is unready, not unhealthy: {}",
        health.status()
    );
}

/// Forward a fresh local port to `url`, until the returned handle is dropped.
///
/// Dropping it closes the forwarded sockets, which is how a test takes the
/// coordinator away from one node without taking it away from the rest of the
/// suite running against the same server.
async fn proxy_to(url: &str) -> (String, tokio::sync::broadcast::Sender<()>) {
    let target = url
        .trim_start_matches("redis://")
        .trim_end_matches('/')
        .to_owned();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    let (cut, rx) = tokio::sync::broadcast::channel::<()>(1);
    // Receivers, never a `Sender` clone: the signal *is* the last sender being
    // dropped, so a clone held in here would keep the channel open forever and
    // the cut would never arrive.
    tokio::spawn(async move {
        let mut stop = rx;
        loop {
            let accepted = tokio::select! {
                a = listener.accept() => a,
                _ = stop.recv() => return,
            };
            let Ok((mut client, _)) = accepted else {
                return;
            };
            let target = target.clone();
            let mut stop = stop.resubscribe();
            tokio::spawn(async move {
                let Ok(mut server) = tokio::net::TcpStream::connect(&target).await else {
                    return;
                };
                tokio::select! {
                    _ = tokio::io::copy_bidirectional(&mut client, &mut server) => {}
                    _ = stop.recv() => {}
                }
            });
        }
    });
    (format!("redis://{at}"), cut)
}

async fn scrape(addr: SocketAddr) -> String {
    reqwest::get(format!("http://{addr}/metrics"))
        .await
        .expect("metrics answered")
        .text()
        .await
        .expect("a body")
}

// --- Two nodes, one document (ADR-017) --------------------------------------
//
// The only arrangement in which a relay exists at all. Everything below this
// has been tested with one node, which is precisely the configuration where the
// relay code never runs.

/// Start a node that is part of a cluster, and return where to reach it.
async fn start_clustered(node: &str, namespace: &str) -> Option<SocketAddr> {
    start_clustered_watching(node, namespace, SavePolicy::default())
        .await
        .map(|(addr, _)| addr)
}

/// The same, keeping hold of what this node delivered to the host.
///
/// `start_clustered` builds its `Collected` and drops it, so a test can see
/// where a node's edits went but not whether that node *saved* — which is the
/// only way to observe DEP-02.
async fn start_clustered_watching(
    node: &str,
    namespace: &str,
    save: SavePolicy,
) -> Option<(SocketAddr, Collected)> {
    let delivered = Collected::default();
    let url = std::env::var("OPENCALC_TEST_REDIS").ok()?;
    let addr = start_clustered_with(&url, node, namespace, save, delivered.clone()).await?;
    Some((addr, delivered))
}

async fn start_clustered_with(
    url: &str,
    node: &str,
    namespace: &str,
    save: SavePolicy,
    delivered: Collected,
) -> Option<SocketAddr> {
    let store = crate::cluster::redis::Redis::connect_within(url, namespace)
        .await
        .ok()?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let config = ServiceConfig {
        bind: addr,
        verifier: Verifier::fixed(
            TokenPolicy {
                audience: "opencalc-collab".into(),
                leeway_secs: 60,
                allowed_hosts: BTreeSet::new(),
                require_https: true,
            },
            KeySet::shared_secret(SECRET),
        ),
        save,
        snapshots: SnapshotPolicy::default(),
        fetch: Arc::new(Canned(package())),
        deliver: Arc::new(delivered),
        limits: Limits::default(),
        membership: Some(Membership {
            node: node.to_owned(),
            store: Arc::new(store),
            // Short, so a test that wants a leadership change does not wait on
            // a production-sized lease.
            lease_ms: 1_000,
            advertise: format!("10.0.0.1:9443/{node}"),
        }),
        tls: None,
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

/// DEP-04. A refused append is told to the person whose edit it was.
///
/// `order` commits locally *before* appending, so when the log refuses it the
/// node holds an edit the cluster does not have. Returning in silence — which
/// is what it did — left the author watching their change sit on screen,
/// un-acknowledged, resent forever against a node that believed it had landed.
/// The work is at risk, and `NotSaving` is exactly that statement.
///
/// Fenced deliberately rather than by unplugging Redis: a stolen lease is the
/// reachable, deterministic way to make an append fail, and it is the same
/// refusal path a genuine outage takes.
#[tokio::test]
async fn a_refused_append_is_reported_to_the_client_rather_than_swallowed() {
    let space = namespace("refused-append");
    let Some(addr) = start_clustered("node-one", &space).await else {
        eprintln!("skipped: set OPENCALC_TEST_REDIS to a reachable server to run it");
        return;
    };
    let mut socket = connect_to(addr).await;
    let Some(ServerMessage::Welcome {
        client, revision, ..
    }) = join(&mut socket, &claims("Ada", Access::Edit)).await
    else {
        panic!("joined")
    };

    // **Leadership is taken on the first edit, not on the join** — `order`
    // claims when it has something to order, deliberately, to save a round trip
    // on documents nobody edits. So the first submission is what makes this node
    // the leader; waiting for the lease before submitting waits for something
    // that only the renewal loop might do, which is why an earlier version of
    // this test passed alone and failed in a full run.
    say(
        &mut socket,
        &ClientMessage::Submit(Submission {
            client,
            seq: 1,
            base: Base::Revision(revision),
            ops: vec![cell_edit(1.0)],
        }),
    )
    .await;
    let first = hear_edit(&mut socket, std::time::Duration::from_secs(10)).await;
    assert!(
        matches!(first, Some(ServerMessage::Ack { through: 1, .. })),
        "the node leads and the first edit landed; got {first:?}"
    );

    let store = crate::cluster::redis::Redis::connect_within(
        &std::env::var("OPENCALC_TEST_REDIS").unwrap(),
        &space,
    )
    .await
    .expect("a store");
    assert_eq!(
        store.holder_of(DOC).await.as_deref(),
        Some("node-one"),
        "the first edit made this node the leader"
    );

    // Now take the lease away. Claiming far in the future expires what it holds
    // and bumps the epoch, which is what fences its next append — the same
    // refusal path a genuine Redis outage takes, reached deterministically.
    let stolen = store
        .claim(
            DOC.to_owned(),
            "an-intruder".to_owned(),
            60_000,
            now_ms() + 600_000,
        )
        .await
        .expect("stole the lease");
    assert!(
        stolen.epoch >= 2,
        "the epoch moved past this node's: {stolen:?}"
    );

    say(
        &mut socket,
        &ClientMessage::Submit(Submission {
            client,
            seq: 2,
            base: Base::Revision(revision + 1),
            ops: vec![cell_edit(64.0)],
        }),
    )
    .await;

    // The claim: something comes back, and it says the work is not being saved.
    // Silence is the defect, so a timeout here is the failure this test exists
    // to catch.
    let answer = hear_edit(&mut socket, std::time::Duration::from_secs(10)).await;
    assert!(
        matches!(
            answer,
            Some(ServerMessage::Refused {
                reason: Refusal::NotSaving,
                ..
            })
        ),
        "the author was told their edit did not land; got {answer:?}"
    );
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

/// DEP-02. Exactly one node returns the document to the host — the one that
/// leads it.
///
/// `leads` had two callers, the submit path and the inbox, so **ordering** was
/// leadership-gated and **saving** was not. Every node holding the document ran
/// the same save cadence, so each assembled the whole workbook and POSTed it.
/// The cost is N times the CPU and N times the callback traffic; the danger is
/// that a node momentarily behind delivers an older package *after* the leader
/// delivered a newer one, and a host that writes what arrives keeps the older.
///
/// A short quiesce so the save fires while the test is watching, and both nodes
/// get the same policy — a replica held back by a longer timer would pass this
/// for the wrong reason.
///
/// Which node leads is not knowable from here and must not matter: the
/// assertion is **exactly one**, never "node-one".
#[tokio::test]
async fn only_the_leader_returns_the_document_to_the_host() {
    let space = namespace("leader-saves");
    let brisk = SavePolicy {
        quiesce_ms: 300,
        ..SavePolicy::default()
    };
    let (Some((one, saved_by_one)), Some((two, saved_by_two))) = (
        start_clustered_watching("node-one", &space, brisk).await,
        start_clustered_watching("node-two", &space, brisk).await,
    ) else {
        eprintln!("skipped: set OPENCALC_TEST_REDIS to a reachable server to run it");
        return;
    };

    // Tokens that carry a callback, or the lifecycle records the save as
    // accepted without delivering anything and the assertion below could never
    // distinguish one saver from two.
    let ada = saving_claims("Ada");
    let bob = saving_claims("Bob");
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

    // Both nodes have the edit before either could have saved it: the relaying
    // node adopts it, which is the state that used to start its own save.
    let acknowledged = hear_edit(&mut hers, std::time::Duration::from_secs(10)).await;
    assert!(
        matches!(acknowledged, Some(ServerMessage::Ack { through: 1, .. })),
        "the edit was ordered; got {acknowledged:?}"
    );
    let arrived = hear_edit(&mut his, std::time::Duration::from_secs(10)).await;
    assert!(
        matches!(arrived, Some(ServerMessage::Apply { .. })),
        "and reached the other node; got {arrived:?}"
    );

    // Long enough for the quiesce timer and several sweeper ticks on both nodes.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let from_one = saved_by_one.0.lock().unwrap().len();
    let from_two = saved_by_two.0.lock().unwrap().len();
    assert!(
        from_one + from_two > 0,
        "nobody saved the document at all, so this proves nothing about who did"
    );
    assert!(
        from_one == 0 || from_two == 0,
        "both nodes returned the document to the host: {from_one} and {from_two}"
    );
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

#[tokio::test]
async fn a_node_that_missed_a_publication_catches_up_without_being_prompted() {
    // The gap ADR-017 named and did not close. A batch is written straight into
    // the log, bypassing the channel entirely — which is what a leader dying
    // between its append and its publish looks like from every other node. The
    // usual gap detection cannot help, because it fires on the *next*
    // publication and there is no next: the document goes quiet.
    //
    // Without the periodic reconciliation, the client below waits forever and
    // is shown a document that is silently a revision behind.
    let space = namespace("reconcile");
    let Some(addr) = start_clustered("node-one", &space).await else {
        eprintln!("skipped: set OPENCALC_TEST_REDIS to a reachable server to run it");
        return;
    };
    let mut socket = connect_to(addr).await;
    let Some(ServerMessage::Welcome { .. }) = join(&mut socket, &claims("Ada", Access::Edit)).await
    else {
        panic!("joined")
    };

    // Wait for this node to hold the lease, so the append below is not fenced.
    let store = crate::cluster::redis::Redis::connect_within(
        &std::env::var("OPENCALC_TEST_REDIS").unwrap(),
        &space,
    )
    .await
    .expect("connected");
    let epoch = loop {
        let lease = store
            .claim(DOC.to_owned(), "node-one".to_owned(), 1_000, now_ms())
            .await
            .expect("claimed");
        if lease.node == "node-one" {
            break lease.epoch;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };

    let batch = crate::relay::Committed {
        revision: 1,
        node: "somebody-else".to_owned(),
        client: ClientId(999),
        seq: 1,
        ops: vec![cell_at(30, 7.0)],
    };
    store
        .append(
            DOC.to_owned(),
            epoch,
            0,
            1,
            serde_json::to_vec(&batch).unwrap(),
            now_ms(),
        )
        .await
        .expect("appended straight to the log, and never published");

    // Nothing else happens on this document. The only thing that can rescue it
    // is the node reconciling against the log of its own accord.
    let arrived = hear_edit(&mut socket, std::time::Duration::from_secs(15)).await;
    let Some(ServerMessage::Apply { revision, ops }) = arrived else {
        panic!("a quiet document left this node behind forever; got {arrived:?}")
    };
    assert_eq!(revision, 1);
    assert_eq!(ops.len(), 1, "and it is the batch that was never announced");
}

#[tokio::test]
async fn a_node_announces_itself_and_says_how_loaded_it_is() {
    // `peers` and `elect` were built and tested and called by nothing, because
    // no node ever registered — both returned empty forever, and the cluster
    // worked regardless, since leadership is a lease and a lease needs no
    // discovery. A gap that looks exactly like working.
    let space = namespace("announce");
    let Some(addr) = start_clustered("node-one", &space).await else {
        eprintln!("skipped: set OPENCALC_TEST_REDIS to a reachable server to run it");
        return;
    };
    let store = crate::cluster::redis::Redis::connect_within(
        &std::env::var("OPENCALC_TEST_REDIS").unwrap(),
        &space,
    )
    .await
    .expect("connected");

    // Open a document, so the load this node reports is not zero for want of
    // anything to count.
    let mut socket = connect_to(addr).await;
    join(&mut socket, &claims("Ada", Access::Edit))
        .await
        .unwrap();

    let found = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let peers = store.peers(now_ms()).await.expect("the store answered");
            if let Some(peer) = peers.into_iter().find(|p| p.id == "node-one")
                && peer.load > 0
            {
                return peer;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("the node announced itself, with a load, within twenty seconds");

    assert_eq!(
        found.load, 1,
        "the load is the document count, not a constant"
    );
    assert!(
        found.advertise.contains("10.0.0.1"),
        "peers are told the internal address, not the public one: {}",
        found.advertise
    );
}

/// **A connection that never authenticates is dropped, and is bounded.**
///
/// The upgrade cannot require a token — the token arrives in the first frame —
/// so anyone who can reach the port could open sockets and simply never speak.
/// Each one completed the upgrade, spawned a task, and parked in `authorise`'s
/// `recv().await` with no deadline: the heartbeat and idle timers only start
/// *after* a successful join, and every limit in `Limits` is per-document or
/// per-message, so an unauthenticated connection was attributed to nothing and
/// counted against nothing. They accumulate to the process's descriptor limit,
/// at which point real joins fail at accept — while `/healthz` goes on
/// answering "ok" and the load balancer keeps sending traffic to a node that
/// can no longer take any.
#[tokio::test]
async fn a_connection_that_never_authenticates_is_dropped() {
    let addr = start_with(
        Arc::new(Canned(package())),
        Arc::new(Collected::default()),
        Limits {
            join_timeout_ms: 150,
            ..Limits::default()
        },
    )
    .await;

    let mut silent = connect(addr).await;
    // Say nothing at all. Before the deadline this waited forever.
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        futures_util::StreamExt::next(&mut silent),
    )
    .await
    .expect("the server must close a socket that never joins");
    assert!(
        outcome.is_none()
            || outcome.is_some_and(|m| m.is_err()
                || matches!(m, Ok(tokio_tungstenite::tungstenite::Message::Close(_)))),
        "the connection should be closed, not held"
    );

    // And the node is still able to take a real participant afterwards, which
    // is the property that actually matters.
    let mut good = connect(addr).await;
    let first = join(&mut good, &claims("ada", Access::Edit)).await;
    assert!(
        matches!(
            first,
            Some(ServerMessage::Opening { .. } | ServerMessage::Welcome { .. })
        ),
        "a legitimate join still works, got {first:?}"
    );
}

/// The pending-connection cap is separate from `max_participants`, because a
/// socket that has not presented a token belongs to no document yet.
#[tokio::test]
async fn connections_waiting_to_authenticate_are_capped() {
    let addr = start_with(
        Arc::new(Canned(package())),
        Arc::new(Collected::default()),
        Limits {
            max_pending_connections: 1,
            // Long enough that the first socket is still parked when the
            // second arrives, so the cap is what refuses it rather than a
            // timeout racing the test.
            join_timeout_ms: 5_000,
            ..Limits::default()
        },
    )
    .await;

    let _first = connect(addr).await;
    // Give the first connection time to take the only permit.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let mut second = connect(addr).await;
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        futures_util::StreamExt::next(&mut second),
    )
    .await
    .expect("the second connection must be refused rather than parked");
    assert!(
        outcome.is_none()
            || outcome.is_some_and(|m| m.is_err()
                || matches!(m, Ok(tokio_tungstenite::tungstenite::Message::Close(_)))),
        "over the cap, a connection is closed rather than held"
    );
}
