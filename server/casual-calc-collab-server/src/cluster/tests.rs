//! Cluster tests: the failures that appear once a quarter, in production, at
//! three in the morning — made to happen on demand.
//!
//! Every one of these is a timing bug in disguise, which is why time is an
//! argument throughout. A test that has to sleep to reach a lease expiry is a
//! test that is slow *and* flaky; these reach it by saying so.

use super::*;

const TTL: u64 = 1_000;

fn peer(id: &str, load: u32) -> Peer {
    Peer {
        id: id.to_owned(),
        advertise: format!("10.0.0.1:944{}", id.len()),
        load,
        seen_ms: 0,
    }
}

// --- Leadership -------------------------------------------------------------

#[test]
fn the_first_claimant_leads() {
    let c = Memory::default();
    let lease = c.claim("doc", "node-a", TTL, 0);
    assert_eq!(lease.node, "node-a");
    assert_eq!(lease.epoch, 1);
    assert_eq!(lease.expires_ms, TTL);
}

#[test]
fn a_second_node_is_told_who_leads_rather_than_taking_it() {
    // Returning the *holder's* lease rather than an error is what lets the
    // caller relay: it needs to know who leads, not merely that it does not.
    let c = Memory::default();
    c.claim("doc", "node-a", TTL, 0);
    let lease = c.claim("doc", "node-b", TTL, 100);
    assert_eq!(lease.node, "node-a", "leadership did not move");
    assert_eq!(lease.epoch, 1, "and the epoch did not move either");
}

#[test]
fn renewing_does_not_move_the_epoch() {
    // A renewal is not a change of leadership. Bumping the epoch here would
    // fence the holder against its own appends, which is the subtlest way to
    // break this: everything looks alive and nothing can be written.
    let c = Memory::default();
    let first = c.claim("doc", "node-a", TTL, 0);
    let renewed = c.claim("doc", "node-a", TTL, 500);
    assert_eq!(renewed.epoch, first.epoch);
    assert_eq!(renewed.expires_ms, 1_500, "but the expiry moves");
}

#[test]
fn a_lapsed_lease_is_taken_by_the_next_claimant_and_the_epoch_moves() {
    let c = Memory::default();
    c.claim("doc", "node-a", TTL, 0);
    let taken = c.claim("doc", "node-b", TTL, TTL + 1);
    assert_eq!(taken.node, "node-b");
    assert_eq!(taken.epoch, 2, "a change of hands raises the fence");
}

#[test]
fn leadership_of_one_document_says_nothing_about_another() {
    let c = Memory::default();
    c.claim("doc-1", "node-a", TTL, 0);
    let other = c.claim("doc-2", "node-b", TTL, 0);
    assert_eq!(other.node, "node-b");
    assert_eq!(other.epoch, 1, "each document has its own generation");
}

// --- Fencing: the reason the cheap lease is safe ----------------------------

#[test]
fn a_zombie_leaders_appends_are_refused() {
    // The failure this whole design is arranged around. Node A holds the lease
    // and is alive and working; its lease expires anyway because the node was
    // busy, or the network hiccuped, or the clock moved. Node B takes over. A
    // does not know, and keeps writing.
    let c = Memory::default();
    let a = c.claim("doc", "node-a", TTL, 0);
    let b = c.claim("doc", "node-b", TTL, TTL + 1);
    assert_eq!(b.epoch, a.epoch + 1);

    assert_eq!(
        c.append("doc", a.epoch, 0, b"from the zombie".to_vec(), 0),
        Err(AppendError::Fenced { current: b.epoch }),
        "the old leader must be refused, and told which epoch replaced it"
    );
    assert!(
        c.append("doc", b.epoch, 0, b"from the new leader".to_vec(), 0)
            .is_ok(),
        "while the new one proceeds"
    );
}

#[test]
fn the_fence_is_checked_before_the_revision() {
    // A zombie whose revision happens to line up must still be refused. Telling
    // it `Stale` would send it to re-read the log and try again — forever, and
    // politely.
    let c = Memory::default();
    let a = c.claim("doc", "node-a", TTL, 0);
    c.claim("doc", "node-b", TTL, TTL + 1);
    assert!(matches!(
        c.append("doc", a.epoch, 0, b"x".to_vec(), 0),
        Err(AppendError::Fenced { .. })
    ));
}

#[test]
fn an_append_to_a_document_nobody_leads_is_refused() {
    let c = Memory::default();
    assert_eq!(
        c.append("doc", 1, 0, b"x".to_vec(), 0),
        Err(AppendError::Unled)
    );
}

// --- Conditional append: why divergence is impossible -----------------------

#[test]
fn an_append_against_a_stale_revision_is_refused_with_the_real_one() {
    // Not a leadership problem: the leader is right and merely behind. It is
    // told where the log actually is so it can catch up, which is the
    // difference between a recoverable state and a stuck one.
    let c = Memory::default();
    let lease = c.claim("doc", "node-a", TTL, 0);
    assert_eq!(c.append("doc", lease.epoch, 0, b"one".to_vec(), 0), Ok(1));
    assert_eq!(
        c.append("doc", lease.epoch, 0, b"again".to_vec(), 0),
        Err(AppendError::Stale { current: 1 })
    );
    assert_eq!(c.append("doc", lease.epoch, 1, b"two".to_vec(), 0), Ok(2));
}

#[test]
fn the_log_reads_back_in_order_from_any_point() {
    let c = Memory::default();
    let lease = c.claim("doc", "node-a", TTL, 0);
    for (i, payload) in [b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]
        .into_iter()
        .enumerate()
    {
        c.append("doc", lease.epoch, i as u64, payload, 0).unwrap();
    }
    assert_eq!(
        c.since("doc", 0),
        vec![(1, b"a".to_vec()), (2, b"b".to_vec()), (3, b"c".to_vec())]
    );
    assert_eq!(c.since("doc", 2), vec![(3, b"c".to_vec())]);
    assert!(c.since("doc", 3).is_empty());
    assert!(c.since("unknown", 0).is_empty());
}

#[test]
fn a_failover_loses_nothing_that_was_acknowledged() {
    // The promise ADR-014 makes by appending before acknowledging: whatever the
    // old leader was told had landed is still there for the new one.
    let c = Memory::default();
    let a = c.claim("doc", "node-a", TTL, 0);
    c.append("doc", a.epoch, 0, b"acknowledged".to_vec(), 0)
        .unwrap();

    let b = c.claim("doc", "node-b", TTL, TTL + 1);
    assert_eq!(
        c.since("doc", 0),
        vec![(1, b"acknowledged".to_vec())],
        "the new leader replays what the old one committed"
    );
    assert_eq!(
        c.append("doc", b.epoch, 1, b"and continues".to_vec(), 0),
        Ok(2)
    );
}

// --- Discovery --------------------------------------------------------------

#[test]
fn a_node_that_stops_announcing_itself_is_forgotten() {
    let c = Memory::default();
    c.register(peer("node-a", 0), 5_000, 0);
    c.register(peer("node-b", 0), 5_000, 0);
    assert_eq!(c.peers(1_000).len(), 2);

    // node-a keeps talking; node-b does not.
    c.register(peer("node-a", 0), 5_000, 14_000);
    let alive = c.peers(20_000);
    assert_eq!(alive.len(), 1);
    assert_eq!(alive[0].id, "node-a");
}

#[test]
fn re_announcing_updates_rather_than_duplicates() {
    let c = Memory::default();
    c.register(peer("node-a", 5), 5_000, 0);
    c.register(peer("node-a", 1), 5_000, 100);
    let peers = c.peers(200);
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].load, 1, "the newer announcement wins");
}

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

#[test]
fn no_interleaving_of_two_nodes_can_produce_two_writers() {
    // The safety property stated as a search rather than an example. Two nodes
    // claim and append at every offset across a lease lifetime; at no point may
    // both succeed at the same revision, because that is what divergence is.
    let mut accepted_at = std::collections::BTreeMap::<u64, String>::new();

    for step in 0..40u64 {
        let c = Memory::default();
        let now = step * 100;

        let a = c.claim("doc", "node-a", TTL, 0);
        let b = c.claim("doc", "node-b", TTL, now);

        let from_a = c.append("doc", a.epoch, 0, b"a".to_vec(), now);
        let from_b = c.append("doc", b.epoch, 0, b"b".to_vec(), now);

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
    let first_b = sequence.iter().position(|w| *w == "b");
    if let Some(at) = first_b {
        assert!(
            sequence[at..].iter().all(|w| *w == "b"),
            "leadership oscillated: {sequence:?}"
        );
    }
}

// --- Failover happens without anybody deciding anything ---------------------

#[test]
fn a_relay_takes_over_by_claiming_periodically_rather_than_by_declaring_a_death() {
    // There is no failure detector. A node that wants the document simply calls
    // `claim` on a timer: while the lease is held it is told who holds it and
    // relays there, and once the lease has lapsed the same call takes it over.
    // The changeover is a consequence of an atomic operation, not a decision.
    let c = Memory::default();
    c.claim("doc", "leader", TTL, 0);

    // The relay claims every 200ms. While the leader keeps renewing, the relay
    // is told "not you" every time, and never has to conclude anything.
    let mut relayed_to = Vec::new();
    for step in 1..=4u64 {
        let now = step * 200;
        c.claim("doc", "leader", TTL, now); // the leader renews
        relayed_to.push(c.claim("doc", "relay", TTL, now).node);
    }
    assert!(
        relayed_to.iter().all(|who| who == "leader"),
        "a live leader keeps it: {relayed_to:?}"
    );

    // The leader stops renewing — for whatever reason, which nobody
    // investigates. The relay's next scheduled claim, past the expiry, takes it.
    let taken = c.claim("doc", "relay", TTL, 800 + TTL + 1);
    assert_eq!(taken.node, "relay");
    assert_eq!(
        taken.epoch, 2,
        "and it is a new generation, so the old one is fenced"
    );
}

#[test]
fn two_nodes_claiming_at_the_same_instant_do_not_both_win() {
    // The race a heartbeat-and-vote design has to solve and this one does not
    // have: the store settles it, and the loser is *told who won* rather than
    // being left to find out.
    let c = Memory::default();
    let first = c.claim("doc", "node-a", TTL, 5_000);
    let second = c.claim("doc", "node-b", TTL, 5_000);
    assert_eq!(first.node, "node-a");
    assert_eq!(second.node, "node-a", "the loser learns who leads");
    assert_eq!(first.epoch, second.epoch);
}

#[test]
fn a_partition_that_heals_leaves_the_stale_side_fenced_rather_than_writing() {
    // Both sides of a partition see the other's silence. In a design where a
    // replica declares the leader dead, both promote and both write. Here the
    // one that lost the lease is refused as soon as it tries.
    let c = Memory::default();
    let isolated = c.claim("doc", "node-a", TTL, 0);
    let promoted = c.claim("doc", "node-b", TTL, TTL + 1);

    // The partition heals. Node A never learned anything and carries on.
    assert!(matches!(
        c.append(
            "doc",
            isolated.epoch,
            0,
            b"from the isolated side".to_vec(),
            TTL + 2
        ),
        Err(AppendError::Fenced { .. })
    ));
    assert!(
        c.append(
            "doc",
            promoted.epoch,
            0,
            b"from the promoted side".to_vec(),
            TTL + 2
        )
        .is_ok()
    );
}
