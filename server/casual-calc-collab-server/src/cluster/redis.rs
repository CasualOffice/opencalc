//! The [`Coordinator`] a cluster actually runs on.
//!
//! [ADR-014](../../../../docs/59-COLLABORATION-SERVICE-STACK.md) chose Redis to
//! coordinate three things — who leads a document, who is alive, and the log —
//! and this is those three, and nothing else. Every rule they follow is stated
//! in [`super`], tested against a clock the test controls, and held identically
//! by [`Memory`](super::Memory). What is here is only how to say the same thing
//! to a database.
//!
//! # Two operations must be atomic, so they are scripts
//!
//! Taking a lease is read-then-write: look at who holds it, decide whether it
//! has lapsed, write yourself in with a raised epoch. Between the read and the
//! write another node can do the same, and then two nodes lead the same
//! document — which is precisely the outcome the whole design exists to
//! prevent. `SET NX PX` alone cannot express it, because the epoch has to be
//! read and conditionally incremented in the same breath.
//!
//! Appending is the same shape: check the fence, check the length, push. A gap
//! between the check and the push is a stale leader's write landing after a new
//! leader's.
//!
//! So both are Lua, which Redis runs to completion without interleaving. This
//! is the one place in the crate where logic lives somewhere other than Rust,
//! and it is worth being uncomfortable about; the alternative is a lock around
//! a database that exists to be the lock.
//!
//! # A list, not a stream
//!
//! ADR-014 says Streams for the op log, and this uses a `LIST`. The reason is
//! that [`Coordinator`] speaks in **revisions** — an append returns `n`, and
//! `since(n)` returns everything after it — and a list index *is* that number,
//! whereas a stream id is a timestamp-sequence pair that would need a parallel
//! mapping to become one. Streams earn their keep through consumer groups,
//! which nothing here uses: fan-out is pub/sub's job.
//!
//! Recorded rather than quietly done. If the log later needs what a stream
//! offers — trimming by memory, several independent readers with their own
//! positions — this is the thing to revisit, and the revision mapping is what
//! it will cost.
//!
//! # Time is still supplied
//!
//! Every method takes `now_ms` and Redis is never asked what time it is. The
//! store's clock is a second clock, and a lease decided against one clock and
//! renewed against another expires at a moment neither agrees on. Keeping the
//! caller's clock is also what lets the same tests run against both backends.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use redis::AsyncCommands;
use redis::aio::ConnectionManager;

use super::sentinel::{self, Resolver, Target};
use super::{Answer, AppendError, Coordinator, Lease, Logged, Peer, Unavailable};

/// The default prefix every key sits under.
///
/// Configurable rather than fixed because one Redis is routinely shared —
/// staging beside production, several tenants, a test suite beside a developer's
/// own server. Two deployments sharing a prefix would share leases, and a
/// staging node would take leadership of a production document and be believed.
pub const DEFAULT_NAMESPACE: &str = "opencalc";

/// Take or renew a lease, atomically.
///
/// `KEYS[1]` the lease key; `ARGV` node, ttl, now. Returns `{node, epoch,
/// expires}`.
///
/// The three cases are exactly [`Memory::claim`](super::Memory)'s, in the same
/// order, because they are the same rules: held by somebody else and live,
/// theirs; ours and live, renewed **without** moving the epoch, since a renewal
/// is not a change of leadership and bumping it would fence the holder against
/// itself; otherwise taken, and the epoch moves, which is what fences whoever
/// had it before — including a holder that is alive and merely slow.
const CLAIM: &str = r"
local raw = redis.call('GET', KEYS[1])
local node, ttl, now = ARGV[1], tonumber(ARGV[2]), tonumber(ARGV[3])
local epoch, expires = 1, now + ttl
if raw then
  local held = cjson.decode(raw)
  if held.expires > now then
    if held.node ~= node then
      return {held.node, tostring(held.epoch), tostring(held.expires)}
    end
    epoch = held.epoch
  else
    epoch = held.epoch + 1
  end
end
local mine = {node = node, epoch = epoch, expires = expires}
-- Expiry on the key as well as in the value: the value is what decides, and
-- the TTL is what stops a document nobody has touched in a week from being
-- remembered forever. Generous, so the value is always the authority.
redis.call('SET', KEYS[1], cjson.encode(mine), 'PX', ttl * 4)
return {node, tostring(epoch), tostring(expires)}
";

/// Append to the log, fenced and conditional.
///
/// `KEYS[1]` lease, `KEYS[2]` log; `ARGV` epoch, after, payload, now. Returns
/// the new revision, or a two-element failure `{reason, detail}`.
///
/// The fence is checked **before** the length, deliberately. A zombie whose
/// revision happens to line up must still be refused, and telling it "stale"
/// would send it to re-read the log and try again, forever.
/// The length was the revision, and it is not: `LLEN` counts *appends* while a
/// revision counts *operations*, so a chunk carrying two edits — which is what
/// typing produces — was refused forever. Each entry now records the revision it
/// carried the document to, as a decimal prefix ahead of a newline, and the gate
/// reads the last one. See [`Coordinator::append`](crate::cluster::Coordinator::append).
///
/// The comparison is `~=` rather than `<`, and that is the coordinator-failover
/// case: see [`Coordinator::append`](crate::cluster::Coordinator::append).
const APPEND: &str = r"
local raw = redis.call('GET', KEYS[1])
if not raw then return {'unled', '0'} end
local held = cjson.decode(raw)
local epoch, after, revision = tonumber(ARGV[1]), tonumber(ARGV[2]), tonumber(ARGV[3])
if epoch ~= held.epoch then return {'fenced', tostring(held.epoch)} end
local last = redis.call('LINDEX', KEYS[2], -1)
local current = 0
if last then current = tonumber(string.match(last, '^(%d+)')) end
if after ~= current then return {'stale', tostring(current)} end
redis.call('RPUSH', KEYS[2], revision .. '\n' .. ARGV[4])
-- Bounded here rather than by a sweeper, because this is the only moment the
-- log is known to be consistent and already locked by the script. A comment in
-- `since` claimed for a long time that 'the log is compacted, which is what
-- keeps this bounded'; nothing compacted it, so a document edited for an
-- afternoon accumulated every batch and re-read all of them every few seconds
-- on every node holding it (DEP-03).
redis.call('LTRIM', KEYS[2], -tonumber(ARGV[6]), -1)
-- And a document nobody comes back to must not leave its log behind forever.
-- Refreshed on every append, so an actively edited document never expires; the
-- window is far longer than idle eviction, so by the time it fires no node
-- holds the document and the next open fetches it from the host.
redis.call('PEXPIRE', KEYS[2], tonumber(ARGV[7]))
return {'ok', tostring(revision)}
";

/// How many batches one document's log keeps.
///
/// The log exists so a node that missed a publication can catch up. A node that
/// cannot be caught up from it is already handled — `adopt` refuses a batch that
/// does not follow, and that refusal is now reported rather than swallowed — so
/// the window only has to be longer than any real gap between a publication
/// being missed and the next reconciliation, which runs every lease tick.
///
/// Ten thousand batches is minutes of continuous typing by a room full of
/// people, against a reconciliation that runs every few seconds.
pub(crate) const LOG_MAX_ENTRIES: u64 = 10_000;

/// How long an untouched log survives.
///
/// Refreshed on every append, so a document being edited never expires. An hour
/// is two orders of magnitude beyond idle eviction (30s by default), so when it
/// fires no node holds the document and the next open fetches it from the host.
pub(crate) const LOG_TTL_MS: u64 = 60 * 60 * 1000;

/// How many times a lost coordinator connection is re-dialled before a command
/// gives up.
///
/// **Bounded on purpose, and the bound is the interesting half.** Reconnecting
/// fixes the case where the coordinator comes back; it introduces the opposite
/// hazard when it does not, because a command that waits for a connection that
/// will never be established is a command that never returns — and `/readyz` is
/// a `peers()` call, so an unbounded wait turns DEP-04's prompt 503 into a probe
/// that hangs and a node that is neither drained nor restarted.
///
/// Four attempts at [`RECONNECT_BACKOFF_MS`] doubling to at most
/// [`RECONNECT_MAX_DELAY_MS`] is under two seconds of trying, against a renewal
/// tick of a few seconds. A coordinator that is back answers on the first
/// attempt; one that is not is reported as unreachable, which is an answer.
const RECONNECT_ATTEMPTS: usize = 4;

/// The first re-dial delay, in milliseconds, doubling on each attempt.
const RECONNECT_BACKOFF_MS: u64 = 100;

/// The longest a re-dial waits between attempts.
const RECONNECT_MAX_DELAY_MS: u64 = 500;

/// The longest a *subscription* waits between attempts to re-establish itself.
///
/// Longer than [`RECONNECT_MAX_DELAY_MS`], and unbounded in count where commands
/// are bounded, because the two failures are not alike. A command has a caller
/// waiting on an answer, and "unreachable" is one. A subscription has nobody
/// waiting: it retries until the document is evicted, so the ceiling only has to
/// keep a long outage from becoming a re-dial storm across every document on
/// every node.
const RESUBSCRIBE_MAX_DELAY_MS: u64 = 5_000;

/// How long one command may take before it is called unreachable.
///
/// Distinct from the re-dial budget: this bounds a connection that is *up* and
/// not answering, which is what a coordinator under memory pressure looks like.
const COMMAND_TIMEOUT_MS: u64 = 5_000;

/// What this node presents, and what it will accept, on the coordinator link.
///
/// Separate from [`crate::config::Endpoint`]'s TLS because the direction is the
/// other way round: there this node is the server proving who it is, and here it
/// is the client deciding whom to believe.
///
/// Both fields are `None` in the ordinary public-CA case, where the system trust
/// store already answers. Neither is a way to *disable* verification: this build
/// does not compile `redis`'s insecure mode, so `rediss://…/#insecure` fails
/// rather than quietly accepting anything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkTls {
    /// A CA the coordinator's certificate must chain to, instead of the system
    /// trust store.
    ///
    /// The case that matters in practice: an internal Redis is not issued a
    /// certificate by a public authority, so a client that can only trust the
    /// system roots can only be pointed at a plaintext port.
    pub root_ca: Option<PathBuf>,
    /// This node's own certificate and key, when the coordinator requires one.
    ///
    /// Mutual TLS, for the same reason the internal endpoint offers it: the
    /// nodes are a known, operator-controlled set, and a password proves a
    /// secret was copied where a certificate proves which node is speaking.
    pub client: Option<crate::config::TlsFiles>,
}

impl LinkTls {
    /// Whether anything here needs a certificate to be read.
    fn is_empty(&self) -> bool {
        self.root_ca.is_none() && self.client.is_none()
    }
}

/// Why a coordinator link could not be built, if it could not.
///
/// Checked before the connection is attempted, because every one of these fails
/// later as something that reads like a network problem — and an operator
/// chasing a network problem will not find a misspelled scheme.
///
/// # Errors
///
/// A description of the misconfiguration, in the operator's terms.
pub fn link_problems(url: &str, tls: &LinkTls) -> Result<(), String> {
    let secured = url.starts_with("rediss://") || url.starts_with(sentinel::SECURED_SCHEME);
    if sentinel::is_sentinel_url(url) {
        // Parsed here as well as at connection time, so a URL that names no
        // service — the mistake that reads as "the sentinels are down" — is a
        // refusal at startup with the form printed.
        Target::parse(url)?;
        if !tls.is_empty() {
            // **Silently ignored is the outcome being refused.** `redis` 0.27
            // builds the connection to the resolved primary through
            // `Client::open`, which takes no certificates: the only TLS the
            // sentinel path can express is "verify against the system trust
            // store". A private CA or a client certificate configured here would
            // be read, validated, and then never used, leaving a link that reads
            // as mutually authenticated and is not.
            return Err(format!(
                "certificates are configured for the coordinator link but OPENCALC_REDIS_URL is \
                 a sentinel URL, and sentinel resolution in this build dials the primary through \
                 the system trust store only — the certificates would be silently ignored. Use a \
                 direct rediss:// URL for a private CA or for mutual TLS, or remove the \
                 certificates and use {} with a publicly issued certificate",
                sentinel::SECURED_SCHEME
            ));
        }
    }
    // The *fragment*, not a substring anywhere in the URL: a password is part of
    // this string, and refusing to start because somebody's password happened to
    // contain the word would be a refusal nobody could diagnose.
    if url
        .split_once('#')
        .is_some_and(|(_, tail)| tail == "insecure")
    {
        return Err(
            "the coordinator URL asks for #insecure, which turns off certificate \
             verification entirely — an encrypted link to whoever answers the port is \
             not an encrypted link to your coordinator. This build does not offer it; \
             point OPENCALC_REDIS_CA at the CA that issued the certificate instead."
                .to_owned(),
        );
    }
    if !secured && !tls.is_empty() {
        // The dangerous shape: an operator who supplied certificates believes
        // the link is encrypted. Starting anyway would carry every lease token
        // and every operation in clear under a configuration that says
        // otherwise.
        return Err(format!(
            "certificates are configured for the coordinator link but OPENCALC_REDIS_URL is \
             {url:?}, which is plaintext: use rediss:// or remove the certificates"
        ));
    }
    Ok(())
}

/// What is worth saying out loud at startup about the coordinator link.
///
/// The same job [`crate::config::Exposure::warnings`] does for the listeners,
/// for the one connection that is not a listener. Nothing here will ever
/// *fail* — a plaintext coordinator link works perfectly — which is exactly why
/// nothing else would mention it.
#[must_use]
pub fn link_warnings(url: &str) -> Vec<String> {
    let mut out = Vec::new();
    if !url.starts_with("rediss://") && !url.starts_with(sentinel::SECURED_SCHEME) {
        out.push(
            "the coordinator link is plaintext: the lease that decides which node may write \
             a document, and every operation appended to the log, travel in clear between \
             this node and Redis"
                .to_owned(),
        );
        // Worse than the operations, and quieter: a password sent over a
        // plaintext link is readable by anything on the path, and it is the
        // credential for the whole cluster's coordination state.
        if url.contains('@') {
            out.push(
                "the coordinator URL carries a password over a plaintext link, so the \
                 credential for the cluster's leases and op log is sent in clear on every \
                 connection"
                    .to_owned(),
            );
        }
    }
    out
}

/// How much redundancy a coordinator must be *configured* to insist on.
///
/// Zero — the default — means the node does not check, which is the behaviour
/// every deployment had before ADR-020 and is left alone deliberately: a
/// single-node coordinator has no replicas to be in sync with and refusing to
/// start against one would be a new outage in place of an old risk.
///
/// A positive value is checked against the primary's own
/// `min-replicas-to-write` — at startup **and again on every failover**, because
/// the setting is per server and the mistake this catches is the one where only
/// the original primary was configured. A primary below the floor is not
/// adopted: the node keeps refusing rather than quietly resuming against a
/// coordinator that will accept writes it can lose.
///
/// See [ADR-020](../../../../docs/77-COORDINATOR-AVAILABILITY.md) §2b.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkPolicy {
    /// What this node presents to, and accepts from, the coordinator.
    pub tls: LinkTls,
    /// The `min-replicas-to-write` a primary must already be configured with
    /// before this node will use it. Zero means unchecked.
    pub min_replicas: u32,
}

/// The live connection, and the generation of resolution that produced it.
///
/// The generation is what stops a failover from being re-resolved once per
/// caller: everything reads it before issuing a command and hands it back when
/// the command fails, so a hundred documents failing at the same instant ask the
/// sentinels once between them.
struct Live {
    /// Kept because a **subscription needs its own connection**: a Redis
    /// connection in subscribe mode accepts almost nothing else, so sharing the
    /// multiplexed one would take the coordinator offline the moment anything
    /// subscribed.
    client: redis::Client,
    /// A multiplexed connection **that re-dials**.
    ///
    /// The plain multiplexed connection does not: the task driving the socket
    /// ends when the socket does, and every later command on it — and on every
    /// clone, which is what [`Link::commands`] hands out — fails for the life
    /// of the process. A coordinator restart therefore cost a restart of every
    /// node in the cluster, which is most of what "a single Redis failure stops
    /// ordering cluster-wide" meant (DEP-13).
    ///
    /// It re-dials; it does **not** re-send. A command that was in flight comes
    /// back as an error, which is what the callers already handle and what
    /// `append`'s conditional-on-revision shape needs — a silently retried
    /// append is a second write nobody asked for.
    connection: ConnectionManager,
    generation: u64,
}

/// How the coordinator's address is arrived at.
enum Dial {
    /// The URL *is* the address, and re-dialling it is the whole recovery
    /// story. Nothing to re-ask, so nothing here changes for a deployment that
    /// was working before ADR-020.
    Direct,
    /// The URL names sentinels, and the address is whatever they say **now**.
    ///
    /// Boxed only to keep the two variants a similar size: a [`Resolver`] holds
    /// a cached connection per sentinel, and there is exactly one [`Dial`] per
    /// process either way.
    Sentinel(Box<tokio::sync::Mutex<Resolver>>),
}

/// The coordinator link: a connection, and the means of getting another one.
///
/// Behind an [`Arc`] because the subscription tasks outlive the call that
/// spawned them and must re-resolve on their own — a pub/sub socket is a second
/// connection with a second, quieter way of dying (see [`Redis::subscribe`]).
struct Link {
    dial: Dial,
    live: tokio::sync::RwLock<Live>,
    min_replicas: u32,
}

impl Link {
    /// The connection to issue a command on, and the generation it belongs to.
    async fn commands(&self) -> (ConnectionManager, u64) {
        // Cloned rather than borrowed: a `ConnectionManager` is a handle to a
        // multiplexed connection, designed to be used from many places at once,
        // and every command below wants it `&mut`. The clone is an `Arc` bump —
        // the connection underneath is shared, including the re-dial, so a
        // reconnection is seen by every caller rather than by one of them.
        let live = self.live.read().await;
        (live.connection.clone(), live.generation)
    }

    /// The client to open a *subscription* through, and its generation.
    async fn client(&self) -> (redis::Client, u64) {
        let live = self.live.read().await;
        (live.client.clone(), live.generation)
    }

    /// Pass an outcome through, re-asking the sentinels if it says the primary
    /// moved.
    ///
    /// The command is **not** retried. A command that failed against a coordinator
    /// that has gone away may or may not have been applied, and the callers are
    /// built for that answer — `append`'s conditional-on-revision shape exists
    /// precisely so a retry is the submitter's decision and not this layer's.
    async fn recover<T>(&self, at: u64, outcome: redis::RedisResult<T>) -> redis::RedisResult<T> {
        if let Err(why) = &outcome
            && moved_primary(why)
        {
            self.resolve_again(at).await;
        }
        outcome
    }

    /// Ask the sentinels where the primary is, and adopt the answer.
    ///
    /// `at` is the generation the caller was using. A resolution that has
    /// already happened since then is left alone, which is what keeps a cluster
    /// of documents from producing one sentinel query each.
    async fn resolve_again(&self, at: u64) {
        let Dial::Sentinel(resolver) = &self.dial else {
            // A direct URL has nothing to re-ask: `ConnectionManager` is already
            // re-dialling the one address there is.
            return;
        };
        // Held across the resolution on purpose — this is the serialization.
        // Nothing takes this lock while holding `live`, in either order, so the
        // pair cannot deadlock.
        let mut resolver = resolver.lock().await;
        let current = self.live.read().await.generation;
        if current != at {
            return;
        }

        let client = match resolver.primary().await {
            Ok(client) => client,
            Err(why) => {
                tracing::warn!(service = resolver.service(), %why, "the coordinator's primary could not be found");
                return;
            }
        };
        let connection = match manager(&client).await {
            Ok(connection) => connection,
            Err(why) => {
                tracing::warn!(service = resolver.service(), %why, "the coordinator's primary was named but could not be dialled");
                return;
            }
        };
        if let Err(why) = durability_floor(&mut connection.clone(), self.min_replicas).await {
            // **Not adopted.** A promoted replica that is not configured to
            // require redundancy will accept writes it can lose on the next
            // failover, and resuming against it is exactly the silent loss
            // ADR-020 exists to convert into a refusal. Refusing to adopt it
            // leaves this node reporting `NotSaving` and answering 503, which is
            // an answer.
            tracing::error!(
                service = resolver.service(),
                %why,
                "the coordinator's new primary is below the required replica floor and was not adopted"
            );
            return;
        }

        let mut live = self.live.write().await;
        live.client = client;
        live.connection = connection;
        live.generation = live.generation.wrapping_add(1);
        tracing::info!(
            service = resolver.service(),
            "the sentinels named a new coordinator primary"
        );
    }
}

/// Whether an error means the node this link is dialling is no longer the
/// primary.
///
/// Two shapes, and the second is the one a re-dial cannot fix:
///
/// - the connection died, which is a primary that was killed;
/// - **`READONLY`**, which is a primary that was *demoted* — the socket is
///   healthy, the node answers reads, and every write fails. `redis`
///   classifies `READONLY` as `NoRetry`, so a `ConnectionManager` pointed at a
///   demoted primary re-dials nothing and refuses every claim and every append
///   for the life of the process. That is the original DEP-13 defect wearing a
///   healthy connection.
///
/// `NOREPLICAS` is deliberately **not** here. A primary refusing writes because
/// its in-sync set has collapsed is the right primary doing the right thing, and
/// re-asking the sentinels would only find the same node.
fn moved_primary(why: &redis::RedisError) -> bool {
    matches!(
        why.kind(),
        redis::ErrorKind::ReadOnly | redis::ErrorKind::MasterDown
    ) || why.is_io_error()
        || why.is_connection_refusal()
        || why.is_timeout()
        || why.is_unrecoverable_error()
}

/// A cluster's shared state, in Redis.
pub struct Redis {
    link: Arc<Link>,
    namespace: String,
    claim: redis::Script,
    append: redis::Script,
}

impl core::fmt::Debug for Redis {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Never the connection: a Redis URL routinely carries a password, and a
        // struct like this is exactly what gets logged at startup.
        f.write_str("Redis { .. }")
    }
}

impl Redis {
    /// The remaining lifetime of a document's log, in milliseconds.
    ///
    /// Test-facing: the expiry is set inside the append script, so the only way
    /// to assert it was set is to ask Redis. `-1` means no expiry and `-2` means
    /// no key, and both are failures the test names rather than numbers it
    /// silently compares.
    #[cfg(test)]
    /// Who currently holds a document's lease, without taking it.
    ///
    /// Test-facing, and it exists because `claim` is not a read: given a free
    /// lease it takes one, so polling with it to *observe* leadership races the
    /// node under test and wins whenever the machine is busy.
    #[cfg(test)]
    pub(crate) async fn holder_of(&self, document: &str) -> Option<String> {
        let (mut c, _) = self.link.commands().await;
        let raw: Option<String> = redis::cmd("GET")
            .arg(self.lease_key(document))
            .query_async(&mut c)
            .await
            .ok()?;
        let value: serde_json::Value = serde_json::from_str(&raw?).ok()?;
        value.get("node")?.as_str().map(str::to_owned)
    }

    #[cfg(test)]
    pub(crate) async fn log_ttl_ms(&self, document: &str) -> i64 {
        let (mut c, _) = self.link.commands().await;
        redis::cmd("PTTL")
            .arg(self.log_key(document))
            .query_async(&mut c)
            .await
            .unwrap_or(-2)
    }

    /// Connect to `url`.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] if the URL is unusable or the server cannot be reached.
    /// Failing at startup is the point: a node that comes up believing it is in
    /// a cluster it cannot reach will take leadership of everything it is
    /// asked about, and be wrong about all of it.
    pub async fn connect(url: &str) -> Result<Self, Unavailable> {
        Self::connect_within(url, DEFAULT_NAMESPACE).await
    }

    /// Connect to `url`, keeping every key under `namespace`.
    ///
    /// Trusts the system certificate store when `url` is `rediss://`. Use
    /// [`Self::connect_secured`] for a private CA or for mutual TLS.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] if the URL is unusable or the server cannot be reached.
    pub async fn connect_within(url: &str, namespace: &str) -> Result<Self, Unavailable> {
        Self::connect_secured(url, namespace, &LinkTls::default()).await
    }

    /// Connect to `url` under `namespace`, presenting and trusting `tls`.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] if the URL is unusable, a certificate cannot be read, or
    /// the server cannot be reached. Failing at startup is the point: a node
    /// that comes up believing it is in a cluster it cannot reach will take
    /// leadership of everything it is asked about, and be wrong about all of it.
    pub async fn connect_secured(
        url: &str,
        namespace: &str,
        tls: &LinkTls,
    ) -> Result<Self, Unavailable> {
        Self::connect_under(
            url,
            namespace,
            &LinkPolicy {
                tls: tls.clone(),
                min_replicas: 0,
            },
        )
        .await
    }

    /// Connect to `url` under `namespace`, holding `policy`.
    ///
    /// The one entry point that understands the **sentinel** form of the URL:
    /// `redis+sentinel://host:26379,host:26379/service` is resolved to whichever
    /// node the sentinels currently call the primary, and re-resolved whenever
    /// that answer stops working. See [`super::sentinel`].
    ///
    /// # Errors
    ///
    /// [`Unavailable`] if the URL is unusable, a certificate cannot be read, no
    /// sentinel answers, the server cannot be reached, or the primary is below
    /// the policy's replica floor. Failing at startup is the point: a node that
    /// comes up believing it is in a cluster it cannot reach will take
    /// leadership of everything it is asked about, and be wrong about all of it.
    pub async fn connect_under(
        url: &str,
        namespace: &str,
        policy: &LinkPolicy,
    ) -> Result<Self, Unavailable> {
        link_problems(url, &policy.tls).map_err(Unavailable)?;
        let (dial, client) = if sentinel::is_sentinel_url(url) {
            let mut resolver = Target::parse(url)
                .map_err(Unavailable)?
                .resolver()
                .map_err(Unavailable)?;
            let client = resolver.primary().await?;
            (
                Dial::Sentinel(Box::new(tokio::sync::Mutex::new(resolver))),
                client,
            )
        } else if policy.tls.is_empty() {
            (
                Dial::Direct,
                redis::Client::open(url).map_err(|e| Unavailable(e.to_string()))?,
            )
        } else {
            (
                Dial::Direct,
                redis::Client::build_with_tls(url, certificates(&policy.tls)?)
                    .map_err(|e| Unavailable(e.to_string()))?,
            )
        };

        let connection = manager(&client).await?;
        durability_floor(&mut connection.clone(), policy.min_replicas).await?;

        Ok(Self {
            link: Arc::new(Link {
                dial,
                live: tokio::sync::RwLock::new(Live {
                    client,
                    connection,
                    generation: 0,
                }),
                min_replicas: policy.min_replicas,
            }),
            namespace: namespace.to_owned(),
            claim: redis::Script::new(CLAIM),
            append: redis::Script::new(APPEND),
        })
    }

    /// The namespace every key and channel of this node sits under.
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Publish `payload` to `channel`.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] if the store cannot be reached. Worth handling rather
    /// than logging: a publication that did not go out is a batch every other
    /// node will notice as a gap and read from the log — which is correct, and
    /// slower, and worth knowing about.
    pub async fn publish(&self, channel: &str, payload: Vec<u8>) -> Result<(), Unavailable> {
        let (mut c, at) = self.link.commands().await;
        let sent = c.publish::<_, _, ()>(channel, payload).await;
        self.link
            .recover(at, sent)
            .await
            .map_err(|e| Unavailable(explain(&e)))?;
        Ok(())
    }

    /// Subscribe to `channel`, receiving payloads until the receiver is dropped.
    ///
    /// The subscription runs on its own task with its own connection, and stops
    /// when the returned receiver is dropped — which is what makes a document
    /// being evicted also close its subscription, rather than leaving a task
    /// per document this node has ever held.
    ///
    /// # A subscription that ends is not a subscription that was cancelled
    ///
    /// A pub/sub connection is a second socket, and it dies the same way the
    /// first one does — but far more quietly. The message stream simply *ends*,
    /// which is indistinguishable at the type level from "nothing more will be
    /// published". The receiver closes, and the per-document attendant in
    /// `net.rs` breaks out of its loop: that document stops renewing its lease
    /// and stops reading its inbox, while the node goes on serving everybody
    /// connected to it, as though it were still in the cluster (DEP-13).
    ///
    /// So the task re-dials and re-subscribes, and only stops when the
    /// **receiver** is gone, which is the one signal that really does mean
    /// nobody wants this any more.
    ///
    /// A resubscribe loses whatever was published during the gap, and that is
    /// acceptable for exactly the reason the channel was fire-and-forget to
    /// begin with: the log is the record. `catch_up` runs on every lease tick
    /// and reads from where the node actually is, so a gap here costs latency
    /// rather than correctness.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] if the connection cannot be opened or the subscription
    /// refused. The **first** subscription is awaited and its failure returned,
    /// because a document that opens without one is a document this node cannot
    /// hear about the changes to.
    pub async fn subscribe(
        &self,
        channel: &str,
    ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, Unavailable> {
        let mut pubsub = self.subscription(channel).await?;

        // Bounded. A subscriber that stops reading must not let the channel
        // grow without limit — and dropping the oldest would be worse than
        // stopping, since a missed batch is a gap the receiver detects and
        // closes from the log, whereas unbounded growth is the node dying.
        let (out, into) = tokio::sync::mpsc::channel(256);
        let link = Arc::clone(&self.link);
        let channel = channel.to_owned();
        tokio::spawn(async move {
            use futures_util::StreamExt as _;
            loop {
                {
                    let mut stream = pubsub.on_message();
                    while let Some(message) = stream.next().await {
                        let Ok(payload) = message.get_payload::<Vec<u8>>() else {
                            continue;
                        };
                        if out.send(payload).await.is_err() {
                            // The receiver is gone: the document was evicted, or
                            // the node is shutting down. Either way there is
                            // nobody to deliver to and the connection should be
                            // released.
                            return;
                        }
                    }
                }
                // The stream ended, which means the connection did. Re-dial,
                // giving up only when there is nobody left to deliver to.
                let mut delay = RECONNECT_BACKOFF_MS;
                pubsub = loop {
                    if out.is_closed() {
                        return;
                    }
                    // Which client, not merely another attempt at the same one.
                    // A failover moves the primary, and a subscription reopened
                    // against the node that used to be it hears a channel
                    // nobody publishes to any more. The generation is what keeps
                    // a node holding a hundred documents from asking the
                    // sentinels a hundred times for the same answer.
                    let (client, at) = link.client().await;
                    match Self::open_subscription(&client, &channel).await {
                        Ok(fresh) => break fresh,
                        Err(why) => {
                            link.resolve_again(at).await;
                            tracing::warn!(
                                %channel,
                                %why,
                                "the coordinator's channel is unreachable; retrying"
                            );
                            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                            delay = (delay * 2).min(RESUBSCRIBE_MAX_DELAY_MS);
                        }
                    }
                };
                tracing::info!(%channel, "resubscribed to the coordinator's channel");
            }
        });
        Ok(into)
    }

    async fn subscription(&self, channel: &str) -> Result<redis::aio::PubSub, Unavailable> {
        let (client, _) = self.link.client().await;
        Self::open_subscription(&client, channel).await
    }

    async fn open_subscription(
        client: &redis::Client,
        channel: &str,
    ) -> Result<redis::aio::PubSub, Unavailable> {
        let mut pubsub = client
            .get_async_pubsub()
            .await
            .map_err(|e| Unavailable(e.to_string()))?;
        pubsub
            .subscribe(channel)
            .await
            .map_err(|e| Unavailable(e.to_string()))?;
        Ok(pubsub)
    }
}

/// The re-dialling multiplexed connection every command goes through.
///
/// One place rather than two, because a connection built on the failover path
/// with a different retry budget from the one built at startup is a difference
/// nobody would find until a failover.
async fn manager(client: &redis::Client) -> Result<ConnectionManager, Unavailable> {
    ConnectionManager::new_with_config(
        client.clone(),
        redis::aio::ConnectionManagerConfig::new()
            .set_exponent_base(2)
            .set_factor(RECONNECT_BACKOFF_MS)
            .set_number_of_retries(RECONNECT_ATTEMPTS)
            .set_max_delay(RECONNECT_MAX_DELAY_MS)
            .set_response_timeout(std::time::Duration::from_millis(COMMAND_TIMEOUT_MS))
            .set_connection_timeout(std::time::Duration::from_millis(COMMAND_TIMEOUT_MS)),
    )
    .await
    .map_err(|e| Unavailable(e.to_string()))
}

/// Refuse a primary that is not configured to require `wanted` in-sync replicas.
///
/// **Why a node checks its coordinator's configuration at all.** Redis
/// replication is asynchronous: a primary acknowledges a write before any
/// replica has it, so a promoted replica can be missing appends the old primary
/// accepted. ADR-014 §4 makes one durability promise — an operation is written to
/// the log before the client is told it was accepted — and that promise is only
/// as strong as the log. `min-replicas-to-write` is what converts the loss window
/// into a **refusal**: below the threshold the primary stops accepting writes,
/// which this node already reports as `Refused { NotSaving }` and a 503 on
/// `/readyz`.
///
/// A deployment that asks for that promise and gets a primary without the
/// setting has the promise silently withdrawn, and would not find out until a
/// failover ate an acknowledged edit. So the node checks, and refuses.
///
/// Zero means unchecked, which is the default and the pre-ADR-020 behaviour.
///
/// # Errors
///
/// [`Unavailable`] when the setting is below `wanted`, or when it cannot be
/// read. **Unreadable is a failure rather than a pass**: `CONFIG GET` is
/// routinely renamed or disabled on a managed Redis, and treating "I could not
/// check" as "it is fine" is how the check would silently stop being one.
async fn durability_floor(
    connection: &mut ConnectionManager,
    wanted: u32,
) -> Result<(), Unavailable> {
    if wanted == 0 {
        return Ok(());
    }
    let setting: Vec<String> = redis::cmd("CONFIG")
        .arg("GET")
        .arg(MIN_REPLICAS_SETTING)
        .query_async(connection)
        .await
        .map_err(|e| {
            Unavailable(format!(
                "this node requires the coordinator to be configured with \
                 {MIN_REPLICAS_SETTING} {wanted}, and asking it could not be done ({e}); \
                 unset OPENCALC_REDIS_MIN_REPLICAS if this coordinator does not allow \
                 CONFIG GET"
            ))
        })?;
    // `CONFIG GET` answers with a flat name/value pair, and an unknown setting
    // answers with an empty list rather than an error — which is a Redis old
    // enough not to have it, and is not a pass.
    let held = setting
        .chunks_exact(2)
        .find(|pair| pair[0] == MIN_REPLICAS_SETTING)
        .and_then(|pair| pair[1].parse::<u32>().ok());
    match held {
        Some(held) if held >= wanted => Ok(()),
        Some(held) => Err(Unavailable(format!(
            "the coordinator is configured with {MIN_REPLICAS_SETTING} {held}, and this node \
             requires at least {wanted}: below that, a failover can lose an append this node \
             already told a client was saved"
        ))),
        None => Err(Unavailable(format!(
            "the coordinator does not report {MIN_REPLICAS_SETTING} at all, so it cannot be \
             the {wanted} this node requires"
        ))),
    }
}

/// The Redis setting that turns an asynchronous-replication loss window into a
/// refusal.
const MIN_REPLICAS_SETTING: &str = "min-replicas-to-write";

/// The error code a primary answers with when its in-sync set has collapsed.
///
/// Established by running it rather than by reading about it: on Redis 8.6 a
/// `SET`, a `ZADD` and a write from inside a Lua script all come back
/// `NOREPLICAS Not enough good replicas to write.` — the script form with the
/// sha appended — and the write does **not** happen. `redis` has no
/// `ErrorKind` for it, so it arrives as an extension error whose `code()` is
/// this string.
const NOREPLICAS: &str = "NOREPLICAS";

/// Say what a coordinator error means, in the terms an operator can act on.
///
/// Only `NOREPLICAS` needs the translation, and it needs it badly: the answer
/// is a **refusal**, not a network failure, and its raw form
/// (`An error was signalled by the server`) sends whoever reads it to look at
/// the network. What actually happened is that the primary is holding ADR-014's
/// durability promise — refusing a write it would not be able to keep — and the
/// operator's next move is to look at the replicas, not at the link.
fn explain(why: &redis::RedisError) -> String {
    if why.code() == Some(NOREPLICAS) {
        return format!(
            "the coordinator refused the write: fewer replicas are in sync than its \
             {MIN_REPLICAS_SETTING} requires, so it is refusing writes rather than accepting \
             ones a failover would lose. The write did not happen. Redis said: {why}"
        );
    }
    why.to_string()
}

/// Read the PEM files a [`LinkTls`] names.
///
/// Read here rather than at first use, so an unreadable certificate stops the
/// process at startup instead of appearing as an unreachable coordinator an hour
/// later.
fn certificates(tls: &LinkTls) -> Result<redis::TlsCertificates, Unavailable> {
    let read = |what: &str, path: &Path| {
        std::fs::read(path).map_err(|e| {
            Unavailable(format!(
                "the coordinator link's {what} at {}: {e}",
                path.display()
            ))
        })
    };
    let client = match &tls.client {
        None => None,
        Some(files) => Some(redis::ClientTlsConfig {
            client_cert: read("certificate", &files.certificate)?,
            client_key: read("private key", &files.key)?,
        }),
    };
    Ok(redis::TlsCertificates {
        client_tls: client,
        root_cert: match &tls.root_ca {
            None => None,
            Some(path) => Some(read("CA certificate", path)?),
        },
    })
}

impl Redis {
    fn lease_key(&self, document: &str) -> String {
        format!("{}:lease:{document}", self.namespace)
    }

    fn log_key(&self, document: &str) -> String {
        format!("{}:log:{document}", self.namespace)
    }

    /// The set of announced nodes, scored by when each was last heard from.
    ///
    /// A sorted set rather than a key per node, so finding the living ones is a
    /// range query. `KEYS`/`SCAN` over a prefix is the obvious alternative and
    /// scales with everything in the database rather than with the cluster.
    fn nodes_key(&self) -> String {
        format!("{}:nodes", self.namespace)
    }

    fn node_key(&self, id: &str) -> String {
        format!("{}:node:{id}", self.namespace)
    }

    /// Where a browser reaches that node.
    ///
    /// A **sibling key** rather than a third element of the body, and that is
    /// the whole design. `peers` reads the body with
    /// `from_str::<(String, u32)>` and `continue`s on an error — so a node
    /// running the previous build, meeting a three-element body, would not fail
    /// loudly; it would silently drop the new node from its peer list and
    /// quietly stop relaying to it. A separate key is invisible to a reader
    /// that does not ask for it, which makes a mixed-version cluster during a
    /// rolling upgrade a non-event.
    fn node_public_key(&self, id: &str) -> String {
        format!("{}:node:{id}:public", self.namespace)
    }
}

impl Coordinator for Redis {
    fn register(
        &self,
        peer: Peer,
        ttl_ms: u64,
        now_ms: u64,
    ) -> Answer<'_, Result<(), Unavailable>> {
        Box::pin(async move {
            let (mut c, at) = self.link.commands().await;
            let body = serde_json::to_string(&(&peer.advertise, peer.load))
                .map_err(|e| Unavailable(e.to_string()))?;
            let mut pipe = redis::pipe();
            pipe.atomic()
                // Scored by *now*, so a stale entry is found by score rather
                // than waited out — expiry on read, as `Memory` does it.
                .zadd(self.nodes_key(), &peer.id, now_ms)
                .set_options(
                    self.node_key(&peer.id),
                    body,
                    redis::SetOptions::default()
                        .with_expiration(redis::SetExpiry::PX(ttl_ms.max(1))),
                );
            // The same TTL as the body, so the two cannot disagree about
            // whether this node is still here.
            match &peer.public_url {
                Some(url) => {
                    pipe.set_options(
                        self.node_public_key(&peer.id),
                        url,
                        redis::SetOptions::default()
                            .with_expiration(redis::SetExpiry::PX(ttl_ms.max(1))),
                    );
                }
                // Deleted rather than left: an operator who *removes*
                // OPENCALC_PUBLIC_URL and restarts is saying "stop sending
                // people here", and a stale key would go on doing it for as
                // long as the node kept announcing.
                None => {
                    pipe.del(self.node_public_key(&peer.id));
                }
            }
            let announced = pipe.query_async::<()>(&mut c).await;
            self.link
                .recover(at, announced)
                .await
                .map_err(|e| Unavailable(explain(&e)))?;
            Ok(())
        })
    }

    fn peers(&self, now_ms: u64) -> Answer<'_, Result<Vec<Peer>, Unavailable>> {
        Box::pin(async move {
            let (mut c, at) = self.link.commands().await;
            let oldest = now_ms.saturating_sub(super::PEER_TTL_MS);
            // One `recover` around the whole read rather than one per command:
            // the three of them are a single question, and a primary that moved
            // between the first and the second has moved for all of them.
            let read = async {
                let ids: Vec<String> = c
                    .zrangebyscore(self.nodes_key(), oldest as isize, "+inf")
                    .await?;
                let mut raw = Vec::with_capacity(ids.len());
                for id in ids {
                    let seen: Option<f64> = c.zscore(self.nodes_key(), &id).await?;
                    let body: Option<String> = c.get(self.node_key(&id)).await?;
                    let public: Option<String> = c.get(self.node_public_key(&id)).await?;
                    raw.push((id, seen, body, public));
                }
                Ok::<_, redis::RedisError>(raw)
            }
            .await;
            let raw = self
                .link
                .recover(at, read)
                .await
                .map_err(|e| Unavailable(explain(&e)))?;

            let mut peers = Vec::with_capacity(raw.len());
            for (id, seen, body, public_url) in raw {
                // A node in the set whose details have expired is one that
                // stopped announcing itself mid-window. Skipped rather than
                // guessed at: an address is the one field that must not be
                // invented.
                let (Some(body), Some(seen)) = (body, seen) else {
                    continue;
                };
                let Ok((advertise, load)) = serde_json::from_str::<(String, u32)>(&body) else {
                    continue;
                };
                peers.push(Peer {
                    id,
                    advertise,
                    public_url: public_url.filter(|u| !u.is_empty()),
                    load,
                    seen_ms: seen as u64,
                });
            }
            Ok(peers)
        })
    }

    fn claim(
        &self,
        document: String,
        node: String,
        ttl_ms: u64,
        now_ms: u64,
    ) -> Answer<'_, Result<Lease, Unavailable>> {
        Box::pin(async move {
            let (mut c, at) = self.link.commands().await;
            let taken = self
                .claim
                .key(self.lease_key(&document))
                .arg(&node)
                .arg(ttl_ms)
                .arg(now_ms)
                .invoke_async::<(String, String, String)>(&mut c)
                .await;
            let answer = self
                .link
                .recover(at, taken)
                .await
                .map_err(|e| Unavailable(explain(&e)))?;
            // Numbers come back as strings on purpose: Lua's only number is a
            // double, and a revision or an epoch that has passed through one
            // has quietly lost precision above 2^53.
            let parse = |s: &str| s.parse::<u64>().map_err(|e| Unavailable(e.to_string()));
            Ok(Lease {
                document,
                node: answer.0,
                epoch: parse(&answer.1)?,
                expires_ms: parse(&answer.2)?,
            })
        })
    }

    fn append(
        &self,
        document: String,
        epoch: u64,
        after: u64,
        revision: u64,
        payload: Vec<u8>,
        now_ms: u64,
    ) -> Answer<'_, Result<u64, AppendError>> {
        Box::pin(async move {
            let (mut c, at) = self.link.commands().await;
            let written = self
                .append
                .key(self.lease_key(&document))
                .key(self.log_key(&document))
                .arg(epoch)
                .arg(after)
                .arg(revision)
                .arg(payload)
                .arg(now_ms)
                .arg(LOG_MAX_ENTRIES)
                .arg(LOG_TTL_MS)
                .invoke_async::<(String, String)>(&mut c)
                .await;
            // **The in-sync set collapsing arrives here**, as `NOREPLICAS`, and
            // it is the one refusal this file has to name itself: the script
            // never reached its `RPUSH`, so nothing was written and nothing was
            // lost — but the raw error reads as a network fault and would send
            // an operator to look at the link rather than at the replicas.
            // `AppendError::Unavailable` is the right existing home for it: the
            // caller must not treat it as "it did not land" and must not retry
            // blindly, which is exactly the contract that variant carries, and
            // `order()` already turns any `Err` here into `Refused { NotSaving }`.
            let answer = self
                .link
                .recover(at, written)
                .await
                .map_err(|e| AppendError::Unavailable(explain(&e)))?;
            let detail = answer
                .1
                .parse::<u64>()
                .map_err(|e| AppendError::Unavailable(e.to_string()))?;
            match answer.0.as_str() {
                "ok" => Ok(detail),
                "fenced" => Err(AppendError::Fenced { current: detail }),
                "stale" => Err(AppendError::Stale { current: detail }),
                "unled" => Err(AppendError::Unled),
                other => Err(AppendError::Unavailable(format!(
                    "the append script answered {other:?}, which this does not know how to read"
                ))),
            }
        })
    }

    fn since(
        &self,
        document: String,
        revision: u64,
    ) -> Answer<'_, Result<Vec<Logged>, Unavailable>> {
        Box::pin(async move {
            let (mut c, at) = self.link.commands().await;
            // The whole log, then filtered on each entry's own revision.
            // Slicing by index is what this used to do and it was only ever
            // right while a revision and an entry were the same thing; an entry
            // carrying three operations advances the revision by three, so its
            // position stops predicting its revision at the first real chunk.
            // The log is compacted, which is what keeps this bounded.
            let read = c
                .lrange::<_, Vec<Vec<u8>>>(self.log_key(&document), 0, -1)
                .await;
            let entries = self
                .link
                .recover(at, read)
                .await
                .map_err(|e| Unavailable(explain(&e)))?;
            Ok(entries
                .into_iter()
                .filter_map(|entry| {
                    let cut = entry.iter().position(|b| *b == b'\n')?;
                    let at: u64 = std::str::from_utf8(&entry[..cut]).ok()?.parse().ok()?;
                    (at > revision).then(|| (at, entry[cut + 1..].to_vec()))
                })
                .collect())
        })
    }
}
