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
pub const PROTOCOL_VERSION: u32 = 1;

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
    /// The document could not be read or written.
    Broken,
}

/// Client to server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClientMessage {
    /// The opening message. Nothing else is accepted before it.
    Join {
        /// The protocol the client speaks.
        protocol: u32,
        /// The host-signed token: who, which document, what permission, and
        /// where the server may fetch and send back. The server holds no
        /// per-document state, so the token is the whole contract.
        token: String,
    },
    /// A chunk of the client's own edits.
    ///
    /// One in flight at a time — the server does not require it, but a client
    /// that ignores the rule makes its own bookkeeping wrong, not the
    /// server's.
    Submit(Submission),
    /// Still here. Presence entries expire on silence, so this is what keeps
    /// one alive.
    Heartbeat,
    /// Where this participant is looking.
    ///
    /// Not an edit and never transformed: it is ephemeral, nothing depends on
    /// its history, and losing it costs nothing.
    Presence {
        /// The sheet being viewed.
        sheet: usize,
        /// The selected range, as `[r0, c0, r1, c1]`.
        selection: [u32; 4],
    },
    /// Leaving deliberately, rather than by disconnecting.
    Leave,
}

/// Server to client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerMessage {
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
    /// A submission was committed.
    ///
    /// `revision` is where *this* chunk landed, not where the document has
    /// since reached: acknowledging at a later revision would have the client
    /// skip everything committed in between, which it would then never
    /// receive.
    Ack {
        /// The sequence being acknowledged.
        seq: u64,
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
            base: 12,
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
            },
            ClientMessage::Heartbeat,
            ClientMessage::Presence {
                sheet: 0,
                selection: [1, 2, 3, 4],
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
                seq: 2,
                revision: 10,
            },
            ServerMessage::Apply {
                revision: 11,
                ops: vec![],
            },
            ServerMessage::Departed {
                client: ClientId(1),
            },
            ServerMessage::Stopped {
                reason: Refusal::NotSaving,
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
            base: 0,
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
            base: 0,
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
            seq: 1,
            revision: 2,
        })
        .unwrap();
        assert!(json.contains("\"type\":\"ack\""), "{json}");
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
