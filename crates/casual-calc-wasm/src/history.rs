//! Undo and redo.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

/// What redo would reapply, or empty.
#[wasm_bindgen]
pub fn session_redo_label() -> String {
    with_session(|s| s.redo_label().unwrap_or_default().to_owned()).unwrap_or_default()
}

/// Make the current state the document's starting point, discarding the undo
/// history.
///
/// For a host that has just populated a fresh session — the demo's seeded
/// sheet, or an embedder restoring a document from its own store. Those writes
/// are edits as far as the engine is concerned, and without this Ctrl+Z walks
/// backwards out of the document the user was handed, one cell at a time.
#[wasm_bindgen]
pub fn session_clear_history() {
    SESSION.with(|cell| {
        if let Some(session) = cell.borrow_mut().as_mut() {
            session.clear_history();
        }
    });
}

// --- Collaboration: the client half ---------------------------------------
//
// The seam a browser collaboration client sits on. `ClientSession` in
// `casual-calc-transaction` holds this participant's revision, what it has sent
// and what it has not, and rebases in both directions when something arrives —
// all of it gated, and until now reachable from Rust and from nowhere else, so
// the browser had no way to join a session even though everything it needed
// existed.
//

/// Supply a font for rendering, ahead of the bundled ones.
///
/// The bundled faces cover Latin, and the WebAssembly build carries one family
/// because they were 72% of the bundle and the editor draws its own text with
/// the browser's fonts. What they do not cover — Arabic, Devanagari, Thai, CJK —
/// is supplied here instead of embedded, for two reasons: megabytes in every
/// tab for scripts most deployments never see, and because which languages are
/// worth carrying is not this project's judgement to make on anybody's behalf.
///
/// A host fetches the face it needs and hands over the bytes. It knows which
/// scripts its documents are in and already serves static assets.
///
/// Returns `false` if the bytes are not a readable face — a caller that fetched
/// a 404 page should be told, rather than finding out from a thumbnail full of
/// boxes.
#[wasm_bindgen]
#[must_use]
pub fn register_font(bytes: &[u8]) -> bool {
    casual_calc_render::register_face(bytes.to_vec())
}

/// Redo the last undone edit.
#[wasm_bindgen]
pub fn session_redo() -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        session.redo().map_err(js)
    })
}

/// Whether an edit can be undone.
#[wasm_bindgen]
pub fn session_can_undo() -> bool {
    with_session(|s| s.can_undo()).unwrap_or(false)
}

/// Whether an undone edit can be redone.
#[wasm_bindgen]
pub fn session_can_redo() -> bool {
    with_session(|s| s.can_redo()).unwrap_or(false)
}

/// Save the session workbook to `.xlsx` bytes.
///
/// **Always a package, whatever the session was opened from** — the editor's
/// "Save as Excel" writes these bytes to a name ending `.xlsx`, and a session
/// that opened a `.csv` handing back CSV here would put one format's bytes
/// under another's name. That is the defect `WOPI-05` exists to remove, and
/// making this method format-native would have reintroduced it pointing the
/// other way. [`session_save_native`] is the one that follows the format.
#[wasm_bindgen]
pub fn session_save() -> Result<Vec<u8>, JsError> {
    SESSION.with(|cell| {
        let guard = cell.borrow();
        let session = guard.as_ref().ok_or_else(|| JsError::new("no session"))?;
        session
            .save_as(casual_calc_sdk::SessionFormat::Xlsx)
            .map_err(js)
    })
}
