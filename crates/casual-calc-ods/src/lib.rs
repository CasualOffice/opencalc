//! Reading and writing OpenDocument Spreadsheets — deliberately narrow.
//!
//! # What this is, and what it is not
//!
//! This carries **values, formulas and sheets**, and nothing else. It exists so
//! that a LibreOffice-first shop can open a `.ods`, work on it, and get a `.ods`
//! back — which is the difference between OpenCalc being usable at all in those
//! shops and not. It is explicitly not a fidelity implementation: styles,
//! merges, charts, images, conditional formatting, validation and print setup
//! are **counted and named** on the way through, never dropped quietly — by the
//! [`CompatibilityReport`] [`import_ods`] returns on the way in, and by
//! [`export_loss`] on the way out, which a host asks *before* it overwrites
//! somebody's file.
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

/// The cells a single repeat attribute may **materialise**.
///
/// LibreOffice writes `number-rows-repeated="1048576"` to fill the sheet. That
/// is a description of emptiness, not a million rows of data, and materialising
/// it would exhaust memory reading a file that holds nothing.
///
/// **It bounds what is stored, never how far the cursor moves.** Clamping the
/// cursor is the same defect the repeat attributes exist to avoid: a sheet with
/// data on row 1 and row 50,000 writes the gap as one repeated empty row, and a
/// reader that advanced by 4,096 instead put every later row 45,904 rows too
/// high — silently, in a document that opens without complaint. The gap costs
/// nothing to skip, because an empty run stores no cells at all.
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
    /// How many columns this cell covers — **unclamped**, because it is the
    /// cursor's step. See [`MAX_REPEAT`], which bounds only what is stored.
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
                            .unwrap_or(1);
                        col = 0;
                        // **A self-closing row has no `End` event either.** The
                        // same defect as the cell below, one level up, and the
                        // one LibreOffice actually triggers: it writes the gap
                        // between two blocks of data as a single
                        // `<table:table-row table:number-rows-repeated="N"/>`,
                        // and a reader that waits for a close that never comes
                        // leaves the row cursor where it was — so the second
                        // block lands on top of the first.
                        if self_closing {
                            row = row.saturating_add(row_repeat);
                        }
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
                                .unwrap_or(1),
                        };
                        // **A self-closing cell has no `End` event.** A blank
                        // run is written `<table:table-cell repeated="2"/>`,
                        // and waiting for a close that never comes left the
                        // column cursor where it was — so everything after the
                        // run landed on top of it. LibreOffice writes those
                        // runs on nearly every row.
                        if self_closing {
                            col = col.saturating_add(place(
                                &mut workbook,
                                &mut report,
                                row,
                                col,
                                row_repeat,
                                &cell,
                            ));
                        } else {
                            pending = Some(cell);
                        }
                    }
                    "p" => {
                        in_text = !self_closing;
                        // **The line break belongs here, at the paragraph
                        // boundary.** A cell's text is one `<text:p>` per line,
                        // and putting the newline on every *text event* instead
                        // broke a single line into several: an entity like
                        // `&amp;` arrives as its own event, so "Ada & Co" came
                        // back as three lines.
                        if let Some(p) = pending.as_mut()
                            && !p.text.is_empty()
                        {
                            p.text.push('\n');
                        }
                    }
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
                    // Unescaped, for the same reason the attributes are: a cell
                    // holding "Ada & Co" is written `Ada &amp; Co`, and reading
                    // it raw gains four characters on every save.
                    let raw = t.decode().map_err(|e| OdsError::Malformed(e.to_string()))?;
                    p.text.push_str(&unescaped(&raw));
                }
            }
            // `&amp;` and `&#38;` are their own events, not part of the text
            // around them. Dropped, they take the character with them.
            Event::GeneralRef(r) => {
                if in_text && let Some(p) = pending.as_mut() {
                    let name = r.decode().map_err(|e| OdsError::Malformed(e.to_string()))?;
                    match r.resolve_char_ref() {
                        Ok(Some(c)) => p.text.push(c),
                        _ => p.text.push_str(
                            quick_xml::escape::resolve_predefined_entity(&name)
                                // An entity from a DTD this reader does not
                                // read is kept as written: visibly wrong beats
                                // invisibly gone.
                                .unwrap_or(&format!("&{name};")),
                        ),
                    }
                }
            }
            Event::End(e) => match local_name(e.name().as_ref()).as_str() {
                "p" => in_text = false,
                "table-cell" | "covered-table-cell" => {
                    if let Some(p) = pending.take() {
                        let width = place(&mut workbook, &mut report, row, col, row_repeat, &p);
                        col = col.saturating_add(width);
                    }
                }
                "table-row" => row = row.saturating_add(row_repeat),
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
        return p.repeat.max(1);
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
        // An empty run: move the cursor, store nothing. However long it claims
        // to be, it costs nothing — which is why the cursor is not clamped.
        return p.repeat.max(1);
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
    let mut outside = 0u64;
    let Some(sheet) = workbook.sheets.last_mut() else {
        return p.repeat.max(1);
    };
    // Clamped **here**, where cells are actually stored, and nowhere else.
    for dr in 0..row_repeat.clamp(1, MAX_REPEAT) {
        for dc in 0..p.repeat.clamp(1, MAX_REPEAT) {
            let at = CellRef::new(row.saturating_add(dr), col.saturating_add(dc));
            // A file may address further than this engine's grid reaches. Kept
            // out rather than stored, because a cell past the last column is
            // one no writer here can put back — and counted, because a cell
            // that disappears without a word is the failure this report exists
            // to prevent.
            if !at.in_grid() {
                outside += 1;
                continue;
            }
            let mut cell = Cell::value(value.clone());
            cell.formula = handle;
            sheet.cells.set(at, cell);
        }
    }
    if outside > 0 {
        report.record_n(
            "cells outside this engine's grid",
            ModelOutcome::Omitted,
            RetentionOutcome::NotRetained,
            outside,
        );
    }
    p.repeat.max(1)
}

/// Turn an ODF formula into one this engine can parse, or `None`.
///
/// ODF writes `of:=IF([.A1]>0;[.B1];0)`. Three things differ from what this
/// engine's parser expects, and all three are load-bearing:
///
/// - a namespace prefix (`of:`), which is dropped;
/// - every reference wrapped in brackets, `[.A1]` for this sheet and
///   `[$Sheet2.A1]` for another, with the name quoted when it needs to be;
/// - **`;` between arguments, not `,`.** `of:` is the locale-independent
///   formula namespace and the semicolon is what it uses. A translation that
///   ignored this failed on every multi-argument function — which is nearly
///   every formula in a real document — so the file opened, the numbers looked
///   right, and every `IF` had quietly become a constant.
///
/// String literals are copied through untouched: a `;` inside quotes is data,
/// and rewriting it edits somebody's text.
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
        match c {
            '"' => {
                out.push('"');
                while let Some(d) = chars.next() {
                    out.push(d);
                    // A doubled quote escapes one and does not end the literal.
                    if d == '"' {
                        if chars.peek() == Some(&'"') {
                            out.push('"');
                            chars.next();
                            continue;
                        }
                        break;
                    }
                }
            }
            ';' => out.push(','),
            '[' => {
                let mut inner = String::new();
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                    inner.push(c);
                }
                out.push_str(&a1_from_odf(&inner)?);
            }
            _ => out.push(c),
        }
    }
    casual_calc_formula::parse(&out).ok()
}

/// The A1 form of one bracketed ODF reference, or `None` for one this engine
/// cannot express.
///
/// The shapes: `.A1`, `.$A$1`, `.A1:.B2`, `$Sheet2.A1`, `$'My Sheet'.A1`, and a
/// qualified range `$Sheet2.A1:.B2` whose far end takes the near end's sheet.
///
/// Refused rather than approximated: a reference to a deleted sheet (`#REF!`),
/// a named expression (`$$name`), and a range spanning two different sheets —
/// which is a 3-D reference this engine does not model, and picking one of the
/// two sheets would silently point the formula somewhere it never did.
fn a1_from_odf(inner: &str) -> Option<String> {
    if inner.contains('#') || inner.starts_with("$$") {
        return None;
    }
    let (first, second) = split_odf_range(inner);
    let (sheet, address) = odf_parts(first)?;
    let mut out = String::new();
    if let Some(name) = &sheet {
        out.push_str(name);
        out.push('!');
    }
    out.push_str(address);
    if let Some(second) = second {
        let (far_sheet, far_address) = odf_parts(second)?;
        if far_sheet.is_some() && far_sheet != sheet {
            return None; // A 3-D range.
        }
        out.push(':');
        out.push_str(far_address);
    }
    Some(out)
}

/// Split `a:b` on the colon that separates the two ends of a range, ignoring
/// one inside a quoted sheet name.
fn split_odf_range(inner: &str) -> (&str, Option<&str>) {
    let mut quoted = false;
    for (i, c) in inner.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            ':' if !quoted => return (&inner[..i], Some(&inner[i + 1..])),
            _ => {}
        }
    }
    (inner, None)
}

/// One end of an ODF reference, split into its sheet (already in this engine's
/// quoting) and its A1 address.
fn odf_parts(part: &str) -> Option<(Option<String>, &str)> {
    let rest = part.strip_prefix('$').unwrap_or(part);
    if let Some(quoted) = rest.strip_prefix('\'') {
        // `'My Sheet'.A1`, with `''` escaping a quote inside the name.
        let mut name = String::new();
        let mut chars = quoted.char_indices();
        while let Some((i, c)) = chars.next() {
            if c != '\'' {
                name.push(c);
                continue;
            }
            if quoted[i + 1..].starts_with('\'') {
                name.push('\'');
                chars.next();
                continue;
            }
            let address = quoted[i + 1..].strip_prefix('.')?;
            // Re-quoted the way this engine's parser expects, which is the same
            // convention with the same escape.
            return Some((Some(format!("'{}'", name.replace('\'', "''"))), address));
        }
        return None;
    }
    let (name, address) = rest.split_once('.')?;
    if name.is_empty() {
        return Some((None, address));
    }
    Some((Some(name.to_owned()), address))
}

/// XML text with its entities resolved.
///
/// Text this cannot resolve — an entity declared in a DTD this reader does not
/// read — is kept **as written** rather than dropped. The raw `&thing;` is
/// wrong in a way somebody can see and correct; a silently empty cell is not.
fn unescaped(raw: &str) -> String {
    quick_xml::escape::unescape(raw).map_or_else(|_| raw.to_owned(), |text| text.into_owned())
}

/// The element name without its namespace prefix.
fn local_name(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    text.rsplit(':').next().unwrap_or(&text).to_owned()
}

/// One attribute's value, by its qualified name.
///
/// **Unescaped.** XML has five characters that cannot be written literally, and
/// an attribute holding `A1&amp;"x"` is a formula with an `&` in it — read raw
/// it is nine characters of nonsense that no parser accepts, and a sheet name
/// with an ampersand grows four characters every time the file is opened and
/// saved. Neither failure is visible until somebody looks.
fn attr(e: &quick_xml::events::BytesStart<'_>, want: &str) -> Option<String> {
    let short = want.rsplit(':').next().unwrap_or(want);
    e.attributes().flatten().find_map(|a| {
        let key = String::from_utf8_lossy(a.key.as_ref()).to_string();
        let matches = key == want || key.rsplit(':').next() == Some(short);
        matches.then(|| unescaped(&String::from_utf8_lossy(&a.value)))
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
        // **One pass over the populated cells**, which arrive in row-major
        // order, with the gaps between them written as the repeat runs ODF has
        // for exactly this. The shape it replaces walked every row from zero
        // and re-scanned every cell in the sheet to find that row's columns —
        // quadratic in the cell count, on the save path of a service that
        // accepts uploads, and it also materialised a blank cell for every gap
        // in a sparse sheet.
        let mut open_row: Option<u32> = None;
        let mut next_col = 0u32;
        for (at, cell) in sheet.cells.iter() {
            if open_row != Some(at.row) {
                if open_row.is_some() {
                    out.push_str("</table:table-row>");
                }
                let first_free = open_row.map_or(0, |r| r + 1);
                if at.row > first_free {
                    out.push_str(&repeated("table:table-row", at.row - first_free));
                }
                out.push_str("<table:table-row>");
                open_row = Some(at.row);
                next_col = 0;
            }
            if at.col > next_col {
                out.push_str(&repeated("table:table-cell", at.col - next_col));
            }
            out.push_str(&cell_xml(workbook, cell));
            next_col = at.col + 1;
        }
        if open_row.is_some() {
            out.push_str("</table:table-row>");
        }
        out.push_str("</table:table>");
    }
    out.push_str("</office:spreadsheet></office:body></office:document-content>");
    out
}

/// An empty run of `count` rows or cells, as one element.
fn repeated(element: &str, count: u32) -> String {
    let attribute = if element == "table:table-row" {
        "table:number-rows-repeated"
    } else {
        "table:number-columns-repeated"
    };
    if count == 1 {
        format!("<{element}/>")
    } else {
        format!(r#"<{element} {attribute}="{count}"/>"#)
    }
}

fn cell_xml(workbook: &Workbook, cell: &Cell) -> String {
    // **The formula, first.** Without it a saved `.ods` is a table of constants
    // that opens without complaint and shows the right numbers — until somebody
    // changes an input. `None` where this converter cannot express the formula
    // in ODF; the cached value below still goes out, and [`export_loss`] counts
    // what the file no longer recalculates.
    let formula = cell
        .formula
        .and_then(|handle| workbook.formula(handle))
        .and_then(odf_formula)
        .map(|text| format!(r#" table:formula="{}""#, escape(&text)))
        .unwrap_or_default();

    match &cell.value {
        CellValue::Number(n) => format!(
            r#"<table:table-cell{formula} office:value-type="float" office:value="{n}"><text:p>{n}</text:p></table:table-cell>"#
        ),
        CellValue::Bool(b) => format!(
            r#"<table:table-cell{formula} office:value-type="boolean" office:boolean-value="{b}"><text:p>{b}</text:p></table:table-cell>"#
        ),
        CellValue::SharedString(id) | CellValue::InlineString(id) => format!(
            r#"<table:table-cell{formula} office:value-type="string"><text:p>{}</text:p></table:table-cell>"#,
            escape(workbook.strings.get(*id).unwrap_or_default())
        ),
        // An error is written as its text rather than dropped. It reads back as
        // a string rather than an error, which [`export_loss`] says out loud —
        // a cell that held `#DIV/0!` and comes back blank is the failure that
        // looks like the file was fixed.
        CellValue::Error(e) => format!(
            r#"<table:table-cell{formula} office:value-type="string"><text:p>{}</text:p></table:table-cell>"#,
            escape(&e.to_string())
        ),
        CellValue::Empty => format!("<table:table-cell{formula}/>"),
    }
}

/// One formula in ODF's own syntax (without the `of:=` prefix's `=`), or `None`
/// for one this converter cannot express there.
///
/// The inverse of [`translate_formula`], and it has the same duty: a formula
/// that cannot be written faithfully is **not** approximated. The printed A1
/// text is walked once — string literals copied through, `,` turned into ODF's
/// `;`, and every reference wrapped by [`odf_reference`].
fn odf_formula(expr: &casual_calc_formula::Expr) -> Option<String> {
    if !writable_in_odf(expr) {
        return None;
    }
    let text = expr.to_string();
    let chars: Vec<char> = text.chars().collect();
    let spans = casual_calc_formula::reference_spans(&text);

    let mut out = String::from("of:=");
    let mut next = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        while spans.get(next).is_some_and(|s| s.start < i) {
            next += 1;
        }
        if let Some(span) = spans.get(next).filter(|s| s.start == i) {
            let raw: String = chars[span.start..span.end].iter().collect();
            out.push_str(&odf_reference(&raw)?);
            i = span.end;
            next += 1;
            continue;
        }
        match chars[i] {
            '"' => {
                out.push('"');
                i += 1;
                while i < chars.len() {
                    out.push(chars[i]);
                    if chars[i] == '"' {
                        if chars.get(i + 1) == Some(&'"') {
                            out.push('"');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            ',' => {
                out.push(';');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Some(out)
}

/// Whether every part of this expression has an ODF form this writer can
/// produce.
///
/// Written as an exhaustive match, so a new [`Expr`](casual_calc_formula::Expr)
/// variant has to be decided about rather than silently assumed writable —
/// which is how a construct starts being written as something it is not.
///
/// Refused: a defined name and a structured (table) reference, whose ODF
/// spellings this does not produce; text the parser could not read, which came
/// in from a `.xlsx` and has no ODF meaning; a call of a `LAMBDA` value, which
/// OpenFormula has no notation for; and a whole-row or whole-column reference
/// (`A:A`), which ODF writes as an explicit block to the sheet's last row —
/// a different reference, and one that stops growing with the data.
fn writable_in_odf(expr: &casual_calc_formula::Expr) -> bool {
    use casual_calc_formula::Expr;
    match expr {
        Expr::Number(_) | Expr::Bool(_) | Expr::Text(_) | Expr::Error(_) | Expr::Empty => true,
        Expr::Reference(r) => !r.col_implicit && !r.row_implicit,
        Expr::Range(a, b) => {
            !a.col_implicit && !a.row_implicit && !b.col_implicit && !b.row_implicit
        }
        Expr::Name(_) | Expr::Raw(_) | Expr::StructuredRef { .. } | Expr::Call { .. } => false,
        Expr::Unary { operand, .. } => writable_in_odf(operand),
        Expr::Binary { left, right, .. } => writable_in_odf(left) && writable_in_odf(right),
        Expr::Function { args, .. } => args.iter().all(writable_in_odf),
    }
}

/// One A1 reference as ODF writes it: `A1` → `[.A1]`, `Sheet2!A1:B2` →
/// `[$Sheet2.A1:.B2]`.
fn odf_reference(raw: &str) -> Option<String> {
    let (sheet, address) = match raw.rfind('!') {
        // A sheet name never contains `!` and an address never does either, so
        // the last one is the separator.
        Some(at) => (Some(&raw[..at]), &raw[at + 1..]),
        None => (None, raw),
    };
    let qualifier = match sheet {
        None => ".".to_owned(),
        Some(name) => format!("${name}."),
    };
    match address.split_once(':') {
        // The far end takes the near end's sheet, which is how ODF spells a
        // range: `[$Sheet2.A1:.B2]`.
        Some((near, far)) => Some(format!("[{qualifier}{near}:.{far}]")),
        None => Some(format!("[{qualifier}{address}]")),
    }
}

/// What writing this workbook as a `.ods` cannot carry.
///
/// **Ask before saving.** This converter carries values, formulas and sheets;
/// everything else in the model — formatting, merges, widths, validation,
/// conditional formats, comments, charts, images, print setup — has no writer
/// here yet, and the rule this repository runs on is that nothing is dropped
/// quietly (`AGENTS.md`). Counted per feature, so a host can tell an
/// administrator what a save costs before they commit to it.
///
/// Recomputed on each call rather than kept: it describes the document as it is
/// now, and a merge made a second ago changes the answer.
#[must_use]
pub fn export_loss(workbook: &Workbook) -> CompatibilityReport {
    let mut report = CompatibilityReport::default();
    let mut gone = |feature: &str, count: usize| {
        report.record_n(
            feature,
            ModelOutcome::Omitted,
            RetentionOutcome::NotRetained,
            count as u64,
        );
    };

    gone("defined names", workbook.defined_names.len());

    let mut formatted = 0usize;
    let mut untranslatable = 0usize;
    let mut errors = 0usize;
    for sheet in &workbook.sheets {
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

        for (_, cell) in sheet.cells.iter() {
            if cell.style.is_some() {
                formatted += 1;
            }
            if matches!(cell.value, CellValue::Error(_)) {
                errors += 1;
            }
            // Asked of the same function the writer uses, so the count and the
            // file cannot disagree.
            if cell
                .formula
                .and_then(|handle| workbook.formula(handle))
                .is_some_and(|expr| odf_formula(expr).is_none())
            {
                untranslatable += 1;
            }
        }
    }
    gone("cell formatting", formatted);

    // Degraded rather than omitted, and after the closure's borrow ends: the
    // cell keeps its value in both cases, and what is gone is the formula that
    // produced it or the fact that it was an error rather than text.
    report.record_n(
        "formulas this converter cannot write as ODF",
        ModelOutcome::Degraded,
        RetentionOutcome::NotRetained,
        untranslatable as u64,
    );
    report.record_n(
        "error values written as text",
        ModelOutcome::Degraded,
        RetentionOutcome::NotRetained,
        errors as u64,
    );
    report
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

    /// A `.ods` holding exactly this `content.xml`.
    fn package_of(content: &str) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut out));
            zip.start_file("content.xml", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(content.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        out
    }

    /// A document with `body` between the sheet tags.
    fn sheet_of(body: &str) -> Vec<u8> {
        package_of(&format!(
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8"?>"#,
                r#"<office:document-content"#,
                r#" xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0""#,
                r#" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0""#,
                r#" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0">"#,
                r#"<office:body><office:spreadsheet>"#,
                r#"<table:table table:name="s">{}</table:table>"#,
                r#"</office:spreadsheet></office:body></office:document-content>"#,
            ),
            body
        ))
    }

    fn cell(text: &str) -> String {
        format!(
            r#"<table:table-cell office:value-type="string"><text:p>{text}</text:p></table:table-cell>"#
        )
    }

    /// **A long empty run moves the cursor by its whole length.**
    ///
    /// The bound on repeats exists so that `number-rows-repeated="1048576"` —
    /// which LibreOffice writes on nearly every sheet — does not materialise a
    /// million rows. Applying it to the *cursor* as well turned any gap longer
    /// than the bound into a silent shift: a sheet with data on row 1 and row
    /// 9,000 came back with the second block 4,900 rows too high, in a document
    /// that opens without complaint. An empty run costs nothing to skip, which
    /// is exactly why the clamp does not belong on the cursor.
    #[test]
    fn a_long_empty_run_does_not_shift_what_follows_it() {
        let rows = sheet_of(&format!(
            concat!(
                r#"<table:table-row>{}</table:table-row>"#,
                r#"<table:table-row table:number-rows-repeated="9000"/>"#,
                r#"<table:table-row>{}</table:table-row>"#,
            ),
            cell("top"),
            cell("bottom")
        ));
        let (wb, _) = import_ods(&rows).expect("opens");
        assert_eq!(text_at(&wb, 0, 0, 0), "top");
        assert_eq!(
            text_at(&wb, 0, 9001, 0),
            "bottom",
            "the row gap was clamped, so everything after it moved up"
        );

        let cols = sheet_of(&format!(
            r#"<table:table-row>{}<table:table-cell table:number-columns-repeated="9000"/>{}</table:table-row>"#,
            cell("left"),
            cell("right")
        ));
        let (wb, _) = import_ods(&cols).expect("opens");
        assert_eq!(
            text_at(&wb, 0, 0, 9001),
            "right",
            "the column gap was clamped, so everything after it moved left"
        );
    }

    /// **A repeat that reaches past the grid is counted, not stored.**
    ///
    /// A file can address further than this engine's grid goes, and a cell
    /// stored outside it is one no writer can put back.
    #[test]
    fn cells_beyond_the_grid_are_named_rather_than_stored() {
        let body = format!(
            r#"<table:table-row><table:table-cell table:number-columns-repeated="20000"/>{}</table:table-row>"#,
            cell("far")
        );
        let (wb, report) = import_ods(&sheet_of(&body)).expect("opens");
        assert!(
            wb.sheets[0].cells.iter().all(|(at, _)| at.in_grid()),
            "a cell was stored outside the grid"
        );
        assert!(
            report
                .entries()
                .iter()
                .any(|e| e.feature == "cells outside this engine's grid"),
            "a cell was dropped and nothing said so: {:?}",
            report.entries()
        );
    }

    /// **ODF separates arguments with `;`, and this engine with `,`.**
    ///
    /// Not a detail: `of:` is the locale-independent formula namespace, and it
    /// uses the semicolon. Handing `IF(A1>0;1;2)` to a parser that expects
    /// commas fails on *every multi-argument function*, which is nearly every
    /// formula in a real document — so the file opens, the numbers look right,
    /// and every `IF`, `SUM(a;b)` and `VLOOKUP` has quietly become a constant.
    #[test]
    fn odf_argument_separators_are_translated() {
        let expr = translate_formula("of:=IF([.A1]>0;[.B1];0)")
            .expect("a multi-argument function is the common case, not an exotic one");
        assert_eq!(format!("{expr}"), "IF(A1>0,B1,0)");

        // And a semicolon *inside a string* is text, not a separator. Replacing
        // them blindly rewrites somebody's data.
        let expr = translate_formula(r#"of:=IF([.A1];"a;b";"c")"#).expect("translates");
        assert_eq!(format!("{expr}"), r#"IF(A1,"a;b","c")"#);
    }

    /// **`&` and `<` come back as themselves.**
    ///
    /// XML has five characters that cannot be written literally, and the writer
    /// escapes them. A reader that does not *un*escape reads `Ada &amp; Co`
    /// back as those nine characters — so a name with an ampersand in it gains
    /// four characters every time the file is opened and saved, and a formula
    /// with a `&` concat operator stops parsing altogether. Nothing about
    /// either failure is visible until somebody looks at the cell.
    #[test]
    fn xml_entities_survive_the_round_trip() {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        wb.sheets
            .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "P&L <2024>"));
        let text = wb.strings.intern("Ada & Co <ltd>");
        wb.sheets[0].cells.set(
            CellRef::new(0, 0),
            Cell::value(CellValue::InlineString(text)),
        );
        // A formula whose text carries an escaped character in an *attribute*.
        let concat = wb.store_formula(casual_calc_formula::parse(r#"A1&"x""#).expect("parses"));
        let mut cell = Cell::value(CellValue::Number(0.0));
        cell.formula = Some(concat);
        wb.sheets[0].cells.set(CellRef::new(1, 0), cell);

        let (again, _) = import_ods(&export_ods(&wb).expect("writes")).expect("opens");
        assert_eq!(
            again.sheets[0].name, "P&L <2024>",
            "the sheet name came back escaped"
        );
        assert_eq!(
            text_at(&again, 0, 0, 0),
            "Ada & Co <ltd>",
            "the cell text came back escaped"
        );
        let formula = again.sheets[0]
            .cells
            .get(CellRef::new(1, 0))
            .and_then(|c| c.formula)
            .and_then(|h| again.formula(h))
            .expect("a formula with an `&` in it did not survive");
        assert_eq!(format!("{formula}"), r#"A1&"x""#);
    }

    /// **A reference to another sheet points at that sheet.**
    ///
    /// ODF qualifies with `$Sheet.A1`, quoting the name when it needs it. The
    /// alternative to translating this is dropping every cross-sheet formula in
    /// any workbook with more than one tab.
    #[test]
    fn a_sheet_qualified_reference_is_translated() {
        let expr = translate_formula("of:=[$Sheet2.A1]+[$'My Sheet'.B2]")
            .expect("a cross-sheet reference is not exotic either");
        assert_eq!(format!("{expr}"), "Sheet2!A1+'My Sheet'!B2");

        // A range keeps its qualifier, and the far end's implied sheet is the
        // same one — `[$S.A1:.B2]`.
        let expr = translate_formula("of:=SUM([$Sheet2.A1:.B2])").expect("translates");
        assert_eq!(format!("{expr}"), "SUM(Sheet2!A1:B2)");
    }

    /// **A formula this converter cannot express is refused, not guessed at.**
    #[test]
    fn an_untranslatable_reference_is_refused() {
        // A reference to a deleted sheet.
        assert!(translate_formula("of:=[#REF!.A1]").is_none());
        // Two different sheets in one range is a 3-D reference, which this
        // engine does not model — pointing it at one of the two would be an
        // answer nobody asked for.
        assert!(translate_formula("of:=SUM([$One.A1:$Two.B2])").is_none());
    }

    /// **A formula survives the round trip as a formula, not as its answer.**
    ///
    /// The reader translates `of:=[.B2]*[.C2]` in; a writer that emitted only
    /// the cached number would turn a LibreOffice user's model into a table of
    /// constants the first time they saved it — and the file would open
    /// without complaint, showing the right numbers, until somebody changed an
    /// input. That is the failure this crate's own first paragraph promises not
    /// to have: it says this carries "values, **formulas** and sheets".
    #[test]
    fn formulas_survive_a_round_trip() {
        let (original, _) = import_ods(REAL).expect("opens");
        let written = export_ods(&original).expect("writes");
        let (again, _) = import_ods(&written).expect("what we wrote must open");

        // D2 is the row total, `=B2*C2`.
        let before = original.sheets[0]
            .cells
            .get(CellRef::new(1, 3))
            .and_then(|c| c.formula)
            .and_then(|h| original.formula(h))
            .expect("the fixture's D2 has a formula to begin with");
        let after = again.sheets[0]
            .cells
            .get(CellRef::new(1, 3))
            .and_then(|c| c.formula)
            .and_then(|h| again.formula(h))
            .expect("D2 came back as a constant: the writer dropped the formula");
        assert_eq!(
            format!("{after}"),
            format!("{before}"),
            "D2's formula came back as something else"
        );

        // And the range form, where the reference wrapping has to reach inside
        // a colon: `SUM([.D2:.D3])`.
        let sum = again.sheets[0]
            .cells
            .get(CellRef::new(3, 3))
            .and_then(|c| c.formula)
            .and_then(|h| again.formula(h))
            .expect("D4's SUM came back as a constant");
        assert_eq!(format!("{sum}"), "SUM(D2:D3)");
    }

    /// **A formula reaches the file in ODF's own syntax, not this engine's.**
    ///
    /// Asserted on the bytes, because the reader above is forgiving — it takes
    /// a bare `=B2*C2` as happily as `of:=[.B2]*[.C2]`, so a round trip through
    /// our own reader proves nothing about whether LibreOffice can read what we
    /// wrote. ODF wants the bracketed references, and a file without them opens
    /// there with `#NAME?` in every formula cell.
    #[test]
    fn a_written_formula_is_in_odf_syntax() {
        let (original, _) = import_ods(REAL).expect("opens");
        let written = export_ods(&original).expect("writes");

        let mut zip = zip::ZipArchive::new(Cursor::new(&written[..])).expect("a zip");
        let mut xml = String::new();
        zip.by_name("content.xml")
            .expect("content.xml")
            .read_to_string(&mut xml)
            .expect("utf-8");

        assert!(
            xml.contains(r#"table:formula="of:=[.B2]*[.C2]""#),
            "D2's formula is not in ODF syntax: {xml}"
        );
        assert!(
            xml.contains(r#"table:formula="of:=SUM([.D2:.D3])""#),
            "the range form is not in ODF syntax: {xml}"
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

    /// **What the writer cannot carry is counted before it is written.**
    ///
    /// The report is what makes advertising `.ods` honest: this converter
    /// carries values, formulas and sheets, and a host that saves one has to be
    /// able to tell somebody what it cost *before* the file is overwritten.
    /// Asserted against the bytes as well, so the report cannot drift into
    /// warning about things that actually survived.
    #[test]
    fn what_the_writer_cannot_carry_is_counted() {
        use casual_calc_model::{CellRange, Style};

        let mut wb = Workbook::new(Id::from_parts(1, 1));
        wb.sheets
            .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "S"));
        // A merge, which has no writer here.
        wb.sheets[0]
            .merges
            .push(CellRange::new(CellRef::new(0, 0), CellRef::new(0, 1)));
        // A formatted cell.
        let bold = wb.styles.intern(Style {
            bold: true,
            ..Style::default()
        });
        let text = wb.strings.intern("Item");
        let mut cell = Cell::value(CellValue::InlineString(text));
        cell.style = Some(bold);
        wb.sheets[0].cells.set(CellRef::new(0, 0), cell);
        // A formula with no ODF spelling this converter produces.
        let named = wb.store_formula(casual_calc_formula::parse("TAX_RATE").expect("parses"));
        let mut cell = Cell::value(CellValue::Number(0.2));
        cell.formula = Some(named);
        wb.sheets[0].cells.set(CellRef::new(1, 0), cell);

        let report = export_loss(&wb);
        let named_features: Vec<(String, u64)> = report
            .entries()
            .into_iter()
            .map(|e| (e.feature.to_string(), e.count))
            .collect();
        let count = |want: &str| {
            named_features
                .iter()
                .find(|(feature, _)| feature == want)
                .map_or(0, |(_, count)| *count)
        };
        assert_eq!(count("merged cells"), 1, "{named_features:?}");
        assert_eq!(count("cell formatting"), 1, "{named_features:?}");
        assert_eq!(
            count("formulas this converter cannot write as ODF"),
            1,
            "{named_features:?}"
        );

        // And what was named is genuinely absent, rather than a warning about
        // something that survived after all.
        let written = export_ods(&wb).expect("writes");
        let mut zip = zip::ZipArchive::new(Cursor::new(&written[..])).expect("a zip");
        let mut xml = String::new();
        zip.by_name("content.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(!xml.contains("covered-table-cell"), "{xml}");
        assert!(!xml.contains("TAX_RATE"), "{xml}");
        // A workbook that loses nothing must not cry wolf either.
        let mut plain = Workbook::new(Id::from_parts(1, 1));
        plain
            .sheets
            .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "S"));
        assert!(export_loss(&plain).is_empty());
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
