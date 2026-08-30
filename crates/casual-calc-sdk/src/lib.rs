//! `casual-calc-sdk` — the host-facing engine facade.
//!
//! A single surface that composes the whole pipeline: open a `.xlsx` into a
//! [`WorkbookSession`], read the model, edit through the transaction op set
//! (with undo/redo), recalculate, lay out and render a viewport, and save back.
//! This is the crate a host (a Tauri desktop app, a headless service, the WASM
//! bridge) embeds. The public surface is deliberately narrower than the internal
//! crates. See `docs/02-ARCHITECTURE.md`.

use casual_calc_eval::{Recalculated, Recalculator, recalculate, recalculate_cancellable};
use casual_calc_export::{ExportError, PackageKind, write_workbook_as};
use casual_calc_formula::parse;
use casual_calc_formula::stored::Origin;
use casual_calc_import::{ImportError, import_package_cancellable};
use casual_calc_io::{IoError, read_delimited, write_delimited};
use casual_calc_layout::print::PrintContext;
use casual_calc_layout::{DisplayList, Freeze, GridGeometry, Viewport, panes};
use casual_calc_model::{Id, Workbook};
use casual_calc_render::pdf::{PdfBand, PdfFurniture, PdfMetadata, PdfPage, write_pdf};
use casual_calc_render::{ImageSource, PanePaint, RenderError, render_panes_png_with_images};
use casual_calc_transaction::{
    Axis, History, Operation, SheetFields, TxnError, WouldDiscard, apply, undo_would_discard,
};

// Re-export the vocabulary a host needs, so embedders depend on one crate.
//
// The report types among them are returned by two methods on the session
// ([`WorkbookSession::compatibility_report`] and
// [`WorkbookSession::format_loss`]), and a return type a caller cannot name
// without adding a second dependency is a leak rather than a facade.
pub use casual_calc_import::{
    CompatibilityEntry, CompatibilityReport, ModelOutcome, RetentionOutcome,
};
pub use casual_calc_layout::Viewport as GridViewport;
pub use casual_calc_model::{Cell, CellRef, CellValue, Sheet, SheetId, Style};
// The picture vocabulary, for the same reason: `render_png_with_report` hands
// back what the renderer could not draw, and a host that cannot name
// `UndrawnReason` cannot tell an EMF it will never draw from a media part the
// package was missing.
pub use casual_calc_render::{ImageReport, UndrawnImage, UndrawnReason};
pub use casual_calc_transaction::{Operation as EditOperation, SheetMetadata};

pub use casual_calc_ooxml::OoxmlLimits;

const SESSION_NAMESPACE: u64 = 0x5345_5300_0000_0000; // "SES"

/// The sheet a delimited save writes.
///
/// The first, not the active one: a `.csv` holds exactly one sheet, and the one
/// the file arrived as is the first. Saving whichever tab the user happened to
/// be looking at would let a click decide which half of the document survives.
const DELIMITED_SHEET: usize = 0;

/// The file format a session was opened from, and the one [`WorkbookSession::save`]
/// writes back.
///
/// Remembered because a host that hands us a `.csv` and gets an OOXML package
/// back has had its file replaced with a different one under the same name —
/// silent data loss, and the reason the WOPI discovery document advertises one
/// extension (`WOPI-05`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SessionFormat {
    /// An OOXML package: the format the model is shaped for, and the default
    /// for a session built in memory rather than opened from a file.
    #[default]
    Xlsx,
    /// A **macro-enabled** OOXML package: the same schema, the same parts and
    /// the same reader as [`Xlsx`](Self::Xlsx), plus a `vbaProject.bin` part
    /// and a different content type on `xl/workbook.xml`.
    ///
    /// A separate variant rather than a synonym for `Xlsx` precisely because
    /// the difference survives the read: a session that forgot it opened an
    /// `.xlsm` saves an `.xlsx` back, and the macros in a file somebody has
    /// work in disappear with nothing said. What this engine does with the
    /// macro project is documented on
    /// [`Workbook::macro_project`](casual_calc_model::Workbook::macro_project)
    /// — it retains the bytes and never reads them.
    Xlsm,
    /// Delimited text (CSV / TSV / PSV), carrying the separator byte it was
    /// read with so the save uses the same one.
    Delimited(u8),
    /// An OpenDocument spreadsheet.
    ///
    /// Carries **values, formulas, sheets and the document's own metadata**;
    /// everything else in the model — formatting, merges, widths, validation,
    /// charts, images — has no writer in `casual-calc-ods` yet and is counted
    /// by [`loss_writing`](WorkbookSession::loss_writing) rather than dropped
    /// quietly.
    Ods,
}

impl SessionFormat {
    /// The format a filename extension names, or `None` for one this engine
    /// does not write.
    ///
    /// `None` rather than a fallback to [`Xlsx`](Self::Xlsx): guessing is how a
    /// `.numbers` would be opened and saved as a package under its original
    /// name.
    #[must_use]
    pub fn for_extension(ext: &str) -> Option<Self> {
        if ext.eq_ignore_ascii_case("xlsx") {
            return Some(Self::Xlsx);
        }
        if ext.eq_ignore_ascii_case("xlsm") {
            return Some(Self::Xlsm);
        }
        if ext.eq_ignore_ascii_case("ods") {
            return Some(Self::Ods);
        }
        casual_calc_io::delimiter_for_extension(ext).map(Self::Delimited)
    }

    /// The format these bytes **are**, read from the bytes themselves.
    ///
    /// For the caller that has a file and no reliable name for it: an upload,
    /// a clipboard drop, a blob from an API. A filename extension is the file's
    /// own claim about itself, and this engine believed that claim for whole
    /// documents while already refusing to for the pictures inside them
    /// (`ODS-01`).
    ///
    /// `None` when the bytes do not clearly say. A caller that knows the format
    /// should still use [`for_extension`](Self::for_extension) and say so —
    /// guessing is how a binary file becomes a sheet full of mojibake.
    ///
    /// Detection lives in `casual-calc-io`, which needs no format crate to do
    /// it; **dispatch stays here**, where the format crates are. See ADR-022 in
    /// `docs/19`.
    #[must_use]
    pub fn for_bytes(bytes: &[u8]) -> Option<Self> {
        match casual_calc_io::detect(bytes)? {
            casual_calc_io::Detected::Xlsx => Some(Self::Xlsx),
            casual_calc_io::Detected::Ods => Some(Self::Ods),
            casual_calc_io::Detected::Delimited(sep) => Some(Self::Delimited(sep)),
        }
    }

    /// The extension a save in this format writes.
    ///
    /// A delimiter with no conventional extension is `txt`, which is what a
    /// separator nobody standardised produces.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Xlsx => "xlsx",
            Self::Xlsm => "xlsm",
            Self::Ods => "ods",
            Self::Delimited(casual_calc_io::COMMA) => "csv",
            Self::Delimited(casual_calc_io::TAB) => "tsv",
            Self::Delimited(casual_calc_io::PIPE) => "psv",
            Self::Delimited(_) => "txt",
        }
    }

    /// The MIME type a save in this format should be served as.
    #[must_use]
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            // Note the shape: this is the *package* type a host serves the
            // download as, which is not the `…macroEnabled.main+xml` that
            // `xl/workbook.xml` carries inside the package.
            Self::Xlsm => "application/vnd.ms-excel.sheet.macroEnabled.12",
            Self::Ods => "application/vnd.oasis.opendocument.spreadsheet",
            Self::Delimited(casual_calc_io::COMMA) => "text/csv;charset=utf-8",
            Self::Delimited(casual_calc_io::TAB) => "text/tab-separated-values;charset=utf-8",
            Self::Delimited(_) => "text/plain;charset=utf-8",
        }
    }
}

/// When the engine recalculates.
///
/// Read from the file's `<calcPr calcMode>` on open, because a workbook saved
/// in manual mode was saved that way for a reason — usually that a full recalc
/// is slow enough to be disruptive. Recalculating it anyway on the first edit
/// is the opposite of what its author asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalculationMode {
    /// After every edit that can change a value.
    #[default]
    Automatic,
    /// Only when the host asks. Edits still mark the workbook stale, so a host
    /// can show "Calculate" the way Excel shows it in the status bar.
    Manual,
}

impl CalculationMode {
    /// The mode an OOXML `<calcPr calcMode>` token names.
    ///
    /// `autoNoTable` is automatic for everything except data-table cells, which
    /// this engine does not have — so it is automatic, not a third state that
    /// would silently behave like manual.
    #[must_use]
    pub fn from_token(token: &str) -> Self {
        match token {
            "manual" => Self::Manual,
            _ => Self::Automatic,
        }
    }

    /// The token to write back.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Automatic => "auto",
            Self::Manual => "manual",
        }
    }
}

/// The environment a calculation reads instead of the machine it runs on.
///
/// `TODAY()` and `NOW()` come from `now`, and the random functions from `seed`.
/// Supplied rather than sampled because an engine that reaches for the wall
/// clock cannot be tested, replayed, or agreed on by two hosts computing the
/// same workbook — which is the whole determinism contract.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Environment {
    /// The moment `TODAY()`/`NOW()` report, as a date serial.
    pub now: f64,
    /// The seed the random functions draw from. Changing it is what makes
    /// `RAND` reroll; leaving it alone reproduces the previous values exactly.
    pub seed: u64,
}

/// Everything a host can decide about a session.
///
/// Every field here was previously a constant somewhere inside the pipeline,
/// which meant a desktop app opening a file its user chose and a service
/// admitting anonymous uploads got identical behaviour whether or not that
/// suited either of them.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct SessionConfig {
    /// Admission bounds for opening a package. A **security** bound: it caps
    /// what an untrusted file can make the reader allocate before it is
    /// refused.
    pub limits: OoxmlLimits,
    /// When to recalculate. `None` takes the file's own `<calcPr calcMode>`,
    /// which is the right default for an editor; a host that always wants one
    /// or the other says so.
    pub calculation: Option<CalculationMode>,
    /// The clock and seed calculations read.
    pub environment: Environment,
    /// How many edits undo can reverse. `None` is unbounded, which is what a
    /// short-lived session wants and a long-lived one cannot afford: each entry
    /// holds a whole inverse operation, and a metadata edit's inverse is a
    /// snapshot of the sheet's metadata.
    pub undo_depth: Option<usize>,
    /// Refuse every edit — a viewer rather than an editor.
    ///
    /// Enforced **here**, not by hiding buttons. A read-only mode that only
    /// removes the toolbar is read-only until someone calls the API, which
    /// makes it a suggestion rather than a mode. Reading, selecting, scrolling
    /// and copying all still work: a viewer you cannot copy out of is hostile,
    /// and copying changes nothing.
    pub read_only: bool,
}

impl SessionConfig {
    /// The defaults: stock limits, the file's own calculation mode, a zero
    /// clock, and unbounded undo.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bound how far back undo can reach.
    #[must_use]
    pub fn with_undo_depth(mut self, depth: usize) -> Self {
        self.undo_depth = Some(depth);
        self
    }

    /// Force a calculation mode regardless of what the file asks for.
    #[must_use]
    pub fn with_calculation(mut self, mode: CalculationMode) -> Self {
        self.calculation = Some(mode);
        self
    }

    /// Set the clock and seed calculations read.
    #[must_use]
    pub fn with_environment(mut self, environment: Environment) -> Self {
        self.environment = environment;
        self
    }

    /// Set the package admission bounds.
    #[must_use]
    pub fn with_limits(mut self, limits: OoxmlLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Open the workbook for reading only.
    #[must_use]
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }
}

/// An error from an SDK operation.
#[derive(Debug)]
#[non_exhaustive]
pub enum SdkError {
    /// Import failed.
    Import(ImportError),
    /// Reading a delimited (CSV/TSV/PSV) file failed.
    Io(IoError),
    /// Reading or writing an OpenDocument spreadsheet failed.
    Ods(casual_calc_ods::OdsError),
    /// Export failed.
    Export(ExportError),
    /// A render failed.
    Render(RenderError),
    /// An edit operation failed.
    Edit(TxnError),
    /// The session is read-only and refused an edit.
    ReadOnly,
    /// The workbook is not in a state this engine will write out.
    ///
    /// Reached through [`workbook_mut`](WorkbookSession::workbook_mut), which
    /// hands a host the right to change anything (`SDK-008`). Returned rather
    /// than written, because a corrupt workbook in memory is a bug and one that
    /// became a file is data loss.
    Model(casual_calc_model::ModelError),
    /// Undo would have deleted a band somebody else has since filled (docs/69,
    /// `COL-28`). Refused, and refused loudly: the alternative is a structural
    /// undo that destroys work no undo stack anywhere can bring back.
    UndoWouldDiscard(WouldDiscard),
}

impl core::fmt::Display for SdkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SdkError::Import(e) => write!(f, "{e}"),
            SdkError::Io(e) => write!(f, "{e}"),
            SdkError::Ods(e) => write!(f, "{e}"),
            SdkError::Export(e) => write!(f, "{e}"),
            SdkError::Render(e) => write!(f, "{e}"),
            SdkError::Edit(e) => write!(f, "{e}"),
            SdkError::Model(e) => write!(f, "{e}"),
            SdkError::ReadOnly => f.write_str("this workbook is open for reading only"),
            SdkError::UndoWouldDiscard(what) => {
                let (line, kind) = match what.axis {
                    Axis::Row => (what.at + 1, if what.count == 1 { "row" } else { "rows" }),
                    Axis::Col => (
                        what.at + 1,
                        if what.count == 1 { "column" } else { "columns" },
                    ),
                };
                write!(
                    f,
                    "undo would remove {} {kind} starting at {line}, and somebody has since \
                     put data there ({} cell{}). Undo it from their end, or clear the {kind} first.",
                    what.count,
                    what.cells,
                    if what.cells == 1 { "" } else { "s" },
                )
            }
        }
    }
}

impl std::error::Error for SdkError {}

impl From<casual_calc_model::ModelError> for SdkError {
    fn from(e: casual_calc_model::ModelError) -> Self {
        SdkError::Model(e)
    }
}

impl From<ImportError> for SdkError {
    fn from(e: ImportError) -> Self {
        SdkError::Import(e)
    }
}
impl From<IoError> for SdkError {
    fn from(e: IoError) -> Self {
        SdkError::Io(e)
    }
}
impl From<casual_calc_ods::OdsError> for SdkError {
    fn from(e: casual_calc_ods::OdsError) -> Self {
        SdkError::Ods(e)
    }
}
impl From<ExportError> for SdkError {
    fn from(e: ExportError) -> Self {
        SdkError::Export(e)
    }
}
impl From<RenderError> for SdkError {
    fn from(e: RenderError) -> Self {
        SdkError::Render(e)
    }
}
impl From<TxnError> for SdkError {
    fn from(e: TxnError) -> Self {
        SdkError::Edit(e)
    }
}

/// An open workbook and its editing state.
#[derive(Debug)]
pub struct WorkbookSession {
    workbook: Workbook,
    history: History,
    report: CompatibilityReport,
    config: SessionConfig,
    /// The mode in force, after resolving the config against the file.
    calculation: CalculationMode,
    /// The format this session was opened from, which is the one it saves back.
    ///
    /// Recorded **here**, at the moment the bytes are read, because that is the
    /// only place the answer is known: by the time [`save`](Self::save) runs
    /// there is a `Workbook` and nothing on it remembers whether it arrived as
    /// a package or as a line of commas.
    format: SessionFormat,
    /// Whether an edit has changed a value since the last recalculation. Only
    /// meaningful in manual mode; automatic never leaves it set.
    stale: bool,
    /// Recalculation, and from step three of docs/66 the precedent graph it
    /// keeps between edits.
    recalc: Recalculator,
    /// Operations applied since the host last collected them, **narrowed**.
    ///
    /// Off unless a host asks for it. A collaborative host has to send what it
    /// applied, and the alternative is threading a return value through every
    /// one of the forty-odd entry points that edit — which is the same thing
    /// done forty times, and wrong the first time somebody adds the forty-first.
    ///
    /// Narrowed here rather than by the collector, because narrowing needs the
    /// state the operation was written against and this is the last moment that
    /// state exists.
    applied: Option<Vec<Operation>>,
    /// The package this session was opened from, kept while nothing has been
    /// edited so that saving an untouched file returns it unchanged (P1B-002).
    ///
    /// Dropped the moment anything is edited, which both frees the memory and
    /// makes the invariant impossible to get wrong: there is no way to hand
    /// back stale bytes, because after an edit there are no bytes to hand back.
    ///
    /// The cost is the package's own size, held until the first edit. For a
    /// host that opens from a `Vec<u8>` and drops it — which is the normal
    /// shape — this is the only copy rather than a second one.
    source: Option<Vec<u8>>,
    /// Rows this participant hides for themselves alone (`COL-32`, docs/71).
    ///
    /// On the session and **not** on `Sheet`, because `SUBTOTAL`'s 101–111
    /// codes and `AGGREGATE` reach the sheet: anything stored there is shared
    /// by definition, and a personal view that moved a subtotal would let two
    /// people hold different numbers for the same cell. Never relayed, never
    /// undoable, never saved.
    views: PersonalViews,
}

/// Whether a number-format code renders everything as text — OOXML's `@`.
///
/// Moved with [`WorkbookSession::input_edit`] (`TAURI-002`): it is part of what
/// typed text *means*, not part of any one host.
///
/// A format containing `;` has sections, and only one of them may be textual,
/// so it is not a text format in the sense that matters here — that of whether
/// an entry should be stored verbatim rather than parsed as a number.
fn is_text_format(code: &str) -> bool {
    let trimmed = code.trim();
    !trimmed.is_empty() && !trimmed.contains(';') && trimmed.contains('@')
}

/// Whether an entry has a leading zero worth preserving — `007`, `0123`.
///
/// Parsed as a number these lose the zero, and the entry a person typed is not
/// the one they get back. A single `0` is not this case, which is why a second
/// digit is required.
fn has_leading_zero(input: &str) -> bool {
    let digits = input.strip_prefix(['+', '-']).unwrap_or(input);
    let mut chars = digits.chars();
    chars.next() == Some('0') && chars.next().is_some_and(|c| c.is_ascii_digit())
}

impl WorkbookSession {
    /// A new, empty session with the default configuration.
    pub fn blank() -> Self {
        Self::blank_with(SessionConfig::new())
    }

    /// A new session with one empty sheet — what a host opens a window on.
    ///
    /// [`Self::blank`] returns a workbook of **no** sheets, which is right for
    /// building one up programmatically and wrong for every interactive host:
    /// a window whose workbook has no sheets has nothing to draw, no tab strip
    /// and no cell to put the caret in.
    ///
    /// Both hosts had already worked that out separately and pushed a `Sheet1`
    /// of their own — the browser in `session_new`, the desktop shell in its
    /// own constructor. Two copies of a rule written down nowhere, and a third
    /// host would have had to rediscover it or ship a blank window (`SDK-011`).
    ///
    /// Beside `blank` rather than replacing it: thirty-six callers use `blank`
    /// and most add their own sheets, so changing what it returns would give
    /// them two. A name that says which one you want costs less than a subtle
    /// change to what an existing one means.
    ///
    /// The sheet is called `Sheet1`, because that is what both hosts called it
    /// and what every spreadsheet application names a first sheet.
    #[must_use]
    pub fn with_sheet() -> Self {
        Self::with_sheet_from(SessionConfig::new())
    }

    /// [`Self::with_sheet`], with a configuration.
    #[must_use]
    pub fn with_sheet_from(config: SessionConfig) -> Self {
        let mut session = Self::blank_with(config);
        session.workbook_mut().sheets.push(Sheet::new(
            SheetId(Id::from_parts(SESSION_NAMESPACE, 2)),
            "Sheet1",
        ));
        session
    }

    /// A new, empty session.
    pub fn blank_with(config: SessionConfig) -> Self {
        let mut workbook = Workbook::new(Id::from_parts(SESSION_NAMESPACE, 1));
        apply_environment(&mut workbook, &config);
        Self {
            calculation: config.calculation.unwrap_or_default(),
            history: History::with_depth(config.undo_depth),
            workbook,
            report: CompatibilityReport::default(),
            config,
            stale: false,
            applied: None,
            recalc: Recalculator::new(),
            views: PersonalViews::new(),
            // Nothing was opened, so the format is the one this engine writes
            // by default.
            format: SessionFormat::Xlsx,
            // Not opened from a package, so there is nothing to give back
            // unchanged.
            source: None,
        }
    }

    /// A session over an already-built workbook (e.g. from a CSV import).
    pub fn from_workbook(workbook: Workbook) -> Self {
        Self::from_workbook_with(workbook, SessionConfig::new())
    }

    /// A session over an already-built workbook, configured.
    ///
    /// The format is [`SessionFormat::Xlsx`], because a workbook handed over as
    /// a model came from nowhere in particular. A host that built it by reading
    /// a `.csv` wants [`open_delimited`](Self::open_delimited) instead, which is
    /// the path that remembers.
    pub fn from_workbook_with(mut workbook: Workbook, config: SessionConfig) -> Self {
        apply_environment(&mut workbook, &config);
        recalculate(&mut workbook);
        Self {
            calculation: config.calculation.unwrap_or_default(),
            history: History::with_depth(config.undo_depth),
            workbook,
            report: CompatibilityReport::default(),
            config,
            stale: false,
            applied: None,
            recalc: Recalculator::new(),
            views: PersonalViews::new(),
            format: SessionFormat::Xlsx,
            // Not opened from a package, so there is nothing to give back
            // unchanged.
            source: None,
        }
    }

    /// Open a `.xlsx` package, importing and recalculating it.
    pub fn open(bytes: Vec<u8>) -> Result<Self, SdkError> {
        Self::open_with(bytes, SessionConfig::new())
    }

    /// Open a `.xlsx` package under a given configuration.
    ///
    /// A workbook saved in manual calculation mode is opened in manual mode
    /// unless the config says otherwise — and is **not** recalculated on the
    /// way in. Its author turned automatic calculation off, usually because a
    /// full recalc is slow enough to be disruptive; doing one anyway before
    /// they have seen the file is the opposite of what they asked for.
    pub fn open_with(bytes: Vec<u8>, config: SessionConfig) -> Result<Self, SdkError> {
        Self::open_cancellable(bytes, config, &casual_calc_model::Never)
    }

    /// The same, with a way to stop it.
    ///
    /// Opening is the longest thing a session does, and until `SEC-012` the
    /// only way out of it was for it to finish — on a browser's single thread,
    /// which docs/07 and docs/21 both said would not be the case. A cancelled
    /// open returns [`SdkError::Import`] carrying `ImportError::Cancelled` and
    /// leaves no session behind.
    ///
    /// # Errors
    ///
    /// As [`open_with`](Self::open_with), plus a cancellation.
    pub fn open_cancellable(
        bytes: Vec<u8>,
        config: SessionConfig,
        cancel: &dyn casual_calc_model::Cancel,
    ) -> Result<Self, SdkError> {
        // Kept before the import consumes it: an untouched file saves as
        // itself, and that is only possible if the original survives the read.
        let source = bytes.clone();
        let outcome = import_package_cancellable(bytes, config.limits, cancel)?;
        let mut workbook = outcome.workbook;
        apply_environment(&mut workbook, &config);
        let calculation = config
            .calculation
            .unwrap_or_else(|| file_calculation_mode(&workbook));
        if calculation == CalculationMode::Automatic {
            recalculate(&mut workbook);
        }
        Ok(Self {
            workbook,
            history: History::with_depth(config.undo_depth),
            report: outcome.report,
            config,
            calculation,
            // A manual-mode file arrives with its own cached values, which are
            // what its author last saw; nothing is stale until something is
            // edited.
            stale: false,
            applied: None,
            recalc: Recalculator::new(),
            views: PersonalViews::new(),
            format: SessionFormat::Xlsx,
            source: Some(source),
        })
    }

    /// Open bytes in a named format, whichever it is.
    ///
    /// For a caller that learned the format from a filename rather than from
    /// the bytes — a WOPI adapter is handed `Q3.csv` and a body, and has no
    /// business branching on the extension itself when
    /// [`SessionFormat::for_extension`] already knows.
    ///
    /// # Errors
    ///
    /// As [`open`](Self::open) or [`open_delimited`](Self::open_delimited),
    /// according to `format`.
    pub fn open_as(bytes: Vec<u8>, format: SessionFormat) -> Result<Self, SdkError> {
        Self::open_as_with(bytes, format, SessionConfig::new())
    }

    /// The same, under a given configuration.
    ///
    /// # Errors
    ///
    /// As [`open_as`](Self::open_as).
    pub fn open_as_with(
        bytes: Vec<u8>,
        format: SessionFormat,
        config: SessionConfig,
    ) -> Result<Self, SdkError> {
        Self::open_as_cancellable(bytes, format, config, &casual_calc_model::Never)
    }

    /// The same, with a way to stop it.
    ///
    /// The package formats are the long ones — admission walks every cell — and
    /// they are the ones this can interrupt; delimited text and OpenDocument run
    /// to completion. A host under a time budget calls **this** rather than
    /// branching on the format itself to decide which open is stoppable, which
    /// is how `.xlsm` would have arrived unstoppable on a browser's single
    /// thread while `.xlsx` was not.
    ///
    /// # Errors
    ///
    /// As [`open_as`](Self::open_as), plus a cancellation.
    pub fn open_as_cancellable(
        bytes: Vec<u8>,
        format: SessionFormat,
        config: SessionConfig,
        cancel: &dyn casual_calc_model::Cancel,
    ) -> Result<Self, SdkError> {
        match format {
            SessionFormat::Xlsx => Self::open_cancellable(bytes, config, cancel),
            // The **same reader**, and that is the whole point: an `.xlsm` is
            // an OOXML package whose sheets this engine has always been able
            // to read, and a name check was all that refused it (`IO-04`).
            // What differs is only what the session remembers, so that saving
            // writes a macro-enabled package back rather than an `.xlsx` with
            // the macros quietly gone.
            SessionFormat::Xlsm => {
                let mut session = Self::open_cancellable(bytes, config, cancel)?;
                session.format = SessionFormat::Xlsm;
                Ok(session)
            }
            SessionFormat::Ods => Self::open_ods_with(bytes, config),
            SessionFormat::Delimited(delimiter) => {
                Self::open_delimited_with(bytes, delimiter, config)
            }
        }
    }

    /// Open an OpenDocument spreadsheet.
    ///
    /// **The format is remembered**, so [`save`](Self::save) writes a `.ods`
    /// back rather than an OOXML package. What that writer cannot carry —
    /// formatting, merges, widths, validation, charts, images — is named by
    /// [`format_loss`](Self::format_loss) before the save, and what the *file*
    /// lost coming in is named by
    /// [`compatibility_report`](Self::compatibility_report). The two are
    /// different questions and both have answers.
    ///
    /// # Errors
    ///
    /// [`SdkError::Ods`] if the bytes are not a readable OpenDocument package.
    pub fn open_ods(bytes: Vec<u8>) -> Result<Self, SdkError> {
        Self::open_ods_with(bytes, SessionConfig::new())
    }

    /// The same, under a given configuration.
    ///
    /// # Errors
    ///
    /// As [`open_ods`](Self::open_ods).
    pub fn open_ods_with(bytes: Vec<u8>, config: SessionConfig) -> Result<Self, SdkError> {
        let (mut workbook, report) = casual_calc_ods::import_ods(&bytes)?;
        apply_environment(&mut workbook, &config);
        // The file's own cached values came from LibreOffice's engine, and this
        // reader does not carry a calculation mode to say otherwise; a formula
        // that translated is recalculated here so the session starts consistent
        // with itself.
        recalculate(&mut workbook);
        Ok(Self {
            calculation: config.calculation.unwrap_or_default(),
            history: History::with_depth(config.undo_depth),
            workbook,
            report,
            config,
            stale: false,
            applied: None,
            recalc: Recalculator::new(),
            views: PersonalViews::new(),
            format: SessionFormat::Ods,
            // Same guarantee as a package: opened and not edited saves as
            // itself, byte for byte. It matters more here than anywhere, since
            // this writer keeps far less than the reader takes in — a save that
            // rebuilt the file would throw away the styles it merely could not
            // model.
            source: Some(bytes),
        })
    }

    /// Open delimited text (CSV / TSV / PSV) by its separator byte.
    ///
    /// **The format is remembered**, so [`save`](Self::save) writes the same
    /// kind of file back rather than an OOXML package. What that file cannot
    /// carry — every sheet but the first, formulas as formulas, formatting — is
    /// named by [`format_loss`](Self::format_loss); it is never dropped
    /// silently.
    ///
    /// # Errors
    ///
    /// [`SdkError::Io`] if the bytes are not UTF-8. Delimited text has no
    /// encoding declaration, so a host that may be handed UTF-16 decodes before
    /// calling this.
    pub fn open_delimited(bytes: Vec<u8>, delimiter: u8) -> Result<Self, SdkError> {
        Self::open_delimited_with(bytes, delimiter, SessionConfig::new())
    }

    /// The same, under a given configuration.
    ///
    /// # Errors
    ///
    /// As [`open_delimited`](Self::open_delimited).
    pub fn open_delimited_with(
        bytes: Vec<u8>,
        delimiter: u8,
        config: SessionConfig,
    ) -> Result<Self, SdkError> {
        let mut workbook = read_delimited(&bytes, delimiter)?;
        apply_environment(&mut workbook, &config);
        // A parsed field is a literal; nothing here has a formula. The pass is
        // still run so a session opened this way starts in the same state as
        // any other — `stale` false and cached values current.
        recalculate(&mut workbook);
        Ok(Self {
            calculation: config.calculation.unwrap_or_default(),
            history: History::with_depth(config.undo_depth),
            workbook,
            // Reading delimited text degrades nothing: every field is either
            // typed or interned, and what it *cannot* represent is not in the
            // file. The losses of this format are on the way out, and
            // `format_loss` is where they are counted.
            report: CompatibilityReport::default(),
            config,
            stale: false,
            applied: None,
            recalc: Recalculator::new(),
            views: PersonalViews::new(),
            format: SessionFormat::Delimited(delimiter),
            // Same guarantee as a package: opened and not edited saves as
            // itself, byte for byte. Without it, merely opening a file
            // rewrites its line endings and re-quotes its fields.
            source: Some(bytes),
        })
    }

    /// The configuration in force.
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// The calculation mode in force, after resolving the config against the
    /// file's own `<calcPr calcMode>`.
    pub fn calculation_mode(&self) -> CalculationMode {
        self.calculation
    }

    /// Switch calculation mode. Switching to automatic recalculates at once if
    /// anything is outstanding — the point of the mode is that values are
    /// current.
    pub fn set_calculation_mode(&mut self, mode: CalculationMode) {
        self.calculation = mode;
        self.config.calculation = Some(mode);
        write_calculation_mode(&mut self.workbook, mode);
        if mode == CalculationMode::Automatic && self.stale {
            self.recalculate();
        }
    }

    /// Whether an edit has changed a value that has not been recalculated.
    /// Always false in automatic mode.
    pub fn needs_recalculation(&self) -> bool {
        self.stale
    }

    /// Replace the clock and seed calculations read.
    ///
    /// Recalculates in automatic mode: the volatile functions have new answers
    /// the moment this changes, and leaving the old ones on screen while
    /// claiming the clock moved is worse than the cost of the pass.
    pub fn set_environment(&mut self, environment: Environment) {
        self.config.environment = environment;
        apply_environment(&mut self.workbook, &self.config);
        if self.calculation == CalculationMode::Automatic {
            recalculate(&mut self.workbook);
        } else {
            self.stale = true;
        }
    }

    /// The normalized workbook model.
    /// Rows this participant hides for themselves alone (`COL-32`).
    ///
    /// See [`views`] for why this is session state and not document state.
    #[must_use]
    pub fn views(&self) -> &PersonalViews {
        &self.views
    }

    /// Hide `rows` on `sheet` **for this participant only**.
    ///
    /// Not an edit: nothing goes on the wire, nothing enters the undo history,
    /// nothing reaches the saved file, and no cell value changes — a `SUBTOTAL`
    /// under a personal view reads the same number here as on every other
    /// participant's screen. That surprises people once; the alternative is a
    /// spreadsheet where one cell holds two different numbers.
    ///
    /// It is deliberately *not* routed through [`edit`](Self::edit), and there
    /// is a test asserting the outgoing log stays empty across it.
    pub fn set_personal_filter(&mut self, sheet: usize, rows: BTreeSet<u32>) {
        self.views.set(sheet, rows);
    }

    /// Drop this participant's view of one sheet.
    pub fn clear_personal_view(&mut self, sheet: usize) {
        self.views.clear(sheet);
    }

    /// Drop every personal view.
    ///
    /// Worth its own call because undo will not do it: a personal view is not a
    /// document edit, so undo after applying one undoes the last thing done to
    /// the *document*. Clearing has to be one obvious action instead.
    pub fn clear_all_personal_views(&mut self) {
        self.views.clear_all();
    }

    /// Whether `row` should be **drawn** on `sheet` — the layout's question.
    ///
    /// The union of the shared hidden sets and this participant's own. The
    /// evaluator asks a different question, of `Sheet::is_row_hidden`, and
    /// gets only the shared half; the two look identical and are not.
    #[must_use]
    pub fn is_row_visible(&self, sheet: usize, row: u32) -> bool {
        let shared = self
            .workbook
            .sheets
            .get(sheet)
            .is_some_and(|s| s.is_row_hidden(row));
        !shared && !self.views.hides(sheet, row)
    }

    /// Keep view keys pointing at the sheets they were applied to.
    ///
    /// Called from **every** path that applies an operation to this session's
    /// workbook — [`edit`](Self::edit), [`undo`](Self::undo),
    /// [`redo`](Self::redo) and [`apply_raw`](Self::apply_raw) — because the
    /// index a personal view is keyed by moves whenever a sheet does, and none
    /// of those four is more or less a sheet move than the others. Only `edit`
    /// called it for a long time, which made the renumbering **one-way**: an
    /// insert moved the view and undoing the insert did not move it back, so
    /// the key was left on a sheet that no longer existed (`FID-38`).
    ///
    /// A `Batch` is walked in order. Its members are applied in order and each
    /// one's indices are written against the index space the previous member
    /// produced, so the remappings compose the same way. Not a corner case: a
    /// `RemoveSheet` whose sheet was charted has a `Batch` for its inverse, so
    /// this is the shape undo hands back for the very operation that moves the
    /// most sheets.
    fn resequence_views(&mut self, op: &Operation) {
        if self.views.is_empty() {
            return;
        }
        match *op {
            Operation::Batch(ref ops) => {
                for member in ops {
                    self.resequence_views(member);
                }
            }
            Operation::InsertSheet { index, .. } => {
                self.views
                    .resequence(|at| Some(if at >= index { at + 1 } else { at }));
            }
            Operation::RemoveSheet { index } => {
                self.views.resequence(|at| match at.cmp(&index) {
                    std::cmp::Ordering::Equal => None,
                    std::cmp::Ordering::Greater => Some(at - 1),
                    std::cmp::Ordering::Less => Some(at),
                });
            }
            Operation::MoveSheet { from, to } => {
                self.views.resequence(|at| {
                    Some(if at == from {
                        to
                    } else if from < to && at > from && at <= to {
                        at - 1
                    } else if to < from && at >= to && at < from {
                        at + 1
                    } else {
                        at
                    })
                });
            }
            _ => {}
        }
    }

    pub fn workbook(&self) -> &Workbook {
        &self.workbook
    }

    /// The compatibility report from import (empty for a blank session).
    pub fn compatibility_report(&self) -> &CompatibilityReport {
        &self.report
    }

    /// Apply an edit operation, then recalculate. Records undo history.
    ///
    /// The recalc is scoped to what the edit can affect: value edits recompute
    /// only the changed cells' transitive dependents (incremental); pure style
    /// or geometry edits skip recalc entirely; structural edits (insert/delete
    /// rows or columns), which shift references workbook-wide, do a full recalc.
    /// What a cell would look like in the formula bar.
    ///
    /// The inverse of [`Self::input_edit`], and moved here for the same reason
    /// (`TAURI-002`): together they are the contract for what typed text means,
    /// and a contract with one half in a host and the other in the engine is one
    /// a second host has to guess at. A formula reads back as `=…`; everything
    /// else reads back as it would be typed, which is what makes Find & Replace
    /// operate on something Replace can actually rewrite.
    pub fn cell_input(&self, sheet: usize, at: CellRef) -> String {
        let wb = self.workbook();
        let Some(cell) = wb.sheets.get(sheet).and_then(|s| s.cells.get(at)) else {
            return String::new();
        };
        if let Some(handle) = cell.formula
            && let Some(expr) = wb.formula(handle)
        {
            // `print_at`, not `Display`. A formula is stored *relative* to the
            // cell holding it (`PERF-11`), so printing it absolutely resolves
            // every reference against A1 — `=A1*2` in B1 reads back as
            // `=#REF!*2`, which is what the first version of this did.
            return format!(
                "={}",
                casual_calc_formula::print_at(
                    expr,
                    casual_calc_formula::stored::Origin::at(at.row, at.col)
                )
            );
        }
        match &cell.value {
            CellValue::Empty => String::new(),
            CellValue::Number(n) => format!("{n}"),
            CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
            CellValue::Error(e) => e.to_string(),
            CellValue::SharedString(id) | CellValue::InlineString(id) => {
                wb.strings.get(*id).unwrap_or_default().to_owned()
            }
        }
    }

    /// The edit that typing `input` into a cell makes.
    ///
    /// **Moved here from the WebAssembly bridge (`TAURI-002`).** It was
    /// `pub(crate)` in `casual-calc-wasm`, which meant the rule for what typed
    /// text *means* — when it is a formula, when it is a number, when a leading
    /// apostrophe forces text, which number format an entry implies — lived in
    /// one host rather than in the engine. A second host could not type into a
    /// cell without reimplementing all of it, and the first thing it would get
    /// wrong is the part nobody remembers.
    ///
    /// Nothing about it was ever WebAssembly-specific; it takes a self and
    /// returns an `Operation`, which is this crate's own vocabulary.
    ///
    /// It returns the operation rather than applying it, so a caller can put it
    /// in a batch — a paste is many of these and one undo.
    pub fn input_edit(&mut self, sheet: usize, at: CellRef, input: &str) -> Operation {
        let trimmed = input.trim();
        let existing_style = self
            .workbook()
            .sheets
            .get(sheet)
            .and_then(|s| s.cells.get(at))
            .and_then(|c| c.style);

        if trimmed.is_empty() {
            return Operation::ClearCell { sheet, at };
        }

        // A leading apostrophe forces the rest to be text, however numeric it
        // looks, and is not part of the value. The marker has to be recorded on the
        // style (`quotePrefix`), not merely obeyed here: without it the cell saves
        // as a plain string and Excel re-reads `0123` as the number 123 the next
        // time the file is opened.
        if let Some(body) = trimmed.strip_prefix('\'') {
            let mut style = existing_style
                .and_then(|id| self.workbook().styles.get(id))
                .cloned()
                .unwrap_or_default();
            style.quote_prefix = true;
            let style = self.workbook_mut().intern_style(style);
            let text = self.workbook_mut().intern_string(body);
            let mut cell = Cell::value(CellValue::InlineString(text));
            cell.style = Some(style);
            return Operation::SetCell {
                sheet,
                at,
                cell: Some(cell),
            };
        }

        if let Some(body) = trimmed.strip_prefix('=')
            && let Ok(expr) = parse(body)
        {
            let handle = self
                .workbook_mut()
                .store_formula_at(expr, Origin::at(at.row, at.col));
            let mut cell = Cell::value(CellValue::Empty);
            cell.style = existing_style;
            cell.formula = Some(handle);
            return Operation::SetCell {
                sheet,
                at,
                cell: Some(cell),
            };
        }

        // A cell formatted as Text (`@`) keeps what was typed as text — that is the
        // entire point of the format, and coercing "007" or "1-2" to a number here
        // is a silent edit of what the user entered.
        let text_formatted = existing_style
            .and_then(|id| self.workbook().styles.get(id))
            .and_then(|st| st.number_format.as_deref())
            .is_some_and(is_text_format);
        // An ISO date becomes a real date, keeping the same rules the importer uses
        // so that typing a date and pasting one from a file agree. It brings its own
        // format, since a bare serial displayed as a number is not what was typed.
        if !text_formatted && let Some((serial, code)) = casual_calc_io::parse_iso_datetime(trimmed)
        {
            // An existing date format wins — someone who set dd/mm/yyyy on the
            // column means it, and retyping a cell should not reset the column.
            let keep = existing_style
                .and_then(|id| self.workbook().styles.get(id))
                .and_then(|st| st.number_format.as_deref())
                .is_some_and(casual_calc_io::is_date_format);
            let style = if keep {
                existing_style
            } else {
                let mut style = existing_style
                    .and_then(|id| self.workbook().styles.get(id))
                    .cloned()
                    .unwrap_or_default();
                style.number_format = Some(code.to_owned());
                Some(self.workbook_mut().intern_style(style))
            };
            let mut cell = Cell::value(CellValue::Number(serial));
            cell.style = style;
            return Operation::SetCell {
                sheet,
                at,
                cell: Some(cell),
            };
        }

        let value = match trimmed.parse::<f64>() {
            // **`is_finite`, and the reason is a round trip rather than taste**
            // (`WASM-02`). `f64::from_str` accepts `1e400` and answers `inf`, so
            // typing it stored `Number(inf)`. There is no CSV spelling of infinity
            // that reads back as a number — `casual_calc_io::type_field` requires
            // finite, correctly — so the value left the editor as a number, was
            // written as `inf`, and returned as **text**. A cell that changes kind
            // by being saved.
            //
            // The model should not hold one at all: `inf` propagates through every
            // arithmetic it touches, and `.xlsx` cannot spell it either.
            //
            // Kept as what was typed, which is what the reader does with anything
            // it will not type as a number. The person sees their own text back
            // rather than a number they did not write.
            Ok(n) if n.is_finite() && !text_formatted && !has_leading_zero(trimmed) => {
                CellValue::Number(n)
            }
            _ => CellValue::InlineString(self.workbook_mut().intern_string(trimmed)),
        };
        Operation::SetValue { sheet, at, value }
    }

    pub fn edit(&mut self, op: Operation) -> Result<(), SdkError> {
        // Before anything else, and before the history records a step: a
        // refused edit must leave no trace, or undo has an entry that undoes
        // nothing.
        if self.config.read_only {
            return Err(SdkError::ReadOnly);
        }
        let plan = recalc_plan(&op);
        // Narrowed **before** the edit is applied, because afterwards the state
        // it was written against is gone and an operation still claiming to
        // change everything contends with every concurrent edit.
        //
        // Recorded **after** it succeeds, which is a different question and used
        // to have the same answer. A refused edit was already in the outgoing
        // log by the time `apply` said no, so this client would send the server
        // — and through it every peer — an operation it had itself rejected.
        // Nothing downstream can detect that: the operation is well formed, it
        // is simply not what happened here.
        let candidate = self
            .applied
            .is_some()
            .then(|| op.clone().narrowed(&self.workbook));
        // A personal view is keyed by sheet index, so the structural operations
        // renumber it. Done here rather than in the editor because the index is
        // the session's own bookkeeping: leave it and the view keeps hiding
        // rows on whichever sheet inherits the number, with nothing on the wire
        // to explain it and nothing in the history to undo.
        self.resequence_views(&op);
        self.history.apply(&mut self.workbook, op)?;
        if let Some(narrowed) = candidate
            && let Some(log) = self.applied.as_mut()
        {
            log.push(narrowed);
        }
        self.source = None;
        // Dropped **before** the manual-mode return, not inside the match
        // below. Whether a recalculation is wanted right now and whether the
        // graph still describes this document are different questions, and
        // answering only the first left manual mode holding a graph about a
        // document that had moved under it — the deferred recalculation then
        // ran against it and was wrong in exactly the way deferring was
        // supposed to be safe.
        if matches!(plan, RecalcPlan::Full) {
            self.recalc.invalidate();
        }
        // Manual mode still applies the edit — it is calculation that is
        // deferred, not editing — and records that something is outstanding so
        // the host can say so.
        if self.calculation == CalculationMode::Manual {
            self.stale |= !matches!(plan, RecalcPlan::Skip);
            return Ok(());
        }
        match plan {
            RecalcPlan::Skip => {}
            RecalcPlan::Cells(cells) => self.recalc.recalculate(&mut self.workbook, &cells),
            // The graph was already dropped above; a structural edit shifts
            // every reference past its insertion point, so whatever it said
            // was about a document that no longer exists.
            RecalcPlan::Full => recalculate(&mut self.workbook),
        }
        Ok(())
    }

    /// How many edits have ever been applied to this session, counting up and
    /// never down.
    ///
    /// A host records this when it saves and compares it later to answer "is
    /// there unsaved work?". Undo and redo both count as edits, so undoing back
    /// to the save point still reports a difference — which is the safe
    /// direction: a needless warning costs a click, and the other mistake costs
    /// the document.
    pub fn edits_applied(&self) -> u64 {
        self.history.edits_applied()
    }

    /// Whether an edit can be undone.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// What undo would reverse, for a menu label.
    pub fn undo_label(&self) -> Option<&'static str> {
        self.history.undo_label()
    }

    /// What redo would reapply.
    pub fn redo_label(&self) -> Option<&'static str> {
        self.history.redo_label()
    }

    /// Whether an undone edit can be redone.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Undo the last edit, then recalculate.
    ///
    /// **An undo is an edit**, and travels like one. It changes the document, so
    /// a collaborating host has to send it; until it did, one participant
    /// reverted while the server and every peer kept the change — a divergence
    /// nothing later disagrees loudly enough to reveal, and one that no amount
    /// of subsequent editing repairs.
    ///
    /// It rides the same outgoing log as [`edit`](Self::edit) rather than a
    /// second channel: the server transforms whatever arrives against everything
    /// committed since the sender's revision, which is exactly the treatment an
    /// inverse needs and already exists.
    pub fn undo(&mut self) -> Result<(), SdkError> {
        // Before anything is applied, because the state this would destroy is
        // the evidence that it must not be (docs/69). Cell edits are allowed to
        // clobber — that is last-writer-wins, and the value is one cell somebody
        // else's stack still holds. A structural undo is different in kind: it
        // takes work that was never in the band when the undo was recorded, and
        // no undo stack anywhere contains it.
        if let Some(next) = self.history.peek_undo()
            && let Some(blocked) = undo_would_discard(&self.workbook, next)
        {
            return Err(SdkError::UndoWouldDiscard(blocked));
        }
        let applied = self.history.undo(&mut self.workbook)?;
        // The undo stack holds *inverses*, so what came back is the operation
        // that just ran against this workbook — an insert's undo is a removal,
        // and it renumbers exactly as the removal a user typed would.
        if let Some(op) = &applied {
            self.resequence_views(op);
        }
        self.record_for_peers(applied);
        self.source = None;
        // Undo replays whatever it reverses, which may have been structural.
        // The history does not say which, and guessing to keep a graph is
        // exactly the trade that makes staleness possible for a saving that
        // does not matter here: undo is not a keystroke.
        self.recalc.invalidate();
        self.recalculate_if_automatic();
        Ok(())
    }

    /// Redo the last undone edit, then recalculate.
    ///
    /// A redo is a fresh intention rather than the cancellation of one, and is
    /// transmitted for the same reason as [`undo`](Self::undo).
    pub fn redo(&mut self) -> Result<(), SdkError> {
        let applied = self.history.redo(&mut self.workbook)?;
        if let Some(op) = &applied {
            self.resequence_views(op);
        }
        self.record_for_peers(applied);
        self.source = None;
        self.recalc.invalidate();
        self.recalculate_if_automatic();
        Ok(())
    }

    /// Queue an already-applied operation for collaborators.
    ///
    /// Narrowed against the workbook **after** the operation ran, unlike
    /// [`edit`](Self::edit), which must narrow before. The difference is which
    /// state the operation describes: an edit is written against the document
    /// the user was looking at, while an undo's inverse has just been executed
    /// and is described by the document it produced.
    ///
    /// Nothing to do when the stack was empty — pressing undo with nothing to
    /// undo is not an event anybody needs to hear about.
    fn record_for_peers(&mut self, applied: Option<Operation>) {
        let (Some(op), Some(_)) = (applied, self.applied.as_ref()) else {
            return;
        };
        let narrowed = op.narrowed(&self.workbook);
        if let Some(log) = self.applied.as_mut() {
            log.push(narrowed);
        }
    }

    /// Start recording what this session applies, for a host that has to send
    /// it on.
    pub fn record_applied(&mut self) {
        self.applied.get_or_insert_with(Vec::new);
    }

    /// Stop recording, discarding anything uncollected.
    pub fn stop_recording(&mut self) {
        self.applied = None;
    }

    /// Whether anything applied is waiting to be collected.
    ///
    /// A read, where [`take_applied`](Self::take_applied) is a take. A host
    /// asking "is there unsaved work" must not empty the queue to find out, and
    /// must not miss what is sitting here: between an edit and the next flush,
    /// this buffer is the *only* place that work exists.
    #[must_use]
    pub fn has_applied(&self) -> bool {
        self.applied.as_ref().is_some_and(|log| !log.is_empty())
    }

    /// Take everything applied since the last call.
    #[must_use]
    pub fn take_applied(&mut self) -> Vec<Operation> {
        self.applied
            .as_mut()
            .map(core::mem::take)
            .unwrap_or_default()
    }

    /// Discard the undo history, making the current state the document's
    /// starting point.
    ///
    /// Call it after populating a fresh session, the way a host seeds a
    /// template or restores a document from its own store. Those writes go
    /// through [`edit`](Self::edit) so they recalculate and stay consistent,
    /// which also means they land on the undo stack — and a user who presses
    /// Ctrl+Z enough times then walks backwards out of the document they were
    /// given, one cell at a time, into an empty sheet. No spreadsheet does
    /// that, because opening a file is not an edit.
    ///
    /// [`open`](Self::open) has no need of it: it builds the workbook by import
    /// rather than by editing, so its history is empty already.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Recompute all formula cells — Excel's Calculate Now, and the only thing
    /// that brings a manual-mode workbook up to date.
    pub fn recalculate(&mut self) {
        recalculate(&mut self.workbook);
        self.stale = false;
    }

    /// The same, with a way to stop it.
    ///
    /// A cancelled recalculation **keeps what it computed** and leaves the
    /// session marked stale, so the next automatic pass finishes the job. That
    /// is the difference from a cancelled open: there is no half-built document
    /// to throw away, only a document whose cached values were already stale
    /// when this started.
    pub fn recalculate_cancellable(
        &mut self,
        cancel: &dyn casual_calc_model::Cancel,
    ) -> Recalculated {
        let outcome = recalculate_cancellable(&mut self.workbook, cancel);
        self.stale = outcome != Recalculated::Fully;
        outcome
    }

    fn recalculate_if_automatic(&mut self) {
        if self.calculation == CalculationMode::Automatic {
            self.recalculate();
        } else {
            self.stale = true;
        }
    }

    /// Whether this session refuses edits.
    pub fn is_read_only(&self) -> bool {
        self.config.read_only
    }

    /// Change the configuration of a live session.
    ///
    /// Mutable rather than replace-only because a host toggles read-only far
    /// more often than it opens a workbook — a preview that becomes editable
    /// once permissions load should not have to reopen the file to say so.
    pub fn config_mut(&mut self) -> &mut SessionConfig {
        &mut self.config
    }

    /// Apply an edit without recording history (e.g. programmatic setup),
    /// returning the inverse operation.
    ///
    /// Refused in a read-only session too. This is the path a host reaches for
    /// when it wants to bypass undo, and a read-only mode with a documented
    /// bypass is not one.
    pub fn apply_raw(&mut self, op: Operation) -> Result<Operation, SdkError> {
        // Refusal first. Clearing `source` before deciding whether to refuse
        // threw away the untouched-original guarantee on behalf of an edit that
        // never happened — the same "a refused edit must leave no trace" rule
        // the outgoing log above now keeps.
        if self.config.read_only {
            return Err(SdkError::ReadOnly);
        }
        self.source = None;
        // Bypassing the *history* is what this is for; bypassing the view
        // renumbering was never part of it. Before the apply, as in `edit`.
        self.resequence_views(&op);
        // The op is applied without classification, so it may have been a
        // structural one. Same reasoning as `workbook_mut` below.
        self.recalc.invalidate();
        // Bypassing history is the point; bypassing the dirty signal was not
        // (`FID-39`). `edits_applied` promises a host that it answers "is there
        // unsaved work?", and this changes the document.
        self.history.note_foreign_change();
        Ok(apply(&mut self.workbook, op)?)
    }

    /// A mutable view of the workbook, for programmatic construction/setup.
    pub fn workbook_mut(&mut self) -> &mut Workbook {
        // Handing out a `&mut Workbook` is handing out the right to change
        // anything, and this cannot see what happens next — so the untouched
        // guarantee ends here whether or not the caller writes a single byte.
        // Conservative on purpose: giving back a stale package is silent data
        // loss, and re-serializing an unchanged workbook costs only time.
        self.source = None;
        // And for exactly that reason the kept precedent graph ends here too: a
        // caller may rewrite every formula in the book through this reference
        // and nothing would observe it.
        self.recalc.invalidate();
        // **The dirty counter is deliberately not moved here** (`FID-44`).
        //
        // `FID-39` did move it, on the reasoning that this cannot see whether a
        // write happens so it should assume one — the same conservatism that
        // drops `source` two lines above. That was wrong about *who calls this*.
        // `workbook_mut` is not only a host's escape hatch: the wasm layer uses
        // it as an ordinary accessor **inside** an edit, to intern a style or
        // store a formula. So every such edit counted twice, and the editor's
        // draft bar — which reads this number and states it to the user —
        // reported an edit count that had roughly doubled. Three browser tests
        // caught it on `main`; the workspace tests could not, because nothing
        // in Rust reads the count the way that bar does.
        //
        // `source` can be conservative because a needless re-serialize costs
        // only time. A count is *shown to a person*, so an over-count is not
        // the safe direction — it is a wrong statement.
        &mut self.workbook
    }

    /// Which scripts this document contains that the renderer cannot draw.
    ///
    /// Ask before shipping a PNG. Fonts are supplied by the host rather than
    /// bundled ([ADR-018](../../../docs/64-TEXT-SHAPING.md)), which is the right
    /// trade and leaves one sharp edge: a sheet in a script nobody registered
    /// renders as a row of boxes, and a box looks like a bug in the renderer
    /// rather than a font that was never installed. This is the difference
    /// between a support ticket and a sentence naming what to install.
    ///
    /// Empty is the answer for the overwhelming majority of documents, and an
    /// empty vector costs one pass over the string table to obtain.
    ///
    /// Only the **editor** is exempt from caring: it draws through the browser,
    /// which supplies its own faces. This is about the headless renderer —
    /// thumbnails, previews, server-side exports.
    #[must_use]
    pub fn missing_font_coverage(&self) -> Vec<casual_calc_render::MissingScript> {
        // The string table holds every piece of text in the document exactly
        // once, which is both cheaper than walking cells and more complete:
        // a string is there whether or not any cell currently shows it.
        let mut text = String::new();
        for s in self.workbook.strings.iter() {
            text.push_str(s);
        }
        // Sheet names are drawn by the editor's tab strip rather than by this
        // renderer, so they are deliberately not included: reporting a script
        // that no PNG will ever contain is a false alarm, and a false alarm in
        // a diagnostic is worse than no diagnostic.
        casual_calc_render::missing_scripts(&text)
    }

    /// The grid geometry (column widths / row heights) of a sheet.
    fn geometry(&self, sheet_index: usize) -> GridGeometry {
        self.workbook
            .sheets
            .get(sheet_index)
            .map(GridGeometry::for_sheet)
            .unwrap_or_default()
    }

    /// Lay out a viewport of a sheet into a display list.
    ///
    /// Formula conditional-format rules are resolved, so what this returns is
    /// what the canvas and the PNG show.
    pub fn layout(&self, sheet_index: usize, viewport: &Viewport) -> DisplayList {
        casual_calc_layout::layout_viewport_with(
            &self.workbook,
            sheet_index,
            &self.geometry(sheet_index),
            viewport,
            &casual_calc_eval::conditional::CfExpressionRules::new(&self.workbook, sheet_index),
        )
    }

    /// Render a viewport of a sheet to PNG bytes.
    ///
    /// Honours the sheet's frozen rows and columns: the pinned bands hold still
    /// and the body scrolls under them, as they do in the editor canvas. An
    /// unfrozen sheet renders exactly as before.
    pub fn render_png(
        &self,
        sheet_index: usize,
        viewport: &Viewport,
        dpi: u32,
    ) -> Result<Vec<u8>, SdkError> {
        let geometry = self.geometry(sheet_index);
        render_sheet_png(&self.workbook, sheet_index, &geometry, viewport, dpi)
    }

    /// Render a viewport, and say what its pictures cost.
    ///
    /// The same bytes [`render_png`](Self::render_png) returns, plus the half
    /// of the answer that signature cannot carry: the pictures are drawn from
    /// this session's own media either way, and this one names the ones that
    /// could not be.
    ///
    /// The report is a [`CompatibilityReport`] — the type
    /// [`compatibility_report`](Self::compatibility_report) and
    /// [`format_loss`](Self::format_loss) already speak — so a host has one
    /// place to show what a document lost rather than two that must be merged
    /// by hand. [`image_loss`] is the fold, and
    /// [`render_sheet_png_with_report`] the unfolded form for a caller that
    /// wants the part paths.
    ///
    /// # Errors
    ///
    /// As [`render_png`](Self::render_png).
    pub fn render_png_with_report(
        &self,
        sheet_index: usize,
        viewport: &Viewport,
        dpi: u32,
    ) -> Result<(Vec<u8>, CompatibilityReport), SdkError> {
        let geometry = self.geometry(sheet_index);
        let (png, images) =
            render_sheet_png_with_report(&self.workbook, sheet_index, &geometry, viewport, dpi)?;
        Ok((png, image_loss(&images)))
    }

    /// Export a sheet as a paginated PDF.
    ///
    /// The whole sheet, cut into pages by
    /// [`casual_calc_layout::print::paginate`] — not a viewport. A printout is
    /// not a screenshot, and a host asking for one wants the document, not the
    /// part of it somebody had scrolled to.
    ///
    /// An empty sheet gives a PDF with no pages, which is a file every viewer
    /// opens and shows as empty. That is deliberate: a blank sheet of paper
    /// would be indistinguishable from a page whose content failed to draw.
    ///
    /// # Errors
    ///
    /// [`SdkError::Render`] if a page has no printable area at all.
    pub fn export_pdf(&self, sheet_index: usize) -> Result<Vec<u8>, SdkError> {
        self.export_pdf_with_report(sheet_index).map(|(pdf, _)| pdf)
    }

    /// [`export_pdf`](Self::export_pdf), and what the printout could not carry.
    ///
    /// The PDF backend draws no pictures (`IO-03`), so a sheet with images
    /// gives a report naming them rather than a page with holes in it. The
    /// report is the same [`CompatibilityReport`] the rest of this facade
    /// speaks, so a host has one place to show loss and not two.
    ///
    /// # Errors
    ///
    /// As [`export_pdf`](Self::export_pdf).
    pub fn export_pdf_with_report(
        &self,
        sheet_index: usize,
    ) -> Result<(Vec<u8>, CompatibilityReport), SdkError> {
        self.export_pdf_with_context(sheet_index, &PrintContext::default())
    }

    /// [`export_pdf_with_report`](Self::export_pdf_with_report), with the host
    /// values a header or footer may ask for.
    ///
    /// `&D`, `&T`, `&F` and `&Z` cannot be answered by the engine: it reads no
    /// clock and knows no file name, because **the host owns policy**
    /// (AGENTS.md). A host that wants a dated printout passes the date here.
    /// One that does not gets those codes **refused by name** in the report —
    /// never a header that says "Printed on" and stops.
    ///
    /// # Errors
    ///
    /// As [`export_pdf`](Self::export_pdf).
    pub fn export_pdf_with_context(
        &self,
        sheet_index: usize,
        ctx: &PrintContext<'_>,
    ) -> Result<(Vec<u8>, CompatibilityReport), SdkError> {
        let geometry = self.geometry(sheet_index);
        let title = self
            .workbook
            .sheets
            .get(sheet_index)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        sheet_pdf_with_context(&self.workbook, sheet_index, &geometry, &title, ctx)
    }

    /// The format this session was opened from, and the one
    /// [`save`](Self::save) writes.
    ///
    /// A host needs it to name the file and to set a content type: bytes that
    /// are CSV inside a name ending `.xlsx` are the same lie in the other
    /// direction.
    #[must_use]
    pub fn format(&self) -> SessionFormat {
        self.format
    }

    /// What saving in this session's format cannot carry.
    ///
    /// **Ask before saving.** A delimited file holds the values of one sheet
    /// and nothing else: every other sheet, every formula (written as its
    /// computed value), and all formatting, merges, comments, charts and images
    /// are gone from the file even though they are still in this session. The
    /// engine will not refuse the save — the host asked for that format — but
    /// [no loss is silent](../../../docs/34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md),
    /// so everything it costs is counted and named here.
    ///
    /// Empty for [`SessionFormat::Xlsx`], which is the format the model is
    /// shaped for. What a *file* lost coming in is a different question, and
    /// [`compatibility_report`](Self::compatibility_report) answers it.
    ///
    /// Recomputed on each call rather than kept, because it describes the
    /// document as it is now: a formula typed a second ago changes the answer.
    #[must_use]
    pub fn format_loss(&self) -> CompatibilityReport {
        self.loss_writing(self.format)
    }

    /// What writing this document in `format` would cost, whatever it was
    /// opened from.
    ///
    /// The general form of [`format_loss`](Self::format_loss). A converter has
    /// the same duty as a session — a service that reads an OOXML package and
    /// writes a `.csv` back to a host is dropping exactly as much as an editor
    /// would, and its report must be asked for before the write, not after.
    #[must_use]
    pub fn loss_writing(&self, format: SessionFormat) -> CompatibilityReport {
        let mut report = match format {
            // A macro-enabled package carries everything a plain one does; the
            // extra part it carries is the reason it is a separate format.
            SessionFormat::Xlsx | SessionFormat::Xlsm => CompatibilityReport::default(),
            SessionFormat::Ods => casual_calc_ods::export_loss(&self.workbook),
            SessionFormat::Delimited(_) => delimited_loss(&self.workbook, DELIMITED_SHEET),
        };
        // **The `.xlsm` half of `IO-04`.** Macros can only travel in a
        // macro-enabled package, so writing this document as anything else
        // leaves them behind — and the whole point of the row is that this had
        // been happening with nothing said. Counted here rather than in each
        // format's own loss function because it is a fact about the *target*
        // format, and the three writers below it would each have had to
        // remember the same thing.
        //
        // Not reported when the save hands back the opened file untouched:
        // those bytes still contain the project, so claiming a loss would be
        // as wrong in the other direction.
        if format != SessionFormat::Xlsm
            && !self.writes_source_verbatim(format)
            && self.workbook.macro_project().is_some()
        {
            report.record(
                "macros (VBA project)",
                ModelOutcome::Omitted,
                RetentionOutcome::NotRetained,
            );
        }
        report
    }

    /// Whether [`save_as`](Self::save_as) would hand `format` the opened bytes
    /// unchanged rather than running a writer.
    ///
    /// The byte-for-byte guarantee and the loss report have to agree: a save
    /// that returns the original file has lost nothing, whatever the writer for
    /// that format cannot carry.
    fn writes_source_verbatim(&self, format: SessionFormat) -> bool {
        format == self.format && self.source.is_some()
    }

    /// Serialize the workbook to the format it was opened from.
    ///
    /// A `.csv` opened here comes back as `.csv` — see [`format`](Self::format)
    /// for which, and [`format_loss`](Self::format_loss) for what that format
    /// cannot carry. Everything below describes the `.xlsx` path.
    ///
    /// A file that was **opened and not edited saves as itself, byte for byte**
    /// (P1B-002). That is not an optimisation: the semantic writer reconstructs
    /// canonical OOXML, so anything this engine does not model is rebuilt from
    /// what it *does* model, and a part it merely retained comes back in a
    /// package that is no longer the one the author had. Opening a workbook to
    /// look at it and saving it should not rewrite it.
    ///
    /// After any edit the semantic writer runs, and the guarantee narrows to the
    /// documented one: retained parts survive, and the model round-trips to an
    /// equal model.
    ///
    /// The limit worth knowing: an unedited save returns the file's **own**
    /// cached values, not the ones this engine computed on open. If the two
    /// disagree the file was already inconsistent, and reproducing it exactly is
    /// the more conservative answer — this engine does not model every construct
    /// that might explain the difference, so overwriting the author's cached
    /// values on the strength of its own recalculation is the riskier of the two.
    pub fn save(&self) -> Result<Vec<u8>, SdkError> {
        self.save_as(self.format)
    }

    /// Serialize the workbook to a named format, whatever it was opened from.
    ///
    /// The general form of [`save`](Self::save), and the one a *converter*
    /// wants: a service that has to hand an OOXML package to something that
    /// only reads packages, while the file on the host is a `.csv`, is doing
    /// two conversions and neither of them is "save".
    ///
    /// **Ask [`loss_writing`](Self::loss_writing) first.** Writing a format
    /// that cannot carry this document is allowed — the caller chose it — but
    /// the caller is the one that has to say what it cost.
    ///
    /// The byte-for-byte guarantee documented on [`save`](Self::save) applies
    /// only when `format` is the one the session was opened from. Asking for a
    /// different format is by definition asking for different bytes.
    ///
    /// # Macros
    ///
    /// A VBA project can travel in [`SessionFormat::Xlsm`] and nowhere else.
    /// Written as `.xlsm` it is carried through byte for byte, like any other
    /// [retained part](casual_calc_model::Workbook::macro_project); written as
    /// anything else it is **removed**, and
    /// [`loss_writing`](Self::loss_writing) names the loss under
    /// `macros (VBA project)`. Removing it is the deliberate choice: a package
    /// that holds macros while declaring itself a plain workbook is one Excel
    /// opens as damaged, so smuggling the bytes across would cost the whole
    /// file rather than the macros.
    ///
    /// # Errors
    ///
    /// As [`save`](Self::save).
    pub fn save_as(&self, format: SessionFormat) -> Result<Vec<u8>, SdkError> {
        if self.writes_source_verbatim(format)
            && let Some(source) = &self.source
        {
            // The original bytes, untouched. Nothing has edited them, so there
            // is nothing to validate — and validating here would refuse to hand
            // back a file this engine merely does not fully model.
            return Ok(source.clone());
        }
        // **The last place an invalid workbook can be stopped** (`SDK-008`).
        //
        // `workbook_mut` hands a host the right to change anything, including
        // into a state the model calls invalid: a duplicate sheet id, a cell
        // pointing at a string that was never interned. The session cannot see
        // what happens through that reference, and checking on every call is
        // not affordable — `validate` walks every cell, and hosts reach for
        // that reference per keystroke.
        //
        // Here it is affordable, because writing the package already walks
        // every cell, and here is where being wrong stops being recoverable: a
        // corrupt workbook that stayed in memory is a bug, and one that became
        // a file the author will open tomorrow is data loss.
        self.workbook.validate()?;
        match format {
            // Macros cannot travel here. `write_workbook` would otherwise
            // declare the package macro-enabled on their account — which is
            // the correct thing for it to do and the wrong file to hand back
            // to somebody who asked for an `.xlsx`. So the project is removed
            // and the loss is named by `loss_writing`, which reports exactly
            // this case. Cloning only when there is something to remove keeps
            // the ordinary save free of it.
            SessionFormat::Xlsx if self.workbook.macro_project().is_some() => {
                let mut demacroed = self.workbook.clone();
                demacroed.remove_macro_project();
                Ok(write_workbook_as(&demacroed, PackageKind::Workbook)?)
            }
            SessionFormat::Xlsx => Ok(write_workbook_as(&self.workbook, PackageKind::Workbook)?),
            // Named rather than derived: a workbook whose macros were all
            // deleted is still an `.xlsm` if that is what the caller asked
            // for, and a `.xlsm` declaring itself a plain workbook is a file
            // Excel argues with.
            SessionFormat::Xlsm => Ok(write_workbook_as(
                &self.workbook,
                PackageKind::MacroEnabled,
            )?),
            SessionFormat::Ods => Ok(casual_calc_ods::export_ods(&self.workbook)?),
            // No byte-order mark. `read_delimited` does not strip one, so a BOM
            // written here comes back as part of the first field — a save that
            // makes its own output unreadable by the reader that produced it.
            SessionFormat::Delimited(delimiter) => {
                Ok(write_delimited(&self.workbook, DELIMITED_SHEET, delimiter).into_bytes())
            }
        }
    }

    /// Whether saving now would return the opened file unchanged.
    ///
    /// A host uses it to leave "Save" disabled, or to skip a write it does not
    /// need. `false` for a session that was not opened from a package, and for
    /// one where anything has been edited.
    #[must_use]
    pub fn is_unmodified(&self) -> bool {
        self.source.is_some()
    }
}

impl Default for WorkbookSession {
    fn default() -> Self {
        Self::blank()
    }
}

/// Render a sheet's viewport to PNG bytes, splitting its frozen panes.
///
/// The composition step of the headless renderer: it reads the sheet's frozen
/// row and column counts, splits the viewport into the panes they imply, lays
/// out each one, and hands the set to the render backend. A sheet with nothing
/// frozen yields a single pane and the same bytes the unsplit path produced.
///
/// Free-standing because more than one host needs it — the session below and
/// the WASM `render_xlsx` entry point — and a freeze honoured in one renderer
/// and not the other is the gap this closes rather than moves.
pub fn render_sheet_png(
    workbook: &Workbook,
    sheet_index: usize,
    geometry: &GridGeometry,
    viewport: &Viewport,
    dpi: u32,
) -> Result<Vec<u8>, SdkError> {
    render_sheet_png_with_report(workbook, sheet_index, geometry, viewport, dpi).map(|(png, _)| png)
}

/// Render a sheet's viewport to PNG bytes **and** say what its pictures cost.
///
/// The same render as [`render_sheet_png`], with the half of the answer that
/// signature has nowhere to return. A picture the backend could not draw leaves
/// a frame-shaped hole indistinguishable from a sheet that never had one, and
/// [nothing here is lost without being named](../../../docs/34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)
/// — so a caller producing a thumbnail somebody will trust should call this
/// one and show what it returns.
///
/// # Errors
///
/// As [`render_sheet_png`].
pub fn render_sheet_png_with_report(
    workbook: &Workbook,
    sheet_index: usize,
    geometry: &GridGeometry,
    viewport: &Viewport,
    dpi: u32,
) -> Result<(Vec<u8>, ImageReport), SdkError> {
    let freeze = workbook
        .sheets
        .get(sheet_index)
        .map(|sheet| Freeze {
            rows: sheet.view.frozen_rows,
            cols: sheet.view.frozen_cols,
        })
        .unwrap_or_default();

    let regions = panes(geometry, viewport, freeze);
    // Formula conditional-format rules resolved here too, not only on the
    // canvas: the PNG and the browser show one sheet, and the whole reason
    // conditional formatting lives in `casual-calc-layout` is that the two
    // cannot be allowed to disagree (`RND-05`).
    let cf_exprs = casual_calc_eval::conditional::CfExpressionRules::new(workbook, sheet_index);
    let lists: Vec<DisplayList> = regions
        .iter()
        .map(|pane| {
            casual_calc_layout::layout_viewport_with(
                workbook,
                sheet_index,
                geometry,
                &pane.viewport,
                &cf_exprs,
            )
        })
        .collect();
    let paints: Vec<PanePaint<'_>> = regions
        .iter()
        .zip(&lists)
        .map(|(pane, display_list)| PanePaint {
            pane: *pane,
            display_list,
        })
        .collect();

    let media = RetainedMedia::of(workbook);
    Ok(render_panes_png_with_images(
        &paints, geometry, viewport, dpi, &media,
    )?)
}

/// Cut a sheet into pages and write them as a PDF.
///
/// The **composition** step of the PDF path, and the exact counterpart of
/// [`render_sheet_png_with_report`]: the paginator says which rows and columns
/// land on which sheet of paper, this lays each band out into a display list,
/// and [`casual_calc_render::pdf::write_pdf`] executes them. Neither of the two
/// halves knows about the other's business, which is what stops a second
/// opinion about the page geometry existing.
///
/// A page is up to four bands, for the same reason a frozen sheet is up to four
/// panes: the repeated title rows and columns hold still while the body moves
/// under them. A band is laid out **once** and referred to from every page that
/// shows it — the repeated header of a forty-page report is one display list,
/// not forty.
///
/// The report carries three losses, all of them named rather than counted
/// silently: pictures (this backend draws none), a print area of more than one
/// rectangle, and a sheet so large the [page cap](casual_calc_layout::print::MAX_PAGES)
/// cut it short.
///
/// # Errors
///
/// [`SdkError::Render`] if the paper has no area.
pub fn sheet_pdf(
    workbook: &Workbook,
    sheet_index: usize,
    geometry: &GridGeometry,
    title: &str,
) -> Result<(Vec<u8>, CompatibilityReport), SdkError> {
    sheet_pdf_with_context(
        workbook,
        sheet_index,
        geometry,
        title,
        &PrintContext::default(),
    )
}

/// [`sheet_pdf`], with the host values `&D`, `&T`, `&F` and `&Z` are
/// substituted from.
///
/// # Errors
///
/// As [`sheet_pdf`].
pub fn sheet_pdf_with_context(
    workbook: &Workbook,
    sheet_index: usize,
    geometry: &GridGeometry,
    title: &str,
    ctx: &PrintContext<'_>,
) -> Result<(Vec<u8>, CompatibilityReport), SdkError> {
    let meta = PdfMetadata {
        title: title.to_owned(),
    };
    let Some(plan) =
        casual_calc_layout::print::paginate_with_context(workbook, sheet_index, geometry, ctx)
    else {
        // Nothing to print. A document with no pages, rather than one blank
        // page that cannot be told from a page that failed to draw.
        let (bytes, _) = write_pdf(&[], geometry, &meta)?;
        return Ok((bytes, CompatibilityReport::default()));
    };

    // Which bands each page is made of, before any of them has been laid out.
    struct BandSpec {
        list: usize,
        rows: (u32, u32),
        cols: (u32, u32),
        origin: (i64, i64),
    }
    let title_rows = plan.scope.title_rows.filter(|_| plan.title_height > 0);
    let title_cols = plan.scope.title_cols.filter(|_| plan.title_width > 0);
    let mut page_specs: Vec<Vec<BandSpec>> = Vec::with_capacity(plan.pages.len());
    for page in &plan.pages {
        let mut specs = Vec::new();
        // Painter's order, corner first, exactly as `panes` orders a freeze.
        if let (Some(rows), Some(cols)) = (title_rows, title_cols) {
            specs.push(BandSpec {
                list: 0,
                rows,
                cols,
                origin: (0, 0),
            });
        }
        if let Some(rows) = title_rows {
            specs.push(BandSpec {
                list: 0,
                rows,
                cols: page.cols,
                origin: (plan.title_width, 0),
            });
        }
        if let Some(cols) = title_cols {
            specs.push(BandSpec {
                list: 0,
                rows: page.rows,
                cols,
                origin: (0, plan.title_height),
            });
        }
        specs.push(BandSpec {
            list: 0,
            rows: page.rows,
            cols: page.cols,
            origin: (plan.title_width, plan.title_height),
        });
        page_specs.push(specs);
    }

    // Formula conditional-format rules resolved here too: the PNG, the canvas
    // and the printout show one sheet (`RND-05`).
    let cf_exprs = casual_calc_eval::conditional::CfExpressionRules::new(workbook, sheet_index);
    /// The rows and columns a band covers — what makes two bands the same
    /// picture, and therefore the same display list.
    type BandKey = ((u32, u32), (u32, u32));
    let mut cache: BTreeMap<BandKey, usize> = BTreeMap::new();
    let mut lists: Vec<DisplayList> = Vec::new();
    for specs in &mut page_specs {
        for spec in specs.iter_mut() {
            let key = (spec.rows, spec.cols);
            spec.list = *cache.entry(key).or_insert_with(|| {
                lists.push(casual_calc_layout::layout_range(
                    workbook,
                    sheet_index,
                    geometry,
                    casual_calc_layout::VisibleRange {
                        rows: spec.rows,
                        cols: spec.cols,
                    },
                    &cf_exprs,
                ));
                lists.len() - 1
            });
        }
    }

    let (width, height) = if plan.landscape {
        (plan.paper.height, plan.paper.width)
    } else {
        (plan.paper.width, plan.paper.height)
    };
    // The box the header and footer lay out in. `alignWithMargins` is the
    // difference between "line the page number up with the table" (the default,
    // and what a reader expects) and "centre it on the sheet of paper".
    let (furniture_left, furniture_width) = if plan.header_footers.align_with_margins {
        (
            plan.margins[3],
            (width - plan.margins[3] - plan.margins[1]).max(0),
        )
    } else {
        (0, width)
    };
    // Excel's `scaleWithDoc` shrinks the header with the sheet. The HTML print
    // path cannot — its header is in a CSS margin box, outside the scaled table
    // — so a fit-to-page workbook prints a slightly larger header there. The
    // difference is stated rather than split: this is the one Excel does.
    let furniture_scale = if plan.header_footers.scale_with_doc {
        plan.scale
    } else {
        1.0
    };
    let total_pages = plan.pages.len();
    let pages: Vec<PdfPage<'_>> = page_specs
        .iter()
        .enumerate()
        .map(|(index, specs)| {
            let (header, footer) = plan.furniture(index);
            let number = plan.page_number(index);
            PdfPage {
                width,
                height,
                margin_left: plan.margins[3],
                margin_top: plan.margins[0],
                scale: plan.scale,
                bands: specs
                    .iter()
                    .map(|spec| PdfBand {
                        display_list: &lists[spec.list],
                        rows: spec.rows,
                        cols: spec.cols,
                        origin: spec.origin,
                        gridlines: plan.gridlines,
                    })
                    .collect(),
                header: PdfFurniture {
                    sections: header.resolve(number, total_pages),
                    left: furniture_left,
                    width: furniture_width,
                    inset: plan.header_footers.header_margin,
                    scale: furniture_scale,
                },
                footer: PdfFurniture {
                    sections: footer.resolve(number, total_pages),
                    left: furniture_left,
                    width: furniture_width,
                    inset: plan.header_footers.footer_margin,
                    scale: furniture_scale,
                },
            }
        })
        .collect();

    let (bytes, images) = write_pdf(&pages, geometry, &meta)?;
    let mut report = image_loss(&images);
    if plan.scope.extra_areas > 0 {
        report.record_n(
            "print area (only the first rectangle)",
            ModelOutcome::Omitted,
            RetentionOutcome::NotRetained,
            u64::from(plan.scope.extra_areas),
        );
    }
    if plan.truncated {
        report.record(
            "printed pages (over the page cap)",
            ModelOutcome::Omitted,
            RetentionOutcome::NotRetained,
        );
    }
    // The header/footer codes that were read, understood and not drawn. Named
    // one by one rather than as "some header formatting": a reader who is told
    // a *picture* is missing looks for a logo, and one told "formatting" looks
    // for nothing.
    for (code, count) in &plan.header_footers.refused {
        report.record_n(
            code,
            ModelOutcome::Omitted,
            RetentionOutcome::NotRetained,
            *count,
        );
    }
    Ok((bytes, report))
}

/// The workbook's own media, as something the renderer can draw from.
///
/// A [`PaintItem::Image`](casual_calc_layout::PaintItem::Image) carries the
/// package path of a media part rather than its bytes, and the bytes are
/// already in the model:
/// [`ImageView::part`](casual_calc_model::ImageView::part) names an entry of
/// [`Workbook::retained_parts`](casual_calc_model::Workbook::retained_parts),
/// which the importer filled with the part **under that same path**. So the
/// wiring the renderer asks for is an index over what a session is already
/// holding — no copy of any picture is made here, and none needs to be.
///
/// Built per render rather than kept on the session, because a retained part
/// can be replaced by an edit and an index outliving its workbook would draw
/// yesterday's logo.
#[derive(Debug, Default)]
struct RetainedMedia<'a> {
    /// Borrowed, not owned: a picture is megabytes and a render is not the
    /// place to duplicate one.
    parts: BTreeMap<&'a str, &'a [u8]>,
}

impl<'a> RetainedMedia<'a> {
    /// Index a workbook's retained parts by package path.
    ///
    /// Every retained part, not only the ones some sheet anchors: which parts
    /// a viewport asks for depends on where it is scrolled, and a map is built
    /// once where a linear scan of the retained parts would run per picture per
    /// pane.
    fn of(workbook: &'a Workbook) -> Self {
        Self {
            parts: workbook
                .retained_parts
                .iter()
                .map(|part| (part.path.as_str(), part.bytes.as_slice()))
                .collect(),
        }
    }
}

impl ImageSource for RetainedMedia<'_> {
    fn part_bytes(&self, path: &str) -> Option<&[u8]> {
        self.parts.get(path).copied()
    }
}

/// What a render's pictures cost, in the report type a host already shows.
///
/// **One answer, not two.** A host that has to merge an [`ImageReport`] with a
/// [`CompatibilityReport`] itself will render the first and forget the second,
/// or show them in two places and let a reader believe the file has two
/// unrelated problems. The reason keys come from
/// [`UndrawnReason::feature`], so an EMF that will never be drawn and a media
/// part the package was missing stay distinguishable after the fold — which is
/// the difference between "install nothing, this file uses EMF" and "this file
/// is damaged".
///
/// Counted as [`Omitted`](ModelOutcome::Omitted) /
/// [`NotRetained`](RetentionOutcome::NotRetained): the picture is missing from
/// *this rendering*. The workbook still holds the part, and saving it back
/// writes it out untouched — a render is a projection, and losing something in
/// one is not losing it from the document.
#[must_use]
pub fn image_loss(images: &ImageReport) -> CompatibilityReport {
    let mut report = CompatibilityReport::default();
    for undrawn in &images.undrawn {
        report.record(
            undrawn.reason.feature(),
            ModelOutcome::Omitted,
            RetentionOutcome::NotRetained,
        );
    }
    report
}

/// How an operation should be recalculated.
enum RecalcPlan {
    /// No value can have changed (pure style / geometry edit) — skip recalc.
    Skip,
    /// Value edits: recompute the transitive dependents of these cells.
    Cells(Vec<(usize, CellRef)>),
    /// Reference-shifting edits (insert/delete rows or columns) — full recalc.
    Full,
}

/// Classify an operation's recalc scope. A `Batch` is `Full` if any member is,
/// otherwise the union of its members' changed cells (or `Skip` if none touch
/// values).
fn recalc_plan(op: &Operation) -> RecalcPlan {
    match op {
        Operation::SetCell { sheet, at, .. }
        | Operation::SetValue { sheet, at, .. }
        | Operation::ClearCell { sheet, at } => RecalcPlan::Cells(vec![(*sheet, *at)]),
        Operation::SetStyle { .. }
        | Operation::SetColumnWidth { .. }
        | Operation::SetRowHeight { .. }
        | Operation::SetTabColor { .. } => RecalcPlan::Skip,
        // Most of this bundle is presentation, but two of its fields are read
        // by the evaluator: `SUBTOTAL`'s 101–111 codes and `AGGREGATE` skip
        // hidden rows, and `Sheet::is_row_hidden` is the union of the
        // hand-hidden set and the set the autofilter hides. So applying,
        // changing or clearing a filter changes what a subtotal *is* — while
        // changing nothing the dependency graph can see, since no cell was
        // written. Nothing localizes it either: the graph records which cells a
        // formula reads, and "the visibility of the rows under it" is not a
        // cell. Hence full, and only for the two fields that can do it.
        Operation::SetSheetMetadata { changed, .. } => {
            if changed.intersects(SheetFields::HIDDEN_ROWS.union(SheetFields::FILTER_HIDDEN)) {
                RecalcPlan::Full
            } else {
                RecalcPlan::Skip
            }
        }
        // Reordering tabs changes values, which is not the obvious part. It
        // was classified `Skip` on the grounds that no reference resolves
        // differently — true, and not the whole question. `SHEET()` returns a
        // sheet's *position*, so a tab drag changes its result; and the kept
        // precedent graph is keyed by sheet **index**, which `MoveSheet`
        // renumbers wholesale by removing and re-inserting. Left as `Skip`,
        // the graph went on describing the old numbering and every later edit
        // to the moved sheet stopped propagating — silently, permanently, and
        // into the saved file.
        Operation::MoveSheet { .. } => RecalcPlan::Full,
        // A move of cells is the same class of hazard `CALC-01` found in
        // `MoveSheet`, one level down: the kept precedent graph is keyed by
        // `(sheet, CellRef)`, and all three of these **renumber addresses**.
        // Classified anything but `Full`, the graph would go on describing a
        // document that no longer exists — every later edit to a moved cell
        // silently stopping propagating, permanently, into the saved file.
        // References are rewritten too, so what each formula reads changes
        // outright.
        Operation::MoveColumns { .. }
        | Operation::MoveRows { .. }
        | Operation::MoveRange { .. }
        | Operation::InsertRows { .. }
        | Operation::DeleteRows { .. }
        | Operation::InsertColumns { .. }
        | Operation::DeleteColumns { .. }
        // Adding, removing, or renaming a sheet changes which name a cross-sheet
        // reference resolves to (or turns it into #REF!), so recompute fully.
        | Operation::InsertSheet { .. }
        | Operation::RemoveSheet { .. }
        | Operation::RenameSheet { .. }
        // Defining, renaming, or deleting a name changes what any formula
        // referencing it resolves to (or turns it into #NAME?).
        | Operation::SetDefinedNames(_) => RecalcPlan::Full,
        Operation::Batch(ops) => {
            let mut cells = Vec::new();
            for o in ops {
                match recalc_plan(o) {
                    RecalcPlan::Full => return RecalcPlan::Full,
                    RecalcPlan::Cells(mut c) => cells.append(&mut c),
                    RecalcPlan::Skip => {}
                }
            }
            if cells.is_empty() {
                RecalcPlan::Skip
            } else {
                RecalcPlan::Cells(cells)
            }
        }
    }
}

pub mod views;

use std::collections::{BTreeMap, BTreeSet};

use crate::views::PersonalViews;

#[cfg(test)]
mod tests;

/// Everything in a workbook that a delimited save leaves behind, counted.
///
/// The enumeration is deliberately exhaustive over [`Sheet`]'s fields rather
/// than a list of the three losses people remember (one sheet, no formulas, no
/// styles). A file's comments, its charts and its data validations disappear
/// just as completely, and a report that names only the famous three tells a
/// host its document survived when a third of it did not.
///
/// Written against the sheet that is actually saved, `sheet_index`; everything
/// on every other sheet is covered by the `other sheets` entry, which is the
/// coarser and more truthful statement — none of it is written at all.
fn delimited_loss(workbook: &Workbook, sheet_index: usize) -> CompatibilityReport {
    let mut report = CompatibilityReport::default();
    let mut gone = |feature: &str, count: usize| {
        report.record_n(
            feature,
            ModelOutcome::Omitted,
            RetentionOutcome::NotRetained,
            count as u64,
        );
    };

    gone("other sheets", workbook.sheets.len().saturating_sub(1));
    gone("defined names", workbook.defined_names.len());

    let Some(sheet) = workbook.sheets.get(sheet_index) else {
        return report;
    };

    let mut formulas = 0usize;
    let mut formatted = 0usize;
    for (_, cell) in sheet.cells.iter() {
        if cell.formula.is_some() {
            formulas += 1;
        }
        if cell
            .style
            .and_then(|id| workbook.styles.get(id))
            .is_some_and(|style| !delimited_carries_style(style))
        {
            formatted += 1;
        }
    }
    gone("cell formatting", formatted);
    gone("merged cells", sheet.merges.len());
    gone("column widths", usize::from(!sheet.columns.is_empty()));
    gone("row heights", usize::from(!sheet.rows.is_empty()));
    gone("hidden rows", sheet.hidden_rows.len());
    gone("hidden columns", sheet.hidden_cols.len());
    gone(
        "outline groups",
        sheet.row_outline_levels.len() + sheet.col_outline_levels.len(),
    );
    gone(
        "frozen panes",
        usize::from(sheet.view != Default::default()),
    );
    gone("tab colour", usize::from(sheet.tab_color.is_some()));
    gone("data validation", sheet.validations.len());
    gone("conditional formatting", sheet.conditional_formats.len());
    gone("comments", sheet.comments.len());
    gone("hyperlinks", sheet.hyperlinks.len());
    gone(
        "print setup",
        usize::from(sheet.print != Default::default()),
    );
    gone("charts", sheet.charts.len());
    gone("images", sheet.images.len());
    gone("sort state", usize::from(sheet.sort_state.is_some()));
    gone("sheet format", sheet.format_pr.len());

    // Degraded rather than omitted, and recorded last so the borrow of the
    // closure above is over: a formula's *value* is written, which is why a
    // delimited export is useful at all. What is gone is the formula.
    report.record_n(
        "formulas",
        ModelOutcome::Degraded,
        RetentionOutcome::NotRetained,
        formulas as u64,
    );
    report
}

/// Whether a delimited write carries everything a style says.
///
/// Almost nothing survives: a field is text, so weight, colour, borders and
/// alignment have nowhere to go. The exception is a **date or time number
/// format**, which [`write_delimited`](casual_calc_io::write_delimited) honours
/// by writing the date as it reads on the sheet — and which `read_delimited`
/// recognises again on the way back in, so that one round-trips.
///
/// Decided by clearing the number format and comparing what is left to the
/// default style, rather than by listing the fields that matter. A style gains
/// fields (four Mac font effects arrived at once), and a list would go on
/// claiming fidelity for whichever one was added last.
fn delimited_carries_style(style: &Style) -> bool {
    let mut bare = style.clone();
    let format = bare.number_format.take();
    bare == Style::default() && format.is_none_or(|code| casual_calc_io::is_date_format(&code))
}

/// Install the configured clock and seed on the workbook.
///
/// They live on the model rather than in the evaluator because a formula's
/// answer has to be reproducible from the workbook alone — hand the same model
/// and the same environment to two hosts and they agree.
fn apply_environment(workbook: &mut Workbook, config: &SessionConfig) {
    workbook.volatile_now = config.environment.now;
    workbook.volatile_seed = config.environment.seed;
}

/// The calculation mode the file itself asks for.
fn file_calculation_mode(workbook: &Workbook) -> CalculationMode {
    workbook
        .settings
        .calc
        .get("calcMode")
        .map_or(CalculationMode::Automatic, |m| {
            CalculationMode::from_token(m)
        })
}

/// Record the mode so a save carries it, as Excel does.
///
/// Without this, turning calculation off and saving produced a file that turns
/// itself back on when reopened — and the reason it was turned off does not go
/// away just because the file was closed.
fn write_calculation_mode(workbook: &mut Workbook, mode: CalculationMode) {
    match mode {
        // `auto` is the schema default, so it is written by omission — leaving
        // the attribute behind would be a difference in a file nobody changed.
        CalculationMode::Automatic => {
            workbook.settings.calc.remove("calcMode");
        }
        CalculationMode::Manual => {
            workbook
                .settings
                .calc
                .insert("calcMode".to_owned(), mode.token().to_owned());
        }
    }
}
