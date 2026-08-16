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

use redis::AsyncCommands;
use redis::aio::MultiplexedConnection;
use tokio::sync::Mutex;

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
const APPEND: &str = r"
local raw = redis.call('GET', KEYS[1])
if not raw then return {'unled', '0'} end
local held = cjson.decode(raw)
local epoch, after, revision = tonumber(ARGV[1]), tonumber(ARGV[2]), tonumber(ARGV[3])
if epoch < held.epoch then return {'fenced', tostring(held.epoch)} end
local last = redis.call('LINDEX', KEYS[2], -1)
local current = 0
if last then current = tonumber(string.match(last, '^(%d+)')) end
if after ~= current then return {'stale', tostring(current)} end
redis.call('RPUSH', KEYS[2], revision .. '\n' .. ARGV[4])
return {'ok', tostring(revision)}
";

/// A cluster's shared state, in Redis.
pub struct Redis {
    connection: Mutex<MultiplexedConnection>,
    /// Kept because a **subscription needs its own connection**: a Redis
    /// connection in subscribe mode accepts almost nothing else, so sharing the
    /// multiplexed one would take the coordinator offline the moment anything
    /// subscribed.
    client: redis::Client,
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
    /// # Errors
    ///
    /// [`Unavailable`] if the URL is unusable or the server cannot be reached.
    pub async fn connect_within(url: &str, namespace: &str) -> Result<Self, Unavailable> {
        let client = redis::Client::open(url).map_err(|e| Unavailable(e.to_string()))?;
        let connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| Unavailable(e.to_string()))?;
        Ok(Self {
            connection: Mutex::new(connection),
            client,
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
        let mut c = self.connection().await;
        let _: () = c
            .publish(channel, payload)
            .await
            .map_err(|e| Unavailable(e.to_string()))?;
        Ok(())
    }

    /// Subscribe to `channel`, receiving payloads until the receiver is dropped.
    ///
    /// The subscription runs on its own task with its own connection, and stops
    /// when the returned receiver is dropped — which is what makes a document
    /// being evicted also close its subscription, rather than leaving a task
    /// per document this node has ever held.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] if the connection cannot be opened or the subscription
    /// refused.
    pub async fn subscribe(
        &self,
        channel: &str,
    ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, Unavailable> {
        let mut pubsub = self
            .client
            .get_async_pubsub()
            .await
            .map_err(|e| Unavailable(e.to_string()))?;
        pubsub
            .subscribe(channel)
            .await
            .map_err(|e| Unavailable(e.to_string()))?;

        // Bounded. A subscriber that stops reading must not let the channel
        // grow without limit — and dropping the oldest would be worse than
        // stopping, since a missed batch is a gap the receiver detects and
        // closes from the log, whereas unbounded growth is the node dying.
        let (out, into) = tokio::sync::mpsc::channel(256);
        tokio::spawn(async move {
            use futures_util::StreamExt as _;
            let mut stream = pubsub.on_message();
            while let Some(message) = stream.next().await {
                let Ok(payload) = message.get_payload::<Vec<u8>>() else {
                    continue;
                };
                if out.send(payload).await.is_err() {
                    // The receiver is gone: the document was evicted, or the
                    // node is shutting down. Either way there is nobody to
                    // deliver to and the connection should be released.
                    break;
                }
            }
        });
        Ok(into)
    }

    async fn connection(&self) -> MultiplexedConnection {
        // Cloned rather than held: a multiplexed connection is designed to be
        // used from many places at once, and holding the lock across the round
        // trip would serialise every node's coordination behind one command.
        self.connection.lock().await.clone()
    }
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
}

impl Coordinator for Redis {
    fn register(
        &self,
        peer: Peer,
        ttl_ms: u64,
        now_ms: u64,
    ) -> Answer<'_, Result<(), Unavailable>> {
        Box::pin(async move {
            let mut c = self.connection().await;
            let body = serde_json::to_string(&(&peer.advertise, peer.load))
                .map_err(|e| Unavailable(e.to_string()))?;
            redis::pipe()
                .atomic()
                // Scored by *now*, so a stale entry is found by score rather
                // than waited out — expiry on read, as `Memory` does it.
                .zadd(self.nodes_key(), &peer.id, now_ms)
                .set_options(
                    self.node_key(&peer.id),
                    body,
                    redis::SetOptions::default()
                        .with_expiration(redis::SetExpiry::PX(ttl_ms.max(1))),
                )
                .query_async::<()>(&mut c)
                .await
                .map_err(|e| Unavailable(e.to_string()))?;
            Ok(())
        })
    }

    fn peers(&self, now_ms: u64) -> Answer<'_, Result<Vec<Peer>, Unavailable>> {
        Box::pin(async move {
            let mut c = self.connection().await;
            let oldest = now_ms.saturating_sub(super::PEER_TTL_MS);
            let ids: Vec<String> = c
                .zrangebyscore(self.nodes_key(), oldest as isize, "+inf")
                .await
                .map_err(|e| Unavailable(e.to_string()))?;

            let mut peers = Vec::with_capacity(ids.len());
            for id in ids {
                let seen: Option<f64> = c
                    .zscore(self.nodes_key(), &id)
                    .await
                    .map_err(|e| Unavailable(e.to_string()))?;
                let body: Option<String> = c
                    .get(self.node_key(&id))
                    .await
                    .map_err(|e| Unavailable(e.to_string()))?;
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
            let mut c = self.connection().await;
            let answer: (String, String, String) = self
                .claim
                .key(self.lease_key(&document))
                .arg(&node)
                .arg(ttl_ms)
                .arg(now_ms)
                .invoke_async(&mut c)
                .await
                .map_err(|e| Unavailable(e.to_string()))?;
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
            let mut c = self.connection().await;
            let answer: (String, String) = self
                .append
                .key(self.lease_key(&document))
                .key(self.log_key(&document))
                .arg(epoch)
                .arg(after)
                .arg(revision)
                .arg(payload)
                .arg(now_ms)
                .invoke_async(&mut c)
                .await
                .map_err(|e| AppendError::Unavailable(e.to_string()))?;
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
            let mut c = self.connection().await;
            // The whole log, then filtered on each entry's own revision.
            // Slicing by index is what this used to do and it was only ever
            // right while a revision and an entry were the same thing; an entry
            // carrying three operations advances the revision by three, so its
            // position stops predicting its revision at the first real chunk.
            // The log is compacted, which is what keeps this bounded.
            let entries: Vec<Vec<u8>> = c
                .lrange(self.log_key(&document), 0, -1)
                .await
                .map_err(|e| Unavailable(e.to_string()))?;
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
