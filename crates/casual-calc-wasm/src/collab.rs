//! The collaboration client half: the wire protocol and snapshots.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

// Deliberately no transport here. The protocol is message-shaped and the
// messages serialize, so the JavaScript side owns the socket, the reconnect and
// the backoff — which is where a browser wants them, and which keeps this
// testable without one.

thread_local! {
    /// This participant's view, once it has joined a document.
    static COLLAB: RefCell<Option<ClientSession>> = const { RefCell::new(None) };
}

/// Replace the workbook with a normalized-model snapshot.
///
/// What a joining participant is given, and **not** the file: everyone in a
/// session must start from the same revision, and a client that fetched the
/// document itself would arrive at revision zero while the session was at five
/// hundred.
///
/// # Errors
///
/// If the bytes are not a snapshot this engine can read — including one written
/// by a different `SCHEMA_VERSION`, which is refused rather than half-loaded.
#[wasm_bindgen]
pub fn session_load_snapshot(bytes: &[u8]) -> Result<(), JsError> {
    // `from_snapshot`, not bare `serde_json`: the snapshot format carries a
    // `SCHEMA_VERSION` and refuses a version it does not know, and the server
    // writes it with the matching `to_snapshot`. Reading it as a plain
    // `Workbook` happens to work today and would fail the first time the schema
    // moved — at runtime, in a browser, on somebody's document.
    // Parsed before anything is replaced, so a snapshot this cannot read
    // leaves the open document untouched rather than half-loaded. (Not tested
    // natively: constructing the `JsError` that failure returns panics off
    // wasm, and a test that cannot run is worse than one that is missing.)
    let workbook = Workbook::from_snapshot(bytes).map_err(js)?;
    SESSION.with(|cell| {
        *cell.borrow_mut() = Some(WorkbookSession::from_workbook(workbook));
    });
    Ok(())
}

/// The workbook as a snapshot, for handing to a joining participant.
#[wasm_bindgen]
pub fn session_snapshot() -> Result<Vec<u8>, JsError> {
    SESSION.with(|cell| {
        let guard = cell.borrow();
        let session = guard.as_ref().ok_or_else(|| JsError::new("no session"))?;
        session.workbook().to_snapshot().map_err(js)
    })
}

/// Join a collaborative session as `client`, starting from `revision`.
///
/// The revision comes from the server's `Welcome`, alongside the snapshot the
/// document was loaded from: everyone in a session must start from the same
/// one, and a client that guessed would rebase against a history it never saw.
#[wasm_bindgen]
pub fn collab_begin(client: f64, revision: f64) {
    let session = ClientSession::new(
        casual_calc_transaction::session::ClientId(client as u64),
        revision as u64,
    );
    COLLAB.with(|cell| *cell.borrow_mut() = Some(session));
    // From here the editor's own edit path reports what it applies, which is
    // what makes local work sendable. Without it every entry point that edits —
    // and there are more than forty — would have to know about collaboration.
    SESSION.with(|cell| {
        if let Some(session) = cell.borrow_mut().as_mut() {
            session.record_applied();
        }
    });
}

/// Continue an existing session after a reconnect, keeping unsent work.
///
/// The counterpart to [`collab_begin`], and using the wrong one is the bug this
/// exists to prevent: `collab_begin` starts a participant with nothing
/// outstanding, so calling it on a reconnect silently discards the edits made
/// just before the socket dropped — the ones most likely to be unacknowledged,
/// and the ones a user most recently watched themselves type.
///
/// The document must **not** be reloaded around this. A resuming client's
/// workbook is continuous; the server sends only what was missed, precisely so
/// the local unsent operations still mean something against it.
///
/// Returns `false` when there was no session to continue, in which case the
/// caller must join afresh.
///
/// See [ADR-015](../../../docs/61-COLLABORATION-RESUME.md).
#[wasm_bindgen]
pub fn collab_resume(client: f64, revision: f64) -> bool {
    COLLAB.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(session) = guard.as_mut() else {
            return false;
        };
        session.resume(
            casual_calc_transaction::session::ClientId(client as u64),
            revision as u64,
        );
        true
    })
}

/// Whether anything is written and not yet acknowledged.
///
/// What a host shows as "saving", and what tells a user whether closing the tab
/// now would lose something.
///
/// **Both** places have to be asked. Work made by the editor lands in the
/// session's applied log first and only moves into the collaborative session at
/// the next flush — so between an edit and that flush, which is where a
/// disconnected client spends all of its time, the collaborative session knows
/// nothing about it. Asking only that one reports "nothing unsaved" to somebody
/// who has been typing for a minute into a dropped connection.
#[wasm_bindgen]
pub fn collab_unacknowledged() -> bool {
    let in_flight = COLLAB.with(|cell| {
        cell.borrow()
            .as_ref()
            .is_some_and(ClientSession::has_unacknowledged)
    });
    in_flight || with_session(WorkbookSession::has_applied).unwrap_or(false)
}

/// Where this participant is looking, and what they are typing, as a JSON
/// `ClientMessage`.
///
/// A whole message rather than a shape for the host to assemble, for the same
/// reason [`collab_flush`] returns one: the host carries the string to a socket
/// and no further, and a host that had to build the message would be
/// reimplementing the protocol in whatever language it happens to be written
/// in. That is not a hypothetical — the first version of `collab_flush` handed
/// out a bare `Submission` and the server could not parse a single edit any
/// browser ever made.
///
/// `draft_text` is what is in the cell editor *right now*, or `None` when
/// nothing is being edited — which is how an abandoned edit is cleared
/// everywhere, since each participant owns one presence entry that is
/// overwritten whole. `draft_row`/`draft_col` name the cell the edit belongs
/// to, which is not always the selection: a formula being written wanders off
/// to pick references.
///
/// The text is bounded here rather than left to the caller, because "the caller
/// remembers to truncate" is a rule with one keystroke between it and a
/// megabyte on the wire.
///
/// **This does not touch the document.** No operation is recorded, nothing
/// enters the undo history, and nothing becomes outstanding work — a draft is
/// presence (ADR-011), and the whole point is that losing it costs nothing.
#[wasm_bindgen]
pub fn collab_presence(
    sheet: usize,
    selection: &[u32],
    draft_row: u32,
    draft_col: u32,
    draft_text: Option<String>,
) -> String {
    // Padded rather than refused: a malformed selection from a host is worth a
    // cursor in the wrong place, not a dropped presence channel.
    let at = |i: usize| selection.get(i).copied().unwrap_or(0);
    let message = ClientMessage::Presence {
        sheet,
        selection: [at(0), at(1), at(2), at(3)],
        editing: draft_text
            .map(|text| casual_calc_transaction::protocol::Draft::new(draft_row, draft_col, text)),
    };
    serde_json::to_string(&message).unwrap_or_default()
}

/// Leave the session. Local edits stop being tracked for submission.
#[wasm_bindgen]
pub fn collab_end() {
    COLLAB.with(|cell| *cell.borrow_mut() = None);
    SESSION.with(|cell| {
        if let Some(session) = cell.borrow_mut().as_mut() {
            session.stop_recording();
        }
    });
}

/// Whether this participant is in a session.
#[wasm_bindgen]
pub fn collab_active() -> bool {
    COLLAB.with(|cell| cell.borrow().is_some())
}

/// The revision this participant believes the document is at.
#[wasm_bindgen]
pub fn collab_revision() -> f64 {
    COLLAB.with(|cell| cell.borrow().as_ref().map_or(0, ClientSession::revision)) as f64
}

/// Take the next chunk of local edits to send, as a JSON `ClientMessage`.
///
/// Empty string when there is nothing to send **or** a chunk is already in
/// flight — one at a time, by design, because a client with two outstanding
/// chunks cannot say which the server's acknowledgement was for.
///
/// A whole `ClientMessage`, not the bare `Submission` inside it. The host
/// carries this string to a socket and no further, so the tag that tells the
/// server which message it is has to be put on here — a host that had to wrap
/// it would be reimplementing the protocol in whatever language it is written
/// in, and the first version of this returned the bare submission and produced
/// a chunk the server could not parse at all.
#[wasm_bindgen]
pub fn collab_flush() -> String {
    // Collect what the editor applied since last time, then package it. Two
    // steps rather than one because the editor owns the apply path — it has its
    // own undo history, and an operation applied twice is worse than one sent
    // late.
    let applied = SESSION.with(|cell| {
        cell.borrow_mut()
            .as_mut()
            .map(WorkbookSession::take_applied)
            .unwrap_or_default()
    });
    COLLAB.with(|cell| {
        if let Some(collab) = cell.borrow_mut().as_mut() {
            for op in applied {
                collab.record(op);
            }
        }
    });
    with_session_and_collab(|workbook, collab| {
        collab
            .flush(workbook)
            .and_then(|s| serde_json::to_string(&ClientMessage::Submit(s)).ok())
            .unwrap_or_default()
    })
    .unwrap_or_default()
}

/// Everything still outstanding, to send again after a reconnect.
///
/// A JSON **array** of messages, oldest first, and the order is not cosmetic:
/// each chunk was written on top of the one before it and only the first names
/// a revision, so delivering them out of order asks the server to resolve a
/// chain whose start it has not seen.
///
/// A resend reuses each chunk's original sequence number, so a server that
/// already applied one answers `Duplicate` rather than applying it twice. That
/// is what makes reconnecting safe rather than merely likely to work.
///
/// An array rather than one chunk since ADR-016, which allows several in
/// flight: a client that reconnects mid-flight may have any number of them.
#[wasm_bindgen]
pub fn collab_resend() -> String {
    with_session_and_collab(|workbook, collab| {
        let messages: Vec<ClientMessage> = collab
            .resend(workbook)
            .into_iter()
            .map(ClientMessage::Submit)
            .collect();
        serde_json::to_string(&messages).unwrap_or_else(|_| "[]".to_owned())
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Record that everything up to `through` was ordered, the last of it at
/// `revision`.
///
/// **Cumulative**: chunks before `through` are settled by it without being
/// named, which is what lets a lost acknowledgement heal on the next one rather
/// than stranding a chunk in flight forever.
#[wasm_bindgen]
pub fn collab_acknowledge(through: f64, revision: f64) {
    COLLAB.with(|cell| {
        if let Some(collab) = cell.borrow_mut().as_mut() {
            collab.acknowledge(through as u64, revision as u64);
        }
    });
}

/// Apply an operation from another participant, arriving at `revision`.
///
/// `wire` is one `WireOperation` as JSON. It is localised into this workbook's
/// own tables before anything is compared — an interned id is replica-local, so
/// another participant's style 7 is not this one's (COL-12) — and then rebased
/// against every local edit still outstanding, in both directions.
///
/// # Errors
///
/// If the JSON is not a `WireOperation`, if there is no session, or if the
/// transform refuses the pair.
#[wasm_bindgen]
pub fn collab_receive(wire: &str, revision: f64) -> Result<String, JsError> {
    let incoming: casual_calc_transaction::wire::WireOperation =
        serde_json::from_str(wire).map_err(js)?;
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        COLLAB.with(|collab| {
            let mut collab = collab.borrow_mut();
            let collab = collab
                .as_mut()
                .ok_or_else(|| JsError::new("not in a collaborative session"))?;
            collab
                .receive(session.workbook_mut(), &incoming, revision as u64)
                .map_err(js)
        })
    })?;
    // A remote edit changes values, so the same recalculation a local one gets
    // — **including the way out of it** (`COL-43`). This called plain
    // `recalculate()`, so a peer whose edit triggered an expensive pass held
    // this tab exactly as an oversized open used to, and nothing the person in
    // front of it did could stop that.
    //
    // What a cancelled pass means here is the part that needed deciding, and it
    // is decided on the *model*: the operation was applied and acknowledged
    // above, before this runs, so the document converges on cell content
    // whatever happens next. Only derived values are left behind, the session
    // is marked stale, and the outcome is returned so the host can finish the
    // job rather than present a half-fresh sheet as final.
    let outcome = SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(session) = guard.as_mut() else {
            return "none";
        };
        match session.recalculate_cancellable(&budget_token()) {
            Recalculated::Fully => "full",
            Recalculated::Cancelled => "cancelled",
            Recalculated::OverBudget => "over-budget",
        }
    });
    Ok(outcome.to_owned())
}

/// Run `f` with the workbook and the collaborative session, if both exist.
pub(crate) fn with_session_and_collab<T>(
    f: impl FnOnce(&Workbook, &mut ClientSession) -> T,
) -> Option<T> {
    SESSION.with(|cell| {
        let guard = cell.borrow();
        let session = guard.as_ref()?;
        COLLAB.with(|collab| {
            let mut collab = collab.borrow_mut();
            let collab = collab.as_mut()?;
            Some(f(session.workbook(), collab))
        })
    })
}

/// Undo the last edit.
#[wasm_bindgen]
pub fn session_undo() -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        session.undo().map_err(js)
    })
}
