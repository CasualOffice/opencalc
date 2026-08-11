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
use casual_calc_transaction::protocol::{
    ClientMessage, PROTOCOL_VERSION, Refusal, Resume, ServerMessage,
};
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
        tracing::error!(
            "recovered a poisoned lock: a task panicked while holding it; the document continues"
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
    /// How often the server pings a quiet connection.
    ///
    /// A WebSocket ping is answered automatically by any live client, so this
    /// is a liveness check that costs a browser nothing and does not depend on
    /// the application protocol. It is also the only thing that notices a
    /// **half-open** connection — a laptop that slept, a network that vanished
    /// — where the socket looks open to us and there is nobody on the far end.
    pub client_ping_ms: u64,
    /// How long a connection may go without a word before it is closed.
    ///
    /// Deliberately longer than [`presence_ttl_ms`](Self::presence_ttl_ms) and
    /// several times [`client_ping_ms`](Self::client_ping_ms): a client that
    /// answers pings is alive even if nobody is typing, and closing a
    /// connection because its user went to lunch is worse than holding it.
    pub client_idle_ms: u64,
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
            client_ping_ms: 15_000,
            client_idle_ms: 90_000,
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
    /// Resume keys, to the participant each one continues.
    ///
    /// What lets a reconnecting client be *the same* participant rather than a
    /// new one — which is what makes the server's `(client, seq)` duplicate
    /// suppression work across a dropped socket, and so what makes resending an
    /// unacknowledged chunk safe. See
    /// [ADR-015](../../../docs/61-COLLABORATION-RESUME.md).
    ///
    /// The user id is stored beside the client id and checked before a key is
    /// honoured. Without that, anyone holding a valid token for this document
    /// could adopt another participant's identity by presenting their key, and
    /// have that participant's submissions suppressed as duplicates of their
    /// own. With it, a key is only ever useful to the person it was issued to.
    resumes: Mutex<Resumes>,
}

/// The resume keys one document has issued.
///
/// Bounded, and evicted oldest-first. A key costs memory and is supplied by the
/// client, so an unbounded map is a participant with a loop and a document that
/// grows until the node dies. The cap is generous next to any real number of
/// tabs on one document, and reaching it only means the oldest participant
/// cannot resume — it rejoins fresh, which is what happened before this existed.
#[derive(Debug, Default)]
struct Resumes {
    /// Key to `(user id, client id)`, in insertion order.
    entries: Vec<(String, String, ClientId)>,
}

impl Resumes {
    /// How many keys one document will remember.
    const CAP: usize = 512;
    /// The longest key accepted. A key is an opaque identifier, not a payload.
    const MAX_KEY: usize = 128;

    /// The participant `key` continues, if it is this user's.
    fn honour(&self, key: &str, user: &str) -> Option<ClientId> {
        self.entries
            .iter()
            .find(|(k, u, _)| k == key && u == user)
            .map(|(_, _, client)| *client)
    }

    /// Remember that `key` names `client`, replacing any earlier meaning.
    fn remember(&mut self, key: &str, user: &str, client: ClientId) {
        if key.len() > Self::MAX_KEY {
            return;
        }
        self.entries.retain(|(k, _, _)| k != key);
        if self.entries.len() >= Self::CAP {
            self.entries.remove(0);
        }
        self.entries.push((key.to_owned(), user.to_owned(), client));
    }
}

/// Every document this node currently holds.
#[derive(Default)]
struct Registry {
    live: Mutex<HashMap<String, Arc<Live>>>,
    /// Documents currently being fetched and opened, so it happens **once**.
    ///
    /// Without this, a document is opened once per arriving participant. Thirty
    /// people opening the same workbook at the start of a meeting is thirty
    /// simultaneous downloads of the same file from the integrator, twenty-nine
    /// of them thrown away — each holding a connection, a task and however much
    /// memory that workbook takes, and all of it aimed at somebody else's
    /// server at the moment it is already busiest.
    ///
    /// The previous code noticed the *race* and settled it after the fact
    /// ("theirs wins, the wasted fetch is the cheaper mistake"), which is true
    /// for two and badly false for thirty. A [`OnceCell`] settles it before the
    /// fact: the first arrival fetches and the rest wait on that same fetch.
    ///
    /// `get_or_try_init` and not `get_or_init`, so a failure is **not**
    /// remembered. An origin that was briefly down would otherwise poison the
    /// document until the process restarted.
    opening: Mutex<HashMap<String, Arc<tokio::sync::OnceCell<Arc<Live>>>>>,
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

/// Build a rustls server configuration from a [`crate::config::Endpoint`]'s files.
///
/// Client authentication is wired here rather than left to the caller because
/// it is the half that is easy to configure and forget: TLS on the internal
/// endpoint proves the traffic is private, and a **client CA** is what proves
/// the peer is one of yours. `Exposure::warnings` says so at startup; this is
/// what honours it.
///
/// # Errors
///
/// If a file cannot be read, or does not hold what its name says it does.
pub fn tls_config(endpoint: &crate::config::Endpoint) -> Result<rustls::ServerConfig, String> {
    use std::io::BufReader;

    let files = endpoint
        .tls
        .as_ref()
        .ok_or_else(|| "this endpoint is not configured for TLS".to_owned())?;

    let read = |path: &std::path::Path| -> Result<Vec<u8>, String> {
        std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
    };

    let certs: Vec<_> = rustls_pemfile::certs(&mut BufReader::new(&read(&files.certificate)?[..]))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("{}: {e}", files.certificate.display()))?;
    if certs.is_empty() {
        return Err(format!(
            "{} holds no certificate",
            files.certificate.display()
        ));
    }
    let key = rustls_pemfile::private_key(&mut BufReader::new(&read(&files.key)?[..]))
        .map_err(|e| format!("{}: {e}", files.key.display()))?
        .ok_or_else(|| format!("{} holds no private key", files.key.display()))?;

    let builder = if let Some(ca_path) = &endpoint.client_ca {
        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls_pemfile::certs(&mut BufReader::new(&read(ca_path)?[..])) {
            let cert = cert.map_err(|e| format!("{}: {e}", ca_path.display()))?;
            roots
                .add(cert)
                .map_err(|e| format!("{}: {e}", ca_path.display()))?;
        }
        if roots.is_empty() {
            return Err(format!("{} holds no CA certificate", ca_path.display()));
        }
        let verifier = rustls::server::WebPkiClientVerifier::builder(std::sync::Arc::new(roots))
            .build()
            .map_err(|e| e.to_string())?;
        rustls::ServerConfig::builder().with_client_cert_verifier(verifier)
    } else {
        rustls::ServerConfig::builder().with_no_client_auth()
    };

    builder
        .with_single_cert(certs, key)
        .map_err(|e| format!("the certificate and key do not go together: {e}"))
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
    let sweeper = tokio::spawn(sweep(Arc::clone(&state), shutdown.clone()));

    let signalled = shutdown.clone();
    axum::serve(listener, router(Arc::clone(&state)))
        .with_graceful_shutdown(async move { signalled.wait().await })
        .await?;

    // Wait for the sweeper to actually stop, rather than sleeping long enough
    // that it probably has. The first version slept twice the tick interval,
    // which was two seconds of dead time on every deploy for a task that
    // notices in microseconds — and would still have been a guess at a longer
    // tick. Bounded anyway, because a sweeper wedged inside a save must not
    // hold the process open.
    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(state.config.limits.drain_timeout_ms),
        sweeper,
    )
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
    tracing::info!(
        documents = outstanding,
        "draining documents with unsaved work"
    );

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
                tracing::error!(document = %live.title, error = %why, "the final save failed");
                CallbackOutcome::Failed
            }
            Err(_) => {
                tracing::error!(document = %live.title, "the final save timed out");
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
                            tracing::warn!(document = %live.title, error = %why, "the callback failed");
                            CallbackOutcome::Failed
                        }
                    }
                }
                Err(why) => {
                    tracing::error!(document = %live.title, error = %why, "could not assemble the document");
                    CallbackOutcome::Failed
                }
            };
            if matches!(outcome, CallbackOutcome::Accepted(_)) {
                tracing::info!(document = %live.title, revision, "saved");
            }
            let follow_up = { lock(&live.session).callback(outcome, now) };
            act_on_follow_up(live, follow_up);
        }
        other => act_on_follow_up(live, Some(other)),
    }
}

/// Tell the participants what the lifecycle decided, when it concerns them.
fn act_on_follow_up(live: &Arc<Live>, action: Option<Action>) {
    match action {
        Some(Action::WarnNotSaving { attempt }) => {
            tracing::warn!(
                attempt,
                "telling participants their work is not being saved"
            );
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
            tracing::error!("a document has gone read-only: its work cannot be saved");
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
    let Some((claims, resume)) = authorise(&state, &document_key, &mut socket).await else {
        return;
    };

    // One span per connection, carrying the fields every event under it should
    // be filterable by. The document *key* is deliberately absent: it names a
    // customer's file, and a log is the easiest place for that to leak.
    let span = tracing::info_span!(
        "connection",
        user = %claims.user.id,
        document = %claims.document.id,
        access = ?claims.permissions.access,
    );
    let _guard = span.enter();

    // Said before the document is fetched, not after. Opening one is a request
    // to the integrator's server, and until it answers there is nothing to send
    // — which from the client's side looks exactly like a server that has hung.
    // A user responds to that by reloading, which starts the same wait again
    // while the first one is still running.
    //
    // This is also what keeps the *connection* legible during the wait: the
    // client can go on pinging, so it can tell a slow origin from a dead
    // socket. Those need completely different reactions and used to be the same
    // silence.
    if send(
        &mut socket,
        &ServerMessage::Opening {
            title: claims.document.title.clone(),
        },
    )
    .await
    .is_err()
    {
        return;
    }

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

    // A reconnecting client that can be resumed keeps the id it had. That is
    // what makes the server's `(client, seq)` duplicate suppression span the
    // gap, and so what makes it safe for the client to resend a chunk it never
    // heard back about — the alternative being to lose it or to apply it twice
    // (ADR-015). A resume is honoured only for the user it was issued to.
    // Two questions, and they must be kept apart. *Is this a participant we
    // know?* — and only if so, *can we still catch it up?* A client whose key we
    // have never seen is simply joining for the first time and has lost
    // nothing; a client we recognise and cannot catch up is about to have its
    // unsent work replaced by a snapshot, and has to be told.
    let recognised = resume
        .as_ref()
        .and_then(|ask| lock(&live.resumes).honour(&ask.key, &claims.user.id));
    let resumed = recognised.and_then(|client| {
        // Locking twice rather than holding one guard across both: no lock is
        // held across an await anywhere in this module.
        let mut session = lock(&live.session);
        session
            .rejoin(resume.as_ref().map_or(0, |ask| ask.revision))
            .map(|caught_up| (client, caught_up))
    });

    let client = match &resumed {
        Some((client, _)) => *client,
        None => {
            let mut next = lock(&live.next_client);
            *next += 1;
            ClientId(*next)
        }
    };
    if let Some(ask) = &resume {
        // Remembered on every join that offers a key, not only on a resumed
        // one: the point of a key is the *next* reconnect.
        lock(&live.resumes).remember(&ask.key, &claims.user.id, client);
    }

    let read_only = lock(&live.session).is_read_only();
    let editable = claims.permissions.access >= crate::token::Access::Comment && !read_only;

    if let Some((_, caught_up)) = resumed {
        lock(&live.roster).joined(
            client,
            claims.user.name.clone(),
            claims.user.color.clone(),
            now_ms(),
        );
        if send(
            &mut socket,
            &ServerMessage::Resumed {
                protocol: PROTOCOL_VERSION,
                client,
                revision: caught_up.revision,
                editable,
                missed: caught_up.missed,
            },
        )
        .await
        .is_err()
        {
            return;
        }
    } else {
        // Either a first join, or a participant we recognise and cannot catch
        // up. Only the second has anything to lose, and it is told *before* the
        // snapshot arrives — because the snapshot is what destroys the unsent
        // work, and a client that is told can offer to put it somewhere first.
        // Silent is the one thing this must not be.
        if recognised.is_some() {
            let (oldest, current) = {
                let session = lock(&live.session);
                (session.oldest_rebasable(), session.revision())
            };
            let _ = send(
                &mut socket,
                &ServerMessage::Refused {
                    seq: None,
                    reason: Refusal::TooFarBehind { oldest, current },
                },
            )
            .await;
        }

        // No lock is ever held across an `await` in this module. That is not a
        // style preference: a `MutexGuard` held over a suspension point makes
        // the future non-`Send`, and — worse — parks every other participant on
        // this document behind one slow socket.
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
    }

    // A quiet connection is pinged, and one that has not answered anything for
    // long enough is closed. Without this a client that vanished — a closed
    // laptop, a dropped network — leaves a socket that looks open forever,
    // holding a slot, a broadcast subscription and a place in the participant
    // cap, while presence has already forgotten it. That inconsistency is the
    // worse half: a connection nobody is on that can still submit edits.
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_millis(
        state.config.limits.client_ping_ms.max(1),
    ));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_heard = now_ms();

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if now_ms().saturating_sub(last_heard) > state.config.limits.client_idle_ms {
                    tracing::debug!("closing a connection nobody answered on");
                    break;
                }
                // Any live client answers this without the page doing anything,
                // so it is a liveness check that does not depend on the
                // application protocol being spoken.
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
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
                // *Anything* from the far end is proof somebody is there,
                // including the pong a browser sends without being asked. That
                // is what keeps a connected-but-quiet participant in the roster
                // rather than expiring their cursor while they read.
                last_heard = now_ms();
                lock(&live.roster).heartbeat(client, last_heard);
                let Message::Text(text) = frame else { continue };
                let message = match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(message) => message,
                    Err(why) => {
                        // Said, rather than dropped. This used to `continue`
                        // silently, and a client whose messages could not be
                        // read looked *connected and working* from both ends:
                        // the socket was open, the heartbeat was answered, the
                        // roster showed them present, and every edit they made
                        // went nowhere. It took two browsers and a real server
                        // to notice, because every unit test on both sides
                        // constructed the message rather than parsing one.
                        //
                        // The client is told which submission — when the text
                        // is well-formed enough to say — so it can stop waiting
                        // for an acknowledgement that is never coming.
                        tracing::warn!(%why, "could not read a message from a client");
                        let seq = serde_json::from_str::<serde_json::Value>(&text)
                            .ok()
                            .and_then(|v| v.get("seq").and_then(serde_json::Value::as_u64));
                        let _ = send(
                            &mut socket,
                            &ServerMessage::Refused {
                                seq,
                                reason: Refusal::CannotMerge,
                            },
                        )
                        .await;
                        continue;
                    }
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
) -> Option<(Claims, Option<Resume>)> {
    let first = socket.recv().await?;
    let Ok(Message::Text(text)) = first else {
        return None;
    };
    let Ok(ClientMessage::Join {
        protocol,
        token,
        resume,
    }) = serde_json::from_str(&text)
    else {
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
        Ok(claims) => Some((claims, resume)),
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

    // Refused before a byte is fetched. Downloading a document in order to
    // decide there is no room for it wastes the origin's bandwidth and this
    // node's memory to reach the same answer more slowly. The count is checked
    // again after, because it can change while this awaits, and that one is the
    // authority.
    if lock(&state.registry.live).len() >= state.config.limits.max_documents {
        return Err(Refusal::NotSaving);
    }

    // One cell per document key, so everybody who wants this document waits on
    // the *same* fetch rather than starting their own.
    let cell = {
        let mut opening = lock(&state.registry.opening);
        Arc::clone(
            opening
                .entry(key.clone())
                .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new())),
        )
    };

    let outcome = cell
        .get_or_try_init(|| async {
            // Fetched with no lock held: a slow origin must not stop every
            // other document on this node, and this await can last as long as
            // the configured HTTP timeout.
            let bytes = state
                .config
                .fetch
                .get(claims.document.url.clone())
                .await
                .map_err(|_| Refusal::NotSaving)?;

            let session =
                DocumentSession::open(bytes, state.config.save, state.config.snapshots, now_ms())
                    .map_err(|_| Refusal::NotSaving)?;

            let mut registry = lock(&state.registry.live);
            if !registry.contains_key(&key) && registry.len() >= state.config.limits.max_documents {
                return Err(Refusal::NotSaving);
            }
            // `entry` rather than `insert`: a document evicted and reopened
            // between the two checks would otherwise be replaced under the
            // people still in it. Two sessions over one document is the one
            // outcome that must not happen.
            Ok(Arc::clone(registry.entry(key.clone()).or_insert_with(
                || {
                    Arc::new(Live {
                        session: Mutex::new(session),
                        roster: Mutex::new(Roster::new(state.config.limits.presence_ttl_ms)),
                        fan_out: broadcast::channel(256).0,
                        next_client: Mutex::new(0),
                        callback: claims.callback.clone(),
                        title: claims.document.title.clone(),
                        idle_since: std::sync::atomic::AtomicU64::new(0),
                        resumes: Mutex::default(),
                    })
                },
            )))
        })
        .await
        .map(Arc::clone);

    // Dropped whatever happened. On success the document lives in the registry
    // and the cell is a duplicate reference keeping it alive after eviction; on
    // failure leaving it would make the next attempt reuse a cell whose failure
    // it never saw.
    lock(&state.registry.opening).remove(&claims.document.key);
    outcome
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
        ClientMessage::Ping { nonce } => {
            // Answered here rather than anywhere earlier, deliberately: a pong
            // is only worth having if it proves the *whole* path works, and one
            // sent from the read loop before the message was dispatched would
            // prove only that bytes arrived. This is the far end of everything
            // a submission goes through.
            //
            // The nonce goes back untouched. Generating a fresh one would make
            // the answer unmatchable, which is the entire point of having it.
            send(socket, &ServerMessage::Pong { nonce }).await.is_ok()
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
    // oracle. An operator needs it and an attacker must not have it, and a log
    // line is exactly the place that distinction can be kept.
    tracing::warn!(reason = %error, "refused a join");
}

#[cfg(test)]
mod tests;
