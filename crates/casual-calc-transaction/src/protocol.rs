//! What a client and a server say to each other.
//!
//! Plain data, in the engine rather than in the server crate, because **both
//! ends speak it** — the browser client is compiled to WebAssembly from these
//! same crates, and a protocol defined only on the server is a protocol the
//! client has to reimplement and keep in step by hand.
//!
//! There is no transport here. These are the messages; carrying them is
//! somebody else's job, and deliberately so — a WebSocket, a long poll and a
//! test harness passing values between two structs should all be moving the
//! same things.
//!
//! # Versioning
//!
//! [`PROTOCOL_VERSION`] is sent on the first message and checked before
//! anything else. Two peers that disagree stop immediately with
//! [`Refusal::ProtocolVersion`] rather than proceeding until a field is missing
//! and something more confusing goes wrong. It is separate from the model's
//! schema version: a document format can change without the conversation
//! changing, and the reverse.

use serde::{Deserialize, Serialize};

use crate::session::{ClientId, Submission};
use crate::wire::WireOperation;

/// The conversation's version. Bumped when a message changes shape.
///
/// 2 added [`Resume`] and [`ServerMessage::Resumed`]
/// ([ADR-015](../../../docs/61-COLLABORATION-RESUME.md)). 3 added
/// [`ClientMessage::Ping`] and [`ServerMessage::Pong`]. 4 added
/// [`ServerMessage::Opening`]. 5 made [`ServerMessage::Ack`] cumulative and
/// `Submission`'s base a [`Base`](crate::session::Base)
/// ([ADR-016](../../../docs/62-COLLABORATION-PIPELINING.md)).
///
/// **[`Draft`] did not move it, and that is a claim rather than an oversight.**
/// The rule in [62](../../../docs/62-COLLABORATION-PIPELINING.md) is to bump
/// when an old and a new peer would interpret the same message *differently*.
/// `editing` is an optional field on a message neither side reads with
/// `deny_unknown_fields`: an old peer skips it and reads the rest exactly as
/// before, and a new peer reading a message without it concludes "not typing",
/// which is what the sender means. Nobody is misled, so nobody is refused —
/// which matters, because a bump costs every unupgraded tab its session, and
/// spending that to add a cursor decoration would be the wrong trade. Verified,
/// both directions, by `a_peer_that_has_never_heard_of_drafts_still_reads_a_message_carrying_one`.
pub const PROTOCOL_VERSION: u32 = 7;

/// What somebody is typing, before they have decided to keep it.
///
/// **Presence, not an operation** — the line ADR-011 draws, and the reason this
/// type is here rather than in the op set. A half-typed cell has no inverse,
/// nothing depends on its history, and losing it costs nothing; putting it on
/// the operation wire would mean transforming it, ordering it, keeping it in
/// the applied log and offering to undo it, all for a string that will be
/// replaced by the next keystroke.
///
/// # Why the text travels, and not merely the cell
///
/// The cheaper design broadcasts only *where* somebody is editing, and peers
/// show "Ada is in B4". It is a real feature and it answers "am I about to
/// collide with somebody" — but it does not answer what was actually asked for,
/// which was to see the value appear while it is being typed. Watching a
/// colleague fill a column is the thing people mean by co-editing a
/// spreadsheet; being told they are busy in a cell is a lock indicator.
///
/// It costs a throttled message per keystroke instead of one per cell, which is
/// the same order as the selection presence already being sent while somebody
/// drags. And it means half-typed, possibly-abandoned text is visible to
/// others, which is a *product* decision made deliberately: it is what every
/// spreadsheet a user has met does, and a draft that vanishes on Escape is
/// exactly as ephemeral as the presence entry carrying it.
///
/// Two consequences fall out and are handled where they arise: the text is
/// bounded here, and it is untrusted everywhere it is displayed (SEC-001 — the
/// editor draws it onto the canvas and never into markup).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    /// The cell being typed into, as `[row, col]`, on the sheet the presence
    /// message names.
    ///
    /// Carried rather than inferred from the selection: while a formula is
    /// being written the selection wanders off to pick references, and the
    /// draft belongs to the cell the edit started in.
    pub at: [u32; 2],
    /// What has been typed so far, bounded by [`Self::MAX_TEXT`].
    pub text: String,
}

impl Draft {
    /// The most text a draft carries, in characters.
    ///
    /// A cell holds far more than this — the point is that a *preview* does
    /// not need to. Past a couple of hundred characters nothing is legible in a
    /// peer's cell anyway, while the message is sent again on every keystroke
    /// by a party nobody is obliged to trust.
    pub const MAX_TEXT: usize = 256;

    /// A draft of `text` at `row`, `col`, bounded.
    ///
    /// The only constructor, so there is no way to build one that is too long
    /// and then forget to check it.
    #[must_use]
    pub fn new(row: u32, col: u32, text: impl Into<String>) -> Self {
        Self {
            at: [row, col],
            text: text.into(),
        }
        .bounded()
    }

    /// The same draft, with over-long text cut back to [`Self::MAX_TEXT`].
    ///
    /// Applied again wherever one arrives from the network: a peer that does
    /// not bound its own is not a peer that gets to decide how much memory the
    /// roster spends, or how much text everybody else's grid has to draw.
    ///
    /// Cut on a **character** boundary. `String::truncate` panics mid-codepoint,
    /// so a byte-counted bound is a crash triggerable by typing an accent.
    #[must_use]
    pub fn bounded(mut self) -> Self {
        if let Some((end, _)) = self.text.char_indices().nth(Self::MAX_TEXT) {
            self.text.truncate(end);
        }
        self
    }
}

/// What a participant may do, as it travels on the wire.
///
/// Mirrors the server's own `Access`, and the two are kept in step by a `From`
/// implementation written as an **exhaustive match**: adding a variant to
/// either without deciding its counterpart will not compile. A duplicated enum
/// that can silently disagree is a shape this codebase has been bitten by.
///
/// Ordered `View < Comment < Edit`, which is what lets "reduce, never raise" be
/// expressed as a minimum rather than as a table somebody has to keep right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WireAccess {
    /// Read only.
    View,
    /// Read, and attach comments.
    Comment,
    /// Change it.
    Edit,
}

/// Why the server would not do something.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "camelCase")]
pub enum Refusal {
    /// The peers do not speak the same protocol.
    ProtocolVersion {
        /// What the server speaks.
        server: u32,
        /// What the client claimed.
        client: u32,
    },
    /// The token was missing, malformed, expired, or not for this document.
    ///
    /// Deliberately undetailed on the wire: which of those it was is useful to
    /// an operator in a log and useful to an attacker in a response.
    NotAuthorised,
    /// The client's permission does not allow editing.
    ReadOnlyAccess,
    /// The document is not accepting edits — its saves are failing.
    ///
    /// Distinct from [`Self::ReadOnlyAccess`]: one is about who you are, the
    /// other about what is wrong, and a client should say something different
    /// for each.
    NotSaving,
    /// The submission was based on a revision the server no longer retains, so
    /// it cannot be rebased. The client must reload.
    ///
    /// The bounded-offline edge from ADR-011, said out loud rather than
    /// swallowed.
    TooFarBehind {
        /// The oldest revision still rebasable.
        oldest: u64,
        /// Where the document is now.
        current: u64,
    },
    /// Two edits could not be merged. See `TransformError`.
    CannotMerge,
    /// The message could not be read at all.
    ///
    /// Distinct from [`Self::CannotMerge`], and the distinction cost a live
    /// debugging session (`COL-38`). A message the server cannot parse was
    /// answered with `CannotMerge`, which names the **transform** — the one
    /// part that was working. The socket was open, the heartbeat answered, the
    /// roster showed the participant present, and every edit they made went
    /// nowhere.
    ///
    /// A client seeing this should not retry the same message: it will not
    /// parse the second time either. That is the whole reason the two are
    /// different words.
    Malformed,
    /// The document could not be read or written.
    Broken,
}

/// Client to server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClientMessage {
    /// Reduce what another participant may do, for the life of this session.
    ///
    /// Only an **owner** — the token's `owner` claim — is obeyed; anyone else
    /// is refused. And it may only ever *reduce* what that participant's own
    /// token grants: the server takes the minimum of the two, so a compromised
    /// or buggy client cannot promote itself and the token stays the ceiling.
    /// Granting more access means minting a token, which is the existing path
    /// and involves the system of record (docs/72).
    ///
    /// Ephemeral by design. It lives as long as the document is resident; a
    /// document evicted for idleness comes back with the token's own
    /// permissions. Durable policy belongs to the host — ADR-012 is explicit
    /// that the server holds no per-document state — and the alternative is a
    /// collaboration server that has quietly become a permissions database
    /// with no backup, no audit and no owner.
    SetAccess {
        /// Whose access to reduce.
        client: ClientId,
        /// What they may do from now on.
        access: WireAccess,
    },
    /// The opening message. Nothing else is accepted before it.
    Join {
        /// The protocol the client speaks.
        protocol: u32,
        /// The host-signed token: who, which document, what permission, and
        /// where the server may fetch and send back. The server holds no
        /// per-document state, so the token is the whole contract.
        token: String,
        /// Set when this is a reconnect rather than a first join.
        ///
        /// Optional so a client that never disconnected, and a host that has
        /// not implemented resumption, both still work — they simply always
        /// start afresh, which is what happened before ADR-015.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resume: Option<Resume>,
    },
    /// A chunk of the client's own edits.
    ///
    /// One in flight at a time — the server does not require it, but a client
    /// that ignores the rule makes its own bookkeeping wrong, not the
    /// server's.
    Submit(Submission),
    /// Still here. Presence entries expire on silence, so this is what keeps
    /// one alive.
    ///
    /// Distinct from [`Ping`](Self::Ping), which they are easily confused with.
    /// A heartbeat is about **this participant** — it keeps a cursor on other
    /// people's screens while its user reads rather than types — and expects no
    /// answer. A ping is about **the connection**, and its whole purpose is the
    /// answer.
    Heartbeat,
    /// Is this connection still alive?
    ///
    /// The client's own liveness check, and the only one it has. The server
    /// already pings at the WebSocket level and a browser answers those without
    /// the page being told, which detects a dead client — and leaves the
    /// opposite case entirely uncovered.
    ///
    /// That case is not exotic. A **half-open** connection — a laptop that
    /// slept, a network that vanished, a load balancer that dropped the flow
    /// without a FIN — looks perfectly open to the browser holding it, for as
    /// long as the operating system takes to give up on the socket, which is
    /// measured in minutes. For all of those minutes the editor works, accepts
    /// typing, and sends into nothing. Nothing else notices: not the socket,
    /// not the heartbeat, not the flush, which cannot tell a message that was
    /// delivered from one that was written to a socket nobody is reading.
    ///
    /// So the client asks, and starts a clock. Silence past the deadline is the
    /// signal to reconnect — which, since ADR-015, costs nothing and loses
    /// nothing.
    Ping {
        /// Matched by the answer.
        ///
        /// Without it a late pong for an earlier ping satisfies the current
        /// one, and a connection that is dying slowly reads as healthy — which
        /// is precisely the connection this exists to catch.
        nonce: u64,
    },
    /// Where this participant is looking.
    ///
    /// Not an edit and never transformed: it is ephemeral, nothing depends on
    /// its history, and losing it costs nothing.
    Presence {
        /// The sheet being viewed.
        sheet: usize,
        /// The selected range, as `[r0, c0, r1, c1]`.
        selection: [u32; 4],
        /// What this participant is typing, if anything.
        ///
        /// Optional in the wire sense and in the human one. Absent means "not
        /// editing", which is also what a client too old to know about drafts
        /// says by omission — and it is how an **abandoned** edit is cleared:
        /// each participant owns exactly one presence entry that is overwritten
        /// wholesale, so a message with no draft in it leaves no ghost behind
        /// for anybody to have to remember to remove.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        editing: Option<Draft>,
    },
    /// Leaving deliberately, rather than by disconnecting.
    Leave,
}

/// A reconnecting client's claim to be who it was.
///
/// Not a credential. The token authorises; this only says *which participant*
/// of that user's this connection continues, so the server can hand back the
/// same [`ClientId`] and its duplicate suppression keeps working across the
/// gap. The server checks the recorded user matches the token's before honouring
/// it, which is what keeps a guessed key from being worth guessing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resume {
    /// The client's own opaque key, stable for the life of one editor.
    pub key: String,
    /// The revision it last saw, which is where its catch-up starts.
    pub revision: u64,
}

/// Server to client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerMessage {
    /// This participant's access changed while they were in the document.
    ///
    /// Sent to the affected participant, and the roster updates for everybody,
    /// so the change is visible rather than mysterious.
    ///
    /// **Being told is the point.** Somebody demoted mid-edit must learn it in
    /// words, not by discovering that a keystroke has silently stopped working
    /// — and their unsent local edits are not quietly dropped, because from the
    /// user's side that situation is identical to the unresumed reconnect this
    /// project already built a notice for.
    AccessChanged {
        /// What they may do now.
        access: WireAccess,
        /// Which participant changed it, so the message can name somebody.
        by: ClientId,
    },
    /// Authorised, and the document is being fetched.
    ///
    /// Sent as soon as the token checks out, **before** the document exists on
    /// this node. Opening one means fetching it from the integrator, and that
    /// is a request to somebody else's server: it can take as long as the
    /// configured timeout allows, and the first participant of the day pays for
    /// it every time.
    ///
    /// Without this the client sees nothing at all during that wait — an open
    /// socket and silence, which is indistinguishable from a server that has
    /// hung, and which a user reasonably responds to by reloading and starting
    /// the whole wait again. With it there is a difference between *slow* and
    /// *broken*, on both sides: the client can say "opening", and it can go on
    /// pinging, so it still knows whether the connection beneath the wait is
    /// alive.
    ///
    /// It is a promise of nothing except that the token was accepted. A
    /// [`Welcome`](Self::Welcome) or a [`Stopped`](Self::Stopped) follows.
    Opening {
        /// The document's name, so a client can title the window it is showing
        /// the wait in.
        title: String,
    },
    /// Joined. Carries the document and where in its history it is.
    Welcome {
        /// The protocol the server speaks.
        protocol: u32,
        /// The id this participant is known by, which its own submissions
        /// carry and which other participants see in presence.
        client: ClientId,
        /// The revision the snapshot is at.
        revision: u64,
        /// The document as a normalized-model snapshot — **not** the file.
        /// Everyone in a session must start from the same revision, and a
        /// client that fetched the document itself would arrive at revision
        /// zero while the session was at five hundred.
        snapshot: Vec<u8>,
        /// Whether this participant may edit.
        editable: bool,
    },
    /// Rejoined as the same participant, with what was missed.
    ///
    /// Sent instead of [`Welcome`](Self::Welcome) when a reconnecting client
    /// presented a [`Resume`] the server honoured. There is no snapshot: the
    /// client's document is continuous, and replacing it would discard exactly
    /// the unacknowledged work this message exists to preserve.
    Resumed {
        /// The protocol the server speaks.
        protocol: u32,
        /// The **same** id as before, which is what makes a resend of an
        /// already-committed chunk recognisable as a duplicate rather than a
        /// second edit.
        client: ClientId,
        /// The revision the document has reached, after `missed`.
        revision: u64,
        /// Whether this participant may edit. Re-stated rather than assumed:
        /// the token is verified again on reconnect and may say something
        /// different by then.
        editable: bool,
        /// Everything committed while this client was away, oldest first.
        ///
        /// Carried in this message rather than following it, so a client cannot
        /// resend its outstanding chunk before rebasing it past them — which
        /// would submit edits written against a revision the server has moved
        /// beyond.
        missed: Vec<WireOperation>,
    },
    /// Submissions were committed.
    ///
    /// **Cumulative**, as TCP's acknowledgement is: every sequence up to and
    /// including `through` has been ordered, not just that one. The server
    /// orders a client's chunks in sequence, so acknowledging one already
    /// implies the ones before it — saying so lets a lost or skipped
    /// acknowledgement heal itself on the next one, instead of leaving a chunk
    /// outstanding forever with nothing to say why
    /// ([ADR-016](../../../docs/62-COLLABORATION-PIPELINING.md)).
    ///
    /// `revision` is where the *acknowledged chunk* landed, not where the
    /// document has since reached. Naming a later revision would have the
    /// client skip everything committed in between, which it would then never
    /// receive.
    Ack {
        /// Every sequence up to and including this one has been ordered.
        through: u64,
        /// Where it landed.
        revision: u64,
    },
    /// Somebody else's committed operations, to apply locally.
    Apply {
        /// The revision the document reached.
        revision: u64,
        /// The operations, packaged so the receiver can localise them into its
        /// own formula and style tables.
        ops: Vec<WireOperation>,
    },
    /// Where another participant is looking.
    Presence {
        /// Who.
        client: ClientId,
        /// Their display name, from the token — never from the client, which
        /// is the surface where a claimed identity would be believed.
        name: String,
        /// Their colour.
        color: String,
        /// The sheet.
        sheet: usize,
        /// The selection, as `[r0, c0, r1, c1]`.
        selection: [u32; 4],
        /// What they are typing, if anything — bounded by the server before it
        /// is relayed, and **untrusted text** wherever it is shown.
        ///
        /// Absent when they are not editing, which is what makes an abandoned
        /// edit clear itself: the entry is replaced whole, so the draft goes
        /// with it. A departure clears it the same way, by removing the entry.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        editing: Option<Draft>,
    },
    /// Someone left, or went silent long enough to be presumed gone.
    Departed {
        /// Who.
        client: ClientId,
    },
    /// The document has stopped accepting edits.
    ///
    /// Sent when saving has failed often enough that persistence cannot be
    /// relied on. A client shows this rather than letting people keep typing
    /// into something that will not be kept.
    Stopped {
        /// Why.
        reason: Refusal,
        /// A node with room, when this one is full and knows of one.
        ///
        /// **A field rather than a new [`Refusal`] variant, deliberately.**
        /// [ADR-016](../../../docs/62-COLLABORATION-PIPELINING.md) says to move
        /// `PROTOCOL_VERSION` when an old and a new client would read the same
        /// message *differently*. A new `Refusal` variant is worse than that: a
        /// client that has never heard of it cannot read the message at all,
        /// and would show nothing where it used to show "this node is full".
        /// An unknown *field* is skipped — no `deny_unknown_fields` here — and
        /// the refusal underneath still means exactly what it always did. So an
        /// old client keeps today's behaviour and the version does not move.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        elsewhere: Option<String>,
    },
    /// Yes.
    ///
    /// Echoes the nonce it is answering, so the client can match it to the ping
    /// it sent and measure the round trip rather than merely note that
    /// something came back.
    Pong {
        /// The nonce from the [`Ping`](ClientMessage::Ping).
        nonce: u64,
    },
    /// Something was refused.
    Refused {
        /// The submission it refers to, when it refers to one.
        seq: Option<u64>,
        /// Why.
        reason: Refusal,
    },
}

impl ServerMessage {
    /// What to send a client whose protocol does not match.
    ///
    /// [`Stopped`](Self::Stopped) rather than [`Refused`](Self::Refused), and
    /// the distinction is the whole point of having both. A refusal is about
    /// something that might go differently next time; a version mismatch will
    /// not. A client that treats it as retryable — which is the reasonable
    /// reading of "refused" — reconnects, is refused identically, and settles
    /// into a permanent loop against a server that can never accept it.
    #[must_use]
    pub fn version_mismatch(client: u32) -> Self {
        Self::Stopped {
            elsewhere: None,
            reason: Refusal::ProtocolVersion {
                server: PROTOCOL_VERSION,
                client,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

    use super::*;
    use crate::Operation;
    use crate::session::Base;

    fn workbook() -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        wb.sheets
            .push(Sheet::new(SheetId(Id::from_parts(1, 2)), "S"));
        wb
    }

    fn round_trip<T>(value: &T) -> T
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        let json = serde_json::to_string(value).expect("serializes");
        serde_json::from_str(&json).expect("deserializes")
    }

    #[test]
    fn a_submission_survives_the_wire_with_its_formula() {
        let mut wb = workbook();
        let handle = wb.store_formula(casual_calc_formula::parse("1+2").unwrap());
        let mut cell = Cell::value(CellValue::Number(3.0));
        cell.formula = Some(handle);

        let submission = Submission {
            client: ClientId(7),
            seq: 3,
            base: Base::Revision(12),
            ops: vec![WireOperation::of(
                Operation::SetCell {
                    sheet: 0,
                    at: CellRef::new(1, 1),
                    cell: Some(cell),
                },
                &wb,
            )],
        };

        let back: Submission = round_trip(&submission);
        assert_eq!(back, submission, "including the expression it carries");

        // And it still localises on the far side.
        let mut receiver = workbook();
        let op = back.ops[0].clone().localise(&mut receiver);
        let Operation::SetCell {
            cell: Some(cell), ..
        } = op
        else {
            panic!("still a cell edit")
        };
        assert_eq!(
            cell.formula.and_then(|h| receiver.formula(h)).cloned(),
            Some(casual_calc_formula::parse("1+2").unwrap())
        );
    }

    #[test]
    fn structural_and_metadata_operations_survive_the_wire() {
        let mut data = crate::SheetMetadata::default();
        data.hidden_rows.insert(4);
        data.columns.sizes.insert(2, 140);
        let batch = Operation::Batch(vec![
            Operation::InsertRows {
                sheet: 0,
                at: 3,
                count: 2,
            },
            Operation::set_sheet_metadata(0, data),
        ]);
        let wire = WireOperation::of(batch, &workbook());
        assert_eq!(round_trip(&wire), wire);
    }

    #[test]
    fn every_message_round_trips() {
        let messages = vec![
            ClientMessage::Join {
                protocol: PROTOCOL_VERSION,
                token: "signed".into(),
                resume: None,
            },
            ClientMessage::Join {
                protocol: PROTOCOL_VERSION,
                token: "signed".into(),
                resume: Some(Resume {
                    key: "opaque".into(),
                    revision: 12,
                }),
            },
            ClientMessage::Heartbeat,
            ClientMessage::Presence {
                sheet: 0,
                selection: [1, 2, 3, 4],
                editing: None,
            },
            ClientMessage::Presence {
                sheet: 0,
                selection: [1, 2, 3, 4],
                editing: Some(Draft::new(1, 2, "half a formu")),
            },
            ClientMessage::Leave,
        ];
        for message in &messages {
            assert_eq!(&round_trip(message), message);
        }

        let replies = vec![
            ServerMessage::Welcome {
                protocol: PROTOCOL_VERSION,
                client: ClientId(1),
                revision: 9,
                snapshot: vec![1, 2, 3],
                editable: true,
            },
            ServerMessage::Ack {
                through: 2,
                revision: 10,
            },
            ServerMessage::Apply {
                revision: 11,
                ops: vec![],
            },
            ServerMessage::Departed {
                client: ClientId(1),
            },
            ServerMessage::Resumed {
                protocol: PROTOCOL_VERSION,
                client: ClientId(1),
                revision: 12,
                editable: true,
                missed: vec![],
            },
            ServerMessage::Stopped {
                reason: Refusal::NotSaving,
                elsewhere: None,
            },
            ServerMessage::Stopped {
                reason: Refusal::NotSaving,
                elsewhere: Some("wss://node-b.example/collab".to_owned()),
            },
            ServerMessage::version_mismatch(99),
        ];
        for reply in &replies {
            assert_eq!(&round_trip(reply), reply);
        }
    }

    /// `Submit` is the one variant whose tagging is not obvious, and it was the
    /// one variant this test did not cover.
    ///
    /// It is a *newtype* variant in an internally-tagged enum, so the tag has
    /// to be folded in beside the `Submission`'s own fields rather than
    /// wrapping them. That works only because a `Submission` serializes as a
    /// map, and nothing states that requirement anywhere it could be checked —
    /// so it is checked here.
    ///
    /// The absence of this cost a real bug: the WebAssembly binding handed the
    /// browser a bare `Submission`, which is a `ClientMessage::Submit` with the
    /// tag missing, and the server could not parse a single edit any browser
    /// ever made. Both sides were individually correct and no test put them in
    /// a room together.
    #[test]
    fn a_submission_on_the_wire_is_a_tagged_message_not_a_bare_submission() {
        let submission = Submission {
            client: ClientId(3),
            seq: 1,
            base: Base::Revision(0),
            ops: vec![],
        };
        let json = serde_json::to_string(&ClientMessage::Submit(submission.clone())).unwrap();
        assert!(json.contains("\"type\":\"submit\""), "{json}");
        // Folded in beside the submission's fields, not nested under a key.
        assert!(json.contains("\"seq\":1"), "{json}");
        assert_eq!(
            serde_json::from_str::<ClientMessage>(&json).unwrap(),
            ClientMessage::Submit(submission.clone())
        );

        // And the bare form — what the binding used to send — is not accepted,
        // which is what makes the assertion above worth making.
        let bare = serde_json::to_string(&submission).unwrap();
        assert!(
            serde_json::from_str::<ClientMessage>(&bare).is_err(),
            "an untagged submission must not parse as a message"
        );
    }

    /// **Every operation that carries an integer-keyed map, through text.**
    ///
    /// The sibling test below closed this gap for `StyleId`/`StringId`, which
    /// wrap a `NonZeroU32`. It did not close it for the plain `u32`-keyed maps,
    /// and those failed for a different reason that the fix for newtype keys
    /// does not touch.
    ///
    /// [`ClientMessage`] is `#[serde(tag = "type")]`. Serde reads an
    /// internally-tagged enum by buffering the value into its private `Content`
    /// type, reading the tag, then re-deserializing the variant *from the
    /// buffer* — a second pass that never reaches `serde_json`, and so has none
    /// of its string-key-to-integer parsing. Every such field came back
    /// `invalid type: string "0", expected u32`, and the entire message was
    /// unreadable.
    ///
    /// What that meant in a real session: **autofilter rules, every column
    /// width, every row height and every outline level were undeliverable.**
    /// The sender applied the change locally and believed it had sent it; the
    /// server could not parse the message and answered `CannotMerge`, naming
    /// the transform — the one part that was working. Found by driving two
    /// browsers against the real server, because every test on both sides
    /// constructed the message rather than parsing one.
    ///
    /// So this test round-trips through a **string**, and asserts each map
    /// still has its contents. Constructing the struct and comparing would
    /// pass against the bug.
    #[test]
    fn a_submission_carrying_integer_keyed_maps_survives_json_in_both_directions() {
        use casual_calc_model::{AutoFilter, CellRange, FilterRule};

        let wb = workbook();
        let mut data = crate::SheetMetadata {
            // A filter with a rule — keyed by column offset.
            auto_filter: Some(AutoFilter {
                range: CellRange {
                    start: CellRef::new(0, 0),
                    end: CellRef::new(3, 0),
                },
                rules: [(0u32, FilterRule::Values(vec!["1".to_owned()]))]
                    .into_iter()
                    .collect(),
            }),
            ..crate::SheetMetadata::default()
        };
        // A column width and a row height — `AxisSizing::sizes`.
        data.columns.sizes.insert(1, 2800);
        data.rows.sizes.insert(4, 400);
        // Outline levels, on both axes.
        data.row_outline_levels.insert(6, 2);
        data.col_outline_levels.insert(3, 1);

        let message = ClientMessage::Submit(Submission {
            client: ClientId(1),
            seq: 1,
            base: Base::Revision(0),
            ops: vec![WireOperation::of(
                Operation::SetSheetMetadata {
                    sheet: 0,
                    data: Box::new(data.clone()),
                    changed: crate::SheetFields::ALL,
                    restore: Default::default(),
                },
                &wb,
            )],
        });

        let json = serde_json::to_string(&message).expect("serializes");
        // The shape that broke it, asserted so a later change to how keys are
        // written does not silently make this test about something else.
        assert!(
            json.contains(r#""rules":{"0":"#),
            "keys are JSON strings: {json}"
        );

        let back: ClientMessage = serde_json::from_str(&json).unwrap_or_else(|e| {
            panic!("a message carrying an integer-keyed map must be readable: {e}\n{json}")
        });
        assert_eq!(back, message, "and it must mean the same thing");

        // Named individually, so a partial fix cannot pass.
        let ClientMessage::Submit(back) = back else {
            panic!("still a submission")
        };
        let mut receiver = workbook();
        let Operation::SetSheetMetadata { data: got, .. } =
            back.ops[0].clone().localise(&mut receiver)
        else {
            panic!("still a metadata edit")
        };
        assert_eq!(
            got.auto_filter.as_ref().map(|f| f.rules.len()),
            Some(1),
            "the filter rule survived"
        );
        assert_eq!(
            got.columns.sizes.get(&1),
            Some(&2800),
            "the column width survived"
        );
        assert_eq!(
            got.rows.sizes.get(&4),
            Some(&400),
            "the row height survived"
        );
        assert_eq!(
            got.row_outline_levels.get(&6),
            Some(&2),
            "the row outline survived"
        );
        assert_eq!(
            got.col_outline_levels.get(&3),
            Some(&1),
            "the column outline survived"
        );
    }

    /// A submission carrying every interned table, through JSON, both ways.
    ///
    /// The gap this closes: the round-trip tests above went through JSON with
    /// these tables *empty*, and the tests that filled them went through
    /// `localise` rather than through serde. So the one thing no test did was
    /// send a populated table through the format it actually travels in — and
    /// it did not survive. `StyleId` and `StringId` wrap a `NonZeroU32`, JSON
    /// object keys are strings, and `serde_json` parses integer keys back for
    /// the primitive integer types only.
    ///
    /// The result was a message that serialized perfectly and could not be
    /// read by anyone, for every text edit and every style edit there is.
    #[test]
    fn a_submission_carrying_text_and_style_survives_json_in_both_directions() {
        use casual_calc_model::Style;

        let mut wb = workbook();
        let text = wb.intern_string("typed by a person");
        let style = wb.intern_style(Style::default());
        let handle = wb.store_formula(casual_calc_formula::parse("1+2").unwrap());

        let mut cell = Cell::value(CellValue::SharedString(text));
        cell.style = Some(style);
        cell.formula = Some(handle);

        let message = ClientMessage::Submit(Submission {
            client: ClientId(1),
            seq: 1,
            base: Base::Revision(0),
            ops: vec![WireOperation::of(
                Operation::SetCell {
                    sheet: 0,
                    at: CellRef::new(4, 1),
                    cell: Some(cell),
                },
                &wb,
            )],
        });

        let json = serde_json::to_string(&message).unwrap();
        assert!(
            json.contains("typed by a person"),
            "the text is carried: {json}"
        );
        let back: ClientMessage = serde_json::from_str(&json)
            .expect("a populated interned table must survive the format it travels in");
        assert_eq!(back, message);

        // And it still means the same thing on the far side, which is the point
        // of carrying the tables at all.
        let ClientMessage::Submit(back) = back else {
            panic!("still a submission")
        };
        let mut receiver = workbook();
        let op = back.ops[0].clone().localise(&mut receiver);
        let Operation::SetCell {
            cell: Some(cell), ..
        } = op
        else {
            panic!("still a cell edit")
        };
        let CellValue::SharedString(id) = cell.value else {
            panic!("still shared text")
        };
        assert_eq!(receiver.strings.get(id), Some("typed by a person"));
    }

    #[test]
    fn a_message_is_tagged_so_a_reader_can_dispatch_before_parsing_the_rest() {
        let json = serde_json::to_string(&ClientMessage::Heartbeat).unwrap();
        assert!(json.contains("\"type\":\"heartbeat\""), "{json}");

        let json = serde_json::to_string(&ServerMessage::Ack {
            through: 1,
            revision: 2,
        })
        .unwrap();
        assert!(json.contains("\"type\":\"ack\""), "{json}");
    }

    /// **A draft travels on presence and nowhere else.**
    ///
    /// The shape, asserted at the level a peer sees it: a `presence` message
    /// with an `editing` object inside it. Nothing here may look like an
    /// operation, because a draft that could be mistaken for one is a draft
    /// that could end up in somebody's document.
    #[test]
    fn a_draft_rides_the_presence_message_and_is_shaped_like_presence() {
        let message = ClientMessage::Presence {
            sheet: 0,
            selection: [3, 1, 3, 1],
            editing: Some(Draft::new(3, 1, "=SUM(A1:A")),
        };
        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("\"type\":\"presence\""), "{json}");
        assert!(json.contains("\"editing\""), "{json}");
        assert!(!json.contains("\"op\""), "not an operation: {json}");
        assert_eq!(round_trip(&message), message);

        let reply = ServerMessage::Presence {
            client: ClientId(4),
            name: "Ada".into(),
            color: "0891B2".into(),
            sheet: 0,
            selection: [3, 1, 3, 1],
            editing: Some(Draft::new(3, 1, "=SUM(A1:A")),
        };
        assert_eq!(round_trip(&reply), reply);
    }

    /// **Not editing is the absent field, not an empty one.**
    ///
    /// Which is what makes an abandoned edit clearable: a participant who
    /// pressed Escape sends the same message with no draft in it, and each
    /// participant owns exactly one presence entry that is overwritten
    /// wholesale — so the ghost goes without anything having to remember it was
    /// there.
    #[test]
    fn a_participant_who_is_not_typing_carries_no_draft_at_all() {
        let json = serde_json::to_string(&ClientMessage::Presence {
            sheet: 0,
            selection: [0, 0, 0, 0],
            editing: None,
        })
        .unwrap();
        assert!(!json.contains("editing"), "{json}");
        assert_eq!(
            serde_json::from_str::<ClientMessage>(&json).unwrap(),
            ClientMessage::Presence {
                sheet: 0,
                selection: [0, 0, 0, 0],
                editing: None,
            }
        );
    }

    /// **The length is bounded where the text is built, not where it is drawn.**
    ///
    /// A draft arrives once per keystroke from a party the server does not
    /// trust, and a cell holds tens of thousands of characters. Truncated on a
    /// character boundary, because slicing a `String` by bytes panics in the
    /// middle of anything that is not ASCII — which is to say, in front of the
    /// users least likely to be in the test suite.
    #[test]
    fn a_draft_is_bounded_and_truncates_on_a_character_and_not_a_byte() {
        let long = "é".repeat(Draft::MAX_TEXT * 3);
        let draft = Draft::new(0, 0, &long);
        assert_eq!(draft.text.chars().count(), Draft::MAX_TEXT);
        assert!(long.starts_with(&draft.text), "a prefix of what was typed");

        // And a short one is left exactly as it was typed.
        let short = Draft::new(0, 0, "hello");
        assert_eq!(short.text, "hello");
    }

    /// **An old client must still read a stop that names somewhere else.**
    ///
    /// The same question as the draft field below, and the reason `DEP-09` adds
    /// a *field* rather than a new [`Refusal`] variant. A variant an old client
    /// has never heard of makes the whole message unreadable — a tagged enum
    /// with an unknown tag is a deserialization error — so a client that used
    /// to show "this node is full" would show nothing at all. That is strictly
    /// worse than the behaviour being replaced.
    ///
    /// Verified against the shape an old client actually has: a `Stopped` with
    /// no `elsewhere` in it. The extra key is skipped, the refusal underneath
    /// still means what it always did, nothing is read as something else — so
    /// `PROTOCOL_VERSION` does not move.
    #[test]
    fn a_client_that_has_never_heard_of_a_redirect_still_reads_the_refusal() {
        /// `ServerMessage::Stopped` as it stood before `DEP-09`.
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct OldStopped {
            reason: Refusal,
        }

        let json = serde_json::to_string(&ServerMessage::Stopped {
            reason: Refusal::NotSaving,
            elsewhere: Some("wss://node-b.example/collab".to_owned()),
        })
        .expect("serialize");

        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        let old: OldStopped = serde_json::from_value(value["reason"].clone())
            .map(|reason| OldStopped { reason })
            .expect("an old client must be able to read this");
        assert_eq!(
            old.reason,
            Refusal::NotSaving,
            "the refusal an old client acts on changed meaning"
        );

        // And the whole message, not just the reason: this is the shape the old
        // client's `serde` actually meets.
        let whole: OldStopped = serde_json::from_str(
            &serde_json::to_string(&serde_json::json!({
                "type": "stopped",
                "reason": { "reason": "notSaving" },
                "elsewhere": "wss://node-b.example/collab",
            }))
            .unwrap(),
        )
        .expect("an unknown field must be skipped, not refused");
        assert_eq!(whole.reason, Refusal::NotSaving);
    }

    /// A stop that names nowhere does not carry the key at all, so nothing on
    /// the wire changes for the deployments that are not clustered.
    #[test]
    fn a_stop_that_names_nowhere_is_byte_identical_to_before() {
        let json = serde_json::to_string(&ServerMessage::Stopped {
            reason: Refusal::NotSaving,
            elsewhere: None,
        })
        .expect("serialize");
        assert!(
            !json.contains("elsewhere"),
            "an absent redirect still costs every non-clustered deployment a key: {json}"
        );
    }

    /// **An old peer must be able to read a message carrying a draft.**
    ///
    /// The question `PROTOCOL_VERSION` turns on, per
    /// [ADR-016](../../../docs/62-COLLABORATION-PIPELINING.md): bump when an
    /// old and a new client would interpret the same message *differently*.
    ///
    /// Verified rather than assumed, in both directions and against the shape
    /// an old peer actually has — a struct with no `editing` field. Neither
    /// `Presence` variant carries `deny_unknown_fields`, so the extra key is
    /// skipped; and the field defaults, so a message from an old peer reads as
    /// "not typing", which is exactly what an old peer means by it. Nothing is
    /// read as something else, so the version does not move.
    #[test]
    fn a_peer_that_has_never_heard_of_drafts_still_reads_a_message_carrying_one() {
        /// `ServerMessage::Presence` as it stood at `PROTOCOL_VERSION` 5.
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct OldPresence {
            client: ClientId,
            name: String,
            sheet: usize,
            selection: [u32; 4],
        }

        // New → old. The draft is skipped and everything else means what it
        // always did.
        let json = serde_json::to_string(&ServerMessage::Presence {
            client: ClientId(2),
            name: "Ada".into(),
            color: "0891B2".into(),
            sheet: 1,
            selection: [5, 6, 7, 8],
            editing: Some(Draft::new(5, 6, "half typ")),
        })
        .unwrap();
        let old: OldPresence = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("an old client could not read a new presence: {e}\n{json}"));
        assert_eq!(old.client, ClientId(2), "and it still knows whose cursor");
        assert_eq!(old.name, "Ada");
        assert_eq!(old.selection, [5, 6, 7, 8]);
        assert_eq!(old.sheet, 1);

        // Old → new. A message written before drafts existed reads as somebody
        // who is not typing, rather than failing to read at all.
        let was = r#"{"type":"presence","client":2,"name":"Ada","color":"0891B2","sheet":1,"selection":[5,6,7,8]}"#;
        let now: ServerMessage = serde_json::from_str(was)
            .unwrap_or_else(|e| panic!("a new client could not read an old presence: {e}"));
        assert_eq!(
            now,
            ServerMessage::Presence {
                client: ClientId(2),
                name: "Ada".into(),
                color: "0891B2".into(),
                sheet: 1,
                selection: [5, 6, 7, 8],
                editing: None,
            }
        );

        // The same, client to server.
        let was = r#"{"type":"presence","sheet":0,"selection":[1,2,3,4]}"#;
        assert_eq!(
            serde_json::from_str::<ClientMessage>(was).expect("an old client's presence"),
            ClientMessage::Presence {
                sheet: 0,
                selection: [1, 2, 3, 4],
                editing: None,
            }
        );
    }

    #[test]
    fn a_refusal_says_which_kind_without_saying_which_check_failed() {
        // Useful to an operator in a log, useless to someone probing for which
        // part of the token was wrong.
        let json = serde_json::to_string(&Refusal::NotAuthorised).unwrap();
        assert_eq!(json, "{\"reason\":\"notAuthorised\"}");
    }

    #[test]
    fn the_too_far_behind_refusal_carries_the_range_the_client_needed() {
        let refusal = Refusal::TooFarBehind {
            oldest: 400,
            current: 900,
        };
        let back: Refusal = round_trip(&refusal);
        assert_eq!(back, refusal, "so the client can say what happened");
    }
}
