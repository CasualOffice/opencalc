//! The network service: WebSocket in, [`DocumentSession`] behind it.
//!
//! [ADR-014](../../../docs/59-COLLABORATION-SERVICE-STACK.md) chose the
//! transport; this is it. Everything below this module is a state machine over
//! supplied time and supplied bytes, and everything in it is the shell that
//! supplies them — which is the only reason the rest of the crate could be
//! written and tested before any of this existed.
//!
//! # This is the standalone shape
//!
//! One process, leader of every document by definition, with the log in memory.
//! ADR-012 is explicit that this is a **first-class mode** and not a degraded
//! one: it is what most deployments will run, and requiring a cluster for it
//! would be requiring infrastructure to solve a problem the operator does not
//! have. The cluster layer adds a lease, a relay and an external log; it does
//! not change what is here.
//!
//! # Fetching is injected
//!
//! The server is told *how* to fetch a document rather than containing an HTTP
//! client. Two reasons, and the second is the one that matters: an integrator
//! may need mutual TLS, a proxy, or a signed request this crate should not have
//! opinions about — and a test can supply bytes without a network, which is
//! what makes the join path testable at all.

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use casual_calc_transaction::protocol::{ClientMessage, PROTOCOL_VERSION, Refusal, ServerMessage};
use casual_calc_transaction::session::ClientId;
use tokio::sync::broadcast;

use crate::document::DocumentSession;
use crate::lifecycle::SavePolicy;
use crate::presence::Roster;
use crate::token::Claims;
use crate::verify::Verifier;
use casual_calc_transaction::session::SnapshotPolicy;

/// How the server obtains a document's bytes.
///
/// A trait rather than an HTTP client so a deployment can bring its own —
/// mutual TLS, a proxy, a signed request — and so a test can answer without a
/// network.
pub trait Fetch: Send + Sync + 'static {
    /// Fetch the package at `url`.
    fn get(&self, url: String) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send>>;
}

/// What the service needs to run.
pub struct ServiceConfig {
    // `Debug` is hand-written below rather than derived: `Verifier` holds keys,
    // and a config struct is exactly the thing somebody logs at startup.
    /// Where to listen.
    pub bind: SocketAddr,
    /// How tokens are checked.
    pub verifier: Verifier,
    /// When a document is saved back.
    pub save: SavePolicy,
    /// How often the ordered log is compacted into a snapshot.
    pub snapshots: SnapshotPolicy,
    /// How to fetch a document.
    pub fetch: Arc<dyn Fetch>,
}

impl core::fmt::Debug for ServiceConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ServiceConfig")
            .field("bind", &self.bind)
            .field("save", &self.save)
            .field("snapshots", &self.snapshots)
            .finish_non_exhaustive()
    }
}

/// One document being edited, and everyone watching it.
struct Live {
    session: Mutex<DocumentSession>,
    roster: Mutex<Roster>,
    /// Broadcast to every connection on this document, carrying **who caused
    /// it** so a connection can skip its own.
    ///
    /// That envelope is not tidiness. A client has already applied its own
    /// operations locally — that is what makes editing feel immediate — and the
    /// ack is what tells it they are ordered. Echoing them back would have it
    /// apply them a second time.
    ///
    /// Capacity is bounded: a subscriber that falls far enough behind is
    /// dropped by the channel and resyncs, which is better than growing without
    /// limit because one client stopped reading.
    fan_out: broadcast::Sender<(ClientId, ServerMessage)>,
    /// The next participant id. Ids are per-document and per-process, which is
    /// all `ClientId` promises.
    next_client: Mutex<u64>,
}

/// Every document this node currently holds.
#[derive(Default)]
struct Registry {
    live: Mutex<HashMap<String, Arc<Live>>>,
}

/// The service's shared state.
struct Service {
    config: ServiceConfig,
    registry: Registry,
}

/// Run the service until the process ends.
///
/// # Errors
///
/// Whatever binding the listener or serving produces.
pub async fn serve(config: ServiceConfig) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    serve_on(listener, config).await
}

/// Run the service on an already-bound listener.
///
/// Separate from [`serve`] so a test can bind port zero, learn the port the
/// operating system chose, and connect to it — without which an end-to-end test
/// is a race against a hard-coded number.
///
/// # Errors
///
/// Whatever serving produces.
pub async fn serve_on(
    listener: tokio::net::TcpListener,
    config: ServiceConfig,
) -> std::io::Result<()> {
    let state = Arc::new(Service {
        config,
        registry: Registry::default(),
    });
    axum::serve(listener, router(state)).await
}

fn router(state: Arc<Service>) -> Router {
    Router::new()
        // Liveness, for a load balancer. Deliberately says nothing about
        // documents: a node with no documents is healthy, and one that cannot
        // reach Redis is a cluster problem rather than a reason to take this
        // node out of rotation and make it worse.
        .route("/healthz", get(|| async { "ok" }))
        .route("/collab", get(upgrade))
        .with_state(state)
}

#[derive(serde::Deserialize)]
struct Join {
    /// The document session key the client is asking for. Checked against the
    /// token, which must agree: a signature proves the host issued the token,
    /// not that it issued it for this document.
    doc: String,
}

async fn upgrade(
    State(state): State<Arc<Service>>,
    Query(join): Query<Join>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| connection(state, join.doc, socket))
}

/// Now, in milliseconds, for the lifecycle; and in seconds, for the token.
///
/// The one place this crate reads a clock. Everything it feeds takes time as an
/// argument precisely so that this is the only thing a test has to avoid.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

async fn connection(state: Arc<Service>, document_key: String, mut socket: WebSocket) {
    let Some(claims) = authorise(&state, &document_key, &mut socket).await else {
        return;
    };

    let live = match obtain(&state, &claims).await {
        Ok(live) => live,
        Err(reason) => {
            let _ = send(&mut socket, &ServerMessage::Stopped { reason }).await;
            return;
        }
    };

    // Everything from here is a participant in a document that exists.
    let mut updates = live.fan_out.subscribe();
    let client = {
        let mut next = live.next_client.lock().expect("registry lock");
        *next += 1;
        ClientId(*next)
    };

    // No lock is ever held across an `await` in this module. That is not a
    // style preference: a `MutexGuard` held over a suspension point makes the
    // future non-`Send`, and — worse — parks every other participant on this
    // document behind one slow socket.
    let joined = {
        let mut session = live.session.lock().expect("session lock");
        session.join()
    };
    let Ok(joined) = joined else {
        let _ = send(
            &mut socket,
            &ServerMessage::Stopped {
                reason: Refusal::NotSaving,
            },
        )
        .await;
        return;
    };
    live.roster.lock().expect("roster lock").joined(
        client,
        claims.user.name.clone(),
        claims.user.color.clone(),
        now_ms(),
    );

    let read_only = live.session.lock().expect("session lock").is_read_only();
    let editable = claims.permissions.access >= crate::token::Access::Comment && !read_only;
    if send(
        &mut socket,
        &ServerMessage::Welcome {
            protocol: PROTOCOL_VERSION,
            client,
            revision: joined.revision,
            snapshot: joined.snapshot,
            editable,
        },
    )
    .await
    .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            // Something another participant did.
            update = updates.recv() => {
                match update {
                    // Skip what this connection caused: it applied those
                    // locally before sending them, and the ack is what
                    // confirmed the order.
                    Ok((origin, _)) if origin == client => {}
                    Ok((_, message)) => {
                        if send(&mut socket, &message).await.is_err() {
                            break;
                        }
                    }
                    // Fell too far behind. Dropping the connection is right:
                    // the client reconnects and resumes from its last
                    // acknowledged revision, which is cheaper and more certain
                    // than trying to work out what it missed.
                    Err(broadcast::error::RecvError::Lagged(_)) => break,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            // Something this participant did.
            incoming = socket.recv() => {
                let Some(Ok(frame)) = incoming else { break };
                let Message::Text(text) = frame else { continue };
                let Ok(message) = serde_json::from_str::<ClientMessage>(&text) else {
                    continue;
                };
                if !handle(&state, &live, &claims, client, message, &mut socket).await {
                    break;
                }
            }
        }
    }

    live.roster.lock().expect("roster lock").left(client);
    live.session.lock().expect("session lock").left();
    // Addressed from this client so the envelope is consistent; every *other*
    // connection is the audience, which is exactly who needs to know.
    let _ = live
        .fan_out
        .send((client, ServerMessage::Departed { client }));
}

/// Read the first message, which must be a `Join`, and check it.
///
/// Returns `None` having already told the client why, whenever the answer is no.
async fn authorise(
    state: &Arc<Service>,
    document_key: &str,
    socket: &mut WebSocket,
) -> Option<Claims> {
    let first = socket.recv().await?;
    let Ok(Message::Text(text)) = first else {
        return None;
    };
    let Ok(ClientMessage::Join { protocol, token }) = serde_json::from_str(&text) else {
        // A connection that opens with anything else is not speaking this
        // protocol, and there is nothing useful to say back to it.
        return None;
    };
    if protocol != PROTOCOL_VERSION {
        // Said plainly and early: a mismatched peer should stop here rather
        // than proceed until a missing field produces something more confusing.
        // Reporting what the *client* said, not what this server speaks —
        // otherwise the message says the two agree while refusing them.
        let _ = send(socket, &ServerMessage::version_mismatch(protocol)).await;
        return None;
    }

    let now_secs = now_ms() / 1_000;
    match state.config.verifier.verify(&token, document_key, now_secs) {
        Ok(claims) => Some(claims),
        Err(refusal) => {
            // The client is told one thing; the operator's log gets the detail.
            tracing_refusal(&refusal);
            let _ = send(
                socket,
                &ServerMessage::Stopped {
                    reason: refusal.refusal(),
                },
            )
            .await;
            None
        }
    }
}

/// Find the document, or open it from the URL the token named.
async fn obtain(state: &Arc<Service>, claims: &Claims) -> Result<Arc<Live>, Refusal> {
    let key = claims.document.key.clone();
    if let Some(live) = state.registry.live.lock().expect("registry lock").get(&key) {
        return Ok(Arc::clone(live));
    }

    // Fetched outside the lock: a slow origin must not stop every other
    // document on this node.
    let bytes = state
        .config
        .fetch
        .get(claims.document.url.clone())
        .await
        .map_err(|_| Refusal::NotSaving)?;

    let session = DocumentSession::open(bytes, state.config.save, state.config.snapshots, now_ms())
        .map_err(|_| Refusal::NotSaving)?;

    let mut registry = state.registry.live.lock().expect("registry lock");
    // Somebody else may have opened it while this was fetching. Theirs wins:
    // two sessions over one document is the one outcome that must not happen,
    // and the wasted fetch is the cheaper mistake.
    Ok(Arc::clone(registry.entry(key).or_insert_with(|| {
        Arc::new(Live {
            session: Mutex::new(session),
            roster: Mutex::new(Roster::default()),
            fan_out: broadcast::channel(256).0,
            next_client: Mutex::new(0),
        })
    })))
}

/// Act on one client message. Returns whether the connection continues.
async fn handle(
    _state: &Arc<Service>,
    live: &Arc<Live>,
    claims: &Claims,
    client: ClientId,
    message: ClientMessage,
    socket: &mut WebSocket,
) -> bool {
    match message {
        ClientMessage::Join { .. } => {
            // Already joined. A second one is a client bug, not an attack, and
            // ignoring it is kinder than dropping the connection.
            true
        }
        ClientMessage::Heartbeat => {
            live.roster
                .lock()
                .expect("roster lock")
                .heartbeat(client, now_ms());
            true
        }
        ClientMessage::Presence { sheet, selection } => {
            live.roster
                .lock()
                .expect("roster lock")
                .moved(client, sheet, selection, now_ms());
            let _ = live.fan_out.send((
                client,
                ServerMessage::Presence {
                    client,
                    // From the token, never from the client: presence is the one
                    // surface where a claimed identity would be believed.
                    name: claims.user.name.clone(),
                    color: claims
                        .user
                        .color
                        .clone()
                        .unwrap_or_else(|| crate::presence::colour_for(client)),
                    sheet,
                    selection,
                },
            ));
            true
        }
        ClientMessage::Leave => false,
        ClientMessage::Submit(submission) => {
            // The permission is enforced here, at the operation, and not by
            // having hidden a toolbar (COL-17). A viewer or commenter whose
            // client sends an edit anyway is refused by the server.
            if !submission.ops.iter().all(|wire| claims.permits(&wire.op)) {
                let _ = send(
                    socket,
                    &ServerMessage::Refused {
                        seq: Some(submission.seq),
                        reason: Refusal::ReadOnlyAccess,
                    },
                )
                .await;
                return true;
            }

            let outcome = {
                let mut session = live.session.lock().expect("session lock");
                session.commit(&submission, now_ms())
            };
            match outcome {
                Ok(commit) => {
                    let (revision, ops) = match commit {
                        casual_calc_transaction::session::Commit::Applied { ops, revision } => {
                            (revision, ops)
                        }
                        // A redelivery of a chunk already ordered. The client
                        // is told the revision it landed at originally, and
                        // nothing is broadcast: it happened once.
                        casual_calc_transaction::session::Commit::Duplicate { revision } => {
                            (revision, Vec::new())
                        }
                    };
                    if send(
                        socket,
                        &ServerMessage::Ack {
                            seq: submission.seq,
                            revision,
                        },
                    )
                    .await
                    .is_err()
                    {
                        return false;
                    }
                    // Everyone else sees what landed. The sender does not need
                    // it — its own ops are already applied locally, and the ack
                    // is what tells it they are ordered.
                    if !ops.is_empty() {
                        let _ = live
                            .fan_out
                            .send((client, ServerMessage::Apply { revision, ops }));
                    }
                    true
                }
                Err(_) => {
                    let _ = send(
                        socket,
                        &ServerMessage::Refused {
                            seq: Some(submission.seq),
                            reason: Refusal::NotSaving,
                        },
                    )
                    .await;
                    true
                }
            }
        }
    }
}

async fn send(socket: &mut WebSocket, message: &ServerMessage) -> Result<(), axum::Error> {
    let text = serde_json::to_string(message).unwrap_or_default();
    socket.send(Message::Text(text.into())).await
}

/// Where an operator learns *which* refusal it was.
fn tracing_refusal(error: &crate::verify::VerifyError) {
    // Deliberately not returned to the client: telling a caller which of
    // malformed, wrong-key, bad-signature and expired it was hands them an
    // oracle. An operator needs it and an attacker must not have it.
    eprintln!("collab: refused a join: {error}");
}

#[cfg(test)]
mod tests;
