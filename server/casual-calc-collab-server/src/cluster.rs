//! The cluster: who leads a document, and how a stale leader is stopped.
//!
//! [ADR-012](../../../docs/57-COLLABORATION-SERVER-BOUNDARY.md) describes the
//! shape — interchangeable nodes, one leading a given document, the rest
//! relaying to it — and [ADR-014](../../../docs/59-COLLABORATION-SERVICE-STACK.md)
//! chose Redis to coordinate it. This module is the part that has to be right,
//! and it is deliberately **not** the part that talks to Redis.
//!
//! # Why the logic is separate from the store
//!
//! Leadership bugs are timing bugs: a lease that expires under load, two nodes
//! that both believe they lead, an append from a leader that has already been
//! replaced. Those are the failures that appear once a quarter, in production,
//! at three in the morning — and they are only testable if time and the store
//! are both arguments. So [`Coordinator`] is a trait, [`Memory`] implements it
//! for tests and for standalone, and every rule below is exercised against a
//! clock the test controls.
//!
//! # The two mechanisms, and which does what
//!
//! **A lease** is liveness. It says a node holds a document *for now*, expires
//! on its own, and is cheap. It is not correctness: under load a lease can
//! expire while its holder is perfectly alive and still working, and there is
//! no way to prevent that without making it expensive.
//!
//! **An epoch** is correctness. Every time a lease is taken afresh the epoch
//! increases, and an append carries the epoch it was made under. A leader whose
//! lease expired wrongly — a *zombie* — still thinks it leads, and its appends
//! are refused because they name an epoch that has been superseded. It finds
//! out by being told, which is the only way it can.
//!
//! That division is what makes the cheap thing safe. Without the epoch, the
//! lease would have to be correct, and a lease that must never expire wrongly
//! is a consensus protocol.
//!
//! # Who decides the leader is down: nobody
//!
//! There is no failure detector here, and that is the point. No replica watches
//! the leader, no node forms an opinion about another's liveness, and nothing
//! votes.
//!
//! A leader proves it is alive by **renewing its own lease**. If it stops — dead,
//! partitioned, paused by a long garbage collection, or merely slow — the lease
//! lapses on the store's clock, without anybody having judged it. Any node that
//! wants the document calls [`Coordinator::claim`] periodically; while the
//! lease is held it is told who holds it and relays there, and the moment the
//! lease has lapsed the same call takes it over. The changeover is a
//! consequence of an atomic operation, not of a decision.
//!
//! Heartbeat-based detection — a replica noticing silence and declaring the
//! leader dead — is the obvious design and the wrong one. It needs the replicas
//! to *agree* that the leader is down, which is the consensus problem wearing a
//! disguise: under a partition each side sees the other's silence, each
//! concludes the other is gone, and both promote. Liveness cannot be observed
//! remotely, only inferred, and two nodes can infer differently from the same
//! silence.
//!
//! The lease sidesteps it by never asking the question. The only signal is the
//! absence of a renewal, and it is evaluated in one place, atomically, by the
//! store. Two nodes claiming at the same instant do not race: one call succeeds
//! and the other is told who won.
//!
//! Which leaves exactly one hole, and it is the one the epoch fills: a leader
//! that was alive all along and lost its lease to a slow moment. It still
//! believes it leads. It is wrong, it cannot be told in time, and its next
//! append is refused by an epoch that has moved past it.
//!
//! ## Why not gossip
//!
//! The obvious modern answer to "how do nodes learn one is gone" is a
//! SWIM-style gossip membership protocol, and it is the right tool for a
//! question this module does not ask.
//!
//! Gossip is a **weakly consistent failure detector**. That is its design point
//! rather than a shortcoming: it disseminates a view of who is alive cheaply and
//! at scale, and it explicitly tolerates two nodes holding different views at
//! the same moment. What leadership needs here is **mutual exclusion** — one
//! writer per document, because the transform requires a total order — and a
//! protocol that permits divergent views cannot provide it. Under a partition
//! each side sees the other's silence, each concludes the other is gone, and
//! both promote. That is the same objection as replica heartbeats, in a stronger
//! form, because gossip is *designed* to allow it.
//!
//! The distinction worth keeping: gossip gives membership, consensus (Raft,
//! Paxos) gives agreement, and a lease against a linearizable store gives mutual
//! exclusion. This needs the third. A deployment that genuinely wanted no Redis
//! would want Raft, or a Raft-backed store; gossip alone would not be enough.
//!
//! Where it *would* earn its place, and might later: **discovery** — [`elect`]
//! and [`Coordinator::peers`] are advisory, and getting them wrong costs an
//! extra contended claim rather than correctness — and as a **hint** to try
//! claiming early rather than waiting out a lease, which is safe precisely
//! because the claim is still refused while the lease is live. Both are
//! complements. Neither replaces the fence.
//!
//! Against that, two concrete costs here. Gossip needs node-to-node
//! connectivity, which
//! [ADR-017](../../../docs/63-COLLABORATION-RELAY.md) rejected for the relay
//! because nodes are pods behind a service and are not individually addressable
//! without extra machinery. And the marginal cost of Redis-backed discovery is
//! close to nothing, because Redis is **already in the write path** — an
//! operation is appended to the log before it is acknowledged — so gossip would
//! add a second membership mechanism rather than remove a dependency. The fair
//! counterpoint is that Redis then needs its own availability story; true, and
//! unchanged by gossip, since the log needs it regardless.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Mutex;

/// A node, as its peers see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    /// Its stable id.
    pub id: String,
    /// Where to reach it — the **internal** address, not the public one.
    pub advertise: String,
    /// How loaded it is, for election. Lower leads.
    pub load: u32,
    /// When it last said it was alive.
    pub seen_ms: u64,
}

/// A node's claim on leading one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    /// The document session key.
    pub document: String,
    /// Who holds it.
    pub node: String,
    /// Which generation of leadership this is.
    ///
    /// The fence. It increases whenever leadership *changes hands*, never on a
    /// renewal, so a holder keeps its epoch for as long as it keeps the lease.
    pub epoch: u64,
    /// When it lapses unless renewed.
    pub expires_ms: u64,
}

/// The coordination store could not be reached.
///
/// A separate outcome from every "no" the protocol has, and the distinction is
/// load-bearing. "Somebody else leads" is an answer; "I could not ask" is not,
/// and a caller that treats them alike will carry on as though it had been
/// refused — or worse, as though it had been granted. A node that cannot reach
/// the store **does not know** whether it leads, and the only safe thing it can
/// do with that is stop, which it cannot decide to do if the failure is dressed
/// up as an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unavailable(pub String);

impl core::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "the coordination store is unreachable: {}", self.0)
    }
}

impl core::error::Error for Unavailable {}

/// A future returned by a [`Coordinator`] method.
///
/// Boxed rather than `async fn` in the trait, because the service holds an
/// `Arc<dyn Coordinator>` — the whole point being that standalone uses
/// [`Memory`] and a cluster uses Redis, chosen at startup from configuration.
type Answer<'a, T> = core::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One entry of a document's log: the revision it is at, and its bytes.
///
/// Named because the pair appears in a return type nested three deep, where
/// `(u64, Vec<u8>)` reads as neither.
pub type Logged = (u64, Vec<u8>);

/// Why an append was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendError {
    /// The appender's epoch has been superseded — it is a zombie leader.
    ///
    /// Carries the epoch that is current, so the caller learns it has been
    /// replaced rather than merely that it failed.
    Fenced {
        /// The epoch that now leads.
        current: u64,
    },
    /// The document has moved on since the revision this was written against.
    ///
    /// Not a leadership problem: the leader is right and simply behind, which
    /// happens when its own view has not caught up with the log.
    Stale {
        /// The revision the log is actually at.
        current: u64,
    },
    /// Nobody holds a lease on this document.
    Unled,
    /// The store could not be reached, so nothing is known.
    ///
    /// Emphatically not a refusal. An append that was refused did not happen;
    /// an append that could not be attempted **may have happened** — the request
    /// can fail after the store applied it — so a caller must not treat this as
    /// "it did not land" and must not retry blindly either. The sequence number
    /// is what makes the retry safe, which is why it exists.
    Unavailable(String),
}

/// The store a cluster coordinates through.
///
/// Every method takes the time rather than reading it, for the reason the rest
/// of this crate does: these are the operations whose bugs live in rare timing.
pub trait Coordinator: Send + Sync {
    /// Announce this node, or refresh its announcement.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] if the store cannot be reached.
    fn register(&self, peer: Peer, ttl_ms: u64, now_ms: u64)
    -> Answer<'_, Result<(), Unavailable>>;

    /// Every node that has announced itself recently enough.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] if the store cannot be reached. Deliberately not an
    /// empty list: "no peers" and "I cannot see the peers" are opposite
    /// situations, and a node that confuses them concludes it is alone and
    /// takes over everything.
    fn peers(&self, now_ms: u64) -> Answer<'_, Result<Vec<Peer>, Unavailable>>;

    /// Take or renew leadership of `document`.
    ///
    /// Returns the lease whoever holds it now has — which may be somebody
    /// else's, and a caller that assumes otherwise is the bug this signature
    /// exists to prevent.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] if the store cannot be reached, which is **not** the
    /// same as being refused: a node that could not ask does not know whether
    /// it leads, and must not act as though it does.
    fn claim(
        &self,
        document: String,
        node: String,
        ttl_ms: u64,
        now_ms: u64,
    ) -> Answer<'_, Result<Lease, Unavailable>>;

    /// Append to a document's log, fenced by `epoch` and conditional on
    /// `after`, carrying the document to `revision`.
    ///
    /// **`revision` is not derivable from the log's length**, which is the
    /// mistake this signature exists to prevent. A revision counts
    /// *operations* — `ServerSession::commit` advances it once per op — while
    /// the log holds one entry per *append*, because an append is the unit that
    /// gets published and adopted. The two agree only while every chunk carries
    /// exactly one operation, which is true of nothing except a test: the
    /// editor batches everything typed inside a flush window into one chunk.
    ///
    /// When the length was the revision, a two-operation chunk asked an empty
    /// log to accept `after = 1`, was told `Stale`, and the caller returned —
    /// after `commit` had already applied both operations to the leader's own
    /// copy. That node was then diverged from the log permanently: its client
    /// was never acknowledged, no peer ever saw the edit, and every later
    /// append from it failed for the same reason.
    ///
    /// So the caller passes both ends: `after` is where it believed the
    /// document was, and `revision` is where these operations leave it.
    ///
    /// # The fence is an equality, not an ordering
    ///
    /// `epoch` must be **the** epoch the store currently records for this
    /// document. An older one is a zombie leader and is refused; a *newer* one
    /// is refused too, and that second half is not paranoia.
    ///
    /// Epochs only rise while one store keeps its memory, so an appender ahead
    /// of the store looks impossible — and it is what a coordinator failover
    /// produces. Replication to a Redis replica is asynchronous, so a promoted
    /// replica can be missing the last writes the old primary accepted,
    /// including the claim that raised the epoch. The store then remembers an
    /// older generation than the leader is carrying.
    ///
    /// While this was `epoch < current`, that leader was believed — and so was
    /// whoever the rewound store thinks holds the lease, since their lower epoch
    /// is not less than itself either. Two live leaders, each of which commits
    /// into its own copy of the document *before* appending, and the one that
    /// loses the conditional append is diverged from the log permanently, with
    /// no resync built to recover it.
    ///
    /// A lease this store never issued is not a lease. Refusing it turns a
    /// silent divergence into `Fenced`, which DEP-04 already reports to the
    /// client as `NotSaving`.
    ///
    /// This does **not** make a failover safe — see
    /// [77](../../../docs/77-COORDINATOR-AVAILABILITY.md), which is the open
    /// design question. It makes the unsafe case visible instead of silent.
    ///
    /// # Errors
    ///
    /// [`AppendError`] when the epoch has been superseded, the revision has
    /// moved on, nobody leads the document, or the store is unreachable.
    fn append(
        &self,
        document: String,
        epoch: u64,
        after: u64,
        revision: u64,
        payload: Vec<u8>,
        now_ms: u64,
    ) -> Answer<'_, Result<u64, AppendError>>;

    /// Everything logged for `document` after `revision`.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] if the store cannot be reached.
    fn since(
        &self,
        document: String,
        revision: u64,
    ) -> Answer<'_, Result<Vec<Logged>, Unavailable>>;
}

/// A coordinator in this process's memory.
///
/// Two uses, and the second is not a lesser one. It backs the **tests** for
/// every rule in this module, which is what lets leadership be exercised
/// against a clock rather than a stopwatch. And it backs **standalone**, where
/// one node leads every document by definition and a network round-trip to
/// agree with itself would be pure cost.
#[derive(Debug, Default)]
pub struct Memory {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    peers: BTreeMap<String, Peer>,
    leases: BTreeMap<String, Lease>,
    /// Each entry is `(revision it carried the document to, payload)`. The
    /// revision is stored rather than derived from the position, because one
    /// entry can hold several operations — see [`Coordinator::append`].
    logs: BTreeMap<String, Vec<(u64, Vec<u8>)>>,
}

impl Coordinator for Memory {
    fn register(
        &self,
        peer: Peer,
        ttl_ms: u64,
        now_ms: u64,
    ) -> Answer<'_, Result<(), Unavailable>> {
        let _ = ttl_ms;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.peers.insert(
            peer.id.clone(),
            Peer {
                seen_ms: now_ms,
                ..peer
            },
        );
        Box::pin(async { Ok(()) })
    }

    fn peers(&self, now_ms: u64) -> Answer<'_, Result<Vec<Peer>, Unavailable>> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        // A node that stopped announcing itself is gone. Expiry on read rather
        // than on a timer: there is no moment at which a peer needs to have
        // been forgotten except the moment somebody asks.
        let found: Vec<Peer> = state
            .peers
            .values()
            .filter(|p| now_ms.saturating_sub(p.seen_ms) < PEER_TTL_MS)
            .cloned()
            .collect();
        Box::pin(async move { Ok(found) })
    }

    fn claim(
        &self,
        document: String,
        node: String,
        ttl_ms: u64,
        now_ms: u64,
    ) -> Answer<'_, Result<Lease, Unavailable>> {
        let (document, node) = (document.as_str(), node.as_str());
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let existing = state.leases.get(document).cloned();
        let lease = match existing {
            // Held by somebody else and still live: they keep it. Returning
            // theirs rather than an error is deliberate — the caller needs to
            // know *who* leads in order to relay to them.
            Some(held) if held.node != node && held.expires_ms > now_ms => held,
            // Ours: renew without touching the epoch. A renewal is not a change
            // of leadership, and bumping the epoch here would fence the holder
            // against itself.
            Some(held) if held.node == node && held.expires_ms > now_ms => Lease {
                expires_ms: now_ms.saturating_add(ttl_ms),
                ..held
            },
            // Lapsed, or never held. Taking it is a change of leadership, so
            // the epoch moves — which is what fences whoever had it before,
            // including a holder that is still alive and merely slow.
            other => Lease {
                document: document.to_owned(),
                node: node.to_owned(),
                epoch: other.map_or(1, |held| held.epoch.saturating_add(1)),
                expires_ms: now_ms.saturating_add(ttl_ms),
            },
        };
        state.leases.insert(document.to_owned(), lease.clone());
        Box::pin(async move { Ok(lease) })
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
        let _ = now_ms;
        let document = document.as_str();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(lease) = state.leases.get(document).cloned() else {
            return Box::pin(async { Err(AppendError::Unled) });
        };
        // The fence, and it is checked **before** the revision: a zombie whose
        // revision happens to line up must still be refused, and telling it
        // "stale" would send it to re-read the log and try again forever.
        //
        // `!=` rather than `<`, which is the failover case — see the trait.
        if epoch != lease.epoch {
            let current = lease.epoch;
            return Box::pin(async move { Err(AppendError::Fenced { current }) });
        }
        let log = state.logs.entry(document.to_owned()).or_default();
        // The last entry's revision, not the entry count. See the trait.
        let current = log.last().map_or(0, |(at, _)| *at);
        if after != current {
            return Box::pin(async move { Err(AppendError::Stale { current }) });
        }
        log.push((revision, payload));
        // The same window Redis trims to, so the two coordinators answer the
        // same question the same way. They are held to that by `contract!`, and
        // this is what it caught: bounding one of them left the in-memory
        // implementation growing without limit, which is the behaviour every
        // test that does not set OPENCALC_TEST_REDIS would have measured.
        let max = crate::cluster::redis::LOG_MAX_ENTRIES as usize;
        if log.len() > max {
            log.drain(..log.len() - max);
        }
        Box::pin(async move { Ok(revision) })
    }

    fn since(
        &self,
        document: String,
        revision: u64,
    ) -> Answer<'_, Result<Vec<Logged>, Unavailable>> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let found = state
            .logs
            .get(document.as_str())
            .map(|log| {
                // Filtered by the revision each entry recorded, rather than
                // skipping that many entries: one entry can carry several
                // operations, so its position says nothing about its revision.
                log.iter()
                    .filter(|(at, _)| *at > revision)
                    .map(|(at, payload)| (*at, payload.clone()))
                    .collect()
            })
            .unwrap_or_default();
        Box::pin(async move { Ok(found) })
    }
}

/// How long a node may go unannounced before its peers forget it.
const PEER_TTL_MS: u64 = 15_000;

/// Who should lead, given who is available.
///
/// Least loaded first, and **the id breaks a tie** rather than the iteration
/// order. Two nodes electing from the same peer list must reach the same
/// answer, or they both take the lease and the epoch fence has to clean up
/// after an avoidable race every time.
#[must_use]
pub fn elect(peers: &[Peer]) -> Option<&Peer> {
    peers
        .iter()
        .min_by(|a, b| a.load.cmp(&b.load).then_with(|| a.id.cmp(&b.id)))
}

pub mod redis;

#[cfg(test)]
mod tests;
