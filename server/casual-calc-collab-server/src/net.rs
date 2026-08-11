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
use crate::lifecycle::{Action, CallbackOutcome, SavePolicy};
use crate::presence::Roster;
use crate::token::{Callback, Claims};
use crate::verify::Verifier;
use casual_calc_transaction::session::SnapshotPolicy;

/// Take a lock, surviving a poisoned one.
///
/// Rust poisons a mutex when a thread panics holding it, and every later
/// `lock()` then returns `Err`. Turning that into another panic — which
/// `.expect()` does — means one panic makes a document permanently unreachable
/// while the process keeps serving, so nothing restarts it and no health check
/// notices. For the registry lock it would take **every** document on the node.
///
/// The data behind these locks is a document and a roster, not an invariant
/// that a half-finished mutation corrupts beyond use: recovering and carrying
/// on is strictly better than refusing forever. The poisoning is worth a loud
/// line in the log, and is not worth an outage.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| {
        eprintln!(
            "collab: recovered a poisoned lock — a task panicked while holding it; \
             the document continues"
        );
        poisoned.into_inner()
    })
}

/// How the server obtains a document's bytes.
///
/// A trait rather than an HTTP client so a deployment can bring its own —
/// mutual TLS, a proxy, a signed request — and so a test can answer without a
/// network.
pub trait Fetch: Send + Sync + 'static {
    /// Fetch the package at `url`.
    fn get(&self, url: String) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send>>;
}

/// How the server returns a finished document to the integrator.
///
/// Injected for the same reasons as [`Fetch`]: an integrator may need mutual
/// TLS, a proxy or a signed request, and a test needs to observe a save without
/// a network. The two callback shapes — an OnlyOffice-style URL and a WOPI
/// `PutFile` — are both described by [`Callback`], and which request to make is
/// this implementation's business.
pub trait Deliver: Send + Sync + 'static {
    /// Send `bytes` to `destination`, reporting whether the host accepted them.
    fn put(
        &self,
        destination: Callback,
        title: String,
        bytes: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
}

/// Bounds on what a node will hold and accept.
///
/// Every one of these was unbounded, which is the difference between a service
/// that degrades under load and one that dies: without them a node holds every
/// workbook it has ever opened until the OOM killer arrives, and one client can
/// open connections until it runs out of descriptors.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// How many documents this node will hold at once.
    pub max_documents: usize,
    /// How many connections one document will accept.
    pub max_participants: usize,
    /// The largest WebSocket message accepted, in bytes.
    pub max_message_bytes: usize,
    /// How long a document with nobody in it and nothing unsaved is kept before
    /// it is dropped.
    ///
    /// Not zero: a participant who reloads the page is gone for a second or
    /// two, and rebuilding the session from the origin for that is wasteful and
    /// slow. Not long either — it is memory.
    pub idle_eviction_ms: u64,
    /// How often the sweeper runs.
    pub tick_ms: u64,
    /// How long a participant may go unheard before it is presumed gone.
    pub presence_ttl_ms: u64,
    /// How long the final save at shutdown may take before the node exits
    /// anyway.
    ///
    /// Bounded on purpose: the host is often being restarted at the same
    /// moment, and a node that refuses to exit is a worse failure than one that
    /// exits having tried. What it must not do is exit *without* trying.
    pub drain_timeout_ms: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_documents: 1_000,
            max_participants: 200,
            // Comfortably above a large submission and far below a memory
            // problem. A client with more to say than this is misbehaving.
            max_message_bytes: 4 * 1024 * 1024,
            idle_eviction_ms: 30_000,
            tick_ms: 1_000,
            presence_ttl_ms: crate::presence::DEFAULT_TTL_MS,
            drain_timeout_ms: 10_000,
        }
    }
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
    /// How to return a finished document to the integrator. Without one the
    /// server cannot save, and says so at startup rather than discovering it
    /// when the first document quiesces.
    pub deliver: Arc<dyn Deliver>,
    /// What this node will hold and accept.
    pub limits: Limits,
}

impl core::fmt::Debug for ServiceConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ServiceConfig")
            .field("bind", &self.bind)
            .field("save", &self.save)
            .field("snapshots", &self.snapshots)
            .field("limits", &self.limits)
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
    /// Where the finished document goes, from the token that opened it.
    callback: Option<Callback>,
    /// The document's name, for the callback and the log.
    title: String,
    /// When this document became empty, or zero while somebody is in it.
    idle_since: std::sync::atomic::AtomicU64,
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

/// A way to ask the service to stop, and to know when it has.
///
/// Handed out rather than only installed on a signal, because a test has to be
/// able to shut a server down deterministically — and because an orchestrator
/// sometimes wants to drain a node without killing the process.
#[derive(Debug, Clone)]
pub struct Shutdown(tokio::sync::watch::Sender<bool>);

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

impl Shutdown {
    /// A handle that has not been triggered.
    #[must_use]
    pub fn new() -> Self {
        Self(tokio::sync::watch::channel(false).0)
    }

    /// Ask the service to stop accepting connections and drain.
    pub fn begin(&self) {
        let _ = self.0.send(true);
    }

    async fn wait(&self) {
        let mut rx = self.0.subscribe();
        if *rx.borrow() {
            return;
        }
        let _ = rx.changed().await;
    }
}

/// Run the service until the process is asked to stop.
///
/// # Errors
///
/// Whatever binding the listener or serving produces.
pub async fn serve(config: ServiceConfig) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    let shutdown = Shutdown::new();
    let on_signal = shutdown.clone();
    tokio::spawn(async move {
        // SIGTERM is how an orchestrator asks; Ctrl-C is how a person does.
        let mut term =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(term) => term,
                Err(_) => return,
            };
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
        on_signal.begin();
    });
    serve_on_with_shutdown(listener, config, shutdown).await
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
    serve_on_with_shutdown(listener, config, Shutdown::new()).await
}

/// As [`serve_on`], stopping when `shutdown` is triggered.
///
/// # Errors
///
/// Whatever serving produces.
pub async fn serve_on_with_shutdown(
    listener: tokio::net::TcpListener,
    config: ServiceConfig,
    shutdown: Shutdown,
) -> std::io::Result<()> {
    let state = Arc::new(Service {
        config,
        registry: Registry::default(),
    });
    // The sweeper is what makes this a service rather than a relay. Without it
    // the save lifecycle — quiesce timer, ceiling, revision cadence, callback
    // retry, read-only fencing — is built, tested and driven by nothing, and
    // the node holds the only copy of every edit until it restarts.
    tokio::spawn(sweep(Arc::clone(&state), shutdown.clone()));

    let signalled = shutdown.clone();
    axum::serve(listener, router(Arc::clone(&state)))
        .with_graceful_shutdown(async move { signalled.wait().await })
        .await?;

    // The sweeper has been told to stop; give it the moment it needs to notice,
    // so the drain below is the only thing saving.
    tokio::time::sleep(std::time::Duration::from_millis(
        state.config.limits.tick_ms.saturating_mul(2).max(20),
    ))
    .await;

    // Then the part that matters. A rolling deploy that drops connections has
    // merely inconvenienced people; one that drops connections with unsaved
    // edits behind them has lost their work, and the lifecycle's own cadence is
    // no help because it was waiting for a quiesce that will never come.
    drain(&state).await;
    Ok(())
}

/// Save everything outstanding, once, before the process goes.
///
/// Best effort and bounded: a host that is also being restarted cannot be
/// waited for indefinitely, and a node that refuses to exit is a worse failure
/// than one that exits having tried. What it must not do is exit *without*
/// trying, which is what happens with no shutdown path at all.
async fn drain(state: &Arc<Service>) {
    let documents: Vec<Arc<Live>> = lock(&state.registry.live)
        .values()
        .map(Arc::clone)
        .collect();
    let outstanding = documents
        .iter()
        .filter(|live| lock(&live.session).has_unsaved())
        .count();
    if outstanding == 0 {
        return;
    }
    eprintln!("collab: draining {outstanding} document(s) with unsaved work");

    let now = now_ms();
    for live in documents {
        if !lock(&live.session).has_unsaved() {
            continue;
        }
        let Some(destination) = live.callback.clone() else {
            continue;
        };
        let assembled = { lock(&live.session).assemble() };
        let Ok(bytes) = assembled else { continue };
        // Tell everyone still connected that this is the last word, whichever
        // way it goes.
        let outcome = match tokio::time::timeout(
            std::time::Duration::from_millis(state.config.limits.drain_timeout_ms),
            state
                .config
                .deliver
                .put(destination, live.title.clone(), bytes),
        )
        .await
        {
            Ok(Ok(())) => CallbackOutcome::Accepted(lock(&live.session).revision()),
            Ok(Err(why)) => {
                eprintln!("collab: the final save failed for {}: {why}", live.title);
                CallbackOutcome::Failed
            }
            Err(_) => {
                eprintln!("collab: the final save timed out for {}", live.title);
                CallbackOutcome::Failed
            }
        };
        let _ = lock(&live.session).callback(outcome, now);
    }
}

/// Drive every document's clock: save when the lifecycle says so, forget
/// participants who stopped talking, and let go of documents nobody is in.
///
/// One task for all documents rather than one per document. A thousand idle
/// timers is a thousand things to cancel correctly on eviction, and the work
/// per tick is proportional to the documents that actually need something.
async fn sweep(state: Arc<Service>, shutdown: Shutdown) {
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(
        state.config.limits.tick_ms.max(1),
    ));
    // A tick missed under load should not produce a burst of catch-up ticks
    // afterwards, which would run the save cadence several times over.
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        // Stop before the drain runs, not alongside it. Both assemble and
        // deliver, so a sweeper still ticking during shutdown races the final
        // save and can send the host the same document twice.
        tokio::select! {
            _ = ticker.tick() => {}
            _ = shutdown.wait() => return,
        }
        let now = now_ms();

        // Snapshot the keys, so the registry lock is not held while a document
        // is saved — which involves a network round-trip to the integrator.
        let documents: Vec<(String, Arc<Live>)> = lock(&state.registry.live)
            .iter()
            .map(|(key, live)| (key.clone(), Arc::clone(live)))
            .collect();

        for (key, live) in documents {
            expire_presence(&live, now);
            service_lifecycle(&state, &live, now).await;
            evict_if_idle(&state, &key, &live, now);
        }
    }
}

/// Drop participants who stopped talking, and tell the others.
fn expire_presence(live: &Arc<Live>, now: u64) {
    let gone = lock(&live.roster).expire(now);
    for client in gone {
        // Returned rather than merely removed, because the others have to be
        // told: a cursor that stops moving and never disappears reads as
        // somebody watching.
        let _ = live
            .fan_out
            .send((client, ServerMessage::Departed { client }));
    }
}

/// Ask the lifecycle what to do, and do it.
async fn service_lifecycle(state: &Arc<Service>, live: &Arc<Live>, now: u64) {
    let action = { lock(&live.session).tick(now) };
    let Some(action) = action else { return };

    match action {
        Action::Save { revision, .. } => {
            let Some(destination) = live.callback.clone() else {
                // No callback in the token means the host is not asking for the
                // document back — a preview, or a session it collects another
                // way. Recording the attempt as accepted keeps the lifecycle
                // from retrying forever against nothing.
                let follow_up =
                    lock(&live.session).callback(CallbackOutcome::Accepted(revision), now);
                act_on_follow_up(live, follow_up);
                return;
            };

            // Assembled under the lock, delivered outside it: a slow or
            // unreachable integrator must not stop everyone editing.
            let assembled = { lock(&live.session).assemble() };
            let outcome = match assembled {
                Ok(bytes) => {
                    match state
                        .config
                        .deliver
                        .put(destination, live.title.clone(), bytes)
                        .await
                    {
                        Ok(()) => CallbackOutcome::Accepted(revision),
                        Err(why) => {
                            eprintln!("collab: the callback failed for {}: {why}", live.title);
                            CallbackOutcome::Failed
                        }
                    }
                }
                Err(why) => {
                    eprintln!("collab: could not assemble {}: {why}", live.title);
                    CallbackOutcome::Failed
                }
            };
            let follow_up = { lock(&live.session).callback(outcome, now) };
            act_on_follow_up(live, follow_up);
        }
        other => act_on_follow_up(live, Some(other)),
    }
}

/// Tell the participants what the lifecycle decided, when it concerns them.
fn act_on_follow_up(live: &Arc<Live>, action: Option<Action>) {
    match action {
        Some(Action::WarnNotSaving { .. }) => {
            // On the *first* failure, not the last: a warning is only useful
            // while there is still time to copy the work out.
            let _ = live.fan_out.send((
                ClientId(0),
                ServerMessage::Refused {
                    seq: None,
                    reason: Refusal::NotSaving,
                },
            ));
        }
        Some(Action::GoReadOnly) => {
            // Continuing to accept work that provably cannot be persisted is
            // silent loss dressed up as availability.
            let _ = live.fan_out.send((
                ClientId(0),
                ServerMessage::Stopped {
                    reason: Refusal::NotSaving,
                },
            ));
        }
        Some(Action::Save { .. }) | None => {}
    }
}

/// Let go of a document nobody is in and nothing is owed for.
fn evict_if_idle(state: &Arc<Service>, key: &str, live: &Arc<Live>, now: u64) {
    let empty = lock(&live.roster).is_empty();
    if !empty {
        live.idle_since
            .store(0, std::sync::atomic::Ordering::Relaxed);
        return;
    }
    // Never evict with work outstanding: the whole point of the lifecycle is
    // that the host gets the document back.
    if lock(&live.session).has_unsaved() {
        return;
    }
    let since = live.idle_since.load(std::sync::atomic::Ordering::Relaxed);
    if since == 0 {
        live.idle_since
            .store(now, std::sync::atomic::Ordering::Relaxed);
        return;
    }
    if now.saturating_sub(since) < state.config.limits.idle_eviction_ms {
        return;
    }
    lock(&state.registry.live).remove(key);
}

fn router(state: Arc<Service>) -> Router {
    Router::new()
        // Liveness, for a load balancer. Deliberately says nothing about
        // documents: a node with no documents is healthy, and one that cannot
        // reach Redis is a cluster problem rather than a reason to take this
        // node out of rotation and make it worse.
        .route("/healthz", get(|| async { "ok" }))
        .route("/collab", get(upgrade))
        // What an operator needs to answer "is it working" without trying it,
        // and the only window onto state that is otherwise private. Counts
        // rather than identities: a document key names a customer's file.
        .route("/stats", get(stats))
        .with_state(state)
}

/// A node's current load, as counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Stats {
    /// Documents held on this node.
    pub documents: usize,
    /// Participants across all of them.
    pub participants: usize,
}

async fn stats(State(state): State<Arc<Service>>) -> axum::Json<Stats> {
    let live: Vec<Arc<Live>> = lock(&state.registry.live)
        .values()
        .map(Arc::clone)
        .collect();
    axum::Json(Stats {
        documents: live.len(),
        participants: live.iter().map(|l| lock(&l.roster).len()).sum(),
    })
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
    // Bounded before a byte is read. A frame limit is the cheapest of the
    // limits and the one whose absence is most easily exploited: a client that
    // can send an unbounded message can make the server allocate one.
    let ws = ws
        .max_message_size(state.config.limits.max_message_bytes)
        .max_frame_size(state.config.limits.max_message_bytes);
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

    // A document that is already full turns the next arrival away rather than
    // degrading for everybody in it.
    if lock(&live.roster).len() >= state.config.limits.max_participants {
        let _ = send(
            &mut socket,
            &ServerMessage::Stopped {
                reason: Refusal::NotSaving,
            },
        )
        .await;
        return;
    }

    // Everything from here is a participant in a document that exists.
    let mut updates = live.fan_out.subscribe();
    let client = {
        let mut next = lock(&live.next_client);
        *next += 1;
        ClientId(*next)
    };

    // No lock is ever held across an `await` in this module. That is not a
    // style preference: a `MutexGuard` held over a suspension point makes the
    // future non-`Send`, and — worse — parks every other participant on this
    // document behind one slow socket.
    let joined = {
        let mut session = lock(&live.session);
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
    lock(&live.roster).joined(
        client,
        claims.user.name.clone(),
        claims.user.color.clone(),
        now_ms(),
    );

    let read_only = lock(&live.session).is_read_only();
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

    lock(&live.roster).left(client);
    lock(&live.session).left();
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
    if let Some(live) = lock(&state.registry.live).get(&key) {
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

    let mut registry = lock(&state.registry.live);
    // Checked here rather than before the fetch, because the wasted fetch is
    // cheaper than holding the registry lock across a network round-trip.
    if !registry.contains_key(&key) && registry.len() >= state.config.limits.max_documents {
        return Err(Refusal::NotSaving);
    }
    // Somebody else may have opened it while this was fetching. Theirs wins:
    // two sessions over one document is the one outcome that must not happen,
    // and the wasted fetch is the cheaper mistake.
    Ok(Arc::clone(registry.entry(key).or_insert_with(|| {
        Arc::new(Live {
            session: Mutex::new(session),
            roster: Mutex::new(Roster::new(state.config.limits.presence_ttl_ms)),
            fan_out: broadcast::channel(256).0,
            next_client: Mutex::new(0),
            callback: claims.callback.clone(),
            title: claims.document.title.clone(),
            idle_since: std::sync::atomic::AtomicU64::new(0),
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
            lock(&live.roster).heartbeat(client, now_ms());
            true
        }
        ClientMessage::Presence { sheet, selection } => {
            lock(&live.roster).moved(client, sheet, selection, now_ms());
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
                let mut session = lock(&live.session);
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
