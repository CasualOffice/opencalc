//! Opening and saving: format detection, delimited text and the import
//! report.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

/// The workbook's document properties, as JSON.
///
/// **Modelled, imported, exported — and unreachable.** `DocumentProperties`
/// carries nine fields, `docProps/core.xml` round-trips them faithfully, and no
/// binding existed to read one. So a workbook opened here kept its title and
/// author perfectly while the person editing it could not see either, and a
/// workbook created here went out with none (`UX-META-01`).
///
/// Dates are ISO 8601 strings exactly as the file carries them; they are not
/// reformatted for display here, because a host that wants a local format needs
/// the instant, not somebody else's rendering of it.
#[wasm_bindgen]
pub fn session_doc_properties() -> String {
    with_session(|s| {
        let p = &s.workbook().properties;
        format!(
            "{{\"title\":{},\"subject\":{},\"description\":{},\"keywords\":{},\
\"creator\":{},\"lastModifiedBy\":{},\"created\":{},\"modified\":{},\"language\":{}}}",
            json_string(&p.title),
            json_string(&p.subject),
            json_string(&p.description),
            json_string(&p.keywords.join(", ")),
            json_string(&p.creator),
            json_string(&p.last_modified_by),
            json_string(&p.created),
            json_string(&p.modified),
            json_string(&p.language),
        )
    })
    .unwrap_or_else(|| "{}".to_owned())
}

/// Set the editable document properties.
///
/// Only the five a person can meaningfully author: `created` and `modified` are
/// the file's own history, `lastModifiedBy` is set by whoever saves, and
/// language belongs to the content. Offering those as text boxes would invite
/// somebody to write a false history into a document, which is worse than not
/// offering them.
///
/// Keywords arrive as one comma-separated string because that is how every
/// spreadsheet asks for them, and are split here so the model keeps the list it
/// is documented to keep.
#[wasm_bindgen]
pub fn session_set_doc_properties(
    title: &str,
    subject: &str,
    description: &str,
    keywords: &str,
    creator: &str,
) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let p = &mut session.workbook_mut().properties;
        p.title = title.trim().to_owned();
        p.subject = subject.trim().to_owned();
        p.description = description.trim().to_owned();
        p.keywords = keywords
            .split(',')
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(str::to_owned)
            .collect();
        p.creator = creator.trim().to_owned();
        Ok(())
    })
}

/// Admit a package under whatever time budget is in force.
///
/// The stateless helpers below share this so that "the landing page can be
/// frozen by a large file and the editor cannot" is not a state this crate can
/// be in — which is the shape `SEC-017` came in as, one host short.
pub(crate) fn admit(bytes: &[u8]) -> Result<casual_calc_import::Import, JsError> {
    let cancel = budget_token();
    // `Default::default()` rather than naming `OoxmlLimits`: the stock
    // admission bounds are what `import_package` uses, and inferring the type
    // from the signature keeps this crate off a dependency it needs for one
    // word.
    casual_calc_import::import_package_cancellable(bytes.to_vec(), Default::default(), &cancel)
        .map_err(js)
}

/// A short summary of an opened `.xlsx`.
///
/// Stoppable: see [`render_xlsx`].
///
/// # Errors
///
/// If the bytes are not an admissible package, or the import was stopped.
#[wasm_bindgen]
pub fn describe_xlsx(bytes: &[u8]) -> Result<String, JsError> {
    let outcome = admit(bytes)?;
    let wb = outcome.workbook;
    let (name, cells) = wb
        .sheets
        .first()
        .map(|s| (s.name.clone(), s.cells.len()))
        .unwrap_or_default();
    Ok(format!(
        "{} sheet(s); \"{name}\" has {cells} populated cell(s)",
        wb.sheets.len()
    ))
}

// ---------------------------------------------------------------------------
// Editor session.
// ---------------------------------------------------------------------------

/// Start a new blank session with one sheet.
#[wasm_bindgen]
pub fn session_new() {
    // `with_sheet`, not `blank` plus a sheet of our own. A workbook with no
    // sheets has nothing to draw, and both hosts had worked that out separately
    // and written the same fix twice (`SDK-011`).
    set_session(WorkbookSession::with_sheet());
}

/// Open an `.xlsx` into the editor session.
///
/// Stoppable: see [`session_set_time_budget_ms`]. A cancelled open reports
/// `OC-IMP-0007` and leaves whatever session was already loaded untouched,
/// because a half-built workbook under the old one's name is worse than no
/// workbook at all.
///
/// # Errors
///
/// If the bytes are not a package this build admits, or the open was stopped.
#[wasm_bindgen]
pub fn session_open(bytes: &[u8]) -> Result<(), JsError> {
    session_open_as("xlsx", bytes)
}

/// Open bytes whose format is named by a filename extension.
///
/// **The SDK decides what `ext` means**
/// ([`casual_calc_sdk::SessionFormat::for_extension`]) — not this bridge, and
/// not the page. An editor carrying its own extension table opens exactly the
/// formats it was last told about, which is how a `.csv` came to be openable
/// through the WOPI service and not in the editor itself (`WASM-01`). Anything
/// the SDK learns to read becomes openable here with no change to this
/// function.
///
/// The session **remembers** the format, so [`session_save_native`] writes the
/// same kind of file back and [`session_format`] /
/// [`session_format_content_type`] name what those bytes are.
///
/// Delimited text has no encoding declaration, so its bytes must already be
/// UTF-8 — ask [`format_is_text`] and decode before calling this.
///
/// # Errors
///
/// If `ext` is not a format this build can open, if the bytes are not that
/// format, or if the open was stopped ([`session_set_time_budget_ms`]).
#[wasm_bindgen]
pub fn session_open_as(ext: &str, bytes: &[u8]) -> Result<(), JsError> {
    let format = casual_calc_sdk::SessionFormat::for_extension(ext)
        .ok_or_else(|| JsError::new(&format!(".{ext} is not a format this build can open")))?;
    let cancel = budget_token();
    // **One call, no match.** The SDK decides which of its formats can be
    // stopped part-way — admission walking every cell of a package is the long
    // job, and it is the one that can — so this bridge does not. The match that
    // used to be here named `Xlsx` as the stoppable arm and sent every other
    // format down an unstoppable path, which is a decision that goes stale the
    // day the SDK learns a second package format: `.xlsm` would have arrived
    // uninterruptible on a browser's single thread (`IO-04`).
    let mut session = WorkbookSession::open_as_cancellable(
        bytes.to_vec(),
        format,
        casual_calc_sdk::SessionConfig::new(),
        &cancel,
    )
    .map_err(js)?;
    reapply_filters_after_load(&mut session);
    set_session(session);
    Ok(())
}

/// The canonical extension `ext` names, or `""` when this build cannot open it.
///
/// `.tab` answers `tsv`, `.XLSX` answers `xlsx`. A host asks this instead of
/// keeping its own table of which extension is which format — the second table
/// is the defect, not the answer.
#[wasm_bindgen]
pub fn format_for_extension(ext: &str) -> String {
    casual_calc_sdk::SessionFormat::for_extension(ext)
        .map(|f| f.extension().to_owned())
        .unwrap_or_default()
}

/// The MIME type a file with this extension should be served as, or `""` when
/// this build does not know the extension.
///
/// The counterpart of [`format_for_extension`] for a host that is *writing*:
/// `.tsv` is `text/tab-separated-values`, not `text/csv`. A page keeping its
/// own answer to this gets one of the three delimited types right.
#[wasm_bindgen]
pub fn format_content_type(ext: &str) -> String {
    casual_calc_sdk::SessionFormat::for_extension(ext)
        .map(|f| f.content_type().to_owned())
        .unwrap_or_default()
}

/// Whether `ext` names a **text** format, whose bytes carry no encoding
/// declaration and so must be decoded to UTF-8 before the engine reads them.
///
/// A package says its own encoding and must be handed over byte for byte;
/// delimited text does not, and a UTF-16 export read as UTF-8 is not slightly
/// wrong but unreadable.
#[wasm_bindgen]
pub fn format_is_text(ext: &str) -> bool {
    matches!(
        casual_calc_sdk::SessionFormat::for_extension(ext),
        Some(casual_calc_sdk::SessionFormat::Delimited(_))
    )
}

/// The extensions this build can open, as a JSON array of `".ext"` strings.
///
/// What a file picker's `accept` list should be. `SessionFormat` cannot
/// enumerate itself, so this **asks it about candidates** rather than
/// answering from a list of its own: every name below is a format some
/// spreadsheet engine writes, and which of them survive is the SDK's answer.
/// That is the difference that matters — a candidate the SDK does not know is
/// simply absent, and one it learns appears here the day it does.
#[wasm_bindgen]
pub fn openable_extensions() -> String {
    let offered: Vec<String> = CANDIDATE_EXTENSIONS
        .iter()
        .filter(|ext| casual_calc_sdk::SessionFormat::for_extension(ext).is_some())
        .map(|ext| format!("\".{ext}\""))
        .collect();
    format!("[{}]", offered.join(","))
}

/// Every extension worth asking the SDK about.
///
/// Candidates, not an answer: each is a name some spreadsheet engine writes,
/// and which of them this build actually reads or writes is decided by asking
/// [`casual_calc_sdk::SessionFormat`], never by editing this list. That is what
/// keeps a format the SDK learns from needing a change here on the same day.
const CANDIDATE_EXTENSIONS: [&str; 7] = ["xlsx", "xlsm", "ods", "csv", "tsv", "tab", "psv"];

/// The extensions this build can **write**, as a JSON array of `".ext"`
/// strings.
///
/// What a "Save a copy as…" menu should offer, and deliberately **not**
/// [`openable_extensions`] under another name: the two lists differ, and the
/// difference is not a rounding error. `.tab` opens — it is one of the names
/// the TAB delimiter answers to — but the format it names calls itself `tsv`,
/// so a file saved as `.tab` would be a file this engine wrote under a name it
/// does not use. The filter is exactly that: keep `ext` only where the format
/// it names would write itself back under the same `ext`.
///
/// Pair with [`format_content_type`] for the MIME type and
/// [`session_save_loss_for`] for what the format costs, then
/// [`session_save_as`] for the bytes.
#[wasm_bindgen]
pub fn writable_extensions() -> String {
    let offered: Vec<String> = CANDIDATE_EXTENSIONS
        .iter()
        .filter(|ext| writable_format(ext).is_some())
        .map(|ext| format!("\".{ext}\""))
        .collect();
    format!("[{}]", offered.join(","))
}

/// The format a save under the name `ext` would write, or `None` when this
/// build writes no such file.
///
/// **One decision, two callers.** [`writable_extensions`] offers a name and
/// [`session_save_as`] accepts one, and the day those two disagree is the day a
/// menu offers a format the save then refuses. The rule they share: keep `ext`
/// only where the format it names would write itself back under the same `ext`
/// — `.tab` names the TAB delimiter, whose own extension is `tsv`, so it is a
/// name this engine reads and not one it writes.
fn writable_format(ext: &str) -> Option<casual_calc_sdk::SessionFormat> {
    casual_calc_sdk::SessionFormat::for_extension(ext)
        .filter(|format| format.extension().eq_ignore_ascii_case(ext))
}

/// Open delimited text (CSV/TSV/PSV) into the editor session. `delimiter` is the
/// separator byte (e.g. `,`, tab, `|`).
///
/// Goes through the SDK rather than building a workbook and handing it over,
/// because that is what makes the session **remember it opened a `.csv`** —
/// and so what makes [`session_save`] write one back instead of a package with
/// a `.csv` name on it (`WOPI-05`).
#[wasm_bindgen]
pub fn session_open_delimited(bytes: &[u8], delimiter: u8) -> Result<(), JsError> {
    set_session(WorkbookSession::open_delimited(bytes.to_vec(), delimiter).map_err(js)?);
    Ok(())
}

/// The format the session saves as: `xlsx`, `csv`, `tsv`, `psv` or `txt`.
///
/// A host names the file it downloads and picks a content type from this. Bytes
/// that are CSV under a name ending `.xlsx` are the same lie as the one this
/// row exists to fix, pointing the other way.
#[wasm_bindgen]
pub fn session_format() -> String {
    with_session(|s| s.format().extension().to_owned()).unwrap_or_else(|| "xlsx".to_owned())
}

/// The MIME type [`session_save`]'s bytes should be served as.
#[wasm_bindgen]
pub fn session_format_content_type() -> String {
    with_session(|s| s.format().content_type().to_owned()).unwrap_or_else(|| {
        casual_calc_sdk::SessionFormat::Xlsx
            .content_type()
            .to_owned()
    })
}

/// What saving in the session's own format cannot carry, as one sentence — or
/// empty when it carries everything.
///
/// The counterpart of [`session_import_summary`] on the way out: a `.csv`
/// session holds one sheet of values, so a workbook that has grown a second
/// sheet, a formula or any formatting is about to lose it. Said **before** the
/// download, because afterwards the file is already on disk.
#[wasm_bindgen]
pub fn session_save_loss() -> String {
    with_session(|s| describe_loss(&s.format_loss())).unwrap_or_default()
}

/// The same question asked of a format the session was **not** opened from:
/// what saving this document as `.ext` would cost, as one sentence, or empty
/// when it costs nothing.
///
/// The half of "Save a copy as…" that has to come before the download. A `.csv`
/// chosen from that menu drops every sheet but the first whether or not the
/// file on disk was ever a `.csv`, and an `.xlsx` chosen for a macro-enabled
/// workbook leaves the macros behind — so the question is about the format the
/// person picked, not the one they opened.
///
/// An `ext` this build cannot write answers with the empty string, the same as
/// a format that loses nothing; ask [`writable_extensions`] which is which
/// before offering it.
#[wasm_bindgen]
pub fn session_save_loss_for(ext: &str) -> String {
    let Some(format) = casual_calc_sdk::SessionFormat::for_extension(ext) else {
        return String::new();
    };
    with_session(|s| describe_loss(&s.loss_writing(format))).unwrap_or_default()
}

/// A compatibility report as the one sentence a person reads before a download.
///
/// Shared by [`session_save_loss`] and [`session_save_loss_for`] so the two
/// cannot phrase the same loss differently — the second was written as a copy
/// of the first exactly once, in the draft of this change.
/// One sheet as a PDF, laid out the way it would print.
///
/// `IO-14`. The paginator and the PDF writer have been finished and tested
/// since `IO-03`/`IO-10` — print area, print titles, headers and footers, the
/// field-code language, repeated rows — and **none of it reached anybody**.
/// `casual-calc-sdk` is `publish = false`, the server has no PDF route, and
/// this crate, which already compiles `export_pdf` into every build because it
/// depends on that crate, never exposed it. Work that is done and unreachable
/// is indistinguishable from work that was never done, from where a user
/// stands.
///
/// The sheet index is explicit rather than "the active one": a host exporting
/// in the background has no active sheet, and the editor knows which one it
/// means.
///
/// # Errors
///
/// When there is no session, when `sheet_index` is out of range, or when the
/// writer refuses the page — each carried through as the SDK worded it, since
/// this layer knows nothing the SDK does not.
#[wasm_bindgen]
pub fn session_export_pdf(sheet_index: usize) -> Result<Vec<u8>, JsError> {
    SESSION.with(|cell| {
        let guard = cell.borrow();
        let session = guard.as_ref().ok_or_else(|| JsError::new("no session"))?;
        session.export_pdf(sheet_index).map_err(js)
    })
}

/// What the printout could not carry, as one sentence, or empty when it carried
/// everything.
///
/// Deliberately a second call rather than a tuple (`IO-14`). Every other loss in
/// this module is asked for the same way — `session_save_loss`,
/// `session_loss_for` — and a host that shows loss has one place to read it
/// from. The PDF backend draws no pictures, so a sheet with images is the
/// ordinary case for this returning something.
#[wasm_bindgen]
pub fn session_export_pdf_loss(sheet_index: usize) -> String {
    with_session(|s| {
        s.export_pdf_with_report(sheet_index)
            .map(|(_, report)| describe_loss(&report))
            .unwrap_or_default()
    })
    .unwrap_or_default()
}

fn describe_loss(loss: &casual_calc_sdk::CompatibilityReport) -> String {
    if loss.is_empty() {
        return String::new();
    }
    let mut dropped: Vec<String> = Vec::new();
    let mut degraded: Vec<String> = Vec::new();
    for e in loss.entries() {
        match e.model {
            casual_calc_sdk::ModelOutcome::Omitted => dropped.push(e.feature),
            casual_calc_sdk::ModelOutcome::Degraded => degraded.push(e.feature),
            casual_calc_sdk::ModelOutcome::Mapped => {}
        }
    }
    let mut parts = Vec::new();
    if !dropped.is_empty() {
        parts.push(format!("not written: {}", dropped.join(", ")));
    }
    // Named separately because the distinction is the one a user acts on:
    // a formula's *answer* is in the file, the formula itself is not.
    if !degraded.is_empty() {
        parts.push(format!("written as values: {}", degraded.join(", ")));
    }
    parts.join("; ")
}

/// Serialize a sheet to delimited text (CSV/TSV/PSV) using the cached values.
#[wasm_bindgen]
pub fn session_save_delimited(sheet: usize, delimiter: u8) -> String {
    with_session(|s| casual_calc_io::write_delimited(s.workbook(), sheet, delimiter))
        .unwrap_or_default()
}

/// Hide `rows` on `sheet` **for this participant only** (`COL-32`, docs/71).
///
/// Not an edit: no operation is relayed, nothing enters the undo history,
/// nothing is saved, and no cell value moves — the `SUBTOTAL` under a personal
/// view reads the same number here as on every co-editor's screen.
///
/// `rows` is a JSON array of zero-based row indices.
#[wasm_bindgen]
pub fn session_set_personal_filter(sheet: usize, rows: &str) -> Result<(), JsError> {
    let rows: std::collections::BTreeSet<u32> =
        serde_json::from_str(rows).map_err(|why| JsError::new(&why.to_string()))?;
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        session.set_personal_filter(sheet, rows);
        Ok(())
    })
}

/// Excel's stock gallery, with the `builtinId`s Excel keys its own gallery off
/// — the *name* is localized, the id is not, so a file written by a French Excel
/// still lines up. A workbook that already defines a style of the same name uses
/// its definition instead of this one.
pub(crate) fn builtin_cell_styles() -> Vec<(&'static str, u32, Style)> {
    let tinted = |fill: &str, font: &str| Style {
        fill_color: Some(fill.to_owned()),
        font_color: Some(font.to_owned()),
        ..Default::default()
    };
    let heading = |size_hp: u32| Style {
        bold: true,
        font_size_hp: Some(size_hp),
        font_color: Some("1F4E79".to_owned()),
        ..Default::default()
    };
    vec![
        ("Normal", 0, Style::default()),
        ("Good", 26, tinted("C6EFCE", "006100")),
        ("Bad", 27, tinted("FFC7CE", "9C0006")),
        ("Neutral", 28, tinted("FFEB9C", "9C6500")),
        ("Title", 15, heading(36)),
        ("Heading 1", 16, heading(30)),
        ("Heading 2", 17, heading(26)),
        ("Heading 3", 18, heading(22)),
        ("Heading 4", 19, heading(22)),
        (
            "Total",
            25,
            Style {
                bold: true,
                ..Default::default()
            },
        ),
    ]
}

/// The cell styles to offer in a gallery, as JSON
/// `[{n,b,bold,fg,bg,sz}]` — name, builtin id, and enough formatting for the
/// host to preview each entry in its own look.
///
/// The workbook's own styles come first; the stock gallery fills in the rest, so
/// a file that defines "Heading 1" shows *its* Heading 1.
#[wasm_bindgen]
pub fn session_cell_styles() -> String {
    let mut out: Vec<(String, Option<u32>, Style)> = with_session(|s| {
        s.workbook()
            .cell_styles
            .iter()
            .map(|c| (c.name.clone(), c.builtin_id, c.style.clone()))
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    for (name, builtin, style) in builtin_cell_styles() {
        if !out.iter().any(|(n, _, _)| n.eq_ignore_ascii_case(name)) {
            out.push((name.to_owned(), Some(builtin), style));
        }
    }
    let items: Vec<String> = out
        .iter()
        .map(|(name, builtin, st)| {
            let mut parts = vec![format!("\"n\":{}", json_string(name))];
            if let Some(b) = builtin {
                parts.push(format!("\"b\":{b}"));
            }
            if st.bold {
                parts.push("\"bold\":1".to_owned());
            }
            if let Some(c) = &st.font_color {
                parts.push(format!("\"fg\":{}", json_string(c)));
            }
            if let Some(c) = &st.fill_color {
                parts.push(format!("\"bg\":{}", json_string(c)));
            }
            if let Some(hp) = st.font_size_hp {
                parts.push(format!("\"sz\":{}", hp as f64 / 2.0));
            }
            format!("{{{}}}", parts.join(","))
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Save in the format the session was **opened from**: a `.csv` back as CSV.
///
/// Paired with [`session_format`] and [`session_format_content_type`], which
/// name what these bytes are, and with [`session_save_loss`], which says what
/// the format could not carry. A caller that writes these bytes without asking
/// the last of those is dropping part of a document silently.
#[wasm_bindgen]
pub fn session_save_native() -> Result<Vec<u8>, JsError> {
    SESSION.with(|cell| {
        let guard = cell.borrow();
        let session = guard.as_ref().ok_or_else(|| JsError::new("no session"))?;
        session.save().map_err(js)
    })
}

/// Save as a **named** format, whatever the session was opened from: the bytes
/// behind "Save a copy as…".
///
/// The third of the three, and the reason the other two exist. Until it,
/// [`session_save_native`] followed the session's *opened* format and
/// [`session_save_delimited`] wrote text, so a build that could
/// [`export_ods`](casual_calc_sdk::WorkbookSession::save_as) had no route to
/// ODS bytes at all — the engine was capable and no binding could reach it
/// (`IO-07`).
///
/// The order a host calls these in:
///
/// 1. [`writable_extensions`] — what to offer.
/// 2. [`session_save_loss_for`] — what the chosen one costs, shown **before**
///    the download, because afterwards the file is already on disk.
/// 3. this — the bytes.
/// 4. [`format_content_type`] — what to serve them as.
///
/// Saving under the session's own format returns the opened file byte for byte
/// when nothing has been edited, exactly as [`session_save_native`] does; they
/// are the same call underneath.
///
/// # Errors
///
/// If `ext` is not a format this build can write, if there is no session, or
/// if the workbook cannot be serialized.
#[wasm_bindgen]
pub fn session_save_as(ext: &str) -> Result<Vec<u8>, JsError> {
    // Refused rather than defaulted. A `.numbers` that quietly produced an
    // OOXML package would be the `WOPI-05` lie in a new place: bytes of one
    // format under the name of another.
    let format = writable_format(ext)
        .ok_or_else(|| JsError::new(&format!(".{ext} is not a format this build can write")))?;
    SESSION.with(|cell| {
        let guard = cell.borrow();
        let session = guard.as_ref().ok_or_else(|| JsError::new("no session"))?;
        session.save_as(format).map_err(js)
    })
}

// ---------------------------------------------------------------------------
// Internals.
// ---------------------------------------------------------------------------

/// **The engine could write it and no binding could ask** (`IO-07`).
///
/// `SessionFormat::Ods` and `export_ods` have been implemented for as long as
/// the `.ods` reader has, and the editor still could not produce a single byte
/// of OpenDocument: `session_save()` was xlsx-only, `session_save_native()`
/// followed the format the session was *opened* from, and
/// `session_save_delimited` writes text. Three routes out of the engine and not
/// one of them reached a writer the engine already had — the eighth instance of
/// the category `docs/12` §6 names, and the one that doc wrongly filed as
/// host-side wiring.
#[cfg(test)]
mod save_as_tests {
    use super::{
        format_content_type, openable_extensions, session_save_as, session_save_loss,
        session_save_loss_for, writable_extensions,
    };
    use crate::{session_add_sheet, session_new, session_open_as, session_set_cell};

    /// **The list that must not be a copy of the other one.**
    ///
    /// `.tab` is one of the names the TAB delimiter answers to, so it opens —
    /// but the format it names calls itself `tsv`, and a file saved under
    /// `.tab` would be this engine writing a name it does not use. The
    /// temptation is to let `writable_extensions` echo `openable_extensions`,
    /// which is right for six of the seven candidates and wrong for the one
    /// that matters.
    #[test]
    fn writable_is_not_openable_under_another_name() {
        let openable = openable_extensions();
        let writable = writable_extensions();

        assert!(
            openable.contains("\".tab\""),
            "`.tab` opens, and this test's premise is that it does: {openable}"
        );
        assert!(
            !writable.contains("\".tab\""),
            "`.tab` names the TSV format, which does not write itself back under \
             that name: {writable}"
        );
        // The one the row was raised for, and the one the row uncovered.
        for ext in ["\".ods\"", "\".xlsx\"", "\".xlsm\"", "\".csv\"", "\".tsv\""] {
            assert!(
                writable.contains(ext),
                "{ext} is writable and absent: {writable}"
            );
        }
    }

    /// **The bytes.** End to end through the bindings a host actually calls:
    /// build a workbook, ask for OpenDocument, and open the answer back as one.
    ///
    /// Reopened rather than sniffed, because "the first zip entry is
    /// `mimetype`" would pass for any ODF document this engine cannot read.
    #[test]
    fn session_save_as_produces_ods_bytes_the_engine_reads_back() {
        session_new();
        session_set_cell(0, 0, 0, "Widget").unwrap();
        session_set_cell(0, 1, 0, "7").unwrap();

        let ods = session_save_as("ods").expect("`.ods` is a format this build writes");
        assert_eq!(
            casual_calc_sdk::SessionFormat::for_bytes(&ods),
            Some(casual_calc_sdk::SessionFormat::Ods),
            "the bytes have to be OpenDocument by their own content, not by the \
             name they were asked for"
        );
        assert_eq!(
            format_content_type("ods"),
            "application/vnd.oasis.opendocument.spreadsheet",
            "and the host has to know what to serve them as"
        );

        session_open_as("ods", &ods).expect("the bytes this engine wrote are ones it reads");
        assert_eq!(crate::session_cell_input(0, 0, 0), "Widget");
        assert_eq!(crate::session_cell_input(0, 1, 0), "7");
    }

    /// A name this build does not write is refused, not quietly turned into a
    /// package. Bytes of one format under the name of another is the `WOPI-05`
    /// lie, and `.tab` is the near miss that would slip through a check that
    /// only asked whether the extension names *some* format.
    ///
    /// Asked of [`writable_format`] rather than of `session_save_as` because
    /// the refusal is a `JsError`, and constructing one of those panics off a
    /// wasm target — which is also the reason the decision was pulled out of
    /// the binding: an answer no test can reach is an answer nothing checks.
    #[test]
    fn a_name_this_build_does_not_write_is_not_writable() {
        assert!(super::writable_format("numbers").is_none());
        assert!(
            super::writable_format("tab").is_none(),
            "`.tab` opens but is not a name this engine writes under"
        );
        assert!(super::writable_format("tsv").is_some(), "and `.tsv` is");
        // The binding accepts exactly what the menu offers, which is the
        // property the two sharing a decision exists to keep.
        session_new();
        assert!(session_save_as("tsv").is_ok());
        assert!(session_save_as("xlsm").is_ok());
    }

    /// **Said before the download, about the format the person picked.**
    ///
    /// `session_save_loss` answers for the session's *own* format, which is the
    /// wrong question for "Save a copy as…": a workbook opened as `.xlsx` loses
    /// nothing saving as `.xlsx` and loses every sheet but the first saving as
    /// `.csv`, and until this there was no way to ask the second question.
    #[test]
    fn session_save_loss_for_answers_about_the_chosen_format() {
        session_new();
        session_set_cell(0, 0, 0, "Widget").unwrap();
        // Formatting, which the OpenDocument writer has no styles for and the
        // package writer does. Without something the two formats disagree
        // about, this test would pass on a `session_save_loss_for` that
        // ignored its argument entirely.
        crate::session_toggle_bold(0, 0, 0, 0, 0).unwrap();
        session_add_sheet().unwrap();

        assert_eq!(
            session_save_loss(),
            "",
            "the session's own format is `.xlsx`, which carries all of this"
        );
        assert_eq!(session_save_loss_for("xlsx"), "");
        let csv = session_save_loss_for("csv");
        assert!(
            csv.contains("other sheets"),
            "a second sheet is not written to a `.csv`, and that has to be said \
             before the file is on disk: {csv:?}"
        );
        let ods = session_save_loss_for("ods");
        assert!(
            ods.contains("cell formatting"),
            "the OpenDocument writer carries values, formulas and sheets, and no \
             formatting: {ods:?}"
        );
        assert!(
            !ods.contains("other sheets"),
            "and unlike a `.csv` it carries the second sheet, so it must not \
             claim otherwise: {ods:?}"
        );
    }
}
