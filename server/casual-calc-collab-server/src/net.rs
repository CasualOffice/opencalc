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
    ClientMessage, Draft, PROTOCOL_VERSION, Refusal, Resume, ServerMessage,
};
use casual_calc_transaction::session::ClientId;
use tokio::sync::broadcast;

use crate::cluster::Coordinator;
use crate::document::DocumentSession;
use crate::lifecycle::{Action, CallbackOutcome, SavePolicy};
use crate::presence::Roster;
use crate::token::{Access, Callback, Claims};
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
    /// How many connections may be waiting to authenticate at once.
    ///
    /// Separate from [`max_participants`](Self::max_participants), which counts
    /// people in a document. A socket that has not presented a token belongs to
    /// no document yet, so nothing else can bound it — see `connection`.
    pub max_pending_connections: usize,
    /// How long a connection may take to send its `Join` before it is dropped.
    ///
    /// Generous: this covers a browser on a bad link, not a fast client. What
    /// it stops is the connection that never speaks at all.
    pub join_timeout_ms: u64,
    /// How often the signing keys are re-read from the integrator's JWKS.
    ///
    /// Only the *removal* of a key needs this: a newly published one is picked
    /// up within seconds by the on-demand refresh in `authenticate`, because
    /// somebody presenting a token signed with it is the prompt. Nothing
    /// presents a token for a key that has just been revoked, so a clock is the
    /// only thing that can notice.
    pub jwks_refresh_ms: u64,
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
            max_pending_connections: 256,
            join_timeout_ms: 15_000,
            // Five minutes. Only a *removed* key needs a clock — a new one is
            // picked up on demand — and polling somebody else's endpoint
            // harder than this buys nothing.
            jwks_refresh_ms: 300_000,
            drain_timeout_ms: 10_000,
        }
    }
}

/// What a node needs to be part of a cluster.
///
/// Absent for standalone, which is a **first-class mode** and not a degraded one
/// (ADR-012): one process, leader of every document by definition, and a
/// network round trip to agree with itself would be pure cost. Everything below
/// is written so that `None` here means the cluster code never runs at all,
/// rather than running against a coordinator that happens to be local.
#[derive(Clone)]
pub struct Membership {
    /// This node's stable id, which is what a lease names.
    pub node: String,
    /// The shared store: leases, the log, and the channels.
    pub store: Arc<crate::cluster::redis::Redis>,
    /// How long a lease is taken for.
    ///
    /// Short enough that a dead leader is replaced promptly, long enough that a
    /// live one under load does not lose it constantly — and losing it wrongly
    /// is survivable anyway, which is what the epoch is for.
    pub lease_ms: u64,
    /// Where peers reach this node — the **internal** address, never the public
    /// one.
    ///
    /// A client is given an address by a load balancer and a node is not; the
    /// two are different networks in the deployments this is for, and
    /// announcing the public one would have peers discover an address that
    /// routes back through the proxy they are behind.
    pub advertise: String,
}

impl core::fmt::Debug for Membership {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Membership")
            .field("node", &self.node)
            .finish()
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
    /// The cluster this node belongs to, if any.
    ///
    /// `None` is standalone, and the cluster code then never runs — rather than
    /// running against a coordinator that happens to be local, which would make
    /// the common deployment pay for the uncommon one.
    pub membership: Option<Membership>,
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
    fan_out: broadcast::Sender<(Audience, ServerMessage)>,
    /// The next participant id. Ids are per-document and per-process, which is
    /// all `ClientId` promises.
    next_client: Mutex<u64>,
    /// Where the finished document goes, from the token that opened it.
    callback: Option<Callback>,
    /// The document's name, for the callback and the log.
    title: String,
    /// When this document became empty, or zero while somebody is in it.
    idle_since: std::sync::atomic::AtomicU64,
    /// Held across ordering one submission, so a leader orders them one at a
    /// time.
    ///
    /// The revision is assigned inside `commit` under a lock, and the append
    /// that records it happens afterwards — so without this, two submissions
    /// being ordered concurrently can reach the log in the opposite order to
    /// the one they were given. The conditional append then refuses the one
    /// that arrives second, and its edit is **lost after having already been
    /// applied to this node's document**: the leader has it, the log does not,
    /// and nobody is told.
    ///
    /// Found by two nodes writing at the same moment, which is the first
    /// arrangement in which one connection's submission and one inbox's
    /// submission are ordered at the same time.
    ///
    /// `tokio`'s mutex rather than the standard one because it is held across
    /// awaits — the append and the publish are both round trips.
    writing: tokio::sync::Mutex<()>,
    /// The lease as this node last saw it, or `None` in standalone.
    ///
    /// Kept rather than asked for per submission: a claim is a round trip, and
    /// one per edit would put the coordinator in the typing path. Refreshed on a
    /// timer, and stale by at most that interval — which is safe because being
    /// wrong about it is what the epoch fences.
    leader: Mutex<Option<crate::cluster::Lease>>,
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

/// Who a broadcast message is for.
///
/// A second case exists only because of the relay. A node that forwarded a
/// submission to the leader learns it was ordered from the document's channel,
/// which every node reads — so the acknowledgement has to name the client it
/// belongs to, and only the node holding that client acts on it. Broadcasting
/// it to everybody would acknowledge one client's work to another
/// ([ADR-017](../../../docs/63-COLLABORATION-RELAY.md)).
#[derive(Debug, Clone)]
enum Audience {
    /// Everyone on this document except the one named.
    ///
    /// Excluded rather than included because a client has already applied its
    /// own operations locally — that is what makes editing feel immediate — and
    /// sending them back would apply them a second time.
    OthersThan(ClientId),
    /// Exactly one client, if it is on this node.
    Only(ClientId),
    /// Everyone on this document.
    ///
    /// For a batch written on another node: nobody here wrote it, so nobody
    /// here has applied it already, and excluding a client id would exclude
    /// whichever local participant happens to share a number with the remote
    /// writer.
    All,
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

impl Service {
    /// Turn this node's own counter into an id unique across the cluster.
    ///
    /// A `ClientId` used to be a bare per-node counter, so the first
    /// participant on **every** node was client 1. That is fine for one node
    /// and wrong the moment there are two, because the things keyed by it are
    /// per *document* and now see submissions from every node: the duplicate
    /// suppression that makes reconnecting safe would discard a second
    /// person's first chunk as a redelivery of the first person's, and the
    /// broadcast would acknowledge one node's writer to another node's reader.
    ///
    /// Both were real, and both were invisible until two nodes served one
    /// document. So the node's name goes in the high half. It is a hash rather
    /// than a registry because it must be derivable without coordination, and
    /// a collision costs what the bare counter cost before — which is to say
    /// this is strictly better, and not a guarantee.
    fn identify(&self, counter: u64) -> u64 {
        let Some(membership) = &self.config.membership else {
            // Standalone: one node, so the counter alone is already unique and
            // the ids stay small and readable in a log.
            return counter;
        };
        use std::hash::{Hash as _, Hasher as _};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        membership.node.hash(&mut hasher);
        let node = (hasher.finish() as u32) as u64;
        (node << 32) | (counter & 0xffff_ffff)
    }
}

/// The service's shared state.
struct Service {
    config: ServiceConfig,
    registry: Registry,
    /// Permits for connections that have not authenticated yet.
    ///
    /// An `Arc` so a permit can outlive the borrow that took it — the guard is
    /// held across the join and dropped the moment it succeeds.
    pending: Arc<tokio::sync::Semaphore>,
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
        stop_requested().await;
        on_signal.begin();
    });
    serve_on_with_shutdown(listener, config, shutdown).await
}

/// Wait until the process is asked to stop.
///
/// Split by platform because `SIGTERM` does not exist on Windows and naming it
/// there does not compile. That was not caught locally or by the Linux jobs —
/// only by the Windows leg of the platform matrix, which is the whole reason
/// that leg exists: the engine crates have to build wherever a contributor
/// works, and a workspace that only compiles on Unix is one they cannot test.
///
/// The server itself is deployed in Linux containers, so this is about being
/// buildable rather than about Windows being a target for it.
#[cfg(unix)]
async fn stop_requested() {
    // SIGTERM is how an orchestrator asks; Ctrl-C is how a person does.
    let Ok(mut term) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        return;
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}

/// Wait until the process is asked to stop.
///
/// Ctrl-C alone: Windows has no `SIGTERM`, and its nearest equivalents arrive
/// through console control handlers that `tokio` surfaces separately. Enough for
/// somebody running the server on a workstation, which is what this platform is
/// for here.
#[cfg(not(unix))]
async fn stop_requested() {
    let _ = tokio::signal::ctrl_c().await;
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
    // Read before `config` is moved into the struct.
    let pending = config.limits.max_pending_connections.max(1);
    let state = Arc::new(Service {
        config,
        registry: Registry::default(),
        pending: Arc::new(tokio::sync::Semaphore::new(pending)),
    });
    // The sweeper is what makes this a service rather than a relay. Without it
    // the save lifecycle — quiesce timer, ceiling, revision cadence, callback
    // retry, read-only fencing — is built, tested and driven by nothing, and
    // the node holds the only copy of every edit until it restarts.
    let sweeper = tokio::spawn(sweep(Arc::clone(&state), shutdown.clone()));
    // Spawned here rather than in `serve`, so it can see the registry: the load
    // a node announces is the number of documents it holds, and that is only
    // knowable from this side of the service being built.
    if state.config.membership.is_some() {
        tokio::spawn(announce(Arc::clone(&state)));
    }
    // Keys are re-read while the process runs, because an integrator rotates
    // them while the process runs. Without this a scheduled rotation locks
    // every user out of every document until an operator restarts every node,
    // and revoking a compromised key has no effect at all — see `JwksSource`.
    if state.config.verifier.jwks().is_some() {
        tokio::spawn(refresh_keys(Arc::clone(&state), shutdown.clone()));
    }

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
/// Re-read the signing keys, for as long as the server runs.
///
/// Slow on purpose. The on-demand path in `authenticate` is what makes a
/// *newly published* key usable within seconds; this exists so a key **removed**
/// from the set stops being accepted without anybody presenting a token that
/// prompts a look. Nothing asks for a key that is being revoked, so nothing but
/// a clock will notice it is gone.
///
/// A failed fetch leaves the previous keys in place and says so once. docs/59:
/// *"a cached key set keeps working, since an integrator's key server going
/// down should not evict everybody"* — the failure mode of refreshing too
/// eagerly is locking out every user because somebody else's endpoint blinked.
async fn refresh_keys(state: Arc<Service>, shutdown: Shutdown) {
    let interval_ms = state.config.limits.jwks_refresh_ms.max(1_000);
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(interval_ms));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // The first tick fires immediately and the keys were just loaded.
    ticker.tick().await;
    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            () = shutdown.wait() => return,
        }
        let Some(source) = state.config.verifier.jwks() else {
            return;
        };
        match crate::verify::fetch_keys(&source.url, &source.accepted).await {
            Ok(keys) => {
                let before = state.config.verifier.key_count();
                let after = keys.len();
                state.config.verifier.install(keys);
                if before != after {
                    tracing::info!(
                        url = %source.url,
                        before,
                        after,
                        "the signing key set changed"
                    );
                }
            }
            Err(why) => {
                tracing::warn!(%why, "could not refresh the signing keys; keeping the ones held")
            }
        }
    }
}

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
        let _ = live.fan_out.send((
            Audience::OthersThan(client),
            ServerMessage::Departed { client },
        ));
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
                Audience::OthersThan(ClientId(0)),
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
                Audience::OthersThan(ClientId(0)),
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
    // **A connection that has not authenticated is bounded twice: in number and
    // in time.**
    //
    // Neither bound existed. The upgrade needs no token — it cannot, the token
    // arrives in the first frame — so anyone who can reach the port could open
    // sockets and simply never speak. Each one completed the upgrade, spawned a
    // task, and parked in `authorise`'s `recv().await` forever: the heartbeat
    // and idle timers are only started *after* a successful join, and every
    // limit in `Limits` is per-document or per-message, so an unauthenticated
    // connection was attributed to nothing and counted against nothing. They
    // accumulate to the process's descriptor limit, at which point real joins
    // fail at accept — while `/healthz` goes on answering "ok", so the load
    // balancer keeps sending traffic to a node that can no longer take it.
    //
    // The permit is released the moment the join succeeds, because from then on
    // the connection is a *participant* and `max_participants` is what bounds
    // it. Holding it for the life of the session would make this a second,
    // quieter connection cap that nobody configured.
    let Ok(_pending) = state.pending.clone().try_acquire_owned() else {
        tracing::warn!(
            limit = state.config.limits.max_pending_connections,
            "refusing a connection: too many are waiting to authenticate"
        );
        return;
    };
    let joining = tokio::time::timeout(
        std::time::Duration::from_millis(state.config.limits.join_timeout_ms.max(1)),
        authorise(&state, &document_key, &mut socket),
    );
    let Ok(Some((claims, resume))) = joining.await else {
        // Either it refused, or it never said anything. Both end here, and a
        // client that is merely slow reconnects — which is a far better failure
        // than a node that cannot accept anybody.
        return;
    };
    drop(_pending);

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
            ClientId(state.identify(*next))
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

    // Who is already here.
    //
    // Presence is only broadcast when somebody *moves*, so without this a
    // participant who joins a room where everyone is reading rather than typing
    // sees an empty document with nobody in it — and stays that way until one of
    // them happens to click. The roster already knows; it was simply never
    // asked on the way in.
    //
    // Sent after the snapshot deliberately: a cursor is meaningless until there
    // is a document under it, and this way a client that fails at the snapshot
    // never had to skip past a burst of presence to notice.
    {
        let others: Vec<ServerMessage> = lock(&live.roster)
            .everyone()
            .filter(|(who, _)| *who != client)
            .map(|(who, seen)| ServerMessage::Presence {
                client: who,
                name: seen.name.clone(),
                color: seen.color.clone(),
                sheet: seen.sheet,
                selection: seen.selection,
                // Including what they are mid-way through typing. A joiner who
                // was told only where everybody's cursor was would sit watching
                // an empty cell somebody is actively filling, until the next
                // keystroke happened to arrive — and if they had stopped to
                // think, that could be a minute.
                editing: seen.editing.clone(),
            })
            .collect();
        for message in others {
            if send(&mut socket, &message).await.is_err() {
                return;
            }
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
                    Ok((audience, message)) => {
                        let mine = match audience {
                            Audience::OthersThan(origin) => origin != client,
                            Audience::Only(target) => target == client,
                            Audience::All => true,
                        };
                        if mine && send(&mut socket, &message).await.is_err() {
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
    let _ = live.fan_out.send((
        Audience::OthersThan(client),
        ServerMessage::Departed { client },
    ));
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
    let mut outcome = state.config.verifier.verify(&token, document_key, now_secs);
    // **An unknown `kid` is the one refusal worth asking again about.**
    //
    // It means the token was signed with a key this server has not read yet,
    // which is exactly what an integrator publishing a new key looks like from
    // here. Every other refusal is about the token and re-reading the key set
    // cannot change it.
    //
    // Throttled, because the trigger is attacker-reachable: a token naming a
    // `kid` nobody has would otherwise turn every connection attempt into a
    // request to somebody else's key endpoint. `may_attempt` records the try,
    // so a burst of them costs one fetch.
    //
    // Retried once. If the freshly-read set still has no such key, the answer
    // was already correct.
    if matches!(outcome, Err(crate::verify::VerifyError::UnknownKey))
        && let Some(source) = state.config.verifier.jwks()
        && source.may_attempt(now_ms())
    {
        match crate::verify::fetch_keys(&source.url, &source.accepted).await {
            Ok(keys) => {
                tracing::info!(url = %source.url, keys = keys.len(), "re-read the signing keys for an unknown kid");
                state.config.verifier.install(keys);
                outcome = state.config.verifier.verify(&token, document_key, now_secs);
            }
            Err(why) => {
                tracing::warn!(%why, "could not re-read the signing keys; keeping the ones held")
            }
        }
    }
    match outcome {
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
                        writing: tokio::sync::Mutex::default(),
                        leader: Mutex::default(),
                        resumes: Mutex::default(),
                    })
                },
            )))
        })
        .await
        .map(Arc::clone);

    // One attendant per document, started as it is opened. It ends when its
    // subscriptions close, which happens when the document is evicted — so a
    // node does not accumulate a task per document it has ever held.
    if let Ok(live) = &outcome
        && let Some(membership) = state.config.membership.as_ref()
    {
        match listen(membership, &claims.document.key).await {
            Ok(channels) => {
                tokio::spawn(attend(
                    Arc::clone(state),
                    claims.document.key.clone(),
                    Arc::clone(live),
                    channels,
                ));
            }
            Err(why) => {
                tracing::error!(%why, "could not subscribe: this node cannot serve this document");
                lock(&state.registry.live).remove(&claims.document.key);
                return Err(Refusal::NotSaving);
            }
        }
    }

    // Dropped whatever happened. On success the document lives in the registry
    // and the cell is a duplicate reference keeping it alive after eviction; on
    // failure leaving it would make the next attempt reuse a cell whose failure
    // it never saw.
    lock(&state.registry.opening).remove(&claims.document.key);
    outcome
}

/// Act on one client message. Returns whether the connection continues.
async fn handle(
    state: &Arc<Service>,
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
        ClientMessage::Presence {
            sheet,
            selection,
            editing,
        } => {
            // **The trust boundary for a draft is here**, and it is worth being
            // exact about what is and is not done to it.
            //
            // *Bounded*, because this arrives once per keystroke from a party
            // with no obligation to behave, is held for the presence TTL, and
            // is drawn into a cell on everybody else's grid. Once, into a local,
            // so the roster and the relay cannot disagree about what was said.
            //
            // *Refused from anyone who may not edit.* A draft is the preview of
            // an edit and a viewer has no edit to preview — their submissions
            // are refused at the operation (COL-17), so this would be the one
            // channel by which a read-only participant put text of their
            // choosing into everybody's grid. Their cursor still travels:
            // being present is not editing.
            //
            // *Not sanitised*, deliberately. It is a person's half-typed cell
            // and mangling it would be a worse lie than showing it; what makes
            // that safe is where it lands, and SEC-001 is the rule that keeps
            // it out of any markup sink on the way.
            let editing = editing
                .filter(|_| claims.permissions.access == Access::Edit)
                .map(Draft::bounded);
            lock(&live.roster).moved(client, sheet, selection, editing.clone(), now_ms());
            let _ = live.fan_out.send((
                Audience::OthersThan(client),
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
                    editing,
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

            // In a cluster, only the leader orders. A node that does not lead
            // forwards and says nothing to its client yet: the acknowledgement
            // comes back on the document's channel, addressed to that client,
            // and arrives the same way the edit itself would have.
            if let Some(membership) = state.config.membership.as_ref() {
                // Nobody has claimed yet: this is the first edit on a document
                // that has only just opened, and the attendant's first claim is
                // still in flight. Claimed here rather than raced, because the
                // alternative is what the two-node test caught — the submission
                // is forwarded to an inbox no node is yet leading, every node
                // declines it, and the client waits forever for an
                // acknowledgement that nothing will ever send. One round trip on
                // the first edit of a document is a small price for that.
                if lock(&live.leader).is_none()
                    && let Ok(lease) = membership
                        .store
                        .claim(
                            claims.document.key.clone(),
                            membership.node.clone(),
                            membership.lease_ms,
                            now_ms(),
                        )
                        .await
                {
                    *lock(&live.leader) = Some(lease);
                }

                if leads(live, membership) {
                    order(
                        state,
                        &claims.document.key,
                        live,
                        submission,
                        &membership.node,
                    )
                    .await;
                } else {
                    let forwarded = crate::relay::Forwarded {
                        document: claims.document.key.clone(),
                        node: membership.node.clone(),
                        submission,
                    };
                    let channel = crate::relay::inbox_channel(
                        membership.store.namespace(),
                        &claims.document.key,
                    );
                    if let Ok(payload) = serde_json::to_vec(&forwarded)
                        && let Err(why) = membership.store.publish(&channel, payload).await
                    {
                        // Nothing was ordered and the client is still waiting.
                        // Told, rather than left to time out into a resend that
                        // would meet the same unreachable store.
                        tracing::warn!(%why, "could not forward a submission to the leader");
                        let _ = send(
                            socket,
                            &ServerMessage::Refused {
                                seq: Some(forwarded.submission.seq),
                                reason: Refusal::NotSaving,
                            },
                        )
                        .await;
                    }
                }
                return true;
            }

            // **Ordered from commit through broadcast**, which the cluster path
            // does deliberately (`order()`, below) and this one did not.
            //
            // Two things went wrong without it. The broadcast used to happen
            // *after* `await`ing the sender's acknowledgement, and that await is
            // a suspension point: with two connections committing concurrently
            // on a multi-thread runtime, the one whose acknowledgement returned
            // first broadcast first, regardless of which revision it was
            // assigned. A third participant then applied revision 2 before
            // revision 1 — the second already rebased past the first — and set
            // its own revision *backwards*. Nothing detects that: the client
            // applies whatever arrives and assigns the revision it is given.
            //
            // And more certainly: a failed acknowledgement returned early, so an
            // operation committed into the server's own document was never sent
            // to anybody. One slow socket silently cost every other participant
            // the edit.
            //
            // `fan_out.send` is synchronous, so holding the ordering lock across
            // commit and broadcast adds no await between assigning a revision
            // and announcing it. The acknowledgement is sent afterwards, outside
            // the lock, because it is the one part that may block on a socket.
            let outcome = {
                let _ordering = live.writing.lock().await;
                let committed = {
                    let mut session = lock(&live.session);
                    session.commit(&submission, now_ms())
                };
                committed.map(|commit| {
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
                    // Everyone else sees what landed. The sender does not need
                    // it — its own ops are already applied locally, and the ack
                    // is what tells it they are ordered.
                    if !ops.is_empty() {
                        let _ = live.fan_out.send((
                            Audience::OthersThan(client),
                            ServerMessage::Apply { revision, ops },
                        ));
                    }
                    revision
                })
            };
            match outcome {
                Ok(revision) => {
                    if send(
                        socket,
                        &ServerMessage::Ack {
                            through: submission.seq,
                            revision,
                        },
                    )
                    .await
                    .is_err()
                    {
                        return false;
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

// --- The cluster half (ADR-017) ---------------------------------------------

/// Keep this node's claim on `document` current, and act on what arrives.
///
/// One task per document held on this node, started when the document is opened
/// and ended when it is evicted. It does three things, all of them on a timer or
/// a channel and none of them in a client's path:
///
/// - **claims**, so this node either leads or knows who does;
/// - reads the **committed channel**, applying batches other nodes ordered;
/// - reads the **inbox** while it leads, ordering what relays forwarded.
///
/// Nothing here decides that another node is down. A claim that returns somebody
/// else's lease means relay; a claim that returns ours means lead; and the
/// change from one to the other is a consequence of an atomic operation rather
/// than of an opinion about anybody's liveness.
/// Subscribe to a document's channels, before anybody can use it.
///
/// Awaited while the document is being opened rather than spawned alongside it,
/// and that ordering is the whole point. Redis pub/sub has no history: a message
/// published before a subscription exists is simply gone. Spawning this left a
/// window in which the node was serving clients and not yet listening, and a
/// submission forwarded during it vanished — the client waited forever for an
/// acknowledgement nobody would ever send. It is a small window and it is
/// exactly the moment a document is busiest, because opening one is what
/// everybody does at the same time.
///
/// # Errors
///
/// [`Unavailable`](crate::cluster::Unavailable) if either subscription cannot be
/// made, which must prevent the document from opening at all: a node serving a
/// document it cannot hear about the changes to is worse than one that refuses.
async fn listen(
    membership: &Membership,
    key: &str,
) -> Result<Subscriptions, crate::cluster::Unavailable> {
    let namespace = membership.store.namespace();
    Ok(Subscriptions {
        committed: membership
            .store
            .subscribe(&crate::relay::committed_channel(namespace, key))
            .await?,
        inbox: membership
            .store
            .subscribe(&crate::relay::inbox_channel(namespace, key))
            .await?,
    })
}

/// A document's two channels, already subscribed.
struct Subscriptions {
    committed: tokio::sync::mpsc::Receiver<Vec<u8>>,
    inbox: tokio::sync::mpsc::Receiver<Vec<u8>>,
}

async fn attend(state: Arc<Service>, key: String, live: Arc<Live>, channels: Subscriptions) {
    let Some(membership) = state.config.membership.clone() else {
        return;
    };
    let store = Arc::clone(&membership.store);
    let Subscriptions {
        mut committed,
        mut inbox,
    } = channels;

    // Half the lease, so a renewal is missed twice before the lease is lost.
    let mut renewal = tokio::time::interval(std::time::Duration::from_millis(
        (membership.lease_ms / 2).max(1),
    ));
    renewal.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = renewal.tick() => {
                match store
                    .claim(key.clone(), membership.node.clone(), membership.lease_ms, now_ms())
                    .await
                {
                    Ok(lease) => {
                        *lock(&live.leader) = Some(lease);
                        // Every tick, not only when a gap is noticed. A gap is
                        // noticed by the *next* publication failing to follow —
                        // so a document where nothing further happens leaves
                        // whoever missed the last batch sitting behind it
                        // indefinitely, showing a stale document to people who
                        // have no way to tell. ADR-017 named this and did not
                        // close it; this closes it.
                        //
                        // It also covers the moment a node takes over a
                        // document, when its copy may be behind whatever the
                        // previous leader committed and never published.
                        // Ordering anything before catching up would build on a
                        // revision the log has moved past.
                        catch_up(&store, &key, &live, &membership.node).await;
                    }
                    // Left as it was on purpose. A node that cannot reach the
                    // store does not know whether it still leads, and the two
                    // wrong answers are not equally wrong: assuming it does not
                    // lead stops it serving anybody, while assuming it does is
                    // fenced by the epoch the moment it tries to write.
                    Err(why) => tracing::warn!(document = %key, %why, "could not refresh the lease"),
                }
            }
            batch = committed.recv() => {
                let Some(payload) = batch else { break };
                if let Ok(batch) = serde_json::from_slice::<crate::relay::Committed>(&payload) {
                    take(&store, &key, &live, batch, &membership.node).await;
                }
            }
            forwarded = inbox.recv() => {
                let Some(payload) = forwarded else { break };
                if !leads(&live, &membership) {
                    // Somebody else's to order. Every node subscribes to the
                    // inbox so that a leadership change needs no re-subscribe,
                    // and the check is what keeps two nodes from both ordering
                    // the same submission during the moment one is taking over.
                    continue;
                }
                let Ok(forwarded) = serde_json::from_slice::<crate::relay::Forwarded>(&payload)
                else {
                    continue;
                };
                order(&state, &key, &live, forwarded.submission, &forwarded.node).await;
            }
        }
    }
}

/// Whether this node currently holds the document's lease.
fn leads(live: &Arc<Live>, membership: &Membership) -> bool {
    lock(&live.leader)
        .as_ref()
        .is_some_and(|lease| lease.node == membership.node && lease.expires_ms > now_ms())
}

/// Apply a batch another node ordered, and tell this node's clients.
async fn take(
    store: &Arc<crate::cluster::redis::Redis>,
    key: &str,
    live: &Arc<Live>,
    batch: crate::relay::Committed,
    mine: &str,
) {
    let applied = lock(&live.session).revision();
    match crate::relay::react(applied, batch.revision, batch.ops.len()) {
        // Already had it. Redis redelivers, a resubscribe replays, and this is
        // also the leader seeing its own publication — which it applied when it
        // ordered it, because ordering and applying are the same step.
        crate::relay::Reaction::Seen => return,
        crate::relay::Reaction::Apply => {}
        crate::relay::Reaction::CatchUp { .. } => {
            // The batch is *not* applied first. The operations in between are
            // what it was transformed against; without them it lands at
            // coordinates that were never real. Read from where this node
            // actually is, and this batch arrives again as part of that.
            catch_up(store, key, live, mine).await;
            return;
        }
    }
    deliver(live, &batch, batch.revision, mine);
}

/// Read the log from wherever this node is, and apply whatever it has missed.
///
/// The authority. The channel is only a prompt — pub/sub loses messages without
/// saying so — and everything that reaches a client goes through here or through
/// a batch that directly follows what this node already had.
///
/// Reads under no lock and applies one batch at a time, so a long catch-up does
/// not park every other participant on the document.
async fn catch_up(
    store: &Arc<crate::cluster::redis::Redis>,
    key: &str,
    live: &Arc<Live>,
    mine: &str,
) {
    let from = lock(&live.session).revision();
    let missed = match store.since(key.to_owned(), from).await {
        Ok(missed) => missed,
        Err(why) => {
            tracing::warn!(document = %key, %why, "could not read the log to catch up");
            return;
        }
    };
    for (revision, payload) in &missed {
        let Ok(older) = serde_json::from_slice::<crate::relay::Committed>(payload) else {
            continue;
        };
        deliver(live, &older, *revision, mine);
    }
}

/// Apply one batch locally and fan it out.
fn deliver(live: &Arc<Live>, batch: &crate::relay::Committed, revision: u64, mine: &str) {
    {
        let mut session = lock(&live.session);
        if session.adopt(&batch.ops, revision).is_err() {
            return;
        }
        // So a client that reconnects to *this* node is still protected from
        // having a resent chunk applied twice: this node learned the chunk was
        // ordered by seeing it, not by ordering it.
        session.note_accepted(batch.client, batch.seq, revision);
    }
    fan(live, batch, revision, mine);
}

/// Tell this node's clients about a committed batch.
///
/// The one place a batch becomes messages, whether it was ordered here or
/// arrived on the channel. Whose it was is decided by **node and client**, never
/// by client alone: a `ClientId` is a per-node counter, so the first participant
/// on every node is client 1, and matching on it alone acknowledges one node's
/// writer to another node's reader — and withholds the edit from the person who
/// should have received it.
fn fan(live: &Arc<Live>, batch: &crate::relay::Committed, revision: u64, mine: &str) {
    if batch.node == mine {
        let _ = live.fan_out.send((
            Audience::Only(batch.client),
            ServerMessage::Ack {
                through: batch.seq,
                revision,
            },
        ));
        let _ = live.fan_out.send((
            Audience::OthersThan(batch.client),
            ServerMessage::Apply {
                revision,
                ops: batch.ops.clone(),
            },
        ));
    } else {
        // Written elsewhere, so nobody here has it yet.
        let _ = live.fan_out.send((
            Audience::All,
            ServerMessage::Apply {
                revision,
                ops: batch.ops.clone(),
            },
        ));
    }
}

/// Order a submission as the leader, and publish the result.
async fn order(
    state: &Arc<Service>,
    key: &str,
    live: &Arc<Live>,
    submission: casual_calc_transaction::session::Submission,
    origin: &str,
) {
    let Some(membership) = state.config.membership.as_ref() else {
        return;
    };
    // One at a time, from here to the publish. Everything in between assigns a
    // revision and then records it, and those two steps have to stay adjacent
    // or a later revision can reach the log before an earlier one.
    let _ordering = live.writing.lock().await;

    let outcome = {
        let mut session = lock(&live.session);
        session.commit(&submission, now_ms())
    };
    let Ok(casual_calc_transaction::session::Commit::Applied { ops, revision }) = outcome else {
        // A duplicate needs no publication — it happened once — and a failure
        // is the submitting node's to report to its own client, which it will
        // when nothing arrives and it asks again.
        return;
    };

    let batch = crate::relay::Committed {
        revision,
        node: origin.to_owned(),
        client: submission.client,
        seq: submission.seq,
        ops,
    };
    let Ok(payload) = serde_json::to_vec(&batch) else {
        return;
    };

    // Appended before it is published, and published before anybody is told.
    // The order is the whole guarantee: an operation in the log that nobody
    // heard about is caught by the next gap, where one announced and not
    // recorded is lost the moment this node stops.
    let epoch = lock(&live.leader).as_ref().map_or(0, |lease| lease.epoch);
    // Both ends, because a revision counts operations and this batch may carry
    // several. `revision - 1` was right only for a one-operation chunk, and the
    // editor batches everything typed inside a flush window into one — so the
    // ordinary case was the broken one.
    let before = revision - batch.ops.len() as u64;
    match membership
        .store
        .append(
            key.to_owned(),
            epoch,
            before,
            revision,
            payload.clone(),
            now_ms(),
        )
        .await
    {
        Ok(_) => {}
        Err(why) => {
            // Refused: this node's copy has moved somewhere the log has not, so
            // it is no longer a leader in any useful sense. Said plainly; the
            // recovery is a resync, which is not built.
            tracing::error!(document = %key, ?why, "the log refused this leader's append");
            return;
        }
    }
    let channel = crate::relay::committed_channel(membership.store.namespace(), key);
    if let Err(why) = membership.store.publish(&channel, payload).await {
        // Every other node will notice the gap and read the log, so this is
        // slow rather than wrong — and worth knowing about for that reason.
        tracing::warn!(document = %key, %why, "could not publish a committed batch");
    }
    // And this node's own clients, which do not learn of work ordered here from
    // the channel: it was applied at commit, above.
    fan(live, &batch, batch.revision, &membership.node);
}

/// Tell the cluster this node exists, and keep telling it.
///
/// Registration expires, so this is a heartbeat in the only sense the design
/// has one: a node announcing **itself**, never an opinion about anybody else.
/// Nothing here decides another node is down — a node that stops announcing is
/// forgotten when somebody next reads the list, which is expiry rather than a
/// judgement.
///
/// Without this, `peers` returns nothing and `elect` has nothing to pick from,
/// so the load-aware placement they exist for never happens: every node is
/// invisible to every other, and the cluster works only because leadership is
/// decided by a lease that needs no discovery at all. That is a real gap
/// wearing the appearance of working.
///
/// The load announced is this node's **document count**. Documents are what
/// placement should balance: they hold the memory and are what a leader does
/// work for, where connections come and go cheaply.
///
/// An earlier version of this sent a hard zero. That was safe — equal loads make
/// `elect` fall through to the id, so every node still reaches the same answer
/// and leadership stays uncontended — but it left placement unbalanced while
/// looking as though it worked, which is the failure mode this file has now
/// produced twice.
async fn announce(state: Arc<Service>) {
    let Some(membership) = state.config.membership.clone() else {
        return;
    };
    // Comfortably more often than the peer TTL, so a node stays visible through
    // a missed round rather than flickering out of the list and back.
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(5_000));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let peer = crate::cluster::Peer {
            id: membership.node.clone(),
            advertise: membership.advertise.clone(),
            // Read afresh each round rather than cached: the number this is
            // announcing is the one placement will act on, and a stale count
            // sends work to a node that filled up a minute ago.
            load: u32::try_from(lock(&state.registry.live).len()).unwrap_or(u32::MAX),
            seen_ms: now_ms(),
        };
        if let Err(why) = membership.store.register(peer, 30_000, now_ms()).await {
            // Worth saying and not worth stopping for. A node that cannot
            // announce itself is invisible to placement and still perfectly able
            // to serve the documents it leads, because leadership is a lease and
            // does not depend on discovery.
            tracing::warn!(%why, "could not announce this node to its peers");
        }
    }
}
