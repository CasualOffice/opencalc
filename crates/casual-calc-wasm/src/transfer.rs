//! Opening and saving off the page's thread (`PERF-16`).
//!
//! The editor opens and saves in a Web Worker that loads this same module. The
//! worker and the page are two engines with two memories, so a workbook
//! crosses between them as [`casual_calc_model::transfer`] parts — bounded, so
//! the side on the page's thread reads it a few milliseconds at a time between
//! frames instead of in one long call.
//!
//! - **Open**: the worker calls [`session_open_as`](crate::session_open_as)
//!   (every admission bound and the time budget apply there, unchanged), then
//!   [`session_transfer_begin`] / [`session_transfer_next`] and
//!   [`session_opened_report`]. The page feeds each part to
//!   [`session_receive_part`] and installs with
//!   [`session_receive_finish_opened`]. Until that last call the page's
//!   session is untouched, so a cancelled or failed open changes nothing.
//! - **Save**: the page calls [`session_transfer_begin`] — which copies the
//!   workbook, fixing the moment saved — and hands the parts to the worker,
//!   which installs them with [`session_receive_finish_copy`] and calls the
//!   ordinary `session_save_*` writer.
//!
//! The receiving side is held to the snapshot limits (`SEC-013`) as parts
//! arrive.

use super::*;
use casual_calc_import::{CompatibilityEntry, CompatibilityReport, ModelOutcome, RetentionOutcome};
use casual_calc_model::SnapshotLimits;
use casual_calc_model::transfer::{TransferReader, TransferWriter};

thread_local! {
    static OUTGOING: RefCell<Option<TransferWriter>> = const { RefCell::new(None) };
    static INCOMING: RefCell<Option<TransferReader>> = const { RefCell::new(None) };
}

/// Start sending the session's workbook; returns how many items (formulas
/// plus cells) the parts will carry, for a progress bar.
///
/// Sends a **copy**: the session stays in use while the parts go out, and an
/// edit made meanwhile must be either wholly in the file or not in it at all.
/// A transfer already under way is abandoned.
///
/// # Errors
///
/// If there is no session.
#[wasm_bindgen]
pub fn session_transfer_begin() -> Result<f64, JsError> {
    let workbook =
        with_session(|s| s.workbook().clone()).ok_or_else(|| JsError::new("no session"))?;
    let writer = TransferWriter::new(workbook);
    let total = writer.total_items() as f64;
    OUTGOING.with(|cell| *cell.borrow_mut() = Some(writer));
    Ok(total)
}

/// The next part of the transfer [`session_transfer_begin`] started, holding
/// at most `max_items` formulas or cells — or an **empty** array once the
/// last part has been returned, which also ends the transfer.
///
/// # Errors
///
/// If no transfer is under way, or a part cannot be serialised.
#[wasm_bindgen]
pub fn session_transfer_next(max_items: u32) -> Result<Vec<u8>, JsError> {
    OUTGOING.with(|cell| {
        let mut guard = cell.borrow_mut();
        let writer = guard
            .as_mut()
            .ok_or_else(|| JsError::new("no transfer under way"))?;
        match writer.next_part(max_items as usize).map_err(js)? {
            Some(part) => Ok(part),
            None => {
                *guard = None;
                Ok(Vec::new())
            }
        }
    })
}

/// Formulas plus cells sent so far by the current transfer.
#[wasm_bindgen]
pub fn session_transfer_sent() -> f64 {
    OUTGOING.with(|cell| {
        cell.borrow()
            .as_ref()
            .map_or(0.0, |w| w.sent_items() as f64)
    })
}

/// Abandon the outgoing transfer, if any, and free its copy.
#[wasm_bindgen]
pub fn session_transfer_cancel() {
    OUTGOING.with(|cell| *cell.borrow_mut() = None);
}

/// Start receiving a transfer. One already under way is abandoned; the
/// session is not touched.
#[wasm_bindgen]
pub fn session_receive_begin() {
    INCOMING.with(|cell| *cell.borrow_mut() = Some(TransferReader::new(SnapshotLimits::default())));
}

/// Read one part. Returns the formulas plus cells received so far.
///
/// # Errors
///
/// If no transfer is being received, or the part is refused (malformed, out
/// of order, over a limit, naming something the workbook lacks). A refusal
/// ends the transfer; the session is not touched.
#[wasm_bindgen]
pub fn session_receive_part(part: &[u8]) -> Result<f64, JsError> {
    INCOMING.with(|cell| {
        let mut guard = cell.borrow_mut();
        let reader = guard
            .as_mut()
            .ok_or_else(|| JsError::new("no transfer being received"))?;
        match reader.push(part) {
            Ok(()) => Ok(reader.received_items() as f64),
            Err(err) => {
                *guard = None;
                Err(js(err))
            }
        }
    })
}

/// Abandon the incoming transfer, if any. The session is not touched.
#[wasm_bindgen]
pub fn session_receive_cancel() {
    INCOMING.with(|cell| *cell.borrow_mut() = None);
}

fn received() -> Result<Workbook, JsError> {
    let reader = INCOMING
        .with(|cell| cell.borrow_mut().take())
        .ok_or_else(|| JsError::new("no transfer being received"))?;
    reader.finish().map_err(js)
}

/// Install the received workbook as **the file a worker opened**: what
/// [`session_open_as`](crate::session_open_as)`(ext, source)` would have left,
/// given the workbook and `report` (from [`session_opened_report`]) the
/// worker's open produced.
///
/// `source` is the bytes the worker opened, kept so an unedited save returns
/// them unchanged. The host's read-only setting and clock apply, as on any
/// open.
///
/// # Errors
///
/// If no complete transfer was received, `ext` is not an openable format, or
/// `report` is not a report.
#[wasm_bindgen]
pub fn session_receive_finish_opened(
    ext: &str,
    report: &str,
    source: &[u8],
) -> Result<(), JsError> {
    let format = casual_calc_sdk::SessionFormat::for_extension(ext)
        .ok_or_else(|| JsError::new(&format!(".{ext} is not a format this build can open")))?;
    let report = report_from_json(report)?;
    let workbook = received()?;
    set_session(WorkbookSession::from_opened(
        workbook,
        format,
        report,
        Some(source.to_vec()),
        casual_calc_sdk::SessionConfig::new(),
    ));
    Ok(())
}

/// Install the received workbook as a **copy to be written** in `ext`'s
/// format: no opened file behind it, so every `session_save_*` runs the
/// writer. What the editor's save worker calls.
///
/// # Errors
///
/// If no complete transfer was received, or `ext` is not a format.
#[wasm_bindgen]
pub fn session_receive_finish_copy(ext: &str) -> Result<(), JsError> {
    let format = casual_calc_sdk::SessionFormat::for_extension(ext)
        .ok_or_else(|| JsError::new(&format!(".{ext} is not a format")))?;
    let workbook = received()?;
    set_session(WorkbookSession::from_opened(
        workbook,
        format,
        CompatibilityReport::default(),
        None,
        casual_calc_sdk::SessionConfig::new(),
    ));
    Ok(())
}

/// Whether saving as `ext` now hands back the opened file unchanged — in
/// which case there is nothing to write and no reason to start a worker.
#[wasm_bindgen]
pub fn session_save_is_verbatim(ext: &str) -> bool {
    let Some(format) = casual_calc_sdk::SessionFormat::for_extension(ext)
        .filter(|f| f.extension().eq_ignore_ascii_case(ext))
    else {
        return false;
    };
    with_session(|s| s.is_unmodified() && s.format() == format).unwrap_or(false)
}

/// The session's import report, as JSON for [`session_receive_finish_opened`]:
/// `[[feature, model, retention, count], …]`.
#[wasm_bindgen]
pub fn session_opened_report() -> String {
    let entries = with_session(|s| s.compatibility_report().entries()).unwrap_or_default();
    let rows: Vec<serde_json::Value> = entries
        .into_iter()
        .map(|e| {
            serde_json::json!([
                e.feature,
                model_name(e.model),
                retention_name(e.retention),
                e.count
            ])
        })
        .collect();
    serde_json::Value::Array(rows).to_string()
}

fn model_name(m: ModelOutcome) -> &'static str {
    match m {
        ModelOutcome::Mapped => "mapped",
        ModelOutcome::Degraded => "degraded",
        ModelOutcome::Omitted => "omitted",
    }
}

fn retention_name(r: RetentionOutcome) -> &'static str {
    match r {
        RetentionOutcome::Preserved => "preserved",
        RetentionOutcome::NotRetained => "not-retained",
        RetentionOutcome::NotApplicable => "not-applicable",
    }
}

fn report_from_json(text: &str) -> Result<CompatibilityReport, JsError> {
    let bad = || JsError::new("not an import report");
    let rows: Vec<(String, String, String, u64)> = serde_json::from_str(text).map_err(|_| bad())?;
    let mut entries = Vec::with_capacity(rows.len());
    for (feature, model, retention, count) in rows {
        let model = match model.as_str() {
            "mapped" => ModelOutcome::Mapped,
            "degraded" => ModelOutcome::Degraded,
            "omitted" => ModelOutcome::Omitted,
            _ => return Err(bad()),
        };
        let retention = match retention.as_str() {
            "preserved" => RetentionOutcome::Preserved,
            "not-retained" => RetentionOutcome::NotRetained,
            "not-applicable" => RetentionOutcome::NotApplicable,
            _ => return Err(bad()),
        };
        entries.push(CompatibilityEntry {
            feature,
            model,
            retention,
            count,
        });
    }
    Ok(CompatibilityReport::from_entries(entries))
}
