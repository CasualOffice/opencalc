//! Version history: capture, list, name, hide and restore (`HIST-01`).
//!
//! The engine and SDK half was built by `SAVE-08`; this is the half that lets a
//! browser reach it. Nothing here decides policy — the store owns retention and
//! the SDK owns what a restore means. These are bindings, and they are thin on
//! purpose.
//!
//! **The clock is the caller's, in every one of them.** A WebAssembly build has
//! no clock this crate can reach, a captured time invented here could not be
//! tested, and `AGENTS.md` puts I/O and time in the host. So `at_ms` is a
//! parameter and never a default — which is also why the store must not sort by
//! it (`VersionId` is the order).

use super::*;
use casual_calc_transaction::version::VersionKind;

/// Parse the kind a host names, refusing anything else.
///
/// A misspelled kind must not silently become `Autosave` — that is the one tier
/// the retention ring may discard, so a typo would turn a version the user
/// asked to keep into one the store is free to drop.
fn kind_from(name: &str) -> Result<VersionKind, JsError> {
    match name {
        "autosave" => Ok(VersionKind::Autosave),
        "saved" => Ok(VersionKind::Saved),
        "named" => Ok(VersionKind::Named),
        other => Err(JsError::new(&format!(
            "unknown version kind `{other}`; expected autosave, saved or named"
        ))),
    }
}

/// Capture the document as it stands now.
///
/// `at_ms` is the host's clock. `name` is `Named`'s only reason to exist, and
/// an empty one is treated as absent rather than stored as `""` — a version
/// called "" reads as unnamed everywhere it is shown and cannot be told from
/// one.
///
/// Returns `{ id, stored, evicted }`. **`stored: false` is not a failure**: the
/// store resolves a capture of an unchanged document to the version that
/// already holds that state, so an autosave cadence running against an idle
/// document writes nothing and says so.
///
/// # Errors
///
/// If there is no session, the kind is unknown, or the store refuses the
/// capture.
#[wasm_bindgen]
pub fn session_capture_version(
    kind: &str,
    name: Option<String>,
    at_ms: f64,
) -> Result<String, JsError> {
    let kind = kind_from(kind)?;
    let name = name.filter(|n| !n.trim().is_empty());
    with_session_mut(|s| {
        let captured = s.capture_version(kind, name, at_ms as i64)?;
        Ok(serde_json::json!({
            "id": captured.id.0,
            "stored": captured.stored,
            "evicted": captured.evicted.iter().map(|v| v.0).collect::<Vec<_>>(),
        })
        .to_string())
    })
}

/// The same, recording the collaboration revision the snapshot sits at.
///
/// A collaborative host knows the revision and a local one does not. Recording
/// it is what makes a version answerable to "was this before or after the
/// change we are looking at", and it is the one fact the op log cannot supply
/// later, because the log will be gone (`SAVE-09`).
///
/// # Errors
///
/// As [`session_capture_version`].
#[wasm_bindgen]
pub fn session_capture_version_at(
    kind: &str,
    name: Option<String>,
    at_ms: f64,
    revision: f64,
) -> Result<String, JsError> {
    let kind = kind_from(kind)?;
    let name = name.filter(|n| !n.trim().is_empty());
    with_session_mut(|s| {
        let captured = s.capture_version_at(kind, name, at_ms as i64, revision as u64)?;
        Ok(serde_json::json!({
            "id": captured.id.0,
            "stored": captured.stored,
            "evicted": captured.evicted.iter().map(|v| v.0).collect::<Vec<_>>(),
        })
        .to_string())
    })
}

/// Every version a user should see, newest first.
///
/// Hidden entries are omitted — hiding is the user asking not to see one, and
/// the bytes stay so that unhiding is free. A host that wants the hidden ones
/// too is asking a different question and does not have a binding for it yet.
///
/// `bytes` is the **uncompressed** size, which is what the retention arithmetic
/// uses; see `SAVE-13` for why compression is the host's to do.
#[wasm_bindgen]
pub fn session_versions() -> String {
    with_session(|s| {
        let mut out: Vec<serde_json::Value> = s
            .versions()
            .visible()
            .map(|v| {
                serde_json::json!({
                    "id": v.id.0,
                    "kind": match v.kind {
                        VersionKind::Autosave => "autosave",
                        VersionKind::Saved => "saved",
                        VersionKind::Named => "named",
                    },
                    "name": v.name,
                    "at": v.captured_at_ms,
                    "revision": v.revision,
                    "bytes": v.byte_len,
                })
            })
            .collect();
        // Newest first, by id rather than by time: the id is the store's own
        // order and the clock is the caller's, so two hosts with skewed clocks
        // still agree on which version came second.
        out.reverse();
        serde_json::Value::Array(out).to_string()
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Total bytes the store holds, for a host that wants to show a budget.
#[wasm_bindgen]
pub fn session_versions_bytes() -> f64 {
    with_session(|s| s.versions().total_bytes() as f64).unwrap_or(0.0)
}

/// Give a version a name, which also promotes nothing — the kind is unchanged.
///
/// Returns `false` if there is no such version.
#[wasm_bindgen]
pub fn session_name_version(id: f64, name: &str) -> bool {
    with_session_mut(|s| {
        Ok(s.versions_mut()
            .name(casual_calc_transaction::version::VersionId(id as u64), name))
    })
    .unwrap_or(false)
}

/// Hide a version from the list, keeping its bytes.
#[wasm_bindgen]
pub fn session_hide_version(id: f64) -> bool {
    with_session_mut(|s| {
        Ok(s.versions_mut()
            .hide(casual_calc_transaction::version::VersionId(id as u64)))
    })
    .unwrap_or(false)
}

/// What restoring `id` would do, without doing it.
///
/// For the confirmation a restore deserves: "this will change 412 cells" is a
/// different sentence from "this will change 412 cells and cannot bring back
/// two images", and only the second one lets a user decline for the right
/// reason. `unexpressed` is the list the operation set cannot carry.
///
/// # Errors
///
/// If there is no session or no such version.
#[wasm_bindgen]
pub fn session_plan_restore(id: f64) -> Result<String, JsError> {
    with_session_mut(|s| {
        let report = s.plan_restore(casual_calc_transaction::version::VersionId(id as u64))?;
        Ok(serde_json::json!({
            "cellsChanged": report.cells_changed,
            "sheetsAdded": report.sheets_added,
            "sheetsRemoved": report.sheets_removed,
            "empty": report.is_empty(),
            "unexpressed": report
                .unexpressed
                .iter()
                .map(|u| format!("{u:?}"))
                .collect::<Vec<_>>(),
        })
        .to_string())
    })
}

/// Restore the document to `id`, capturing the present first.
///
/// Two things the SDK guarantees and this binding must not obscure. The restore
/// arrives as **one `Operation::Batch` of ordinary edits**, so it travels to
/// collaborators as edits and costs exactly one undo step — a batch has one
/// combined inverse. And the present is captured as a `Saved` version *before*
/// the restore lands; if that capture cannot be stored the restore is refused
/// rather than proceeding with no way back.
///
/// `preserved` in the result is that capture's id, which is what a host shows
/// as "your work before this restore".
///
/// # Errors
///
/// If there is no session, no such version, or the present could not be kept.
#[wasm_bindgen]
pub fn session_restore_version(id: f64, at_ms: f64) -> Result<String, JsError> {
    with_session_mut(|s| {
        let done = s.restore_version(
            casual_calc_transaction::version::VersionId(id as u64),
            at_ms as i64,
        )?;
        Ok(serde_json::json!({
            "restoredFrom": done.restored_from.0,
            "preserved": done.preserved.map(|v| v.0),
            "cellsChanged": done.report.cells_changed,
            "sheetsAdded": done.report.sheets_added,
            "sheetsRemoved": done.report.sheets_removed,
        })
        .to_string())
    })
}

// --- Persistence (`HIST-03`) ------------------------------------------------
//
// `SAVE-08` made the store host-agnostic with `into_parts`/`from_parts` for
// exactly this, and `HIST-01` reached it from the editor — but nothing carried
// it across a reload, so a version survived until the tab closed and no longer.
// A history you lose by pressing F5 is not a history.
//
// Three bindings rather than one blob, because the host stores it in two pieces
// and should not have to take a megabyte apart to read a date. The metadata is
// small, JSON, and enough to draw the whole panel; the bytes are fetched only
// when a version is actually restored. It is the same split
// `webapp/editor.drafts.js` already keeps for drafts (`meta` and `bytes` object
// stores), for the same reason.

/// Every version's metadata, **including hidden ones**, oldest first.
///
/// Deliberately not [`session_versions`], which omits hidden entries because a
/// panel should not show them. Persistence is a different question: hiding is a
/// display choice the user made and losing it on reload would un-hide
/// everything they had tidied away.
#[wasm_bindgen]
pub fn session_versions_manifest() -> String {
    with_session(|s| {
        let all: Vec<&casual_calc_transaction::version::Version> = s.versions().versions().collect();
        serde_json::to_string(&all).unwrap_or_else(|_| "[]".to_owned())
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// One version's snapshot bytes, for the host to write to its own store.
///
/// # Errors
///
/// If there is no session, or no version with that id.
#[wasm_bindgen]
pub fn session_version_bytes(id: f64) -> Result<Vec<u8>, JsError> {
    with_session(|s| {
        s.versions()
            .get(casual_calc_transaction::version::VersionId(id as u64))
            .map(|entry| entry.bytes.clone())
    })
    .flatten()
    .ok_or_else(|| JsError::new("no such version"))
}

/// Put a version back exactly as it was, from the host's store.
///
/// The id, kind, name, capture time and revision all come back with it: a
/// restored history whose ids were renumbered would be a different history, and
/// the id is what the panel and `restore_version` address a version by.
///
/// Adding an id the store already holds replaces it rather than duplicating,
/// so a host that loads twice — a double `initVersions`, a reconnect — does not
/// end up showing everything twice.
///
/// # Errors
///
/// If there is no session, or the metadata is not a version.
#[wasm_bindgen]
pub fn session_version_add(meta: &str, bytes: Vec<u8>) -> Result<(), JsError> {
    let version: casual_calc_transaction::version::Version =
        serde_json::from_str(meta).map_err(|why| JsError::new(&format!("bad version: {why}")))?;
    with_session_mut(|s| {
        // The policy travels with the entries: `from_parts` takes one, and a
        // store rebuilt under the default would quietly change which versions
        // the retention ring may discard.
        let policy = s.versions().policy();
        let store = core::mem::take(s.versions_mut());
        let mut entries = store.into_parts();
        entries.retain(|entry| entry.version.id != version.id);
        entries.push(casual_calc_transaction::version::VersionSnapshot { version, bytes });
        s.set_versions(casual_calc_transaction::version::VersionStore::from_parts(
            policy, entries,
        ));
        Ok(())
    })
}
