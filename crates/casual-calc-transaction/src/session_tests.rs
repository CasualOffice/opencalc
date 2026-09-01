//! The protocol end to end: several clients, a server, no network.
//!
//! Convergence is asserted over *interleavings*, not over single exchanges.
//! Pairwise TP1 says two operations commute correctly; it says nothing about a
//! client that edits three times while two acknowledgements and someone else's
//! insert are in flight. That is where protocols actually break, and it costs
//! nothing to run here because none of this touches a socket.

use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

use crate::session::MAX_FRAME_BYTES;
use crate::{
    Operation,
    session::{Base, ClientId, ClientSession, Commit, ServerSession, Submission},
    wire::WireOperation,
};

fn seed() -> Workbook {
    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    for row in 0..8u32 {
        for col in 0..3u32 {
            sheet.cells.set(
                CellRef::new(row, col),
                Cell::value(CellValue::Number(f64::from(row * 10 + col))),
            );
        }
    }
    workbook.sheets.push(sheet);
    workbook
}

/// What two replicas have to agree on.
///
/// **The formula, not only the value.** A cell holds its formula by handle and
/// its value is a cache nothing here recalculates, so comparing values alone
/// called two clients converged while they held different formulas — which is
/// exactly what `COL-46` was, and this harness could not have seen it. The tree
/// is compared in its stored form, relative to its own cell, and handles are
/// deliberately *not* compared: each replica interns into its own arena, so the
/// same formula legitimately has different handle numbers.
fn observe(workbook: &Workbook) -> String {
    let sheet = &workbook.sheets[0];
    let mut cells: Vec<String> = sheet
        .cells
        .iter()
        .map(|(at, cell)| {
            let formula = cell
                .formula
                .and_then(|handle| workbook.formula(handle))
                .map_or_else(String::new, |expr| format!("={expr:?}"));
            format!("{}:{}={:?}{formula}", at.row, at.col, cell.value)
        })
        .collect();
    cells.sort();
    format!("{} cols{:?}", cells.join(","), sheet.columns.sizes)
}

fn write(row: u32, col: u32, n: f64) -> Operation {
    Operation::SetCell {
        sheet: 0,
        at: CellRef::new(row, col),
        cell: Some(Cell::value(CellValue::Number(n))),
    }
}

/// One client and its own copy of the document.
struct Peer {
    session: ClientSession,
    workbook: Workbook,
}

impl Peer {
    fn new(base: &Workbook, id: u64) -> Self {
        Self {
            session: ClientSession::new(ClientId(id), 0),
            workbook: base.clone(),
        }
    }
}

/// A server, its document, and the clients attached to it.
struct World {
    server: ServerSession,
    workbook: Workbook,
    peers: Vec<Peer>,
    /// Chunks sent but not yet delivered to the server, oldest first — the
    /// network, modelled as a queue we can reorder deliberately.
    inflight: Vec<(usize, Submission)>,
}

impl World {
    fn new(peers: usize) -> Self {
        let workbook = seed();
        Self {
            server: ServerSession::new(),
            peers: (0..peers)
                .map(|i| Peer::new(&workbook, i as u64 + 1))
                .collect(),
            workbook,
            inflight: Vec::new(),
        }
    }

    fn edit(&mut self, peer: usize, op: Operation) {
        let peer = &mut self.peers[peer];
        peer.session
            .edit(&mut peer.workbook, op)
            .expect("local edit");
    }

    /// Send whatever `peer` has pending, if it is allowed to.
    fn send(&mut self, peer: usize) {
        let book = self.peers[peer].workbook.clone();
        if let Some(submission) = self.peers[peer].session.flush(&book) {
            self.inflight.push((peer, submission));
        }
    }

    /// Deliver the `nth` deliverable chunk to the server and broadcast the
    /// result.
    ///
    /// "Deliverable" means the *oldest* chunk still in flight from its client.
    /// One client's chunks travel over one connection and therefore arrive in
    /// the order they were sent — a guarantee the protocol now relies on, since
    /// a chained chunk names no revision and is resolved from where its
    /// predecessor landed.
    ///
    /// Reordering *between* clients stays completely free, which is where the
    /// interesting interleavings are: two participants racing is the case OT
    /// exists for, and one participant's own chunks overtaking each other is
    /// not a thing a network does.
    fn deliver(&mut self, nth: usize) {
        if self.inflight.is_empty() {
            return;
        }
        let mut seen = std::collections::BTreeSet::new();
        let heads: Vec<usize> = self
            .inflight
            .iter()
            .enumerate()
            .filter(|(_, (peer, _))| seen.insert(*peer))
            .map(|(index, _)| index)
            .collect();
        let (from, submission) = self.inflight.remove(heads[nth % heads.len()]);
        let outcome = self
            .server
            .commit(&mut self.workbook, &submission)
            .expect("server commits");
        let (committed, revision) = match outcome {
            Commit::Applied { ops, revision } => (ops, revision),
            Commit::Duplicate { revision } => (Vec::new(), revision),
        };

        for (index, peer) in self.peers.iter_mut().enumerate() {
            if index == from {
                peer.session.acknowledge(submission.seq, revision);
            } else {
                for op in &committed {
                    peer.session
                        .receive(&mut peer.workbook, op, revision)
                        .expect("remote applies");
                }
            }
        }
    }

    /// Drain everything until nothing is outstanding anywhere.
    fn settle(&mut self) {
        for _ in 0..64 {
            for peer in 0..self.peers.len() {
                self.send(peer);
            }
            if self.inflight.is_empty() {
                break;
            }
            self.deliver(0);
        }
        assert!(self.inflight.is_empty(), "the network never drained");
        assert!(
            self.peers.iter().all(|p| !p.session.has_unacknowledged()),
            "a client still holds unacknowledged work"
        );
    }

    fn assert_converged(&self, label: &str) {
        let authoritative = observe(&self.workbook);
        for (index, peer) in self.peers.iter().enumerate() {
            assert_eq!(
                observe(&peer.workbook),
                authoritative,
                "{label}: client {index} diverged from the server"
            );
        }
    }
}

#[test]
fn two_clients_editing_different_cells_converge() {
    let mut world = World::new(2);
    world.edit(0, write(0, 0, 100.0));
    world.edit(1, write(5, 2, 200.0));
    world.send(0);
    world.send(1);
    world.settle();
    world.assert_converged("disjoint edits");
}

#[test]
fn two_clients_editing_the_same_cell_converge_on_one_of_them() {
    let mut world = World::new(2);
    world.edit(0, write(1, 1, 111.0));
    world.edit(1, write(1, 1, 222.0));
    world.send(0);
    world.send(1);
    world.settle();
    world.assert_converged("same cell");

    // Whoever the server ordered last is what everyone sees — but everyone
    // sees the *same* one, which is the property that matters.
    let value = observe(&world.workbook);
    assert!(
        value.contains("1:1=Number(222.0)") || value.contains("1:1=Number(111.0)"),
        "one of the two writes survived intact: {value}"
    );
}

#[test]
fn an_edit_below_a_concurrent_insert_lands_on_the_right_row() {
    let mut world = World::new(2);
    world.edit(
        0,
        Operation::InsertRows {
            sheet: 0,
            at: 0,
            count: 2,
        },
    );
    world.edit(1, write(4, 0, 999.0));
    world.send(0);
    world.send(1);
    world.settle();
    world.assert_converged("insert vs edit");

    // Row 4 was pushed to row 6, and the edit has to have followed it.
    assert!(
        observe(&world.workbook).contains("6:0=Number(999.0)"),
        "the edit followed its row: {}",
        observe(&world.workbook)
    );
}

/// **`COL-46` through the protocol, which is where it would have been paid
/// for.** One participant types `=$C$1`; another inserts a column at the front
/// in the same moment.
///
/// Both edits commit — this was never a refusal — and before the fix the two
/// documents held `$D$1` and `$C$1` at the same address, for ever, with the
/// server agreeing with one of them and nothing anywhere reporting a problem.
#[test]
fn a_formula_typed_beside_a_concurrent_insert_means_the_same_thing_everywhere() {
    let mut world = World::new(2);
    let at = CellRef::new(1, 0);
    let handle = world.peers[1].workbook.store_formula_at(
        casual_calc_formula::parse("$C$1").expect("parses"),
        casual_calc_formula::stored::Origin::at(at.row, at.col),
    );
    let mut cell = Cell::value(CellValue::Number(0.0));
    cell.formula = Some(handle);

    world.edit(
        0,
        Operation::InsertColumns {
            sheet: 0,
            at: 0,
            count: 1,
        },
    );
    world.edit(
        1,
        Operation::SetCell {
            sheet: 0,
            at,
            cell: Some(cell),
        },
    );
    world.send(0);
    world.send(1);
    world.settle();
    world.assert_converged("a formula beside an insert");

    // And what everybody agrees on is the *right* thing: the column that held
    // `C1`'s data is `D1` once a column is inserted in front of it.
    let seen = observe(&world.workbook);
    assert!(
        seen.contains("col: 3, row: 0, col_absolute: true"),
        "the anchor must follow the column it named: {seen}"
    );
}

/// **`COL-44` through the protocol.** One participant drags a column header
/// while another types into a cell the drag renumbers.
///
/// Every one of these exchanges used to fail: the server refused the drag, the
/// dragging client could not take the keystroke, and the two documents stayed
/// different for ever because a resend was refused for the same reason each
/// time. Both edits now commit, and the keystroke lands in the column it was
/// typed into rather than in whatever moved into that index.
#[test]
fn a_column_drag_and_a_concurrent_keystroke_both_land() {
    let mut world = World::new(2);
    // Column 0 is dragged to sit after column 2, so what was column 1 becomes
    // column 0 and what was column 2 becomes column 1.
    world.edit(
        0,
        Operation::MoveColumns {
            sheet: 0,
            at: 0,
            count: 1,
            before: 3,
        },
    );
    world.edit(1, write(4, 2, 555.0));
    world.send(0);
    world.send(1);
    world.settle();
    world.assert_converged("a drag beside a keystroke");

    let seen = observe(&world.workbook);
    assert!(
        seen.contains("4:1=Number(555.0)"),
        "the keystroke follows the column it was typed into: {seen}"
    );
    // And the drag really happened: row 0 read 0,1,2 and now reads 1,2,0.
    assert!(
        seen.contains("0:0=Number(1.0)") && seen.contains("0:2=Number(0.0)"),
        "the columns were reordered: {seen}"
    );
}

#[test]
fn a_client_keeps_editing_while_its_chunk_is_in_flight() {
    // Edits made *after* sending are rebased onto whatever the server accepted
    // meanwhile.
    //
    // This used to assert the opposite of what it asserts now — that nothing
    // further could be sent until the first chunk came back. ADR-016 removed
    // that rule, so what is worth checking is that a second chunk *can* go out
    // and that everybody still converges, which is the property the rule was
    // protecting and the one that has to survive without it.
    let mut world = World::new(2);
    world.edit(0, write(0, 0, 1.0));
    world.send(0); // in flight, unacknowledged

    world.edit(0, write(0, 1, 2.0));
    world.edit(0, write(0, 2, 3.0));
    world.send(0); // and so is this one, chained behind it

    world.edit(
        1,
        Operation::InsertRows {
            sheet: 0,
            at: 0,
            count: 1,
        },
    );
    world.send(1);

    world.settle();
    world.assert_converged("pipelined edits");
}

#[test]
fn convergence_survives_every_delivery_order() {
    // The interleaving matters, so try them all rather than the one that
    // happened to be written first. Each run edits identically and differs
    // only in the order the server sees the chunks.
    let mut results = Vec::new();
    for order in 0..6usize {
        let mut world = World::new(3);
        world.edit(0, write(2, 0, 10.0));
        world.edit(
            1,
            Operation::DeleteRows {
                sheet: 0,
                at: 1,
                count: 2,
            },
        );
        world.edit(2, write(6, 1, 30.0));
        world.edit(
            2,
            Operation::SetColumnWidth {
                sheet: 0,
                col: 1,
                width: Some(150),
            },
        );
        for peer in 0..3 {
            world.send(peer);
        }
        // A different rotation of the queue each time.
        world.deliver(order % 3);
        world.deliver(order / 3);
        world.settle();
        world.assert_converged(&format!("order {order}"));
        results.push(observe(&world.workbook));
    }

    // Different orders may legitimately produce different documents — that is
    // what "the server decides" means. What must never happen is a *client*
    // disagreeing with its server, which `assert_converged` checked for each.
    assert_eq!(results.len(), 6);
}

#[test]
fn an_edit_to_a_deleted_row_becomes_nothing_rather_than_landing_elsewhere() {
    let mut world = World::new(2);
    world.edit(
        0,
        Operation::DeleteRows {
            sheet: 0,
            at: 3,
            count: 2,
        },
    );
    world.edit(1, write(4, 0, 555.0));
    world.send(0);
    world.send(1);
    world.settle();
    world.assert_converged("edit into a deleted band");

    assert!(
        !observe(&world.workbook).contains("Number(555.0)"),
        "the row it was written to is gone, so the edit is gone: {}",
        observe(&world.workbook)
    );
}

#[test]
fn a_client_too_far_behind_is_told_rather_than_dropped() {
    // The bounded-offline edge. Its work is refused with the range it needed,
    // not silently discarded, so the host can say what happened.
    let mut world = World::new(2);
    world.edit(0, write(0, 0, 1.0));
    world.send(0);
    world.deliver(0);
    world.edit(0, write(0, 1, 2.0));
    world.send(0);
    world.deliver(0);

    world.server.compact_to(world.server.revision());

    let stale = Submission {
        client: ClientId(9),
        seq: 1,
        base: Base::Revision(0),
        ops: vec![WireOperation::of(
            write(7, 2, 42.0),
            &Workbook::new(Id::from_parts(1, 1)),
        )],
    };
    let error = world
        .server
        .commit(&mut world.workbook, &stale)
        .expect_err("a base older than the retained history is refused");
    assert!(matches!(
        error,
        crate::session::SessionError::UnknownRevision { claimed: 0, .. }
    ));
}

#[test]
fn a_refused_chunk_leaves_the_document_untouched() {
    let mut world = World::new(1);
    let before = observe(&world.workbook);
    let stale = Submission {
        client: ClientId(9),
        seq: 1,
        base: Base::Revision(99),
        ops: vec![WireOperation::of(
            write(0, 0, 1.0),
            &Workbook::new(Id::from_parts(1, 1)),
        )],
    };
    assert!(world.server.commit(&mut world.workbook, &stale).is_err());
    assert_eq!(observe(&world.workbook), before, "nothing was half-applied");
    assert_eq!(world.server.revision(), 0);
}

#[test]
fn pending_edits_are_rebased_before_they_are_ever_sent() {
    // The client's own buffer has to move when a remote arrives, and this is
    // the arrangement that proves it. If the client flushes *before* the remote
    // lands, its chunk carries a stale base and the server rebases it on the
    // client's behalf — masking a client that never rebases its own buffer at
    // all. Here the remote lands first, so the chunk goes out based on the
    // post-remote revision and the server has nothing to correct with: the
    // buffer must already be right.
    let mut world = World::new(2);
    world.edit(0, write(5, 0, 777.0)); // pending, deliberately not sent

    world.edit(
        1,
        Operation::InsertRows {
            sheet: 0,
            at: 0,
            count: 3,
        },
    );
    world.send(1);
    world.deliver(0);
    assert_eq!(
        world.peers[0].session.revision(),
        world.server.revision(),
        "the client has caught up before it sends"
    );

    world.send(0);
    world.settle();
    world.assert_converged("pending rebased before sending");
    assert!(
        observe(&world.workbook).contains("8:0=Number(777.0)"),
        "row 5 became row 8: {}",
        observe(&world.workbook)
    );
}

#[test]
fn a_multi_operation_chunk_is_threaded_through_the_history() {
    // Within one chunk, the second operation was written against the state the
    // first produced. Rebasing both onto the same unchanged history lands the
    // second one a whole operation out of place — the same threading bug as in
    // `Batch`, one layer up, and it only shows when the chunk has more than one
    // positional operation *and* there is concurrent history to rebase onto.
    let mut world = World::new(2);

    world.edit(
        1,
        Operation::InsertColumns {
            sheet: 0,
            at: 0,
            count: 1,
        },
    );
    world.send(1);

    world.edit(
        0,
        Operation::InsertRows {
            sheet: 0,
            at: 2,
            count: 2,
        },
    );
    world.edit(0, write(4, 0, 4242.0)); // addresses a row the insert above created
    world.send(0);

    world.settle();
    world.assert_converged("multi-op chunk against concurrent history");
    assert!(
        observe(&world.workbook).contains("4:1=Number(4242.0)"),
        "the write kept its row and followed the inserted column: {}",
        observe(&world.workbook)
    );
}

#[test]
fn concurrent_history_advances_past_each_operation_in_a_chunk() {
    // The other half of the threading, and the half that hides. Rebasing a
    // chunk's second operation onto the history as it was — rather than as it
    // is after the chunk's *first* operation — is only visible when the history
    // operation itself moves, which needs both on the same axis and the second
    // operation landing in the span the first one opened.
    //
    // Here: the remote inserts one row at 1. This client inserts three at 0 and
    // writes into row 2 — its own new row. Rebasing that write against the
    // remote's *original* position at 1 would push it to row 3, into a row it
    // never meant to touch. Against the remote's position after the three-row
    // insert, at 4, it correctly stays put.
    let mut world = World::new(2);

    world.edit(
        0,
        Operation::InsertRows {
            sheet: 0,
            at: 0,
            count: 3,
        },
    );
    world.edit(0, write(2, 0, 1234.0));
    world.send(0);

    world.edit(
        1,
        Operation::InsertRows {
            sheet: 0,
            at: 1,
            count: 1,
        },
    );
    world.send(1);

    world.deliver(1); // the remote is ordered first
    world.deliver(0); // then the chunk, rebased onto it
    world.settle();

    world.assert_converged("history threaded through a chunk");
    assert!(
        observe(&world.workbook).contains("2:0=Number(1234.0)"),
        "the write stayed in the row this client opened: {}",
        observe(&world.workbook)
    );
}

// ---------------------------------------------------------------------------
// Snapshots: the cadence, the retained window, and verification.
// ---------------------------------------------------------------------------

use crate::session::{Snapshot, SnapshotPolicy};

#[test]
fn a_snapshot_round_trips_the_document() {
    let mut world = World::new(1);
    world.edit(0, write(2, 1, 55.0));
    world.send(0);
    world.deliver(0);

    let snap = Snapshot::capture(&world.workbook, world.server.revision()).unwrap();
    assert_eq!(observe(&snap.restore().unwrap()), observe(&world.workbook));
}

#[test]
fn replaying_the_log_reproduces_a_later_snapshot_byte_for_byte() {
    // The integrity check the deterministic snapshot format hands us for free:
    // a stored snapshot can be verified instead of trusted.
    let mut world = World::new(1);
    let base = Snapshot::capture(&world.workbook, 0).unwrap();

    for row in 0..6u32 {
        world.edit(0, write(row, 0, f64::from(row) + 0.25));
        world.send(0);
        world.deliver(0);
    }
    world.edit(
        0,
        Operation::InsertRows {
            sheet: 0,
            at: 1,
            count: 2,
        },
    );
    world.send(0);
    world.deliver(0);

    let later = Snapshot::capture(&world.workbook, world.server.revision()).unwrap();
    world
        .server
        .verify_snapshot(&base, &later)
        .expect("replay reproduces it");
}

#[test]
fn a_tampered_snapshot_is_caught_rather_than_trusted() {
    let mut world = World::new(1);
    let base = Snapshot::capture(&world.workbook, 0).unwrap();
    world.edit(0, write(0, 0, 1.0));
    world.send(0);
    world.deliver(0);

    // A stored snapshot that does not match what the log produces: the document
    // and its history have diverged, and that is corruption however it happened.
    let mut tampered = Snapshot::capture(&world.workbook, world.server.revision()).unwrap();
    world.edit(0, write(9, 9, 12345.0));
    world.send(0);
    world.deliver(0);
    tampered.bytes = Snapshot::capture(&world.workbook, tampered.revision)
        .unwrap()
        .bytes;

    assert!(matches!(
        world.server.verify_snapshot(&base, &tampered),
        Err(crate::session::SessionError::SnapshotMismatch { .. })
    ));
}

#[test]
fn the_cadence_fires_on_the_configured_interval() {
    let mut world = World::new(1);
    let policy = SnapshotPolicy {
        every: 3,
        retain_intervals: 2,
    };
    assert!(!world.server.snapshot_due(None, policy));

    for row in 0..3u32 {
        world.edit(0, write(row, 0, 1.0));
        world.send(0);
        world.deliver(0);
    }
    assert!(
        world.server.snapshot_due(None, policy),
        "three revisions in"
    );

    let snap = Snapshot::capture(&world.workbook, world.server.revision()).unwrap();
    assert!(
        !world.server.snapshot_due(Some(&snap), policy),
        "and not again until three more"
    );
}

#[test]
fn compaction_keeps_the_margin_that_defines_bounded_offline() {
    let mut world = World::new(1);
    let policy = SnapshotPolicy {
        every: 2,
        retain_intervals: 2,
    };
    for row in 0..10u32 {
        world.edit(0, write(row, 0, 1.0));
        world.send(0);
        world.deliver(0);
    }
    let snap = Snapshot::capture(&world.workbook, world.server.revision()).unwrap();
    world.server.compact_behind(&snap, policy);

    // Four revisions of margin — two intervals of two — are still rebasable, so
    // a client that stepped away briefly is not forced to reload.
    assert_eq!(world.server.oldest_rebasable(), 10 - 4);
    assert!(world.server.history_since(6).is_some());
    assert!(
        world.server.history_since(5).is_none(),
        "anything older is past the edge, and says so"
    );
}

#[test]
fn a_client_past_the_retained_window_is_refused_with_the_range_it_needed() {
    let mut world = World::new(1);
    let policy = SnapshotPolicy {
        every: 2,
        retain_intervals: 1,
    };
    for row in 0..8u32 {
        world.edit(0, write(row, 0, 1.0));
        world.send(0);
        world.deliver(0);
    }
    let snap = Snapshot::capture(&world.workbook, world.server.revision()).unwrap();
    world.server.compact_behind(&snap, policy);

    let stale = Submission {
        client: ClientId(9),
        seq: 1,
        base: Base::Revision(1),
        ops: vec![WireOperation::of(
            write(0, 2, 7.0),
            &Workbook::new(Id::from_parts(1, 1)),
        )],
    };
    match world.server.commit(&mut world.workbook, &stale) {
        Err(crate::session::SessionError::UnknownRevision {
            claimed,
            oldest,
            current,
        }) => {
            assert_eq!(claimed, 1);
            assert_eq!(current, 8);
            assert!(
                oldest > claimed,
                "the client is told what it would have needed"
            );
        }
        other => panic!("expected a refusal naming the range, got {other:?}"),
    }
}

#[test]
fn a_cold_start_resumes_from_a_snapshot_and_keeps_serving() {
    // What a hibernating serverless object actually does: the memory is gone,
    // so the document is rebuilt from its snapshot and the session carries on
    // at the revision it left off at.
    let mut world = World::new(1);
    for row in 0..4u32 {
        world.edit(0, write(row, 0, f64::from(row)));
        world.send(0);
        world.deliver(0);
    }
    let snap = Snapshot::capture(&world.workbook, world.server.revision()).unwrap();
    let before = observe(&world.workbook);

    // ... the object sleeps and wakes with nothing.
    let mut revived = snap.restore().unwrap();
    let mut server = ServerSession::resumed_at(snap.revision);
    assert_eq!(observe(&revived), before, "rehydrated to where it was");

    let submission = Submission {
        client: ClientId(9),
        seq: 1,
        base: Base::Revision(server.revision()),
        ops: vec![WireOperation::of(
            write(7, 0, 99.0),
            &Workbook::new(Id::from_parts(1, 1)),
        )],
    };
    server
        .commit(&mut revived, &submission)
        .expect("serving again");
    assert_eq!(server.revision(), snap.revision + 1);
    assert!(observe(&revived).contains("7:0=Number(99.0)"));
}

// ---------------------------------------------------------------------------
// Idempotency (COL-09): surviving a leader that commits and then dies before
// acknowledging.
// ---------------------------------------------------------------------------

#[test]
fn a_resent_chunk_is_committed_once() {
    let mut world = World::new(1);
    world.edit(0, write(3, 0, 42.0));
    let submission = world.peers[0]
        .session
        .flush(&world.workbook.clone())
        .expect("a chunk to send");

    let first = world
        .server
        .commit(&mut world.workbook, &submission)
        .unwrap();
    let Commit::Applied { revision, .. } = first else {
        panic!("the first delivery applies");
    };

    // The acknowledgement is lost with the leader; the client sends again.
    let second = world
        .server
        .commit(&mut world.workbook, &submission)
        .unwrap();
    assert_eq!(
        second,
        Commit::Duplicate { revision },
        "recognised, and landing where it originally did"
    );
    assert_eq!(world.server.revision(), revision, "nothing advanced");
}

#[test]
fn a_resent_structural_op_does_not_insert_twice() {
    // The case that makes this required rather than tidy. A repeated value is
    // invisible; a repeated row insert moves every row below it.
    let mut world = World::new(1);
    world.edit(
        0,
        Operation::InsertRows {
            sheet: 0,
            at: 2,
            count: 1,
        },
    );
    let submission = world.peers[0]
        .session
        .flush(&world.workbook.clone())
        .unwrap();

    world
        .server
        .commit(&mut world.workbook, &submission)
        .unwrap();
    let after_once = observe(&world.workbook);
    world
        .server
        .commit(&mut world.workbook, &submission)
        .unwrap();

    assert_eq!(
        observe(&world.workbook),
        after_once,
        "the second delivery changed nothing"
    );
}

#[test]
fn resend_reuses_the_sequence_but_follows_the_clients_revision() {
    let mut world = World::new(2);
    world.edit(0, write(0, 0, 1.0));
    let sent = world.peers[0]
        .session
        .flush(&world.workbook.clone())
        .unwrap();

    // A remote lands while the chunk is in flight, so the client rebases it.
    world.edit(
        1,
        Operation::InsertRows {
            sheet: 0,
            at: 0,
            count: 1,
        },
    );
    world.send(1);
    world.deliver(0);

    let book = world.peers[0].workbook.clone();
    let outstanding = world.peers[0].session.resend(&book);
    let again = outstanding.first().expect("still outstanding");
    assert_eq!(again.seq, sent.seq, "the same chunk, so the same sequence");
    assert_eq!(again.client, sent.client);
    assert!(
        matches!((again.base, sent.base), (Base::Revision(now), Base::Revision(was)) if now > was),
        "but based on where the client now is: {:?} then {:?}",
        sent.base,
        again.base
    );
    assert_ne!(again.ops, sent.ops, "and carrying the rebased operations");
}

#[test]
fn two_clients_sequences_do_not_collide() {
    let mut world = World::new(2);
    world.edit(0, write(0, 0, 1.0));
    world.edit(1, write(5, 0, 2.0));
    let a = world.peers[0]
        .session
        .flush(&world.workbook.clone())
        .unwrap();
    let b = world.peers[1]
        .session
        .flush(&world.workbook.clone())
        .unwrap();
    assert_eq!(a.seq, b.seq, "both are each client's first chunk");
    assert_ne!(a.client, b.client);

    world.server.commit(&mut world.workbook, &a).unwrap();
    let second = world.server.commit(&mut world.workbook, &b).unwrap();
    assert!(
        matches!(second, Commit::Applied { .. }),
        "a different client's first chunk is not a duplicate"
    );
}

#[test]
fn a_successor_leader_without_the_record_would_double_apply() {
    // Why the acceptance record is durable session state and not a cache. The
    // promotion path has to carry it; this is what happens when it does not.
    let mut world = World::new(1);
    world.edit(
        0,
        Operation::InsertRows {
            sheet: 0,
            at: 1,
            count: 1,
        },
    );
    let submission = world.peers[0]
        .session
        .flush(&world.workbook.clone())
        .unwrap();
    world
        .server
        .commit(&mut world.workbook, &submission)
        .unwrap();
    let record = world.server.accepted().clone();
    let after_once = observe(&world.workbook);

    // A successor that carries the record recognises the resend.
    let mut good = ServerSession::resumed_at(world.server.revision());
    good.restore_accepted(record);
    let mut doc = world.workbook.clone();
    assert!(matches!(
        good.commit(&mut doc, &submission).unwrap(),
        Commit::Duplicate { .. }
    ));
    assert_eq!(observe(&doc), after_once);
}

#[test]
fn a_resend_is_recognised_even_after_its_base_was_compacted_away() {
    // A resend arriving late can name a revision the server has since dropped.
    // Refusing it would tell a client its committed work was lost, so the
    // duplicate check runs before the base is validated.
    let mut world = World::new(1);
    world.edit(0, write(1, 0, 7.0));
    let submission = world.peers[0]
        .session
        .flush(&world.workbook.clone())
        .unwrap();
    let Commit::Applied { revision, .. } = world
        .server
        .commit(&mut world.workbook, &submission)
        .unwrap()
    else {
        panic!("applied");
    };

    for row in 5..12u32 {
        let op = write(row, 0, f64::from(row));
        crate::apply(&mut world.workbook, op.clone()).unwrap();
        world
            .server
            .commit(
                &mut world.workbook.clone(),
                &Submission {
                    client: ClientId(2),
                    seq: u64::from(row),
                    base: Base::Revision(world.server.revision()),
                    ops: vec![WireOperation::of(op, &world.workbook)],
                },
            )
            .unwrap();
    }
    world.server.compact_to(world.server.revision());

    assert_eq!(
        world
            .server
            .commit(&mut world.workbook, &submission)
            .unwrap(),
        Commit::Duplicate { revision },
        "recognised rather than refused for a base that is long gone"
    );
}

#[test]
fn a_formula_typed_by_one_client_arrives_intact_at_another() {
    // COL-12 end to end. Each replica has its own formula arena, and the
    // sender's is deliberately further along than the receiver's, so a bare
    // handle would name the wrong entry rather than merely a missing one.
    let mut world = World::new(2);
    world.peers[0]
        .workbook
        .store_formula(casual_calc_formula::parse("111").unwrap());
    world.peers[0]
        .workbook
        .store_formula(casual_calc_formula::parse("222").unwrap());

    let handle = world.peers[0]
        .workbook
        .store_formula(casual_calc_formula::parse("6*7").unwrap());
    let mut cell = Cell::value(CellValue::Number(42.0));
    cell.formula = Some(handle);
    world.edit(
        0,
        Operation::SetCell {
            sheet: 0,
            at: CellRef::new(3, 1),
            cell: Some(cell),
        },
    );

    world.settle();
    world.assert_converged("a formula crossing replicas");

    for (index, peer) in world.peers.iter().enumerate() {
        let landed = peer.workbook.sheets[0]
            .cells
            .get(CellRef::new(3, 1))
            .and_then(|c| c.formula)
            .and_then(|h| peer.workbook.formula(h))
            .cloned();
        assert_eq!(
            landed,
            Some(casual_calc_formula::parse("6*7").unwrap()),
            "client {index} has the expression, whatever index it took locally"
        );
    }

    let on_server = world.workbook.sheets[0]
        .cells
        .get(CellRef::new(3, 1))
        .and_then(|c| c.formula)
        .and_then(|h| world.workbook.formula(h))
        .cloned();
    assert_eq!(on_server, Some(casual_calc_formula::parse("6*7").unwrap()));
}

#[test]
fn a_style_applied_by_one_client_arrives_at_another() {
    let mut world = World::new(2);
    // The receiver's style table is not the sender's, so the id cannot travel.
    world.peers[1]
        .workbook
        .intern_style(casual_calc_model::Style {
            italic: true,
            ..Default::default()
        });
    let bold = casual_calc_model::Style {
        bold: true,
        ..Default::default()
    };
    let id = world.peers[0].workbook.intern_style(bold.clone());

    world.edit(
        0,
        Operation::SetStyle {
            sheet: 0,
            at: CellRef::new(2, 2),
            style: Some(id),
        },
    );
    world.settle();

    for (index, peer) in world.peers.iter().enumerate() {
        let landed = peer.workbook.sheets[0]
            .cells
            .get(CellRef::new(2, 2))
            .and_then(|c| c.style)
            .and_then(|s| peer.workbook.styles.get(s))
            .cloned();
        assert_eq!(landed, Some(bold.clone()), "client {index} sees it bold");
    }
}

// --- Resuming after a disconnect (ADR-015) ----------------------------------

/// The reason [`ClientSession::resume`] exists rather than reusing
/// [`ClientSession::new`].
///
/// A reconnecting client that started over would arrive with nothing
/// outstanding, silently dropping the edits made in the seconds before the
/// socket died — which are precisely the ones least likely to have been
/// acknowledged, and the ones the user most recently watched themselves type.
#[test]
fn resuming_keeps_the_work_a_fresh_start_would_have_thrown_away() {
    let mut workbook = seed();
    let mut client = ClientSession::new(ClientId(1), 0);

    client
        .edit(
            &mut workbook,
            Operation::SetCell {
                sheet: 0,
                at: CellRef::new(0, 0),
                cell: Some(Cell::value(CellValue::Number(99.0))),
            },
        )
        .unwrap();
    let sent = client.flush(&workbook).expect("a chunk to send");
    assert!(client.has_unacknowledged());

    // The socket dies here: sent, never acknowledged.
    client.resume(ClientId(1), 0);

    assert!(
        client.has_unacknowledged(),
        "the chunk in flight survives the reconnect"
    );
    let outstanding = client.resend(&workbook);
    let again = outstanding.first().expect("the same chunk again");
    assert_eq!(
        again.seq, sent.seq,
        "and with its original sequence number, which is what lets the server \
         recognise it instead of applying it twice"
    );
}

/// The other half: the numbering has to continue, not restart.
///
/// The server suppresses duplicates by `(client, seq)`. If a resumed session
/// numbered its next chunk 1 again, that genuinely new work would be discarded
/// as something already seen — a silent loss caused by the mechanism meant to
/// prevent one.
#[test]
fn resuming_continues_the_numbering_so_new_work_is_not_mistaken_for_old() {
    let mut workbook = seed();
    let mut client = ClientSession::new(ClientId(1), 0);

    let write = |client: &mut ClientSession, workbook: &mut Workbook, row: u32| {
        client
            .edit(
                workbook,
                Operation::SetCell {
                    sheet: 0,
                    at: CellRef::new(row, 0),
                    cell: Some(Cell::value(CellValue::Number(f64::from(row)))),
                },
            )
            .unwrap();
    };

    write(&mut client, &mut workbook, 1);
    let first = client.flush(&workbook).unwrap();
    client.acknowledge(1, 1);

    client.resume(ClientId(1), 1);

    write(&mut client, &mut workbook, 2);
    let after = client.flush(&workbook).expect("a second chunk");
    assert!(
        after.seq > first.seq,
        "a resumed session numbers onward ({} must exceed {})",
        after.seq,
        first.seq
    );
}

/// What the client does with the catch-up it is handed.
///
/// The missed operations are not merely applied: they rebase the client's own
/// outstanding chunk, so that when it is resent it is expressed against the
/// revision the server has actually reached.
#[test]
fn what_was_missed_rebases_the_chunk_that_is_about_to_be_resent() {
    let mut server_book = seed();
    let mut server = ServerSession::new();

    let mut mine = seed();
    let mut client = ClientSession::new(ClientId(1), 0);

    // I insert a row at the top and my chunk never arrives.
    client
        .edit(
            &mut mine,
            Operation::InsertRows {
                sheet: 0,
                at: 0,
                count: 1,
            },
        )
        .unwrap();
    let lost = client.flush(&mine).expect("a chunk");

    // Meanwhile somebody else writes a cell, and it is committed.
    let theirs = Submission {
        client: ClientId(2),
        seq: 1,
        base: Base::Revision(0),
        ops: vec![WireOperation::of(
            Operation::SetCell {
                sheet: 0,
                at: CellRef::new(4, 0),
                cell: Some(Cell::value(CellValue::Number(500.0))),
            },
            &server_book,
        )],
    };
    let Commit::Applied { ops, revision } = server.commit(&mut server_book, &theirs).unwrap()
    else {
        panic!("committed")
    };

    // I reconnect and am caught up.
    client.resume(ClientId(1), 0);
    for op in &ops {
        client.receive(&mut mine, op, revision).unwrap();
    }

    // Now the resend, which the server must be able to order.
    let outstanding = client.resend(&mine);
    let again = outstanding.first().expect("the chunk, rebased");
    assert_eq!(
        again.base,
        Base::Revision(revision),
        "written against where the document is now"
    );
    assert_eq!(again.seq, lost.seq, "and still the same chunk");
    let outcome = server.commit(&mut server_book, again);
    assert!(
        outcome.is_ok(),
        "a rebased resend is orderable: {outcome:?}"
    );

    // Both sides agree afterwards, which is the claim that matters.
    assert_eq!(observe(&mine), observe(&server_book));
}

// --- Pipelining and cumulative acknowledgement (ADR-016) ---------------------

/// The point of the whole change: a client does not stop to be acknowledged.
#[test]
fn several_chunks_can_be_in_flight_at_once_and_everyone_still_agrees() {
    let mut world = World::new(2);

    // Four chunks from one client with nothing acknowledged in between, while
    // the other inserts a row underneath them. Before ADR-016 the second, third
    // and fourth could not have been sent at all.
    for col in 0..4u32 {
        world.edit(0, write(0, col, f64::from(col)));
        world.send(0);
    }
    assert_eq!(world.inflight.len(), 4, "all of them are on the wire");

    world.edit(
        1,
        Operation::InsertRows {
            sheet: 0,
            at: 0,
            count: 1,
        },
    );
    world.send(1);

    world.settle();
    world.assert_converged("four chunks in flight against a concurrent insert");

    // And they landed where the insert put them, rather than where they were
    // written — which is what would have gone wrong had a chained chunk been
    // rebased against its own predecessor.
    let seen = observe(&world.workbook);
    for col in 0..4u32 {
        assert!(
            seen.contains(&format!("1:{col}=Number({col}.0)")),
            "chunk {col} followed the inserted row: {seen}"
        );
    }
}

/// A chained chunk must be rebased against what happened *concurrently*, and
/// not against its own predecessor — which it already contains.
///
/// This is the failure the `Base` enum exists to prevent, and it is silent: a
/// double-transformed operation lands at the wrong coordinates with no error
/// anywhere, leaving two documents that quietly disagree.
#[test]
fn a_chained_chunk_is_not_rebased_against_the_chunk_it_follows() {
    let mut world = World::new(1);

    world.edit(
        0,
        Operation::InsertRows {
            sheet: 0,
            at: 0,
            count: 1,
        },
    );
    world.send(0);
    // Written *after* the insert, so already in coordinates that include it.
    world.edit(0, write(3, 0, 42.0));
    world.send(0);
    world.settle();

    let seen = observe(&world.workbook);
    assert!(
        seen.contains("3:0=Number(42.0)"),
        "the second chunk stayed where it was written; shifting it again would \
         put it at row 4: {seen}"
    );
}

#[test]
fn an_acknowledgement_covers_every_chunk_up_to_it() {
    let mut workbook = seed();
    let mut client = ClientSession::new(ClientId(1), 0);

    for col in 0..3u32 {
        client.edit(&mut workbook, write(0, col, 1.0)).unwrap();
        client.flush(&workbook).expect("a chunk");
    }
    assert!(client.has_unacknowledged());

    // One acknowledgement, naming the last of them. Cumulative: the two before
    // it are covered without ever being named, which is what makes a lost
    // acknowledgement heal itself instead of stranding a chunk forever.
    client.acknowledge(3, 7);
    assert!(
        !client.has_unacknowledged(),
        "all three are settled by the one that names the last"
    );
    assert_eq!(client.revision(), 7);
}

#[test]
fn an_acknowledgement_that_covers_only_some_leaves_the_rest_outstanding() {
    let mut workbook = seed();
    let mut client = ClientSession::new(ClientId(1), 0);
    for col in 0..3u32 {
        client.edit(&mut workbook, write(0, col, 1.0)).unwrap();
        client.flush(&workbook).expect("a chunk");
    }

    client.acknowledge(1, 4);
    let still = client.resend(&workbook);
    assert_eq!(still.len(), 2, "the two the server has not confirmed");
    assert_eq!(still[0].seq, 2, "oldest first");
    assert_eq!(still[1].seq, 3);
    assert!(
        matches!(still[0].base, Base::Revision(4)),
        "the first names where the client now is: {:?}",
        still[0].base
    );
    assert_eq!(
        still[1].base,
        Base::Chained,
        "and the rest chain, as they did the first time"
    );
}

/// Pipelining without a bound is a client on a bad link turning into unbounded
/// memory here and unbounded queued work at the server.
#[test]
fn a_client_that_is_never_acknowledged_stops_making_new_chunks() {
    let mut workbook = seed();
    let mut client = ClientSession::new(ClientId(1), 0);

    let mut chunks = 0;
    for row in 0..200u32 {
        client.edit(&mut workbook, write(row, 0, 1.0)).unwrap();
        if client.flush(&workbook).is_some() {
            chunks += 1;
        }
    }

    assert!(
        chunks <= 32,
        "the queue is bounded, not merely large: {chunks}"
    );
    // Nothing was dropped: what could not be sent is still waiting to be.
    assert!(client.has_unacknowledged());
    client.acknowledge(chunks, 1);
    assert!(
        client.flush(&workbook).is_some(),
        "and it resumes once there is room"
    );
}

/// A chained chunk from a client the server has never accepted anything from
/// has nothing to chain to.
#[test]
fn a_chain_with_nothing_to_chain_to_is_refused_rather_than_guessed_at() {
    let mut workbook = seed();
    let mut server = ServerSession::new();
    let submission = Submission {
        client: ClientId(9),
        seq: 1,
        base: Base::Chained,
        ops: vec![WireOperation::of(write(0, 0, 1.0), &workbook)],
    };
    let outcome = server.commit(&mut workbook, &submission);
    assert!(
        outcome.is_err(),
        "inventing a base is how two documents quietly stop agreeing"
    );
}

// --- Adopting an already-ordered batch (ADR-017) -----------------------------

/// A relay's copy has to end up identical to the leader's.
///
/// The whole point of `adopt`: the batch arrives rebased, so it is applied as
/// it is. Transforming it again would rebase it past the operations it was
/// already rebased past — which does not fail, and produces two documents that
/// quietly disagree.
#[test]
fn a_relay_that_adopts_a_committed_batch_matches_the_leader_exactly() {
    let mut leader_book = seed();
    let mut leader = ServerSession::new();
    let mut relay_book = seed();
    let mut relay = ServerSession::new();

    // Two clients write concurrently against the same base, so the leader has
    // to rebase the second — which is what makes this batch different from
    // what its author sent, and therefore worth carrying rather than replaying.
    let first = Submission {
        client: ClientId(1),
        seq: 1,
        base: Base::Revision(0),
        ops: vec![WireOperation::of(
            Operation::InsertRows {
                sheet: 0,
                at: 0,
                count: 2,
            },
            &leader_book,
        )],
    };
    // Carrying *text*, not just a number. A number has no handles, so a batch
    // of them would pass whether or not the relay localised anything — and the
    // handles are the part that means something different on every replica.
    let text = leader_book.intern_string("written by somebody else");
    let second = Submission {
        client: ClientId(2),
        seq: 1,
        base: Base::Revision(0),
        ops: vec![WireOperation::of(
            Operation::SetCell {
                sheet: 0,
                at: CellRef::new(4, 0),
                cell: Some(Cell::value(CellValue::SharedString(text))),
            },
            &leader_book,
        )],
    };

    for submission in [&first, &second] {
        let Commit::Applied { ops, revision } = leader
            .commit(&mut leader_book, submission)
            .expect("committed")
        else {
            panic!("applied")
        };
        relay
            .adopt(&mut relay_book, &ops, revision)
            .expect("the relay adopts what the leader ordered");
    }

    assert_eq!(relay.revision(), leader.revision());
    // Compared as *text*, not as ids: the two replicas intern independently, so
    // the same word is a different id on each and matching ids would prove
    // nothing while matching words proves everything.
    let read = |book: &Workbook| {
        let cell = book.sheets[0].cells.get(CellRef::new(6, 0)).cloned();
        match cell.map(|c| c.value) {
            Some(CellValue::SharedString(id)) => book.strings.get(id).map(str::to_owned),
            other => panic!("expected the text, found {other:?}"),
        }
    };
    assert_eq!(
        read(&relay_book),
        Some("written by somebody else".to_owned()),
        "the relay resolved the text into its own table"
    );
    assert_eq!(read(&relay_book), read(&leader_book));
}

#[test]
fn a_batch_that_does_not_follow_directly_is_refused_rather_than_skipped_to() {
    // The failure this guards is silent. Applying a later batch without the one
    // before it lands operations at coordinates that were never real, because
    // the missing operations are what these were transformed against.
    let mut book = seed();
    let mut relay = ServerSession::new();
    let ops = vec![WireOperation::of(write(0, 0, 1.0), &book)];

    assert!(
        relay.adopt(&mut book, &ops, 5).is_err(),
        "a gap must send the caller to the log, not be skipped over"
    );
    assert_eq!(relay.revision(), 0, "and nothing was applied");
    assert!(
        relay.adopt(&mut book, &ops, 1).is_ok(),
        "while the next one is"
    );
}

#[test]
fn a_relay_records_what_it_adopted_so_a_reconnect_is_still_safe() {
    // A relay learns that a client's chunk was ordered by seeing the batch,
    // not by ordering it. Without recording that, a client reconnecting to
    // this node would resend a chunk this node has no record of, and the
    // duplicate suppression that makes reconnecting safe would not fire.
    let mut book = seed();
    let mut relay = ServerSession::new();
    let ops = vec![WireOperation::of(write(0, 0, 1.0), &book)];
    relay.adopt(&mut book, &ops, 1).unwrap();
    relay.note_accepted(ClientId(7), 3, 1);

    let resent = Submission {
        client: ClientId(7),
        seq: 3,
        base: Base::Revision(0),
        ops,
    };
    assert!(
        matches!(
            relay.commit(&mut book, &resent),
            Ok(Commit::Duplicate { revision: 1 })
        ),
        "recognised as something already ordered, not applied a second time"
    );
}

/// **`COL-47`.** What a client does with a chunk it cannot transform.
///
/// The row said one unmergeable arrival "blocks every later arrival". Measured,
/// it did not, and the truth is worse: the refusal left the client's own
/// outstanding edits half-rebased, and then let the *next*, non-contending
/// arrival straight through. So the client applied revision 2 without ever
/// applying revision 1 — two replicas holding different documents with nothing
/// anywhere saying so, which is exactly the class `COL-46` was. Being deaf
/// would have been the honest failure; this was the quiet one.
///
/// It now latches. The first refusal is reported as itself, everything after it
/// is [`SessionError::Desynced`], and both routes to the network are closed so
/// the divergence cannot be pushed into the shared document. Recovery is a
/// rejoin from a server snapshot, which needs nothing new on the wire.
#[test]
fn a_refused_arrival_stops_the_client_rather_than_letting_it_drift() {
    use crate::session::SessionError;

    // Eight columns, so two overlapping drags have somewhere to overlap. Move
    // versus move is the ordinary way to get a refusal — `COL-44` answers most
    // of the pairs and deliberately not this one.
    let mut wb = seed();
    for row in 0..8u32 {
        for col in 3..8u32 {
            wb.sheets[0].cells.set(
                CellRef::new(row, col),
                Cell::value(CellValue::Number(f64::from(row * 10 + col))),
            );
        }
    }

    let mut client = ClientSession::new(ClientId(1), 0);
    client
        .edit(&mut wb, write(0, 2, 42.0))
        .expect("a keystroke");
    client
        .edit(
            &mut wb,
            Operation::MoveColumns {
                sheet: 0,
                at: 2,
                count: 1,
                before: 0,
            },
        )
        .expect("a drag");
    client.flush(&wb).expect("the chunk goes out");

    // Somebody else's drag, committed at revision 1, that this one cannot be
    // rebased past.
    let contending = WireOperation::of(
        Operation::MoveColumns {
            sheet: 0,
            at: 1,
            count: 3,
            before: 7,
        },
        &wb,
    );
    assert!(
        matches!(
            client.receive(&mut wb, &contending, 1),
            Err(SessionError::Transform(_))
        ),
        "the pair has no transform, and that much was never in doubt"
    );
    assert!(client.is_desynced(), "so the session has stopped");

    // The measured failure, asserted as itself: an ordinary edit from a third
    // party, at revision 2, must NOT be applied. Before this fix it returned
    // `Ok(())` and landed on a document missing revision 1.
    let ordinary = WireOperation::of(write(7, 0, 999.0), &wb);
    assert_eq!(
        client.receive(&mut wb, &ordinary, 2),
        Err(SessionError::Desynced),
        "a later arrival must not be applied onto a document missing a committed revision"
    );

    // And nothing leaves this machine. `resend` carried the half-rebased chunks
    // straight back to the server, which is where a private divergence would
    // have become everybody's.
    assert!(
        client.resend(&wb).is_empty(),
        "a desynced client resends nothing"
    );
    client
        .edit(&mut wb, write(3, 3, 7.0))
        .expect("still typing");
    assert!(
        client.flush(&wb).is_none(),
        "and sends nothing new either, however much is typed into it"
    );

    // The work is kept rather than discarded, which is what lets a host say
    // what is about to be lost instead of losing it quietly.
    assert!(
        client.has_unacknowledged(),
        "the unsent work is still nameable at the rejoin"
    );

    // A resume is not a recovery: it asserts continuity, which is the thing
    // that is false here.
    client.resume(ClientId(1), 2);
    assert!(
        client.is_desynced(),
        "resuming a desynced session must not clear it"
    );
}

/// A paste too large for one frame is split, not sent as one that closes it.
///
/// `COL-63`. `flush` took every pending operation into a single chunk with no
/// byte bound. The transport's frame cap is 4 MiB and a frame over it is not
/// refused — the connection *closes*. At the 84 B per changed cell `SAVE-08`
/// measured, a restore or a paste of more than roughly 50,000 changed cells
/// reached it, and the work was then neither delivered nor kept: the user saw
/// collaboration drop and nothing said why.
///
/// The assertion is about the bytes on the wire, so it measures them. A count
/// of operations would be a restatement of the constant rather than a check of
/// it, and would keep passing if the wire form grew.
#[test]
fn a_submission_never_exceeds_the_frame_the_transport_will_carry() {
    let mut wb = seed();
    let mut client = ClientSession::new(ClientId(1), 0);

    // Comfortably past the cap: at ~84 B a cell this is several times 4 MiB,
    // so a single unbounded chunk could not have fitted under any plausible
    // envelope allowance.
    for i in 0..200_000u32 {
        client
            .edit(&mut wb, write(i % 60_000, i % 40, f64::from(i)))
            .expect("a local edit always applies");
    }

    let mut frames = 0;
    let mut ops_delivered = 0;
    // Acknowledged as they go, because `MAX_OUTSTANDING` would otherwise stop
    // the flush long before the pending edits ran out — this test is about the
    // size bound, and the in-flight bound is a different one.
    while let Some(submission) = client.flush(&wb) {
        let bytes = serde_json::to_vec(&submission).expect("a submission serialises");
        assert!(
            bytes.len() <= MAX_FRAME_BYTES,
            "frame {frames} is {} bytes, over the {MAX_FRAME_BYTES}-byte cap the \
             transport closes the connection for",
            bytes.len(),
        );
        frames += 1;
        ops_delivered += submission.ops.len();
        let seq = submission.seq;
        client.acknowledge(seq, seq);
        assert!(
            frames < 1_000,
            "not converging: {frames} frames and still going"
        );
    }

    assert!(
        frames > 1,
        "the premise is gone: {ops_delivered} operations fitted in one frame, so this \
         test is no longer exercising the split"
    );
    assert_eq!(
        ops_delivered, 200_000,
        "every edit is delivered across the frames, not dropped at the boundary"
    );
}
