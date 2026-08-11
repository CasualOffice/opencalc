//! Serving clients on a document this node does not lead.
//!
//! [ADR-017](../../../docs/63-COLLABORATION-RELAY.md) is the decision; this is
//! the shape of it, and the shape is smaller than it sounds because of one
//! choice: **every node holding a document is a replica, and the leader is only
//! the writer.**
//!
//! # One path, not two
//!
//! A committed batch is published to the document's channel and every node
//! applies it from there — *including the node that wrote it*. There is no
//! local shortcut for one's own edits, which is what keeps "an operation was
//! committed" a single code path rather than one for local and one for remote.
//! Two paths for the same event is where the two would drift apart, and the
//! drift is silent.
//!
//! It also removes a message type. A batch says who wrote it — the client and
//! its sequence number — so a node seeing one already knows whether to
//! acknowledge (that client is mine) or to apply (it is not). No acknowledgement
//! has to be routed back to the node that forwarded, so no routing for it can be
//! wrong.
//!
//! # The channel is a prompt; the log is the authority
//!
//! Pub/sub loses messages and does not say so. A node therefore tracks the
//! revision it has applied and refuses a batch that does not follow directly
//! from it, catching up from the log instead. Fire-and-forget is acceptable
//! *because* of that check, and would be silent divergence without it.

use serde::{Deserialize, Serialize};

/// A submission a relay is asking the leader to order.
///
/// Carries who wrote it because the leader's answer is published to everybody
/// and has to say whose it was — the node holding that client is the one that
/// acknowledges, and it recognises its own by this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Forwarded {
    /// The document's session key.
    pub document: String,
    /// The submission exactly as the client sent it.
    ///
    /// Not re-based, not re-numbered, not re-signed. A relay is a pipe: the
    /// moment it starts adjusting what it carries, its adjustments become a
    /// second implementation of ordering, running on a node that does not have
    /// the log.
    pub submission: casual_calc_transaction::session::Submission,
}

/// A batch the leader has committed, for every node to apply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Committed {
    /// The revision the document reached.
    pub revision: u64,
    /// Who wrote it, so exactly one node acknowledges and the rest apply.
    pub client: casual_calc_transaction::session::ClientId,
    /// The sequence number to acknowledge, cumulative as always.
    pub seq: u64,
    /// The operations **as they landed**, rebased onto whatever had been
    /// committed since the writer's base.
    pub ops: Vec<casual_calc_transaction::wire::WireOperation>,
}

/// Where a document's traffic goes.
///
/// Namespaced with the coordinator's prefix for the reason the keys are: one
/// Redis is routinely shared, and two deployments publishing to one channel
/// would apply each other's edits to documents that merely have the same name.
#[must_use]
pub fn inbox_channel(namespace: &str, document: &str) -> String {
    format!("{namespace}:inbox:{document}")
}

/// Where committed batches are published.
#[must_use]
pub fn committed_channel(namespace: &str, document: &str) -> String {
    format!("{namespace}:committed:{document}")
}

/// What a node should do with a batch that has arrived on the channel.
///
/// Separated from the doing so it can be tested as a decision rather than as a
/// side effect. Gap handling is the part that is easy to get subtly wrong and
/// impossible to notice: applying out of order does not fail, it diverges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    /// It follows what this node has. Apply it.
    Apply,
    /// It is behind — already seen, a duplicate delivery. Ignore it.
    Seen,
    /// It is ahead: something was missed. Read the log from `from` instead.
    ///
    /// Never applied anyway "to catch up faster": the operations in between are
    /// what the missing ones were transformed against, and applying this batch
    /// without them puts it at coordinates that were never real.
    CatchUp {
        /// The revision this node has, which is where the log must be read from.
        from: u64,
    },
}

/// Decide what to do with a batch at `revision`, given what this node has.
///
/// A batch may carry several operations, so it advances the revision by more
/// than one — `applied` is where the node is, `revision` is where the batch
/// leaves it, and anything that starts after where the node is has a gap in
/// front of it.
#[must_use]
pub fn react(applied: u64, revision: u64, ops: usize) -> Reaction {
    let starts_at = revision.saturating_sub(ops as u64);
    if revision <= applied {
        Reaction::Seen
    } else if starts_at == applied {
        Reaction::Apply
    } else {
        Reaction::CatchUp { from: applied }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_that_follows_directly_is_applied() {
        assert_eq!(react(4, 5, 1), Reaction::Apply);
        assert_eq!(react(4, 7, 3), Reaction::Apply, "however many it carries");
    }

    #[test]
    fn a_batch_already_applied_is_ignored_rather_than_applied_twice() {
        // Redis will redeliver, and a node that reconnects its subscription can
        // see the same batch again. Applying an insert-rows a second time is
        // corruption, not a glitch.
        assert_eq!(react(5, 5, 1), Reaction::Seen);
        assert_eq!(react(9, 4, 1), Reaction::Seen);
    }

    #[test]
    fn a_batch_with_a_gap_in_front_of_it_sends_the_node_to_the_log() {
        // The important one. Pub/sub loses messages silently, so this is the
        // only thing standing between a dropped publication and two documents
        // that quietly disagree.
        assert_eq!(react(4, 9, 1), Reaction::CatchUp { from: 4 });
        assert_eq!(
            react(0, 3, 1),
            Reaction::CatchUp { from: 0 },
            "a node that has just opened the document is behind like any other"
        );
    }

    #[test]
    fn a_gap_is_never_closed_by_applying_the_newer_batch_first() {
        // Stated as a test because the tempting optimisation — take this one
        // now, fetch the missing ones after — is wrong in a way nothing would
        // report. The operations in between are what this batch was transformed
        // against; without them it lands at coordinates that were never real.
        let Reaction::CatchUp { from } = react(2, 8, 2) else {
            panic!("a gap is a catch-up, never an apply")
        };
        assert_eq!(from, 2, "and it reads from where this node actually is");
    }

    #[test]
    fn a_document_is_namespaced_so_two_deployments_do_not_share_a_channel() {
        assert_ne!(
            committed_channel("prod", "doc-1"),
            committed_channel("staging", "doc-1")
        );
        assert_ne!(
            inbox_channel("prod", "doc-1"),
            committed_channel("prod", "doc-1"),
            "and the two directions are separate channels"
        );
    }
}
