//! Opening and saving: format detection, delimited text and the import
//! report.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

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
    let opened = match format {
        // The one long job here: admission walks every cell of the package.
        casual_calc_sdk::SessionFormat::Xlsx => WorkbookSession::open_cancellable(
            bytes.to_vec(),
            casual_calc_sdk::SessionConfig::new(),
            &cancel,
        ),
        // `SessionFormat` is `#[non_exhaustive]`, and this arm is the point:
        // a format the SDK grows opens here without this file changing.
        other => WorkbookSession::open_as(bytes.to_vec(), other),
    };
    let mut session = opened.map_err(js)?;
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
    const CANDIDATES: [&str; 6] = ["xlsx", "ods", "csv", "tsv", "tab", "psv"];
    let offered: Vec<String> = CANDIDATES
        .iter()
        .filter(|ext| casual_calc_sdk::SessionFormat::for_extension(ext).is_some())
        .map(|ext| format!("\".{ext}\""))
        .collect();
    format!("[{}]", offered.join(","))
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
    with_session(|s| {
        let loss = s.format_loss();
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
    })
    .unwrap_or_default()
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

// ---------------------------------------------------------------------------
// Internals.
// ---------------------------------------------------------------------------
