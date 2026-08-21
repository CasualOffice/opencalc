//! Cluster tests: the failures that appear once a quarter, in production, at
//! three in the morning — made to happen on demand.
//!
//! Every one of these is a timing bug in disguise, which is why time is an
//! argument throughout. A test that has to sleep to reach a lease expiry is a
//! test that is slow *and* flaky; these reach it by saying so.
//!
//! # One suite, both backends
//!
//! Each rule is written once, against `&dyn Coordinator`, and run against
//! [`Memory`] and against **Redis**. That is the entire point of the trait: the
//! rules are what must not differ, and a second implementation tested by a
//! second set of tests is two implementations of two specifications that happen
//! to share a name.
//!
//! The Redis half is skipped when there is no server to talk to, and says so.
//! A skipped test is a hole and pretending otherwise is worse than the hole —
//! but a suite that cannot run without infrastructure is a suite nobody runs.
//! CI provides one, so there the hole is closed.

use super::*;

const TTL: u64 = 1_000;

/// A lease for a test that does enough work to outrun the ordinary one.
///
/// Ten minutes: far past anything a loaded runner can take for ten thousand
/// round trips, and still an expiry rather than none, so a test that leaks a
/// lease does not leave one behind for ever.
const SLOW_WORK_TTL: u64 = 600_000;

/// Run one rule against both backends.
///
/// The Redis run gets a **fresh key prefix per test** by way of the document
/// name, because a real database persists between runs and a test that passes
/// only on an empty one is a test that fails on the second try.
macro_rules! contract {
    ($name:ident, |$c:ident| $body:block) => {
        mod $name {
            use super::*;

            async fn rule($c: &dyn Coordinator) $body

            #[tokio::test]
            async fn in_memory() {
                rule(&Memory::default()).await;
            }

            #[tokio::test]
            async fn in_redis() {
                let Some(store) = redis_store(stringify!($name)).await else {
                    eprintln!(
                        "skipped {}: set OPENCALC_TEST_REDIS to a reachable server to run it",
                        stringify!($name)
                    );
                    return;
                };
                rule(&store).await;
            }
        }
    };
}

/// A Redis to test against, in a namespace nothing else uses, or nothing.
///
/// A fresh namespace per test is what makes these runnable against a **real**
/// database: it persists between runs, and a suite that only passes on an empty
/// one fails on the second try and passes again after somebody flushes it,
/// which is the worst possible signal.
///
/// The process id and a counter go in the name rather than a random number, so
/// a failure can be looked up in the database afterwards.
async fn redis_store(name: &str) -> Option<crate::cluster::redis::Redis> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    // Unset means a developer without a server, and skipping is the whole
    // reason the suite is runnable at all.
    let url = std::env::var("OPENCALC_TEST_REDIS").ok()?;
    let namespace = format!(
        "opencalc-test:{}:{}:{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    // **Set and unreachable is a failure, not a skip.** It used to be `.ok()`,
    // which meant CI — where the variable is always set, pointing at the
    // service container — reported a green Redis half whenever the container
    // was missing, misconfigured or not yet accepting connections. That is
    // precisely the "a suite that always skips half of itself reports on half
    // of itself while looking green" failure the service block in ci.yml was
    // added to prevent, reintroduced one `.ok()` below it.
    Some(
        crate::cluster::redis::Redis::connect_within(&url, &namespace)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "OPENCALC_TEST_REDIS is set to {url:?} and could not be reached: {e:?}. \
                     Unset it to skip the Redis half; leaving it set and broken silently \
                     halves this suite."
                )
            }),
    )
}

/// Shorthands, so a rule reads as the rule rather than as plumbing.
async fn claim(c: &dyn Coordinator, document: &str, node: &str, now: u64) -> Lease {
    c.claim(document.to_owned(), node.to_owned(), TTL, now)
        .await
        .expect("the store answered")
}

async fn append(
    c: &dyn Coordinator,
    document: &str,
    epoch: u64,
    after: u64,
    payload: &[u8],
    now: u64,
) -> Result<u64, AppendError> {
    // One operation per append, which is what these fencing and staleness
    // tests are about — the multi-operation case has its own test below,
    // because assuming it away here is exactly how it went unnoticed.
    c.append(
        document.to_owned(),
        epoch,
        after,
        after + 1,
        payload.to_vec(),
        now,
    )
    .await
}

/// Append `ops` operations at once, the way a real chunk does.
async fn append_ops(
    c: &dyn Coordinator,
    document: &str,
    epoch: u64,
    after: u64,
    ops: u64,
    payload: &[u8],
    now: u64,
) -> Result<u64, AppendError> {
    c.append(
        document.to_owned(),
        epoch,
        after,
        after + ops,
        payload.to_vec(),
        now,
    )
    .await
}

async fn since(c: &dyn Coordinator, document: &str, revision: u64) -> Vec<Logged> {
    c.since(document.to_owned(), revision)
        .await
        .expect("the store answered")
}

async fn peers(c: &dyn Coordinator, now: u64) -> Vec<Peer> {
    c.peers(now).await.expect("the store answered")
}

async fn register(c: &dyn Coordinator, p: Peer, now: u64) {
    c.register(p, 30_000, now)
        .await
        .expect("the store answered");
}

fn peer(id: &str, load: u32) -> Peer {
    Peer {
        id: id.to_owned(),
        advertise: format!("10.0.0.1:944{}", id.len()),
        // Reachable by a browser. `public` below is the one without.
        public_url: Some(format!("wss://{id}.example/collab")),
        load,
        seen_ms: 0,
    }
}

/// A peer the operator gave no public address, so no client can be sent to it.
fn unreachable(id: &str, load: u32) -> Peer {
    Peer {
        public_url: None,
        ..peer(id, load)
    }
}

// --- Leadership -------------------------------------------------------------

contract!(the_first_claimant_leads, |c| {
    let lease = claim(c, "doc", "node-a", 0).await;
    assert_eq!(lease.node, "node-a");
    assert_eq!(lease.epoch, 1);
    assert_eq!(lease.expires_ms, TTL);
});

contract!(a_second_node_is_told_who_leads_rather_than_taking_it, |c| {
    // Returning the *holder's* lease rather than an error is what lets the
    // caller relay: it needs to know who leads, not merely that it does not.
    claim(c, "doc", "node-a", 0).await;
    let lease = claim(c, "doc", "node-b", 100).await;
    assert_eq!(lease.node, "node-a", "leadership did not move");
    assert_eq!(lease.epoch, 1, "and the epoch did not move either");
});

contract!(renewing_does_not_move_the_epoch, |c| {
    // A renewal is not a change of leadership. Bumping the epoch here would
    // fence the holder against its own appends, which is the subtlest way to
    // break this: everything looks alive and nothing can be written.
    let first = claim(c, "doc", "node-a", 0).await;
    let renewed = claim(c, "doc", "node-a", 500).await;
    assert_eq!(renewed.epoch, first.epoch);
    assert_eq!(renewed.expires_ms, 1_500, "but the expiry moves");
});

contract!(a_lapsed_lease_is_taken_by_the_next_claimant, |c| {
    claim(c, "doc", "node-a", 0).await;
    let taken = claim(c, "doc", "node-b", TTL + 1).await;
    assert_eq!(taken.node, "node-b");
    assert_eq!(
        taken.epoch, 2,
        "and the epoch moves, which fences the old one"
    );
});

contract!(leadership_of_one_document_says_nothing_about_another, |c| {
    claim(c, "doc-1", "node-a", 0).await;
    let other = claim(c, "doc-2", "node-b", 0).await;
    assert_eq!(other.node, "node-b");
    assert_eq!(other.epoch, 1, "each document has its own generation");
});

// --- Fencing: the reason the cheap lease is safe ----------------------------

contract!(a_zombie_leaders_appends_are_refused, |c| {
    // The failure this whole design is arranged around. Node A holds the lease
    // and is alive and working; its lease expires anyway because the node was
    // busy, or the network hiccuped, or the clock moved. Node B takes over. A
    // does not know, and keeps writing.
    let a = claim(c, "doc", "node-a", 0).await;
    let b = claim(c, "doc", "node-b", TTL + 1).await;
    assert_eq!(b.epoch, a.epoch + 1);

    assert_eq!(
        append(c, "doc", a.epoch, 0, b"from the zombie", 0).await,
        Err(AppendError::Fenced { current: b.epoch }),
        "the old leader must be refused, and told which epoch replaced it"
    );
    assert!(
        append(c, "doc", b.epoch, 0, b"from the new leader", 0)
            .await
            .is_ok(),
        "while the new one proceeds"
    );
});

contract!(the_fence_is_checked_before_the_revision, |c| {
    // A zombie whose revision happens to line up must still be refused. Telling
    // it `Stale` would send it to re-read the log and try again — forever, and
    // politely.
    let a = claim(c, "doc", "node-a", 0).await;
    claim(c, "doc", "node-b", TTL + 1).await;
    assert!(matches!(
        append(c, "doc", a.epoch, 0, b"x", 0).await,
        Err(AppendError::Fenced { .. })
    ));
});

contract!(an_append_to_a_document_nobody_leads_is_refused, |c| {
    assert_eq!(
        append(c, "doc", 1, 0, b"x", 0).await,
        Err(AppendError::Unled)
    );
});

contract!(an_append_from_ahead_of_the_stores_epoch_is_refused, |c| {
    // **The fence has two sides, and only one of them used to bite.**
    //
    // Epochs only ever rise while one store keeps its memory, so an appender
    // whose epoch is *higher* than the store's looks impossible — and it is
    // exactly what a coordinator failover produces. Replication to a Redis
    // replica is asynchronous, so a promoted replica can be missing the last
    // writes the old master accepted, including the lease that raised the epoch.
    // The store then remembers an older generation than the leader is carrying.
    //
    // While the test was `epoch < held.epoch`, that leader was **believed**. So
    // was whoever the rewound store thinks holds the lease, because their lower
    // epoch is not less than itself either — two live leaders, each committing
    // into its own copy before appending, and the one that loses the revision
    // CAS is diverged from the log permanently with no resync to recover it.
    //
    // A lease this store never issued is not a lease. `!=` rather than `<`, so
    // an epoch from a store that has forgotten it is refused in the same breath
    // as one that has been superseded — and DEP-04 tells the client `NotSaving`
    // instead of silently building on a rewound log.
    let held = claim(c, "doc", "node-a", 0).await;
    assert_eq!(held.epoch, 1, "the store's own generation");

    assert_eq!(
        append(
            c,
            "doc",
            held.epoch + 1,
            0,
            b"from a lease this store never issued",
            0
        )
        .await,
        Err(AppendError::Fenced {
            current: held.epoch
        }),
        "an epoch the store has no memory of must be refused, not believed"
    );
    // And the holder is unaffected: the log is still where it was, so the node
    // whose epoch the store *does* recognise carries on.
    assert_eq!(
        append(c, "doc", held.epoch, 0, b"the real one", 0).await,
        Ok(1)
    );
});

// --- Conditional append: why divergence is impossible -----------------------

contract!(
    an_append_against_a_stale_revision_is_refused_with_the_real_one,
    |c| {
        // Not a leadership problem: the leader is right and merely behind. It is
        // told where the log actually is so it can catch up, which is the
        // difference between a recoverable state and a stuck one.
        let lease = claim(c, "doc", "node-a", 0).await;
        assert_eq!(append(c, "doc", lease.epoch, 0, b"one", 0).await, Ok(1));
        assert_eq!(
            append(c, "doc", lease.epoch, 0, b"again", 0).await,
            Err(AppendError::Stale { current: 1 })
        );
        assert_eq!(append(c, "doc", lease.epoch, 1, b"two", 0).await, Ok(2));
    }
);

contract!(the_log_is_bounded_rather_than_growing_forever, |c| {
    // DEP-03. `since` carried a comment saying "the log is compacted, which is
    // what keeps this bounded" — and nothing compacted it. `RPUSH` with no
    // `LTRIM`, no `EXPIRE`, no `DEL`, read back with `LRANGE 0 -1` on every
    // lease tick per document per node. A document edited for an afternoon
    // accumulated every batch and re-read all of them every few seconds, and
    // the compose Redis has no `maxmemory`, so the end of it is an OOM.
    //
    // Deliberately more entries than the window, so the assertion is about the
    // bound rather than about a number that happens to fit.
    // **A lease that outlives the work by construction** (`SRV-07`).
    //
    // Every other test here does a handful of appends inside the shared
    // one-second `TTL`, which is fine. This one does ten thousand two hundred
    // and fifty, and under a loaded machine that outran the lease's key
    // expiry: the append came back `Unled` and the failure read as a fencing
    // defect rather than as a stopwatch.
    //
    // A test whose result depends on how busy the machine is teaches people to
    // re-run rather than to read, and this project has already paid for that
    // habit. The bound being asserted has nothing to do with time, so the
    // lease is simply made long enough that no amount of load can reach it.
    let lease = c
        .claim("bounded".to_owned(), "node-a".to_owned(), SLOW_WORK_TTL, 0)
        .await
        .expect("the store answered");
    let over = crate::cluster::redis::LOG_MAX_ENTRIES + 250;
    for i in 0..over {
        append(c, "bounded", lease.epoch, i, format!("op{i}").as_bytes(), 0)
            .await
            .unwrap();
    }

    let kept = since(c, "bounded", 0).await;
    assert!(
        kept.len() as u64 <= crate::cluster::redis::LOG_MAX_ENTRIES,
        "the log kept {} entries against a window of {}",
        kept.len(),
        crate::cluster::redis::LOG_MAX_ENTRIES
    );
    // The *newest* entries are the ones a node catching up needs; trimming from
    // the wrong end would keep history nobody can use and drop the batch that
    // was just published.
    let (last_revision, last_payload) = kept.last().expect("the log is not empty");
    assert_eq!(*last_revision, over);
    assert_eq!(last_payload, format!("op{}", over - 1).as_bytes());
});

/// The other half of DEP-03: a bounded window still leaves one log per document
/// that was ever opened, forever.
///
/// Redis-only, because a TTL is a Redis fact — the in-memory coordinator has no
/// key to expire, and asserting one there would test the test. The expiry is
/// refreshed on every append, so this asserts the key carries one at all rather
/// than waiting an hour for it, and checks it against the constant so changing
/// the constant changes this test.
#[tokio::test]
async fn a_log_nobody_returns_to_does_not_live_forever() {
    let Some(store) = redis_store("log-ttl").await else {
        return;
    };
    let lease = store
        .claim("expiring".to_owned(), "node-a".to_owned(), 5_000, 0)
        .await
        .expect("claimed");
    store
        .append(
            "expiring".to_owned(),
            lease.epoch,
            0,
            1,
            b"only".to_vec(),
            0,
        )
        .await
        .expect("appended");

    let ttl = store.log_ttl_ms("expiring").await;
    assert!(
        ttl > 0,
        "the log key has no expiry ({ttl}), so an abandoned document leaks it"
    );
    assert!(
        ttl <= crate::cluster::redis::LOG_TTL_MS as i64,
        "an expiry of {ttl}ms is longer than the window it was set from"
    );
}

contract!(the_log_reads_back_in_order_from_any_point, |c| {
    let lease = claim(c, "doc", "node-a", 0).await;
    for (i, payload) in [b"a", b"b", b"c"].into_iter().enumerate() {
        append(c, "doc", lease.epoch, i as u64, payload, 0)
            .await
            .unwrap();
    }
    assert_eq!(
        since(c, "doc", 0).await,
        vec![(1, b"a".to_vec()), (2, b"b".to_vec()), (3, b"c".to_vec())]
    );
    assert_eq!(since(c, "doc", 2).await, vec![(3, b"c".to_vec())]);
    assert!(since(c, "doc", 3).await.is_empty());
    assert!(since(c, "unknown", 0).await.is_empty());
});

contract!(
    a_chunk_carrying_several_operations_is_accepted_and_continues,
    |c| {
        // **The bug this exists for.** A revision counts *operations*; the log
        // holds one entry per *append*. While the gate compared the entry count,
        // the two agreed only for a chunk of exactly one operation — and every
        // test here, and every test in `net/tests.rs`, submitted exactly one. Two
        // keystrokes inside the editor's flush window produce two, so the ordinary
        // case was the broken one: the append was refused `Stale`, the leader had
        // already applied both operations to its own copy, and it stayed diverged
        // from the log for the rest of the session — no acknowledgement to its
        // client, nothing published to any peer, and every later append refused
        // for the same reason.
        let lease = claim(c, "doc", "node-a", 0).await;

        // Three operations in one chunk: 0 -> 3.
        assert_eq!(
            append_ops(c, "doc", lease.epoch, 0, 3, b"three", 0).await,
            Ok(3),
            "a multi-operation chunk must be accepted"
        );
        // Then two more: 3 -> 5. This is the append that also failed once the
        // first had been refused, which is what made the divergence permanent.
        assert_eq!(
            append_ops(c, "doc", lease.epoch, 3, 2, b"two", 0).await,
            Ok(5),
            "and the next chunk must follow it"
        );

        // Revisions come back as the document's revisions, not as positions.
        assert_eq!(
            since(c, "doc", 0).await,
            vec![(3, b"three".to_vec()), (5, b"two".to_vec())],
            "two entries carrying five operations between them"
        );
        // A client that has seen revision 3 has seen the first chunk entire, and
        // must be sent the second and only the second.
        assert_eq!(since(c, "doc", 3).await, vec![(5, b"two".to_vec())]);
        assert!(since(c, "doc", 5).await.is_empty());

        // And the gate still bites: a stale `after` is refused, and the refusal
        // reports the real revision rather than an entry count.
        assert_eq!(
            append_ops(c, "doc", lease.epoch, 4, 1, b"stale", 0).await,
            Err(AppendError::Stale { current: 5 })
        );
    }
);

contract!(a_failover_loses_nothing_that_was_acknowledged, |c| {
    // The promise ADR-014 makes by appending before acknowledging: whatever the
    // old leader was told had landed is still there for the new one.
    let a = claim(c, "doc", "node-a", 0).await;
    append(c, "doc", a.epoch, 0, b"acknowledged", 0)
        .await
        .unwrap();

    let b = claim(c, "doc", "node-b", TTL + 1).await;
    assert_eq!(
        since(c, "doc", 0).await,
        vec![(1, b"acknowledged".to_vec())],
        "the new leader replays what the old one committed"
    );
    assert_eq!(
        append(c, "doc", b.epoch, 1, b"and continues", 0).await,
        Ok(2)
    );
});

// --- Discovery --------------------------------------------------------------

contract!(a_node_that_stops_announcing_itself_is_forgotten, |c| {
    register(c, peer("node-a", 0), 0).await;
    register(c, peer("node-b", 0), 0).await;
    assert_eq!(peers(c, 1_000).await.len(), 2);

    // node-a keeps talking; node-b does not.
    register(c, peer("node-a", 0), 14_000).await;
    let alive = peers(c, 20_000).await;
    assert_eq!(alive.len(), 1);
    assert_eq!(alive[0].id, "node-a");
});

contract!(re_announcing_updates_rather_than_duplicates, |c| {
    register(c, peer("node-a", 5), 0).await;
    register(c, peer("node-a", 1), 100).await;
    let found = peers(c, 200).await;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].load, 1, "the newer announcement wins");
});

contract!(a_peer_comes_back_with_the_address_it_announced, |c| {
    // The one field that must never be guessed at: a peer is discovered in
    // order to be *connected to*, and an invented address is a relay pointed at
    // nothing.
    register(c, peer("node-a", 3), 0).await;
    let found = peers(c, 10).await;
    assert_eq!(found[0].advertise, peer("node-a", 3).advertise);
    assert_eq!(found[0].seen_ms, 0, "and when it was last heard from");
});

// --- Election ---------------------------------------------------------------

#[test]
fn the_least_loaded_node_leads() {
    let peers = vec![peer("node-a", 9), peer("node-b", 2), peer("node-c", 7)];
    assert_eq!(elect(&peers).unwrap().id, "node-b");
}

#[test]
fn a_tie_is_broken_by_id_so_two_nodes_reach_the_same_answer() {
    // Two nodes electing from the same peer list must agree. If the tie broke
    // on iteration order they would both take the lease, and the epoch fence
    // would be cleaning up after an avoidable race on every election.
    let forwards = vec![peer("node-a", 3), peer("node-b", 3), peer("node-c", 3)];
    let mut backwards = forwards.clone();
    backwards.reverse();
    assert_eq!(elect(&forwards).unwrap().id, elect(&backwards).unwrap().id);
    assert_eq!(elect(&forwards).unwrap().id, "node-a");
}

#[test]
fn an_empty_cluster_elects_nobody() {
    assert!(elect(&[]).is_none());
}

// --- The property that matters ----------------------------------------------

contract!(no_interleaving_of_two_nodes_can_produce_two_writers, |c| {
    // The safety property stated as a search rather than an example. Two nodes
    // claim and append at every offset across a lease lifetime; at no point may
    // both succeed at the same revision, because that is what divergence is.
    //
    // A fresh document per step rather than a fresh store, so this runs the
    // same way against a database that persists.
    let mut accepted_at = std::collections::BTreeMap::<u64, String>::new();

    for step in 0..40u64 {
        let now = step * 100;
        let doc = format!("interleaving-{step}");

        let a = claim(c, &doc, "node-a", 0).await;
        let b = claim(c, &doc, "node-b", now).await;

        let from_a = append(c, &doc, a.epoch, 0, b"a", now).await;
        let from_b = append(c, &doc, b.epoch, 0, b"b", now).await;

        let winners: Vec<&str> = [("a", &from_a), ("b", &from_b)]
            .iter()
            .filter(|(_, r)| r.is_ok())
            .map(|(who, _)| *who)
            .collect();
        assert_eq!(
            winners.len(),
            1,
            "at t={now} both nodes wrote revision 1: {from_a:?} {from_b:?}"
        );
        accepted_at.insert(now, winners[0].to_owned());
    }

    // And the change-over is monotonic: once B starts winning it keeps winning,
    // rather than leadership oscillating between them.
    let sequence: Vec<&String> = accepted_at.values().collect();
    if let Some(at) = sequence.iter().position(|w| *w == "b") {
        assert!(
            sequence[at..].iter().all(|w| *w == "b"),
            "leadership oscillated: {sequence:?}"
        );
    }
});

// --- Failover happens without anybody deciding anything ---------------------

contract!(
    a_relay_takes_over_by_claiming_rather_than_declaring_a_death,
    |c| {
        // There is no failure detector. A node that wants the document simply calls
        // `claim` on a timer: while the lease is held it is told who holds it and
        // relays there, and once the lease has lapsed the same call takes it over.
        // The changeover is a consequence of an atomic operation, not a decision.
        claim(c, "doc", "leader", 0).await;

        // While the leader keeps renewing, the relay is told "not you" every time
        // and never has to conclude anything.
        for step in 1..5u64 {
            let now = step * 200;
            claim(c, "doc", "leader", now).await;
            let answer = claim(c, "doc", "relay", now).await;
            assert_eq!(answer.node, "leader", "at t={now}");
        }

        // The leader stops. Nobody announces it, nobody votes, and nothing changes
        // until the lease simply lapses on its own.
        let last_renewal = 800;
        let answer = claim(c, "doc", "relay", last_renewal + TTL - 1).await;
        assert_eq!(answer.node, "leader", "still held, still relaying");

        let taken = claim(c, "doc", "relay", last_renewal + TTL + 1).await;
        assert_eq!(taken.node, "relay", "and now it is not");
        assert!(taken.epoch > 1, "with an epoch that fences the old leader");
    }
);

// --- The relay's transport (ADR-017) ----------------------------------------
//
// Only meaningful against Redis: standalone has no second node to relay to, so
// there is nothing for `Memory` to implement and nothing it would prove.

#[tokio::test]
async fn a_published_batch_reaches_a_subscriber() {
    let Some(store) = redis_store("publish").await else {
        eprintln!("skipped: set OPENCALC_TEST_REDIS to a reachable server to run it");
        return;
    };
    let channel = crate::relay::committed_channel(store.namespace(), "doc");
    let mut inbox = store.subscribe(&channel).await.expect("subscribed");

    // Subscribing is asynchronous on Redis's side: a publish that races the
    // subscription is dropped, and pub/sub has no way to say so. Retried rather
    // than slept past, so the test is neither flaky nor slower than it needs to
    // be.
    let heard = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            store
                .publish(&channel, b"a batch".to_vec())
                .await
                .expect("published");
            if let Ok(Some(payload)) =
                tokio::time::timeout(std::time::Duration::from_millis(100), inbox.recv()).await
            {
                return payload;
            }
        }
    })
    .await
    .expect("a subscriber hears what is published");
    assert_eq!(heard, b"a batch");
}

#[tokio::test]
async fn a_batch_published_for_one_document_does_not_reach_another() {
    // Channels are per document, and getting that wrong applies one customer's
    // edits to another customer's file.
    let Some(store) = redis_store("isolation").await else {
        eprintln!("skipped: set OPENCALC_TEST_REDIS to a reachable server to run it");
        return;
    };
    let mine = crate::relay::committed_channel(store.namespace(), "doc-1");
    let theirs = crate::relay::committed_channel(store.namespace(), "doc-2");
    let mut inbox = store.subscribe(&mine).await.expect("subscribed");

    for _ in 0..10 {
        store.publish(&theirs, b"not yours".to_vec()).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(
        inbox.try_recv().is_err(),
        "a subscriber to one document heard another's traffic"
    );
}

#[tokio::test]
async fn a_forwarded_submission_survives_the_wire() {
    // The relay carries a submission *unaltered* — not rebased, not renumbered.
    // A relay that adjusts what it carries has become a second implementation
    // of ordering, running on the node that does not have the log.
    use casual_calc_transaction::session::{Base, ClientId, Submission};

    let forwarded = crate::relay::Forwarded {
        document: "doc".to_owned(),
        node: "node-two".to_owned(),
        submission: Submission {
            client: ClientId(4),
            seq: 9,
            base: Base::Chained,
            ops: vec![],
        },
    };
    let json = serde_json::to_vec(&forwarded).unwrap();
    let back: crate::relay::Forwarded = serde_json::from_slice(&json).unwrap();
    assert_eq!(back, forwarded);
}

// --- The coordinator link (DEP-13) ------------------------------------------
//
// Two properties, and they are the two halves of "a single Redis failure still
// stops ordering cluster-wide":
//
// - the link can be **encrypted**, because it carries document operations and
//   the lease tokens that decide who may write them;
// - the link **comes back**, because a coordinator that restarts — or a replica
//   promoted in its place — leaves every node holding a socket to something
//   that no longer exists.

/// The link must be one this *build* can dial, not merely one the URL syntax
/// admits.
///
/// Always runs, because it is the failure with no symptom to look for: the
/// `redis` dependency's TLS support is a Cargo feature, and without it every
/// `rediss://` URL is refused at parse time with
/// `can't connect with TLS, the feature is not enabled`. A node configured for
/// an encrypted coordinator link then does not start at all, and the only
/// configuration that *does* start is the one that carries lease tokens and
/// document operations in clear.
///
/// Asserted against a port nothing serves, so what is being established is which
/// kind of failure comes back — a connection that could not be made, rather than
/// a client that was never built to try.
#[tokio::test]
async fn a_secured_coordinator_url_is_one_this_build_can_dial() {
    let why = crate::cluster::redis::Redis::connect_within("rediss://127.0.0.1:1/", "unused")
        .await
        .expect_err("nothing serves port 1");
    assert!(
        !why.0.contains("feature is not enabled"),
        "this build cannot speak TLS to a coordinator at all, so every deployment \
         that wants one runs in clear instead: {why}"
    );
}

/// Certificates plus a plaintext URL is the shape that must not start.
///
/// It is the one misconfiguration here that *works*: the node comes up, joins
/// the cluster, orders edits, and carries every one of them in clear — under a
/// configuration that names a CA and a client certificate and so reads, to
/// whoever wrote it, as an encrypted link. Nothing downstream can tell, which is
/// why this is refused at startup rather than warned about.
#[test]
fn certificates_configured_against_a_plaintext_coordinator_url_are_refused() {
    let tls = crate::cluster::redis::LinkTls {
        root_ca: Some("/etc/opencalc/redis-ca.pem".into()),
        client: None,
    };
    let why = crate::cluster::redis::link_problems("redis://coordinator:6379", &tls)
        .expect_err("certificates with a plaintext URL cannot both be meant");
    assert!(why.contains("rediss://"), "and it says what to do: {why}");

    assert!(
        crate::cluster::redis::link_problems("rediss://coordinator:6380", &tls).is_ok(),
        "the same certificates against an encrypted URL are the intended shape"
    );
    assert!(
        crate::cluster::redis::link_problems(
            "redis://coordinator:6379",
            &crate::cluster::redis::LinkTls::default()
        )
        .is_ok(),
        "and a plaintext link with no certificates is a decision, not an error"
    );
}

/// Turning verification off is not on offer, and saying so beats failing oddly.
///
/// `rediss://…/#insecure` encrypts the link to whoever answers the port, which
/// is not the same as encrypting the link to your coordinator — it is the
/// property TLS exists to provide, discarded. This build does not compile
/// `redis`'s insecure mode at all, so the fragment would otherwise surface as an
/// unexplained handshake failure that reads like a certificate problem.
#[test]
fn a_coordinator_url_that_turns_verification_off_is_refused_by_name() {
    let why = crate::cluster::redis::link_problems(
        "rediss://coordinator:6380/#insecure",
        &crate::cluster::redis::LinkTls::default(),
    )
    .expect_err("#insecure must not be accepted");
    assert!(
        why.contains("OPENCALC_REDIS_CA"),
        "and it points at the setting that solves the problem it was reached for: {why}"
    );
}

/// A plaintext coordinator link is said out loud, once, at startup.
///
/// The link carries the lease that decides which node may write a document and
/// every operation appended to the log. It will never *fail*, which is exactly
/// why nothing else would mention it — the same reason
/// [`crate::config::Exposure::warnings`] exists.
#[test]
fn a_plaintext_coordinator_link_is_warned_about() {
    let warnings = crate::cluster::redis::link_warnings("redis://coordinator:6379");
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("clear"));

    assert!(
        crate::cluster::redis::link_warnings("rediss://coordinator:6380").is_empty(),
        "an encrypted link has nothing to say"
    );

    // A password on a plaintext link is worse and quieter: the credential for
    // the cluster's whole coordination state, readable by anything on the path.
    let with_password =
        crate::cluster::redis::link_warnings("redis://default:hunter2@coordinator:6379");
    assert_eq!(with_password.len(), 2, "{with_password:?}");
    assert!(
        with_password.iter().any(|w| w.contains("password")),
        "{with_password:?}"
    );
    assert!(
        !with_password.iter().any(|w| w.contains("hunter2")),
        "and the warning must not print the password it is warning about: {with_password:?}"
    );
}

/// A `redis-server` speaking TLS, started for one test and killed with it.
///
/// A real server rather than a stub, because the thing being established is that
/// a handshake completes against the implementation operators actually run —
/// the same reason `--healthcheck` fetches over TLS rather than opening a
/// socket. Returns `None` when there is no `redis-server` on the path or the one
/// there was not built with TLS, which is a hole and says so.
struct SecuredRedis {
    child: std::process::Child,
    port: u16,
    ca: std::path::PathBuf,
    dir: std::path::PathBuf,
}

impl Drop for SecuredRedis {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl SecuredRedis {
    /// A CA, a certificate for `127.0.0.1`, and a server that requires TLS.
    async fn start(name: &str) -> Option<Self> {
        let dir =
            std::env::temp_dir().join(format!("opencalc-redis-tls-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;

        // A CA and a leaf under it, rather than one self-signed certificate
        // used as its own trust anchor: webpki will not accept a trust anchor
        // without `basicConstraints: CA`, so the shortcut fails for a reason
        // that has nothing to do with what is being tested.
        let ca_key = rcgen::KeyPair::generate().ok()?;
        let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).ok()?;
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "opencalc coordinator test ca");
        let ca_cert = ca_params.self_signed(&ca_key).ok()?;

        let leaf_key = rcgen::KeyPair::generate().ok()?;
        let mut leaf = rcgen::CertificateParams::new(vec!["127.0.0.1".to_owned()]).ok()?;
        leaf.distinguished_name
            .push(rcgen::DnType::CommonName, "coordinator");
        let leaf_cert = leaf.signed_by(&leaf_key, &ca_cert, &ca_key).ok()?;

        let ca = dir.join("ca.pem");
        let cert = dir.join("server.pem");
        let key = dir.join("server.key");
        std::fs::write(&ca, ca_cert.pem()).ok()?;
        std::fs::write(&cert, leaf_cert.pem()).ok()?;
        std::fs::write(&key, leaf_key.serialize_pem()).ok()?;

        let port = free_port().await?;
        let child = std::process::Command::new("redis-server")
            // Plain disabled outright: if the TLS port were somehow not
            // listening, a test that fell back to `redis://` would pass while
            // proving nothing.
            .args(["--port", "0"])
            .args(["--tls-port", &port.to_string()])
            .args(["--tls-cert-file", cert.to_str()?])
            .args(["--tls-key-file", key.to_str()?])
            .args(["--tls-ca-cert-file", ca.to_str()?])
            .args(["--tls-auth-clients", "no"])
            .args(["--save", ""])
            .args(["--appendonly", "no"])
            .args(["--dir", dir.to_str()?])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;

        let mut started = Self {
            child,
            port,
            ca,
            dir,
        };
        for _ in 0..100 {
            if started.child.try_wait().ok()?.is_some() {
                // It exited: this build has no TLS support, which is a skip
                // rather than a failure.
                return None;
            }
            if tokio::net::TcpStream::connect(("127.0.0.1", started.port))
                .await
                .is_ok()
            {
                return Some(started);
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        None
    }

    fn url(&self) -> String {
        format!("rediss://127.0.0.1:{}", self.port)
    }
}

/// A port nothing is listening on, by binding one and letting it go.
async fn free_port() -> Option<u16> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?;
    listener.local_addr().ok().map(|a| a.port())
}

/// A plaintext `redis-server` of this test's own, which it may kill.
///
/// The forwarder above models a *connection* going away, which is the common
/// case and runs anywhere. This is the row's gate taken literally — the process
/// is killed and started again — and it reaches one thing the forwarder cannot:
/// a restarted Redis has an **empty script cache**, so the first `claim` after
/// it comes back is `NOSCRIPT`. If that were not recovered from, a node would
/// re-dial successfully and then fail every claim and every append for the rest
/// of its life, which is the original defect with a healthy-looking connection.
struct OwnRedis {
    child: std::process::Child,
    port: u16,
    dir: std::path::PathBuf,
    /// Kept so a restart is the *same* server: a durability floor that was set
    /// on the first start and not the second is a harness that quietly stops
    /// testing what it was written for.
    extra: Vec<String>,
}

impl Drop for OwnRedis {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl OwnRedis {
    async fn start(name: &str) -> Option<Self> {
        let dir =
            std::env::temp_dir().join(format!("opencalc-redis-own-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        let port = free_port().await?;
        let mut started = Self {
            child: Self::spawn(port, &dir)?,
            port,
            dir,
            extra: Vec::new(),
        };
        started.wait_for_port().await.then_some(started)
    }

    /// The same, with extra `redis-server` arguments — a durability floor, or a
    /// `--replicaof` that makes this one a replica of another.
    async fn start_as(name: &str, extra: &[&str]) -> Option<Self> {
        let dir =
            std::env::temp_dir().join(format!("opencalc-redis-own-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        let port = free_port().await?;
        let extra = extra.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        let mut started = Self {
            child: Self::spawn_with(port, &dir, &extra)?,
            port,
            dir,
            extra,
        };
        started.wait_for_port().await.then_some(started)
    }

    fn spawn(port: u16, dir: &std::path::Path) -> Option<std::process::Child> {
        Self::spawn_with(port, dir, &[])
    }

    fn spawn_with(
        port: u16,
        dir: &std::path::Path,
        extra: &[String],
    ) -> Option<std::process::Child> {
        std::process::Command::new("redis-server")
            .args(["--port", &port.to_string()])
            // Persistence off, as the cluster compose runs it: what is in here
            // is coordination for documents currently open, not the documents.
            .args(["--save", ""])
            .args(["--appendonly", "no"])
            .args(["--dir", dir.to_str()?])
            .args(extra)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()
    }

    /// Stop it and leave it stopped.
    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Start it again on the same port, with whatever it was started with.
    async fn restart(&mut self) -> bool {
        let Some(child) = Self::spawn_with(self.port, &self.dir, &self.extra) else {
            return false;
        };
        self.child = child;
        self.wait_for_port().await
    }

    async fn wait_for_port(&mut self) -> bool {
        for _ in 0..100 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return false;
            }
            if tokio::net::TcpStream::connect(("127.0.0.1", self.port))
                .await
                .is_ok()
            {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        false
    }

    /// Kill it, and bring it back on the same port with nothing remembered.
    async fn kill_and_restart(&mut self) -> bool {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let Some(child) = Self::spawn(self.port, &self.dir) else {
            return false;
        };
        self.child = child;
        self.wait_for_port().await
    }

    fn url(&self) -> String {
        format!("redis://127.0.0.1:{}", self.port)
    }
}

/// **Killing one Redis leaves ordering working** — the row's gate, literally.
///
/// The process is killed and started again on the same port. The node's
/// connection must re-dial, and its scripts must reload into a cache that is now
/// empty, without anybody restarting the node.
///
/// The lease and the log are gone, which is correct and is what
/// docs/65 already says: Redis runs with persistence off and holds coordination
/// for documents currently open, not the documents. What must survive is the
/// node's **ability to coordinate at all**.
#[tokio::test]
async fn ordering_works_again_after_the_coordinator_is_killed_and_restarted() {
    let Some(mut server) = OwnRedis::start("restart").await else {
        eprintln!("skipped: needs a `redis-server` on PATH");
        return;
    };
    let store = crate::cluster::redis::Redis::connect_within(&server.url(), "restart-test")
        .await
        .expect("connected");

    let before = store
        .claim("doc".to_owned(), "node-a".to_owned(), 60_000, 0)
        .await
        .expect("a lease before the kill");
    assert_eq!(
        store
            .append("doc".to_owned(), before.epoch, 0, 1, b"before".to_vec(), 0)
            .await,
        Ok(1)
    );

    assert!(server.kill_and_restart().await, "the coordinator came back");

    let after = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            // A claim is a Lua script, so this is also the `NOSCRIPT` path: the
            // restarted server has never seen it.
            if let Ok(lease) = store
                .claim("doc".to_owned(), "node-a".to_owned(), 60_000, 1)
                .await
            {
                return lease;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the node never coordinated again, so every edit it takes is one it must refuse");

    assert_eq!(after.node, "node-a");
    assert_eq!(
        store
            .append("doc".to_owned(), after.epoch, 0, 1, b"after".to_vec(), 1)
            .await,
        Ok(1),
        "and appending works again — which is ordering working"
    );
}

/// The coordinator link, end to end, over TLS against a private CA.
///
/// The private CA is the case that matters: an internal Redis is not issued a
/// certificate by a public authority, so a client that can only trust the system
/// roots can only be pointed at a plaintext port.
#[tokio::test]
async fn the_coordinator_link_can_be_encrypted_against_a_private_ca() {
    let Some(server) = SecuredRedis::start("link").await else {
        eprintln!(
            "skipped: needs a `redis-server` on PATH built with TLS support \
             (`brew install redis`, or `--tls-port` on the distribution's build)"
        );
        return;
    };

    let tls = crate::cluster::redis::LinkTls {
        root_ca: Some(server.ca.clone()),
        client: None,
    };
    let store = crate::cluster::redis::Redis::connect_secured(&server.url(), "tls-test", &tls)
        .await
        .expect("an encrypted coordinator link");

    // Not merely a handshake: the three things the link exists to carry.
    let lease = store
        .claim("doc".to_owned(), "node-a".to_owned(), 5_000, 0)
        .await
        .expect("a lease over TLS");
    assert_eq!(lease.node, "node-a");
    assert_eq!(
        store
            .append("doc".to_owned(), lease.epoch, 0, 1, b"secret".to_vec(), 0)
            .await,
        Ok(1),
        "and an append over TLS"
    );
    assert_eq!(
        store.since("doc".to_owned(), 0).await,
        Ok(vec![(1, b"secret".to_vec())]),
        "and the log reads back"
    );
}

/// Trusting a private CA must be a decision, not a default.
///
/// Without the CA the same server must be **refused**. Otherwise the test above
/// would pass against a client that verifies nothing, which is the failure it is
/// meant to rule out.
#[tokio::test]
async fn an_untrusted_coordinator_certificate_is_refused() {
    let Some(server) = SecuredRedis::start("untrusted").await else {
        eprintln!("skipped: needs a `redis-server` on PATH built with TLS support");
        return;
    };

    let why = crate::cluster::redis::Redis::connect_within(&server.url(), "tls-test")
        .await
        .expect_err("a certificate from an unknown CA must not be accepted");
    assert!(
        why.0.contains("certificate"),
        "refused, but not over the certificate — so the test above proves a handshake \
         completed rather than that it was checked: {why}"
    );
}

/// A TCP forwarder whose connections can be severed on demand.
///
/// This is how a coordinator restart looks to a node: the socket dies and the
/// address is dialable again a moment later. Doing it this way rather than by
/// killing a server means the test runs anywhere `OPENCALC_TEST_REDIS` points —
/// including CI, where the coordinator is a service container the suite has no
/// business stopping.
struct Interruptible {
    port: u16,
    cut: tokio::sync::broadcast::Sender<()>,
    accepting: tokio::task::JoinHandle<()>,
}

impl Drop for Interruptible {
    fn drop(&mut self) {
        // Both, and in this order: aborting the accept loop drops the listener
        // so the port stops answering, and the signal drops the connections
        // already forwarded through it. Either alone leaves a coordinator that
        // is half gone, which is not a state this models.
        self.accepting.abort();
        self.sever();
    }
}

impl Interruptible {
    async fn in_front_of(upstream: &str) -> Option<Self> {
        // Only the address, because what is forwarded is bytes: any credentials
        // in the URL travel through untouched.
        let upstream = upstream
            .trim_start_matches("redis://")
            .trim_end_matches('/')
            .to_owned();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?;
        let port = listener.local_addr().ok()?.port();
        let (cut, _) = tokio::sync::broadcast::channel(8);
        let signal = cut.clone();
        let accepting = tokio::spawn(async move {
            while let Ok((mut client, _)) = listener.accept().await {
                let Ok(mut server) = tokio::net::TcpStream::connect(&upstream).await else {
                    continue;
                };
                let mut severed = signal.subscribe();
                tokio::spawn(async move {
                    tokio::select! {
                        _ = tokio::io::copy_bidirectional(&mut client, &mut server) => {}
                        // Both halves are dropped here, which is the whole point:
                        // the node's connection goes away without warning.
                        _ = severed.recv() => {}
                    }
                });
            }
        });
        Some(Self {
            port,
            cut,
            accepting,
        })
    }

    fn url(&self) -> String {
        format!("redis://127.0.0.1:{}", self.port)
    }

    fn sever(&self) {
        let _ = self.cut.send(());
    }
}

/// **The gate: killing one Redis leaves ordering working.**
///
/// A coordinator that restarts, or a replica promoted in its place, leaves every
/// node holding a socket to something that is gone. A multiplexed connection
/// does not re-dial: the task driving the socket ends, and every later command
/// on that connection — and on every clone of it, which is what `connection()`
/// hands out — fails forever. The node stays up, keeps refusing every edit for
/// the rest of its life, and the only recovery is a restart of every node in the
/// cluster.
///
/// The first command after the cut is allowed to fail; that is DEP-04's business
/// and the client is told. What must not happen is that it never stops failing.
#[tokio::test]
async fn the_link_recovers_when_the_coordinator_connection_is_lost() {
    let Some(url) = std::env::var("OPENCALC_TEST_REDIS").ok() else {
        eprintln!("skipped: set OPENCALC_TEST_REDIS to a reachable server to run it");
        return;
    };
    let proxy = Interruptible::in_front_of(&url)
        .await
        .expect("a forwarder in front of the coordinator");
    let store =
        crate::cluster::redis::Redis::connect_within(&proxy.url(), "opencalc-test:reconnect")
            .await
            .expect("connected through the forwarder");

    let before = store
        .claim("doc".to_owned(), "node-a".to_owned(), 60_000, 0)
        .await
        .expect("a lease before the coordinator goes away");

    proxy.sever();

    let recovered = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if let Ok(lease) = store
                .claim("doc".to_owned(), "node-a".to_owned(), 60_000, 1)
                .await
            {
                return lease;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the coordinator link never came back, so this node can never order again");

    // The same lease, not a new one: what came back is a connection, and the
    // coordinator's memory of who leads is untouched by it.
    assert_eq!(recovered.node, before.node);
    assert_eq!(recovered.epoch, before.epoch);
}

/// A subscription is a second connection, and it dies the same way.
///
/// Worse, quietly: the message stream simply ends, the channel closes, and the
/// per-document attendant in `net.rs` breaks out of its loop — so the document
/// stops renewing its lease and stops reading its inbox while the node goes on
/// serving the people connected to it.
#[tokio::test]
async fn a_subscription_survives_the_coordinator_going_away() {
    let Some(url) = std::env::var("OPENCALC_TEST_REDIS").ok() else {
        eprintln!("skipped: set OPENCALC_TEST_REDIS to a reachable server to run it");
        return;
    };
    let proxy = Interruptible::in_front_of(&url)
        .await
        .expect("a forwarder in front of the coordinator");
    let store =
        crate::cluster::redis::Redis::connect_within(&proxy.url(), "opencalc-test:resubscribe")
            .await
            .expect("connected through the forwarder");

    let channel = crate::relay::committed_channel(store.namespace(), "doc");
    let mut inbox = store.subscribe(&channel).await.expect("subscribed");

    assert_eq!(
        heard(&store, &channel, &mut inbox, b"before")
            .await
            .expect("a live subscription"),
        b"before"
    );

    proxy.sever();

    assert_eq!(
        heard(&store, &channel, &mut inbox, b"after")
            .await
            .expect("the subscription never came back, so this node hears nothing further"),
        b"after"
    );
}

/// Publish until the subscriber hears it, or give up.
///
/// Retried rather than slept past, because subscribing is asynchronous on
/// Redis's side and a publish that races it is dropped without anything saying
/// so — the same shape as `a_published_batch_reaches_a_subscriber`.
async fn heard(
    store: &crate::cluster::redis::Redis,
    channel: &str,
    inbox: &mut tokio::sync::mpsc::Receiver<Vec<u8>>,
    what: &[u8],
) -> Result<Vec<u8>, tokio::time::error::Elapsed> {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let _ = store.publish(channel, what.to_vec()).await;
            if let Ok(Some(payload)) =
                tokio::time::timeout(std::time::Duration::from_millis(200), inbox.recv()).await
            {
                return payload;
            }
        }
    })
    .await
}

/// A coordinator that is gone **for good** must still produce an answer.
///
/// The hazard in reconnecting is the opposite of the one it fixes: a client that
/// waits for a connection that will never be established turns `/readyz` — which
/// is a call to `peers()` — from a 503 into a probe that hangs, and DEP-04's
/// whole point is that a node which cannot reach the coordinator leaves the pool
/// promptly. So the retry budget is bounded, and this is that bound asserted
/// rather than assumed.
#[tokio::test]
async fn a_coordinator_that_never_returns_produces_an_error_rather_than_a_hang() {
    let Some(url) = std::env::var("OPENCALC_TEST_REDIS").ok() else {
        eprintln!("skipped: set OPENCALC_TEST_REDIS to a reachable server to run it");
        return;
    };
    let proxy = Interruptible::in_front_of(&url)
        .await
        .expect("a forwarder in front of the coordinator");
    let store = crate::cluster::redis::Redis::connect_within(&proxy.url(), "opencalc-test:gone")
        .await
        .expect("connected through the forwarder");
    store.peers(0).await.expect("reachable to begin with");

    // Not merely severed: the forwarder stops answering at all, so every
    // re-dial is refused. This is a node whose Redis is not coming back.
    drop(proxy);

    // **The first failure is not the one to measure**, and writing this test
    // without that distinction made it unable to fail. A command that is in
    // flight when the socket dies comes back with its error at once and starts
    // the re-dial *behind* it; a command issued after that is the one that waits
    // on the re-dial, and so the one that can hang. Raising the retry budget to
    // forty attempts an hour apart left the first version green in ten
    // milliseconds, which is precisely the shape of a test that asserts nothing.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), store.peers(0)).await;

    let answered = tokio::time::timeout(std::time::Duration::from_secs(10), store.peers(0)).await;
    let Ok(outcome) = answered else {
        panic!(
            "a command against a coordinator that is gone waited past the retry budget, \
             so /readyz hangs instead of answering 503"
        );
    };
    assert!(
        outcome.is_err(),
        "and the answer must be that it is unreachable, not a silent success"
    );
}

// --- Coordinator replication and failover (ADR-020) -------------------------
//
// Two properties, and the second is the one the ADR exists for:
//
// - **a failover is survived**, so losing the coordinator's primary is a pause
//   rather than a cluster-wide stop that needs a human;
// - **the window a failover could lose an acknowledged append in produces a
//   refusal**, because Redis replication is asynchronous and ADR-014 §4 promises
//   an operation is in the log before a client is told it was accepted.

/// A sentinel URL is a question — whom to ask, and what to ask about.
///
/// Parsed here rather than handed to `redis` whole, because `redis` has no URL
/// form for sentinel at all: `SentinelClient::build` takes the addresses and the
/// service name as separate arguments. Every field this drops is a field that
/// fails later as "the sentinels are down".
#[test]
fn a_sentinel_url_names_its_sentinels_and_its_service() {
    use crate::cluster::sentinel::Target;

    let plain = Target::parse("redis+sentinel://10.0.0.1:26379,10.0.0.2:26379/opencalc")
        .expect("the ordinary form");
    assert_eq!(plain.sentinels, ["10.0.0.1:26379", "10.0.0.2:26379"]);
    assert_eq!(plain.service, "opencalc");
    assert_eq!(plain.db, 0);
    assert!(!plain.secured);
    assert_eq!(plain.password, None);

    // A missing port is the one default worth supplying: 26379 is what every
    // sentinel deployment uses, and getting it wrong reads as "they are down".
    let bare = Target::parse("redis+sentinel://sentinel-a,sentinel-b/mymaster/3").expect("parsed");
    assert_eq!(bare.sentinels, ["sentinel-a:26379", "sentinel-b:26379"]);
    assert_eq!(bare.db, 3);

    // Credentials, with the password percent-decoded: a Redis password
    // routinely contains the characters a URL reserves, and handing the escapes
    // to Redis verbatim is an authentication failure nobody can read.
    let secured =
        Target::parse("rediss+sentinel://user:p%40ss%2Fword@s1:26379/mymaster").expect("parsed");
    assert!(secured.secured);
    assert_eq!(secured.username.as_deref(), Some("user"));
    assert_eq!(secured.password.as_deref(), Some("p@ss/word"));
    assert_eq!(secured.sentinels, ["s1:26379"]);

    // And the shapes that must not be guessed at.
    let no_service = Target::parse("redis+sentinel://s1:26379")
        .expect_err("a sentinel URL without a service names no primary");
    assert!(
        no_service.contains("mymaster"),
        "and it shows the form: {no_service}"
    );
    assert!(
        Target::parse("redis+sentinel://s1:26379/mymaster?failover=fast").is_err(),
        "a query parameter nothing reads must be refused rather than ignored"
    );
    assert!(
        Target::parse("redis://one-box:6379/").is_err(),
        "a direct URL is not a sentinel URL"
    );
}

/// A private CA behind a sentinel URL would be read and never used.
///
/// `redis` 0.27 dials the resolved primary through `Client::open`, which takes
/// no certificates — `build_with_tls` takes a *URL*, and the URL here names
/// sentinels rather than the primary. So a deployment that points
/// `OPENCALC_REDIS_CA` at its internal CA and sets a sentinel URL gets a link
/// verified against the system trust store, under a configuration that reads as
/// pinned to its own CA. That is the same shape as certificates against a
/// `redis://` URL, which is already refused, and it is refused for the same
/// reason: nothing downstream can tell.
#[test]
fn a_private_ca_cannot_be_hidden_behind_a_sentinel_url() {
    let tls = crate::cluster::redis::LinkTls {
        root_ca: Some("/etc/opencalc/redis-ca.pem".into()),
        client: None,
    };
    let why =
        crate::cluster::redis::link_problems("rediss+sentinel://s1:26379,s2:26379/mymaster", &tls)
            .expect_err("a CA that cannot be used must not be accepted as though it were");
    assert!(
        why.contains("silently ignored"),
        "and it says what would have happened: {why}"
    );

    assert!(
        crate::cluster::redis::link_problems(
            "rediss+sentinel://s1:26379/mymaster",
            &crate::cluster::redis::LinkTls::default()
        )
        .is_ok(),
        "the same URL with no certificates is the intended encrypted shape"
    );
    // And a sentinel URL that cannot be parsed is refused at startup rather
    // than at the first failover.
    assert!(
        crate::cluster::redis::link_problems(
            "redis+sentinel://s1:26379",
            &crate::cluster::redis::LinkTls::default()
        )
        .is_err(),
        "a sentinel URL naming no service must not start"
    );
}

/// The plaintext warning knows the sentinel schemes too.
///
/// `rediss+sentinel://` is encrypted and must not be warned about; a warning
/// that cries wolf on the correct configuration is a warning operators learn to
/// ignore on the incorrect one.
#[test]
fn a_sentinel_link_is_warned_about_by_the_same_rule() {
    let plain = crate::cluster::redis::link_warnings("redis+sentinel://s1:26379/mymaster");
    assert_eq!(plain.len(), 1, "{plain:?}");
    assert!(plain[0].contains("clear"));

    assert!(
        crate::cluster::redis::link_warnings("rediss+sentinel://s1:26379/mymaster").is_empty(),
        "an encrypted sentinel link has nothing to say"
    );
}

/// One primary, one replica and three sentinels, started for one test.
///
/// Three rather than one because a sentinel failover needs a **quorum** to
/// declare the primary down, and a single sentinel would test a code path
/// nobody deploys. The whole thing is killed with the test, including the
/// sentinels, which do not stop on their own.
///
/// Returns `None` when there is no `redis-server` or `redis-sentinel` on the
/// path, which is a hole and says so.
struct SentinelCluster {
    nodes: Vec<OwnRedis>,
    sentinels: Vec<std::process::Child>,
    sentinel_ports: Vec<u16>,
    service: String,
    dir: std::path::PathBuf,
}

impl Drop for SentinelCluster {
    fn drop(&mut self) {
        for sentinel in &mut self.sentinels {
            let _ = sentinel.kill();
            let _ = sentinel.wait();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl SentinelCluster {
    /// `extra` is passed to **both** data nodes, so a durability floor set here
    /// survives the promotion — which is the case a floor set on the original
    /// primary alone would miss.
    async fn start(name: &str, extra: &[&str]) -> Option<Self> {
        Self::start_split(name, extra, extra).await
    }

    /// The same, with different arguments for the node that starts as the
    /// replica — which is how "only the original primary was configured" is
    /// modelled.
    async fn start_split(name: &str, primary: &[&str], replica: &[&str]) -> Option<Self> {
        let dir =
            std::env::temp_dir().join(format!("opencalc-sentinel-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;

        // **`repl-diskless-sync-delay` is why the first version of this could not
        // fail.** Redis waits five seconds before starting a diskless full sync,
        // to batch replicas that arrive together. The whole failover here happens
        // inside that window, so the replica sat in `wait_bgsave` with an empty
        // dataset, was promoted with nothing in it, and the test that asserted
        // "the log survived" passed against a store where *nothing* had survived
        // — because a claim on an empty store simply takes a fresh lease that
        // looks exactly like the old one.
        let mut prompt = vec!["--repl-diskless-sync-delay", "0"];
        prompt.extend(primary);
        let first = OwnRedis::start_as(&format!("{name}-a"), &prompt).await?;
        let mut following = vec!["--repl-diskless-sync-delay", "0"];
        following.extend(replica);
        let port = first.port.to_string();
        following.extend(["--replicaof", "127.0.0.1", &port]);
        let second = OwnRedis::start_as(&format!("{name}-b"), &following).await?;

        let service = format!("opencalc-{name}");
        let mut sentinels = Vec::new();
        let mut sentinel_ports = Vec::new();
        for which in 0..3 {
            let at = free_port().await?;
            let home = dir.join(format!("s{which}"));
            std::fs::create_dir_all(&home).ok()?;
            let conf = home.join("sentinel.conf");
            // Its own file per sentinel, because a sentinel **rewrites** its
            // configuration — it stores the id it generated and everything it
            // learns — and three processes sharing one file corrupt each
            // other's view of the cluster.
            std::fs::write(
                &conf,
                format!(
                    "port {at}\n\
                     dir {}\n\
                     sentinel monitor {service} 127.0.0.1 {} 2\n\
                     sentinel down-after-milliseconds {service} 1000\n\
                     sentinel failover-timeout {service} 5000\n\
                     sentinel parallel-syncs {service} 1\n",
                    home.display(),
                    first.port
                ),
            )
            .ok()?;
            sentinels.push(
                std::process::Command::new("redis-sentinel")
                    .arg(&conf)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .ok()?,
            );
            sentinel_ports.push(at);
        }

        let cluster = Self {
            nodes: vec![first, second],
            sentinels,
            sentinel_ports,
            service,
            dir,
        };
        // Not merely started: every sentinel must agree on the primary and see
        // the replica, or a failover triggered a moment later has no quorum and
        // no candidate, and the test fails as a timeout that reads like a bug
        // in the code under test.
        cluster.settled().await.then_some(cluster)
    }

    async fn settled(&self) -> bool {
        for _ in 0..300 {
            let mut agreed = 0;
            for at in &self.sentinel_ports {
                if self.master_seen_by(*at).await.is_some() && self.replicas_seen_by(*at).await >= 1
                {
                    agreed += 1;
                }
            }
            // **Sentinel seeing a replica is not the replica having the data.**
            // A replica appears in `SENTINEL replicas` the moment it announces
            // itself, which is before its first full sync has run — and a
            // failover in that window promotes an empty node. Asked of the
            // replica itself, because `master_link_status:up` is the state that
            // means "everything the primary has, I have".
            let synced = self.linked_up(self.nodes[1].port).await;
            if agreed == self.sentinel_ports.len() && synced {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        false
    }

    /// Whether the node at `port` is a replica whose link to its primary is up.
    async fn linked_up(&self, port: u16) -> bool {
        let info: Option<String> =
            Self::ask(port, &mut ::redis::cmd("INFO").arg("replication").clone())
                .await
                .and_then(|v| ::redis::FromRedisValue::from_owned_redis_value(v).ok());
        info.is_some_and(|info| {
            info.lines()
                .any(|line| line.trim() == "master_link_status:up")
        })
    }

    async fn ask(at: u16, cmd: &mut ::redis::Cmd) -> Option<::redis::Value> {
        Self::ask_for(at, cmd).await.ok()
    }

    /// The same, keeping the refusal — which some of these commands answer with
    /// meaningfully.
    async fn ask_for(at: u16, cmd: &mut ::redis::Cmd) -> Result<::redis::Value, String> {
        let client =
            ::redis::Client::open(format!("redis://127.0.0.1:{at}")).map_err(|e| e.to_string())?;
        let mut c = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| e.to_string())?;
        cmd.query_async(&mut c).await.map_err(|e| e.to_string())
    }

    async fn master_seen_by(&self, at: u16) -> Option<u16> {
        let answer: Option<(String, u16)> = Self::ask(
            at,
            ::redis::cmd("SENTINEL")
                .arg("get-master-addr-by-name")
                .arg(&self.service),
        )
        .await
        .and_then(|v| ::redis::FromRedisValue::from_owned_redis_value(v).ok());
        answer.map(|(_, port)| port)
    }

    async fn replicas_seen_by(&self, at: u16) -> usize {
        let answer: Option<Vec<::redis::Value>> = Self::ask(
            at,
            ::redis::cmd("SENTINEL").arg("replicas").arg(&self.service),
        )
        .await
        .and_then(|v| ::redis::FromRedisValue::from_owned_redis_value(v).ok());
        answer.map_or(0, |seen| seen.len())
    }

    /// Whichever node the sentinels currently call the primary.
    async fn primary(&self) -> Option<u16> {
        self.master_seen_by(*self.sentinel_ports.first()?).await
    }

    /// The URL a node is configured with. Note what is **not** in it: the
    /// address of any data node.
    fn url(&self) -> String {
        let hosts = self
            .sentinel_ports
            .iter()
            .map(|p| format!("127.0.0.1:{p}"))
            .collect::<Vec<_>>()
            .join(",");
        format!("redis+sentinel://{hosts}/{}", self.service)
    }

    /// Ask a sentinel to promote the replica **without anything dying**.
    ///
    /// The interesting half of a failover, and the half a re-dial cannot
    /// survive: the old primary stays up, healthy and reachable, and is demoted
    /// to a replica. Every write to it answers `READONLY`, which `redis`
    /// classifies as `NoRetry`, so a connection pointed at it reconnects to
    /// nothing and fails forever while looking perfectly alive.
    /// **Retried, because the refusals are transient and say so.** A sentinel
    /// answers `NOGOODSLAVE` while it considers the replica subjectively down —
    /// which a `down-after-milliseconds` of one second reaches on a machine
    /// running the rest of this suite in parallel — and `INPROGRESS` if another
    /// sentinel got there first. Taking the first refusal as final made this
    /// fail under load as "asked for a failover", which reads like a bug in the
    /// thing under test and is not one.
    async fn promote_the_replica(&self) -> Result<(), String> {
        let mut last = "no sentinel was asked".to_owned();
        for _ in 0..200 {
            for at in &self.sentinel_ports {
                match Self::ask_for(
                    *at,
                    ::redis::cmd("SENTINEL").arg("FAILOVER").arg(&self.service),
                )
                .await
                {
                    Ok(_) => return Ok(()),
                    Err(why) => last = why,
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        Err(last)
    }

    /// Kill whichever node is primary now, and say which port that was.
    async fn kill_the_primary(&mut self) -> Option<u16> {
        let port = self.primary().await?;
        let node = self.nodes.iter_mut().find(|n| n.port == port)?;
        node.kill();
        Some(port)
    }

    /// Wait until `port` has actually been told it is a replica now.
    ///
    /// **Not the same moment as the sentinels agreeing**, and the difference is
    /// what made the first version of the failover test unable to fail. A
    /// sentinel promotes the replica and only then reconfigures the old primary,
    /// which takes seconds — and in between, the old primary is still
    /// `role:master` and still accepts writes. A test that asserted recovery in
    /// that window passed by writing to the node it was supposed to have stopped
    /// using, with re-resolution disabled.
    ///
    /// It is also the honest shape of the risk ADR-020 names: writes taken in
    /// that window are accepted by a node that is about to be rewound, and
    /// nothing on the client side can see it. What closes it is
    /// `min-replicas-to-write`, not this code.
    async fn demoted(&self, port: u16) -> bool {
        for _ in 0..300 {
            let role: Option<Vec<::redis::Value>> = Self::ask(port, &mut ::redis::cmd("ROLE"))
                .await
                .and_then(|v| ::redis::FromRedisValue::from_owned_redis_value(v).ok());
            let says = role.as_ref().and_then(|r| r.first()).and_then(|first| {
                ::redis::FromRedisValue::from_owned_redis_value(first.clone()).ok()
            });
            if says == Some("slave".to_owned()) {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        false
    }

    /// Wait until the sentinels agree the primary is somewhere other than
    /// `was`.
    async fn moved_away_from(&self, was: u16) -> Option<u16> {
        for _ in 0..300 {
            match self.primary().await {
                Some(now) if now != was => return Some(now),
                _ => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
            }
        }
        None
    }
}

/// Retry `claim` until the link finds the coordinator again, or give up.
///
/// The first failures are expected and are DEP-04's business — the client is
/// told `NotSaving` and `/readyz` answers 503. What must not happen is that it
/// never stops failing.
async fn coordinates_again(
    store: &crate::cluster::redis::Redis,
    document: &str,
    now: u64,
) -> Option<Lease> {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            if let Ok(lease) = store
                .claim(document.to_owned(), "node-a".to_owned(), 60_000, now)
                .await
            {
                return lease;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .ok()
}

/// **The gate: killing one Redis leaves ordering working — through a real
/// failover.**
///
/// The primary is not killed here; it is **demoted**, which is the case the
/// re-dial delivered by `DEP-13` cannot survive and the case that makes ADR-020
/// a change rather than a configuration note. The socket stays up, the node goes
/// on answering reads, and every write comes back `READONLY` forever.
///
/// What is asserted is both halves: that ordering resumes, and that **nothing a
/// client was already told about is missing from the log afterwards** — three
/// appends acknowledged before the failover are still there, and the next append
/// follows them rather than overwriting them.
#[tokio::test]
async fn ordering_survives_a_coordinator_failover() {
    let Some(cluster) = SentinelCluster::start("promote", &[]).await else {
        eprintln!("skipped: needs `redis-server` and `redis-sentinel` on PATH");
        return;
    };
    let store =
        crate::cluster::redis::Redis::connect_within(&cluster.url(), "opencalc-test:promote")
            .await
            .expect("the sentinels named a primary");

    let before = store
        .claim("doc".to_owned(), "node-a".to_owned(), 60_000, 0)
        .await
        .expect("a lease before the failover");
    for at in 0..3u64 {
        assert_eq!(
            store
                .append(
                    "doc".to_owned(),
                    before.epoch,
                    at,
                    at + 1,
                    format!("before-{at}").into_bytes(),
                    0,
                )
                .await,
            Ok(at + 1),
            "the log must be working before anything is taken away"
        );
    }

    let was = cluster.primary().await.expect("a primary to start with");
    if let Err(why) = cluster.promote_the_replica().await {
        panic!("no sentinel would start a failover, so there is nothing to survive: {why}");
    }
    let now = cluster
        .moved_away_from(was)
        .await
        .expect("the sentinels never promoted the replica, so this is not a failover test");
    assert_ne!(now, was);
    assert!(
        cluster.demoted(was).await,
        "the old primary was never reconfigured as a replica, so this test would be \
         asserting against a node that is still accepting writes"
    );

    let after = coordinates_again(&store, "doc", 1)
        .await
        .expect("the link never found the new primary, so this node can never order again");
    assert_eq!(
        (after.node.as_str(), after.epoch),
        (before.node.as_str(), before.epoch),
        "the lease is the coordinator's memory of who leads, and a failover must not \
         invent a new one"
    );

    // **No revision a client was given is absent from the log.** A promoted
    // replica missing the last appends is precisely the loss ADR-020 names, and
    // asserting only that ordering resumed would pass straight through it.
    assert_eq!(
        store.since("doc".to_owned(), 0).await,
        Ok(vec![
            (1, b"before-0".to_vec()),
            (2, b"before-1".to_vec()),
            (3, b"before-2".to_vec()),
        ]),
        "the new primary is missing appends this node already acknowledged"
    );
    assert_eq!(
        store
            .append("doc".to_owned(), after.epoch, 3, 4, b"after".to_vec(), 1)
            .await,
        Ok(4),
        "and appending works again — which is ordering working"
    );
}

/// The same gate with the primary **killed**, which is what actually happens.
///
/// Demotion is the subtler case; this is the literal one. The sentinels notice,
/// promote, and the node has to find the new address without anybody restarting
/// it — the thing a `ConnectionManager` alone cannot do, because it re-dials the
/// address it was given and that address is a corpse.
#[tokio::test]
async fn ordering_resumes_after_the_coordinator_primary_is_killed() {
    let Some(mut cluster) = SentinelCluster::start("killed", &[]).await else {
        eprintln!("skipped: needs `redis-server` and `redis-sentinel` on PATH");
        return;
    };
    let store =
        crate::cluster::redis::Redis::connect_within(&cluster.url(), "opencalc-test:killed")
            .await
            .expect("the sentinels named a primary");

    let before = store
        .claim("doc".to_owned(), "node-a".to_owned(), 60_000, 0)
        .await
        .expect("a lease before the kill");
    assert_eq!(
        store
            .append("doc".to_owned(), before.epoch, 0, 1, b"before".to_vec(), 0)
            .await,
        Ok(1)
    );

    let was = cluster.kill_the_primary().await.expect("a primary to kill");
    cluster
        .moved_away_from(was)
        .await
        .expect("the sentinels never promoted anything, so there was no failover to survive");

    let after = coordinates_again(&store, "doc", 1).await.expect(
        "the link never found the promoted primary, so every edit this node takes \
         is one it must refuse for the rest of its life",
    );
    assert_eq!(after.node, "node-a");
    assert_eq!(
        store
            .append("doc".to_owned(), after.epoch, 1, 2, b"after".to_vec(), 1)
            .await,
        Ok(2),
        "and appending works again — which is ordering working"
    );
}

/// **A write with the in-sync set collapsed is refused, not accepted and lost.**
///
/// This is the half of ADR-020 that is not about availability. Redis replication
/// is asynchronous: without `min-replicas-to-write` the primary acknowledges an
/// append no replica has, and a promotion a moment later loses it — from a
/// screen that shows it as saved, which is the "silent data loss with a receipt"
/// ADR-014 refuses.
///
/// With the setting, the same moment is a **refusal**: the primary answers
/// `NOREPLICAS`, the script never reaches its `RPUSH`, and the node reports
/// `Refused { NotSaving }` (DEP-04). Three things are asserted, and the second is
/// the one that makes it a durability test rather than an error-message test:
///
/// 1. the append is refused;
/// 2. the log did **not** grow — nothing was written that could be lost;
/// 3. the refusal is a *state*, not a wedge: redundancy returns and so does
///    ordering.
#[tokio::test]
async fn a_write_with_the_in_sync_set_collapsed_is_refused_rather_than_lost() {
    // `min-replicas-max-lag` is generous because it is not what this exercises:
    // the replica is killed outright, so the primary loses the connection and
    // its count of good replicas drops at once rather than by lag.
    let floor = [
        "--min-replicas-to-write",
        "1",
        "--min-replicas-max-lag",
        "10",
        // Redis waits five seconds before a diskless full sync, to batch
        // replicas arriving together. Nothing here is about that wait.
        "--repl-diskless-sync-delay",
        "0",
    ];
    let Some(primary) = OwnRedis::start_as("insync-primary", &floor).await else {
        eprintln!("skipped: needs a `redis-server` on PATH");
        return;
    };
    let port = primary.port.to_string();
    let Some(mut replica) = OwnRedis::start_as(
        "insync-replica",
        &[
            "--repl-diskless-sync-delay",
            "0",
            "--replicaof",
            "127.0.0.1",
            &port,
        ],
    )
    .await
    else {
        eprintln!("skipped: needs a `redis-server` on PATH");
        return;
    };

    let store =
        crate::cluster::redis::Redis::connect_within(&primary.url(), "opencalc-test:insync")
            .await
            .expect("connected");
    // The floor is met while the replica is up, so the ordinary path works —
    // without which "refused" below would prove only that the server was broken.
    let lease = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            if let Ok(lease) = store
                .claim("doc".to_owned(), "node-a".to_owned(), 60_000, 0)
                .await
            {
                return lease;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the replica never came into sync, so the floor was never met");
    assert_eq!(
        store
            .append("doc".to_owned(), lease.epoch, 0, 1, b"in-sync".to_vec(), 0)
            .await,
        Ok(1),
        "with redundancy in place the append is ordinary"
    );

    replica.kill();

    let refused = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let outcome = store
                .append("doc".to_owned(), lease.epoch, 1, 2, b"alone".to_vec(), 1)
                .await;
            match outcome {
                Ok(_) => panic!(
                    "the append was ACCEPTED with no replica in sync: this is the write a \
                     failover loses, acknowledged to a client that will stop resending it"
                ),
                Err(AppendError::Unavailable(why)) if why.contains("min-replicas-to-write") => {
                    return why;
                }
                // The primary has not noticed the replica is gone yet. It is a
                // socket close rather than a lag window, so this is a moment.
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
            }
        }
    })
    .await
    .expect(
        "no refusal naming min-replicas-to-write arrived: either the coordinator is accepting \
         writes with no replica in sync, or the refusal is reaching an operator as an \
         unexplained server error",
    );
    assert!(
        refused.contains("did not happen"),
        "an operator reading this must learn the write did not land and that the replicas \
         are what to look at — not be sent to chase a network fault: {refused}"
    );

    // **Nothing was written.** The refusal is only worth having if the log is
    // where it was; a refusal on top of a write that landed would be the same
    // loss with a louder log line.
    assert_eq!(
        store.since("doc".to_owned(), 0).await,
        Ok(vec![(1, b"in-sync".to_vec())]),
        "the refused append reached the log after all"
    );

    // And it is a state, not a wedge: redundancy returns, ordering returns.
    assert!(replica.restart().await, "the replica came back");
    let resumed = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            if let Ok(revision) = store
                .append("doc".to_owned(), lease.epoch, 1, 2, b"together".to_vec(), 2)
                .await
            {
                return revision;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("ordering never resumed once the replica was back, so the refusal was a wedge");
    assert_eq!(resumed, 2);
}

/// A coordinator that is not configured to hold the floor is refused at startup.
///
/// The failure this prevents has no symptom until it is too late: a deployment
/// sets `OPENCALC_REDIS_MIN_REPLICAS=1`, believes ADR-014 §4's promise holds
/// across a failover, and is pointed at a primary where nobody set
/// `min-replicas-to-write`. Every append is then accepted, and the first
/// failover eats whichever ones had not replicated — with receipts.
#[tokio::test]
async fn a_coordinator_below_the_required_replica_floor_is_refused() {
    let Some(server) = OwnRedis::start("floor").await else {
        eprintln!("skipped: needs a `redis-server` on PATH");
        return;
    };
    let wanted = crate::cluster::redis::LinkPolicy {
        tls: crate::cluster::redis::LinkTls::default(),
        min_replicas: 1,
    };
    let why = crate::cluster::redis::Redis::connect_under(&server.url(), "floor-test", &wanted)
        .await
        .expect_err(
            "a coordinator that will accept writes it can lose must not be adopted by a node \
             that asked for the opposite",
        );
    assert!(
        why.0.contains("min-replicas-to-write"),
        "and it names the setting to change: {why}"
    );

    // Unset is unchecked, which is the default and the behaviour every
    // deployment had before ADR-020 — the check must not become a new way to
    // fail to start.
    assert!(
        crate::cluster::redis::Redis::connect_under(
            &server.url(),
            "floor-test",
            &crate::cluster::redis::LinkPolicy::default()
        )
        .await
        .is_ok(),
        "an unset floor must leave a single-node coordinator working"
    );
}

/// A promoted primary that does not hold the floor is **not adopted**.
///
/// The mistake this catches is the common one: `min-replicas-to-write` set on
/// the primary and nowhere else. The startup check passes, the deployment
/// believes it is safe, and the first failover promotes a node that will happily
/// accept writes it can lose — silently, because from the node's side the
/// failover looks like a success.
///
/// So the floor is re-checked on every resolution, and a primary below it leaves
/// the node refusing. Refusing is an answer; `NotSaving` and a 503 are what
/// DEP-04 built for exactly this.
#[tokio::test]
async fn a_promoted_primary_below_the_replica_floor_is_not_adopted() {
    let Some(mut cluster) = SentinelCluster::start_split(
        "floor-failover",
        &[
            "--min-replicas-to-write",
            "1",
            "--min-replicas-max-lag",
            "10",
        ],
        &[],
    )
    .await
    else {
        eprintln!("skipped: needs `redis-server` and `redis-sentinel` on PATH");
        return;
    };
    let wanted = crate::cluster::redis::LinkPolicy {
        tls: crate::cluster::redis::LinkTls::default(),
        min_replicas: 1,
    };
    let store = crate::cluster::redis::Redis::connect_under(
        &cluster.url(),
        "opencalc-test:floor-failover",
        &wanted,
    )
    .await
    .expect("the original primary holds the floor");

    let was = cluster.kill_the_primary().await.expect("a primary to kill");
    cluster
        .moved_away_from(was)
        .await
        .expect("the sentinels never promoted anything");

    // Long enough that a link which *was* going to adopt the new primary has
    // done so several times over: the resolution happens on the first failed
    // command, and commands are being issued in this loop.
    let adopted = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if store
                .claim("doc".to_owned(), "node-a".to_owned(), 60_000, 0)
                .await
                .is_ok()
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await;
    assert!(
        adopted.is_err(),
        "the node resumed against a promoted primary with no durability floor, so it is \
         acknowledging appends the next failover can lose — which is the silent loss this \
         setting exists to refuse"
    );
}

// --- Placement (DEP-09) -----------------------------------------------------
//
// `announce` published this node's load every five seconds from the day the
// cluster was built, and nothing ever read it back. A full node refused an
// arrival while the node beside it sat idle.

#[test]
fn the_least_loaded_peer_with_room_takes_the_client() {
    let peers = vec![peer("node-a", 9), peer("node-b", 2), peer("node-c", 7)];
    assert_eq!(place(&peers, "node-a", 10).unwrap().id, "node-b");
}

/// **Never back to the node that just refused.**
///
/// A client that obeys a redirect to the node it came from is a client that
/// never stops. `node-a` is the least loaded here, and is still not the answer.
#[test]
fn a_node_never_redirects_to_itself() {
    let peers = vec![peer("node-a", 0), peer("node-b", 4)];
    assert_eq!(
        place(&peers, "node-a", 10).unwrap().id,
        "node-b",
        "the refusing node sent the client back to itself"
    );
}

/// A node with no public address is not somewhere a browser can go. Sending
/// somebody to a service name on the cluster network is worse than refusing
/// them: it fails in the browser with nothing to explain it.
#[test]
fn a_peer_with_no_public_address_is_never_named() {
    let peers = vec![unreachable("node-b", 0), peer("node-c", 8)];
    assert_eq!(
        place(&peers, "node-a", 10).unwrap().id,
        "node-c",
        "the idle node has no public address and cannot be the answer"
    );
    // And when it is the *only* peer, the answer is nobody rather than it.
    assert!(place(&[unreachable("node-b", 0)], "node-a", 10).is_none());
}

/// A peer already at the cap would refuse in turn, so it is not an answer.
#[test]
fn a_peer_that_is_also_full_is_not_an_answer() {
    let peers = vec![peer("node-b", 10), peer("node-c", 10)];
    assert!(place(&peers, "node-a", 10).is_none());
    // One under the cap is.
    let peers = vec![peer("node-b", 10), peer("node-c", 9)];
    assert_eq!(place(&peers, "node-a", 10).unwrap().id, "node-c");
}

/// Two nodes refusing at the same moment send their clients to the *same*
/// place, rather than splitting them across a tie by iteration order.
#[test]
fn a_tie_places_deterministically() {
    let forwards = vec![peer("node-b", 3), peer("node-c", 3), peer("node-d", 3)];
    let mut backwards = forwards.clone();
    backwards.reverse();
    assert_eq!(
        place(&forwards, "node-a", 10).unwrap().id,
        place(&backwards, "node-a", 10).unwrap().id
    );
    assert_eq!(place(&forwards, "node-a", 10).unwrap().id, "node-b");
}

/// A single-node deployment has nowhere to send anybody, and must say so
/// rather than naming itself.
#[test]
fn a_lone_node_places_nobody() {
    assert!(place(&[peer("node-a", 99)], "node-a", 10).is_none());
    assert!(place(&[], "node-a", 10).is_none());
}
