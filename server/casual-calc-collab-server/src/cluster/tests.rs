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
    let url = std::env::var("OPENCALC_TEST_REDIS").ok()?;
    let namespace = format!(
        "opencalc-test:{}:{}:{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    crate::cluster::redis::Redis::connect_within(&url, &namespace)
        .await
        .ok()
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
    c.append(document.to_owned(), epoch, after, payload.to_vec(), now)
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
        load,
        seen_ms: 0,
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
