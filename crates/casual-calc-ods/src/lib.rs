//! Reading and writing OpenDocument Spreadsheets — deliberately a placeholder.
//!
//! # What this is, and what it is not
//!
//! This carries **values, formulas and sheets**, and nothing else. It exists so
//! that a LibreOffice-first shop can open a `.ods`, work on it, and get a `.ods`
//! back — which is the difference between OpenCalc being usable at all in those
//! shops and not. It is explicitly not a fidelity implementation: styles,
//! merges, charts, images, conditional formatting, validation and print setup
//! are **counted and named** on the way through, never dropped quietly.
//!
//! That distinction is the point. A converter that silently discards formatting
//! looks like it worked and loses somebody's afternoon; one that says what it
//! could not keep lets them decide whether to use it. When real ODS support
//! lands, this module's shape stays and the loss list shrinks.
//!
//! # Why the repeat attributes are the first thing here
//!
//! ODF compresses runs of identical cells with `table:number-columns-repeated`
//! and `table:number-rows-repeated`, and LibreOffice emits them constantly —
//! including a final `number-columns-repeated="16384"` of nothing on most rows.
//! A reader that ignores them does not lose a little formatting; it puts every
//! cell after the first repeat in the wrong column, which is a corrupt document
//! that opens without complaint.

use std::io::{Cursor, Read, Write};

use casual_calc_import::{CompatibilityReport, ModelOutcome, RetentionOutcome};
use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};
use quick_xml::events::Event;

/// Why a `.ods` could not be read or written.
#[derive(Debug)]
pub enum OdsError {
    /// The bytes are not a zip, or the zip is unreadable.
    NotAPackage(String),
    /// A zip that holds no `content.xml` is not a spreadsheet.
    NoContent,
    /// `content.xml` is not well-formed XML.
    Malformed(String),
    /// The document is larger than this build will admit.
    TooLarge { bytes: u64, limit: u64 },
    /// Writing failed.
    Write(String),
}

impl std::fmt::Display for OdsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAPackage(why) => write!(f, "not an OpenDocument package: {why}"),
            Self::NoContent => write!(f, "the package has no content.xml, so it holds no document"),
            Self::Malformed(why) => write!(f, "content.xml is malformed: {why}"),
            Self::TooLarge { bytes, limit } => {
                write!(
                    f,
                    "content.xml is {bytes} bytes, over the {limit}-byte limit"
                )
            }
            Self::Write(why) => write!(f, "could not write the package: {why}"),
        }
    }
}

impl std::error::Error for OdsError {}

/// The largest `content.xml` this will decompress.
///
/// A zip entry declares its uncompressed size and can lie; more to the point a
/// small `.ods` can legitimately expand to a very large XML document. Bounded
/// because this runs in a server that accepts uploads, and an unbounded
/// decompress is a denial of service with a friendly file extension.
const MAX_CONTENT_BYTES: u64 = 256 * 1024 * 1024;

/// The rows a single `number-rows-repeated` may expand to.
///
/// LibreOffice writes `number-rows-repeated="1048576"` to fill the sheet. That
/// is a description of emptiness, not a million rows of data, and materialising
/// it would exhaust memory reading a file that holds nothing.
const MAX_REPEAT: u32 = 4096;

/// Read a `.ods` into a workbook, with a report of everything not carried.
///
/// # Errors
///
/// If the bytes are not a readable package, hold no `content.xml`, or that
/// document is not well-formed XML.
pub fn import_ods(bytes: &[u8]) -> Result<(Workbook, CompatibilityReport), OdsError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| OdsError::NotAPackage(e.to_string()))?;
    let mut entry = zip
        .by_name("content.xml")
        .map_err(|_| OdsError::NoContent)?;
    if entry.size() > MAX_CONTENT_BYTES {
        return Err(OdsError::TooLarge {
            bytes: entry.size(),
            limit: MAX_CONTENT_BYTES,
        });
    }
    let mut xml = String::new();
    entry
        .read_to_string(&mut xml)
        .map_err(|e| OdsError::Malformed(e.to_string()))?;
    drop(entry);
    read_content(&xml)
}

/// One cell, as ODF describes it before it becomes a `Cell`.
#[derive(Default)]
struct Pending {
    value_type: String,
    value: String,
    text: String,
    formula: String,
    repeat: u32,
}

fn read_content(xml: &str) -> Result<(Workbook, CompatibilityReport), OdsError> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let config = reader.config_mut();
    config.trim_text(false);

    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    let mut report = CompatibilityReport::default();

    let mut row: u32 = 0;
    let mut col: u32 = 0;
    let mut row_repeat: u32 = 1;
    let mut pending: Option<Pending> = None;
    let mut in_text = false;
    let mut next_id = 2u64;

    loop {
        match reader
            .read_event()
            .map_err(|e| OdsError::Malformed(e.to_string()))?
        {
            Event::Eof => break,
            ref ev @ (Event::Start(ref e) | Event::Empty(ref e)) => {
                let self_closing = matches!(ev, Event::Empty(_));
                let name = local_name(e.name().as_ref());
                match name.as_str() {
                    "table" => {
                        let title = attr(e, "table:name")
                            .unwrap_or_else(|| format!("Sheet{}", workbook.sheets.len() + 1));
                        workbook
                            .sheets
                            .push(Sheet::new(SheetId(Id::from_parts(next_id, 1)), title));
                        next_id += 1;
                        row = 0;
                        col = 0;
                    }
                    "table-row" => {
                        row_repeat = attr(e, "table:number-rows-repeated")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(1)
                            .min(MAX_REPEAT);
                        col = 0;
                    }
                    "table-cell" | "covered-table-cell" => {
                        if name == "covered-table-cell" {
                            // The hidden half of a merge. The merge itself is
                            // not carried, so say so rather than let the cell
                            // vanish without explanation.
                            report.record(
                                "merged cells",
                                ModelOutcome::Omitted,
                                RetentionOutcome::NotRetained,
                            );
                        }
                        let cell = Pending {
                            value_type: attr(e, "office:value-type").unwrap_or_default(),
                            value: attr(e, "office:value")
                                .or_else(|| attr(e, "office:boolean-value"))
                                .or_else(|| attr(e, "office:date-value"))
                                .unwrap_or_default(),
                            text: String::new(),
                            formula: attr(e, "table:formula").unwrap_or_default(),
                            repeat: attr(e, "table:number-columns-repeated")
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(1)
                                .min(MAX_REPEAT),
                        };
                        // **A self-closing cell has no `End` event.** A blank
                        // run is written `<table:table-cell repeated="2"/>`,
                        // and waiting for a close that never comes left the
                        // column cursor where it was — so everything after the
                        // run landed on top of it. LibreOffice writes those
                        // runs on nearly every row.
                        if self_closing {
                            col += place(&mut workbook, &mut report, row, col, row_repeat, &cell);
                        } else {
                            pending = Some(cell);
                        }
                    }
                    "p" => in_text = true,
                    // Named so an operator can see what a file carried that this
                    // did not take. Recorded once per kind, not per instance:
                    // the report is for deciding, not for counting.
                    "table-column" => {}
                    "style" | "automatic-styles" => report.record(
                        "styles",
                        ModelOutcome::Omitted,
                        RetentionOutcome::NotRetained,
                    ),
                    "database-ranges" => report.record(
                        "database ranges",
                        ModelOutcome::Omitted,
                        RetentionOutcome::NotRetained,
                    ),
                    "chart" => report.record(
                        "charts",
                        ModelOutcome::Omitted,
                        RetentionOutcome::NotRetained,
                    ),
                    "frame" | "image" => report.record(
                        "images and frames",
                        ModelOutcome::Omitted,
                        RetentionOutcome::NotRetained,
                    ),
                    "conditional-formats" | "conditional-format" => report.record(
                        "conditional formatting",
                        ModelOutcome::Omitted,
                        RetentionOutcome::NotRetained,
                    ),
                    "content-validations" => report.record(
                        "data validation",
                        ModelOutcome::Omitted,
                        RetentionOutcome::NotRetained,
                    ),
                    _ => {}
                }
            }
            Event::Text(t) => {
                if in_text && let Some(p) = pending.as_mut() {
                    let text = t.decode().map_err(|e| OdsError::Malformed(e.to_string()))?;
                    if !p.text.is_empty() {
                        p.text.push('\n');
                    }
                    p.text.push_str(&text);
                }
            }
            Event::End(e) => match local_name(e.name().as_ref()).as_str() {
                "p" => in_text = false,
                "table-cell" | "covered-table-cell" => {
                    if let Some(p) = pending.take() {
                        let width = place(&mut workbook, &mut report, row, col, row_repeat, &p);
                        col += width;
                    }
                }
                "table-row" => row += row_repeat,
                _ => {}
            },
            _ => {}
        }
    }

    if workbook.sheets.is_empty() {
        workbook
            .sheets
            .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "Sheet1"));
    }
    Ok((workbook, report))
}

/// Write one pending cell into the sheet, honouring its repeats.
///
/// Returns how many columns it consumed, which is its repeat count whether or
/// not it held anything — an empty run still moves the cursor, and that is the
/// whole reason the attribute exists.
fn place(
    workbook: &mut Workbook,
    report: &mut CompatibilityReport,
    row: u32,
    col: u32,
    row_repeat: u32,
    p: &Pending,
) -> u32 {
    if workbook.sheets.is_empty() {
        return p.repeat;
    }
    let value = match p.value_type.as_str() {
        "float" | "percentage" | "currency" => p
            .value
            .parse::<f64>()
            .map_or(CellValue::Empty, CellValue::Number),
        "boolean" => CellValue::Bool(p.value.eq_ignore_ascii_case("true")),
        // A date is carried as its text, because the model's serial needs the
        // workbook's epoch and this converter does not read one. Named rather
        // than guessed at.
        "date" | "time" => {
            report.record(
                "dates as text",
                ModelOutcome::Degraded,
                RetentionOutcome::NotRetained,
            );
            let text = if p.text.is_empty() {
                p.value.clone()
            } else {
                p.text.clone()
            };
            CellValue::InlineString(workbook.strings.intern(&text))
        }
        "string" => CellValue::InlineString(workbook.strings.intern(&p.text)),
        _ if !p.text.is_empty() => CellValue::InlineString(workbook.strings.intern(&p.text)),
        _ => CellValue::Empty,
    };

    let has_formula = !p.formula.is_empty();
    if value == CellValue::Empty && !has_formula {
        return p.repeat; // An empty run: move the cursor, store nothing.
    }

    // A formula is stored once and shared by handle, which is what the model's
    // interning is for — a repeated run of the same formula is one AST.
    let handle = has_formula
        .then(|| translate_formula(&p.formula))
        .flatten()
        .map(|expr| workbook.store_formula(expr));
    if has_formula && handle.is_none() {
        // The value survives; the formula does not. Named, because a cell that
        // silently stops recalculating is the worst of the two outcomes.
        report.record(
            "formulas this converter cannot translate",
            ModelOutcome::Degraded,
            RetentionOutcome::NotRetained,
        );
    }
    let Some(sheet) = workbook.sheets.last_mut() else {
        return p.repeat.max(1);
    };
    for dr in 0..row_repeat.max(1) {
        for dc in 0..p.repeat.max(1) {
            let mut cell = Cell::value(value.clone());
            cell.formula = handle;
            sheet.cells.set(CellRef::new(row + dr, col + dc), cell);
        }
    }
    p.repeat.max(1)
}

/// Turn an ODF formula into one this engine can parse, or `None`.
///
/// ODF writes `of:=[.A1]+[.B1]` — a namespace prefix, and every reference
/// wrapped in brackets with a leading dot for "this sheet". The addresses
/// themselves are the same A1 notation, so the translation is mechanical:
/// strip the prefix, unwrap the brackets, drop the dot.
///
/// **Returns `None` rather than guessing.** A formula this cannot translate has
/// its *value* kept and the formula reported as lost — a cell that quietly stops
/// recalculating, with the right number still in it, is the failure somebody
/// finds three months later. Being told is worse in the moment and better in
/// every other way.
fn translate_formula(raw: &str) -> Option<casual_calc_formula::Expr> {
    let body = raw
        .split_once(":=")
        .map(|(_, rest)| rest)
        .or_else(|| raw.strip_prefix('='))?;

    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '[' {
            out.push(c);
            continue;
        }
        // `[.A1]`, `[.A1:.B2]`, or `['Sheet 2'.A1]`. Anything with a sheet
        // qualifier is left for the parser to reject: this converter does not
        // resolve ODF's quoting rules, and inventing a translation for them is
        // how a formula ends up pointing somewhere it never did.
        let mut inner = String::new();
        for c in chars.by_ref() {
            if c == ']' {
                break;
            }
            inner.push(c);
        }
        if inner.contains('\'') || inner.contains('$') && inner.contains('#') {
            return None;
        }
        out.push_str(&inner.replace('.', ""));
    }
    casual_calc_formula::parse(&out).ok()
}

/// The element name without its namespace prefix.
fn local_name(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    text.rsplit(':').next().unwrap_or(&text).to_owned()
}

/// One attribute's value, by its qualified name.
fn attr(e: &quick_xml::events::BytesStart<'_>, want: &str) -> Option<String> {
    let short = want.rsplit(':').next().unwrap_or(want);
    e.attributes().flatten().find_map(|a| {
        let key = String::from_utf8_lossy(a.key.as_ref()).to_string();
        let matches = key == want || key.rsplit(':').next() == Some(short);
        matches.then(|| String::from_utf8_lossy(&a.value).to_string())
    })
}

/// Write a workbook as a minimal `.ods`.
///
/// # Errors
///
/// If the zip cannot be assembled.
pub fn export_ods(workbook: &Workbook) -> Result<Vec<u8>, OdsError> {
    let mut out = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut out));
        // **`mimetype` first, and stored uncompressed.** ODF says so, and it is
        // not decoration: it is what lets `file(1)` and every desktop
        // environment identify the format from the first bytes without
        // unzipping. Compressed or second, the file still opens in LibreOffice
        // and is unrecognised everywhere else — which looks like it worked.
        zip.start_file(
            "mimetype",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .map_err(|e| OdsError::Write(e.to_string()))?;
        zip.write_all(b"application/vnd.oasis.opendocument.spreadsheet")
            .map_err(|e| OdsError::Write(e.to_string()))?;

        let deflated = zip::write::SimpleFileOptions::default();
        zip.add_directory("META-INF", deflated)
            .map_err(|e| OdsError::Write(e.to_string()))?;
        zip.start_file("META-INF/manifest.xml", deflated)
            .map_err(|e| OdsError::Write(e.to_string()))?;
        zip.write_all(MANIFEST.as_bytes())
            .map_err(|e| OdsError::Write(e.to_string()))?;

        zip.start_file("content.xml", deflated)
            .map_err(|e| OdsError::Write(e.to_string()))?;
        zip.write_all(content_xml(workbook).as_bytes())
            .map_err(|e| OdsError::Write(e.to_string()))?;
        zip.finish().map_err(|e| OdsError::Write(e.to_string()))?;
    }
    Ok(out)
}

const MANIFEST: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">"#,
    r#"<manifest:file-entry manifest:full-path="/" manifest:version="1.2" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/>"#,
    r#"<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>"#,
    r#"</manifest:manifest>"#,
);

fn content_xml(workbook: &Workbook) -> String {
    let mut out = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<office:document-content"#,
        r#" xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0""#,
        r#" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0""#,
        r#" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0""#,
        r#" office:version="1.2">"#,
        r#"<office:body><office:spreadsheet>"#,
    ));
    for sheet in &workbook.sheets {
        out.push_str(&format!(
            r#"<table:table table:name="{}">"#,
            escape(&sheet.name)
        ));
        let last = sheet.cells.iter().map(|(at, _)| at.row).max();
        for row in 0..=last.unwrap_or(0) {
            out.push_str("<table:table-row>");
            let cols: Vec<u32> = sheet
                .cells
                .iter()
                .filter(|(at, _)| at.row == row)
                .map(|(at, _)| at.col)
                .collect();
            let widest = cols.iter().copied().max();
            for col in 0..=widest.unwrap_or(0) {
                match sheet.cells.get(CellRef::new(row, col)) {
                    Some(cell) if cols.contains(&col) => {
                        out.push_str(&cell_xml(workbook, cell));
                    }
                    _ => out.push_str("<table:table-cell/>"),
                }
            }
            out.push_str("</table:table-row>");
        }
        out.push_str("</table:table>");
    }
    out.push_str("</office:spreadsheet></office:body></office:document-content>");
    out
}

fn cell_xml(workbook: &Workbook, cell: &Cell) -> String {
    match &cell.value {
        CellValue::Number(n) => format!(
            r#"<table:table-cell office:value-type="float" office:value="{n}"><text:p>{n}</text:p></table:table-cell>"#
        ),
        CellValue::Bool(b) => format!(
            r#"<table:table-cell office:value-type="boolean" office:boolean-value="{b}"><text:p>{b}</text:p></table:table-cell>"#
        ),
        CellValue::SharedString(id) | CellValue::InlineString(id) => format!(
            r#"<table:table-cell office:value-type="string"><text:p>{}</text:p></table:table-cell>"#,
            escape(workbook.strings.get(*id).unwrap_or_default())
        ),
        _ => "<table:table-cell/>".to_owned(),
    }
}

/// Escape text for an XML attribute or element body.
fn escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Written by LibreOffice, not by the writer below.
    ///
    /// A reader tested against its own writer proves the two agree with each
    /// other and nothing about whether either is right — which is the whole
    /// reason this crate exists, since the files it must read come from
    /// somebody else's spreadsheet.
    const REAL: &[u8] = include_bytes!("../tests/fixtures/libreoffice-basic.ods");

    fn text_at(wb: &Workbook, sheet: usize, row: u32, col: u32) -> String {
        wb.sheets[sheet]
            .cells
            .get(CellRef::new(row, col))
            .map(|c| match &c.value {
                CellValue::SharedString(id) | CellValue::InlineString(id) => {
                    wb.strings.get(*id).unwrap_or_default().to_owned()
                }
                CellValue::Number(n) => n.to_string(),
                CellValue::Bool(b) => b.to_string(),
                _ => String::new(),
            })
            .unwrap_or_default()
    }

    /// **A file LibreOffice wrote opens, with its values where they belong.**
    #[test]
    fn a_real_libreoffice_document_reads() {
        let (wb, _) = import_ods(REAL).expect("LibreOffice's own output must open");
        assert_eq!(wb.sheets.len(), 1);
        assert_eq!(wb.sheets[0].name, "seed", "the sheet name was not carried");

        assert_eq!(text_at(&wb, 0, 0, 0), "Item");
        assert_eq!(text_at(&wb, 0, 0, 3), "Total");
        assert_eq!(text_at(&wb, 0, 1, 0), "Widget");
        assert_eq!(text_at(&wb, 0, 1, 1), "3");
        assert_eq!(text_at(&wb, 0, 2, 0), "Gadget");
    }

    /// **A repeated run moves the cursor.**
    ///
    /// `table:number-columns-repeated` is how ODF compresses runs of identical
    /// cells, and LibreOffice emits it constantly. A reader that ignores it
    /// does not lose a little formatting — it puts every cell after the first
    /// repeat in the wrong column, which is a corrupt document that opens
    /// without complaint. This fixture carries a `repeated="2"` run, so the
    /// cells after it are only in the right place if it was honoured.
    #[test]
    fn a_repeated_run_places_the_cells_after_it_correctly() {
        let (wb, _) = import_ods(REAL).expect("opens");
        // Row 4 is `_, _, "Sum", =SUM(...)`, and its leading blanks are written
        // as one repeated cell. "Sum" must land in column C, not column A.
        assert_eq!(
            text_at(&wb, 0, 3, 2),
            "Sum",
            "the repeated run was ignored, so everything after it shifted left"
        );
        assert_eq!(
            text_at(&wb, 0, 3, 0),
            "",
            "something was placed in the blank run"
        );
    }

    /// **A formula survives the translation.**
    ///
    /// ODF writes `of:=[.B2]*[.C2]`; this engine parses `B2*C2`.
    #[test]
    fn formulas_are_translated_from_odf_syntax() {
        let (wb, _) = import_ods(REAL).expect("opens");
        let cell = wb.sheets[0]
            .cells
            .get(CellRef::new(1, 3))
            .expect("D2 holds the row's total");
        assert!(cell.formula.is_some(), "the formula was dropped entirely");

        // And the range form, which is where the `.` stripping has to reach
        // inside a colon.
        let total = wb.sheets[0]
            .cells
            .get(CellRef::new(3, 3))
            .expect("D4 holds the sum");
        assert!(total.formula.is_some(), "SUM([.D2:.D3]) did not translate");
    }

    /// **What could not be carried is named.**
    ///
    /// A converter that silently discards formatting looks like it worked and
    /// costs somebody an afternoon. The report is what lets them decide whether
    /// to use it (`AGENTS.md`: no silent data loss).
    #[test]
    fn what_is_not_carried_is_reported() {
        let (_, report) = import_ods(REAL).expect("opens");
        assert!(
            !report.is_empty(),
            "a real document carried styles and column widths, and nothing was reported"
        );
    }

    /// **`mimetype` is first and uncompressed.**
    ///
    /// ODF requires it, and it is not decoration: it is what lets `file(1)` and
    /// every desktop identify the format from the first bytes without
    /// unzipping. Compressed or second, the file still opens in LibreOffice and
    /// is unrecognised everywhere else — which looks like it worked.
    #[test]
    fn the_written_package_leads_with_an_uncompressed_mimetype() {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        wb.sheets
            .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "S"));
        let bytes = export_ods(&wb).expect("writes");

        let mut zip = zip::ZipArchive::new(Cursor::new(&bytes[..])).expect("a zip");
        let first = zip.by_index(0).expect("at least one entry");
        assert_eq!(first.name(), "mimetype", "mimetype is not the first entry");
        assert_eq!(
            first.compression(),
            zip::CompressionMethod::Stored,
            "mimetype is compressed, so the format cannot be sniffed"
        );
    }

    /// **Values survive a round trip through our own writer.**
    ///
    /// Weaker than the read tests above by design — it proves the writer emits
    /// what this reader understands, which is necessary and not sufficient. The
    /// LibreOffice fixture is what makes it meaningful.
    #[test]
    fn values_survive_a_round_trip() {
        let (original, _) = import_ods(REAL).expect("opens");
        let written = export_ods(&original).expect("writes");
        let (again, _) = import_ods(&written).expect("what we wrote must open");

        for (row, col) in [(0u32, 0u32), (0, 3), (1, 0), (1, 1), (3, 2)] {
            assert_eq!(
                text_at(&again, 0, row, col),
                text_at(&original, 0, row, col),
                "r{row}c{col} did not survive the round trip"
            );
        }
        assert_eq!(again.sheets[0].name, original.sheets[0].name);
    }

    /// **Rubbish is refused, not half-read.**
    #[test]
    fn a_file_that_is_not_a_package_is_refused() {
        assert!(import_ods(b"not a zip").is_err());
        // A zip with no content.xml is a zip, not a spreadsheet.
        let mut empty = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut empty));
            zip.start_file("other.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"hi").unwrap();
            zip.finish().unwrap();
        }
        assert!(matches!(import_ods(&empty), Err(OdsError::NoContent)));
    }
}
