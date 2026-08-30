//! Reading and writing OpenDocument Spreadsheets — deliberately narrow.
//!
//! # What this is, and what it is not
//!
//! This carries **values, formulas, sheets and the document's own metadata**,
//! and nothing else. It exists so that a LibreOffice-first shop can open a
//! `.ods`, work on it, and get a `.ods` back — which is the difference between
//! OpenCalc being usable at all in those shops and not. It is explicitly not a
//! fidelity implementation: styles, merges, charts, images, conditional
//! formatting, validation and print setup are **counted and named** on the way
//! through, never dropped quietly — by the [`CompatibilityReport`]
//! [`import_ods`] returns on the way in, and by [`export_loss`] on the way out,
//! which a host asks *before* it overwrites somebody's file.
//!
//! # Why the metadata is carried rather than counted
//!
//! `meta.xml` was the last part of a `.ods` this dropped **silently** — not
//! because anybody decided to, but because the report is assembled while
//! reading `content.xml`, and nothing can count what was never opened
//! (`ODS-03`). It is now read into
//! [`DocumentProperties`](casual_calc_model::DocumentProperties) and written
//! back, so an author's name survives a round trip instead of appearing in a
//! list of regrets. `styles.xml` and `settings.xml` are still not read, and are
//! now named for the same reason.
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
    /// The document declares more cells than this build will materialise.
    ///
    /// Separate from [`OdsError::TooLarge`], which is about bytes: a repeat
    /// attribute makes these two quantities unrelated, and conflating them
    /// would report a 574-byte file as too large.
    TooManyCells { cells: usize, limit: usize },
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
            Self::TooManyCells { cells, limit } => {
                write!(
                    f,
                    "the document declares {cells} cells, over the {limit}-cell limit \
                     (a repeat attribute can declare millions from a few hundred bytes)"
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

/// Populated cells this reader will materialise, across every sheet.
///
/// **`MAX_REPEAT` bounds each repeat attribute and nothing bounded their
/// product.** One `<table:table-cell>` inside one `<table:table-row>` can carry
/// `number-columns-repeated` and `number-rows-repeated` together, so a single
/// element materialises 4096 x 4096 = **16.7 million cells**, measured at
/// 2.0 GB of resident memory from a 574-byte file. Ten such rows fit in 2 KB.
///
/// The ODF reader is reachable from a WOPI host's upload, so that is a denial
/// of service behind a friendly file extension. Found by the fuzz target added
/// with `ODS-02`, which is the first thing that ever fed this reader an input
/// nobody wrote by hand.
///
/// The value is **not invented here**: it is the `max_populated_cells` the
/// OOXML reader has enforced since `SEC-011`. Two readers admitting different
/// amounts of the same workbook is its own defect, so this is deliberately the
/// same number rather than a fresh judgement.
pub const MAX_POPULATED_CELLS: usize = 8_000_000;

/// Read a `.ods` into a workbook, with a report of everything not carried.
///
/// # Errors
///
/// If the bytes are not a readable package, hold no `content.xml`, or that
/// document is not well-formed XML.
pub fn import_ods(bytes: &[u8]) -> Result<(Workbook, CompatibilityReport), OdsError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| OdsError::NotAPackage(e.to_string()))?;
    let Some(xml) = read_part(&mut zip, "content.xml")? else {
        return Err(OdsError::NoContent);
    };
    let (mut workbook, mut report) = read_content(&xml)?;
    read_document_meta(&mut zip, &mut workbook.properties, &mut report);
    report_unread_parts(&mut zip, &mut report);
    // Everything interned so far came out of this document (`FID-36`).
    // OpenDocument writes its text inside the cells, so an unreferenced entry
    // cannot arrive this way — but the watermark is about provenance, not about
    // how many entries happen to be orphaned, and a reader that left it at zero
    // would be claiming this file's strings as the session's.
    workbook.strings.preserve_all();
    Ok((workbook, report))
}

/// Decompress one part of the package as text, or `None` if it has no such
/// part.
///
/// **Bounded twice, and the second bound is the one that holds.** A zip entry
/// declares its own uncompressed size, so checking it refuses an obvious bomb
/// without inflating a byte — but the declaration is written by the archive and
/// a hostile file simply declares something small. The read is therefore capped
/// as well, which is what actually stops the decompression. Both use
/// [`MAX_CONTENT_BYTES`]: a bomb in `meta.xml` is the same attack as one in
/// `content.xml`, and a limit that only guards the part somebody thought of
/// first is not a limit.
fn read_part(
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Option<String>, OdsError> {
    read_part_bounded(zip, name, MAX_CONTENT_BYTES)
}

/// [`read_part`] at an arbitrary limit, so the bound itself can be tested
/// without allocating a quarter of a gigabyte to do it.
fn read_part_bounded(
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
    limit: u64,
) -> Result<Option<String>, OdsError> {
    let Ok(mut entry) = zip.by_name(name) else {
        return Ok(None);
    };
    if entry.size() > limit {
        return Err(OdsError::TooLarge {
            bytes: entry.size(),
            limit,
        });
    }
    let mut xml = String::new();
    // One byte over, so that reaching the cap is distinguishable from a part
    // that is exactly the largest allowed.
    Read::by_ref(&mut entry)
        .take(limit + 1)
        .read_to_string(&mut xml)
        .map_err(|e| OdsError::Malformed(e.to_string()))?;
    if xml.len() as u64 > limit {
        return Err(OdsError::TooLarge {
            bytes: xml.len() as u64,
            limit,
        });
    }
    Ok(Some(xml))
}

/// Read `meta.xml` into the workbook's document properties.
///
/// **The one loss in this crate that used to be silent** (`ODS-03`). Every
/// other thing a `.ods` carries and this does not — styles, merges, charts —
/// is met while reading `content.xml` and named there. Document metadata is not
/// in `content.xml` at all, so nothing ever met it: the author, the title and
/// the timestamps went missing on a round trip with an empty report to say so,
/// because a reader that never opens a part cannot count what is in it.
///
/// Failure here is **not** fatal. A `.ods` whose `meta.xml` is corrupt, or
/// absurdly large, is still a perfectly good spreadsheet, and refusing to open
/// it over its author's name would be a worse answer than the one this gives:
/// open it, and say in the report that the metadata did not come through.
fn read_document_meta(
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    properties: &mut casual_calc_model::DocumentProperties,
    report: &mut CompatibilityReport,
) {
    let unreadable = |report: &mut CompatibilityReport| {
        report.record(
            "document metadata (unreadable)",
            ModelOutcome::Omitted,
            RetentionOutcome::NotRetained,
        );
    };
    let xml = match read_part(zip, "meta.xml") {
        Ok(Some(xml)) => xml,
        // No `meta.xml`: the document said nothing about itself, so there is
        // nothing to carry and nothing to report. A report that names an
        // absence is a report that names something on every file.
        Ok(None) => return,
        Err(_) => return unreadable(report),
    };
    let Ok((read, unmodelled)) = parse_meta(&xml) else {
        return unreadable(report);
    };
    *properties = read;
    for name in unmodelled {
        report.record(
            &format!("document metadata: {name}"),
            ModelOutcome::Omitted,
            RetentionOutcome::NotRetained,
        );
    }
}

/// Name the parts of the package this reader does not open at all.
///
/// `styles.xml` holds the named styles and page layouts, `settings.xml` the
/// view state — the frozen panes, the zoom, which sheet was active. Neither is
/// modelled and neither is written back, and until this was here neither was
/// *counted*, for the same reason the metadata was not: an unopened part
/// contributes nothing to a report assembled while reading a different one.
///
/// Recorded from the package's own directory rather than by parsing them, which
/// is the point — knowing the file had page styles costs no decompression at
/// all.
fn report_unread_parts(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, report: &mut CompatibilityReport) {
    for (part, feature) in [
        ("styles.xml", "document styles (styles.xml)"),
        ("settings.xml", "view settings (settings.xml)"),
    ] {
        if zip.index_for_name(part).is_some() {
            report.record(
                feature,
                ModelOutcome::Omitted,
                RetentionOutcome::NotRetained,
            );
        }
    }
}

/// Parse `meta.xml` into the properties this model holds, plus the local names
/// of everything in `<office:meta>` that it does not.
///
/// Matched on **local** names, as the rest of this crate is: the prefixes
/// (`dc:`, `meta:`) are bound per document and a file is free to call the
/// Dublin Core namespace anything it likes.
#[allow(clippy::type_complexity)]
fn parse_meta(
    xml: &str,
) -> Result<
    (
        casual_calc_model::DocumentProperties,
        std::collections::BTreeSet<String>,
    ),
    OdsError,
> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut properties = casual_calc_model::DocumentProperties::default();
    let mut unmodelled: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut in_meta = false;
    // The element whose text is being collected. `Some` suppresses the start of
    // any element nested inside it, so a field with markup in it contributes
    // its text rather than a second, bogus field name.
    let mut field: Option<String> = None;
    let mut text = String::new();

    loop {
        match reader
            .read_event()
            .map_err(|e| OdsError::Malformed(e.to_string()))?
        {
            Event::Eof => break,
            Event::Start(e) => {
                let name = local_name(e.name().as_ref());
                if name == "meta" {
                    in_meta = true;
                } else if in_meta && field.is_none() {
                    field = Some(name);
                    text.clear();
                }
            }
            // A self-closing element has no text and no `End`, so it can only
            // ever be one this does not model — `<meta:template .../>`,
            // `<meta:document-statistic .../>`. Named, not skipped.
            //
            // Filed through the same function as a closed element rather than
            // against a second copy of the list of modelled names, which is a
            // list that would drift: adding a field in one place and forgetting
            // the other makes an element both stored *and* reported as lost.
            Event::Empty(e) => {
                let name = local_name(e.name().as_ref());
                if in_meta && field.is_none() {
                    store_meta(&mut properties, &mut unmodelled, &name, "");
                }
            }
            Event::Text(t) if field.is_some() => {
                let raw = t.decode().map_err(|e| OdsError::Malformed(e.to_string()))?;
                text.push_str(&unescaped(&raw));
            }
            // As in `content.xml`: an entity arrives as its own event, and
            // dropping it takes the character with it — an author called
            // "Ada & Co" would come back as "Ada  Co".
            Event::GeneralRef(r) if field.is_some() => {
                let name = r.decode().map_err(|e| OdsError::Malformed(e.to_string()))?;
                match r.resolve_char_ref() {
                    Ok(Some(c)) => text.push(c),
                    _ => text.push_str(
                        quick_xml::escape::resolve_predefined_entity(&name)
                            .unwrap_or(&format!("&{name};")),
                    ),
                }
            }
            Event::End(e) => {
                let name = local_name(e.name().as_ref());
                if name == "meta" {
                    in_meta = false;
                } else if field.as_deref() == Some(name.as_str()) {
                    store_meta(&mut properties, &mut unmodelled, &name, &text);
                    field = None;
                }
            }
            _ => {}
        }
    }

    // **Truncation is a failure, not a short document.** A `meta.xml` cut off
    // inside `<office:meta>` yields whatever was read before the cut and loses
    // the rest — and an XML reader does not object, because "no more input" and
    // "the end" look identical to it. Reported as unreadable, which is the
    // difference between a host being told the metadata is incomplete and a
    // host being shown half of it as though it were all.
    if in_meta {
        return Err(OdsError::Malformed(
            "meta.xml ends inside <office:meta>".to_owned(),
        ));
    }

    Ok((properties, unmodelled))
}

/// File one `<office:meta>` child into the model, or note that it has no home.
///
/// **The one list of what this models.** Called for a self-closing element too,
/// with empty text, so that "which names are modelled" is written down once —
/// two copies drift, and the drift shows up as an element both stored and
/// reported as lost.
///
/// **`dc:creator` is the trap.** In ODF it names whoever saved the document
/// last, and the original author is `meta:initial-creator`; OOXML uses the same
/// element name for the opposite one. Read it as the author and every file's
/// author silently becomes whoever last opened it.
fn store_meta(
    properties: &mut casual_calc_model::DocumentProperties,
    unmodelled: &mut std::collections::BTreeSet<String>,
    name: &str,
    text: &str,
) {
    match name {
        "generator" => properties.generator = text.to_owned(),
        "title" => properties.title = text.to_owned(),
        "description" => properties.description = text.to_owned(),
        "subject" => properties.subject = text.to_owned(),
        // One element per keyword, kept as one entry per keyword — a join here
        // would make a keyword containing the separator indistinguishable from
        // two keywords. An empty one is not a keyword.
        "keyword" if !text.is_empty() => properties.keywords.push(text.to_owned()),
        "keyword" => {}
        "initial-creator" => properties.creator = text.to_owned(),
        "creator" => properties.last_modified_by = text.to_owned(),
        "creation-date" => properties.created = text.to_owned(),
        "date" => properties.modified = text.to_owned(),
        "language" => properties.language = text.to_owned(),
        // `meta:editing-cycles`, `meta:editing-duration`, `meta:printed-by`,
        // `meta:user-defined` and the rest. Named individually rather than
        // lumped together, because "this file carries custom user-defined
        // properties" is something an administrator can act on and "some
        // metadata was dropped" is not.
        other => {
            unmodelled.insert(other.to_owned());
        }
    }
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
    // Materialised cells so far, bounded by `MAX_POPULATED_CELLS`.
    let mut cells: usize = 0;

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
                                &mut cells,
                            )?);
                        } else {
                            pending = Some(cell);
                        }
                    }
                    // **Runs of spaces are elements, not text.** ODF collapses
                    // whitespace in `<text:p>`, so LibreOffice writes a leading,
                    // trailing or repeated space as `<text:s text:c="N"/>` — and
                    // this reader walked straight past it, so `"  padded  "`
                    // came back as `"padded"` and a space-only cell came back
                    // empty. A silent change to ordinary documents, on the most
                    // ordinary content there is (`ODS-04`).
                    //
                    // `text:c` defaults to 1 and counts the spaces *this*
                    // element stands for.
                    "s" if in_text => {
                        if let Some(p) = pending.as_mut() {
                            let n: usize = attr(e, "text:c")
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(1)
                                // A repeat count is attacker-controlled like any
                                // other; bounded for the same reason.
                                .min(MAX_REPEAT as usize);
                            p.text.push_str(&" ".repeat(n));
                        }
                    }
                    // The sibling ODF uses for a tab stop, for the same reason.
                    "tab" if in_text => {
                        if let Some(p) = pending.as_mut() {
                            p.text.push('\t');
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
                        let width = place(
                            &mut workbook,
                            &mut report,
                            row,
                            col,
                            row_repeat,
                            &p,
                            &mut cells,
                        )?;
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
    cells: &mut usize,
) -> Result<u32, OdsError> {
    if workbook.sheets.is_empty() {
        return Ok(p.repeat.max(1));
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
        return Ok(p.repeat.max(1));
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
        return Ok(p.repeat.max(1));
    };
    // Clamped **here**, where cells are actually stored, and nowhere else.
    // Bounded on the **product**, not on each factor. Clamping the two
    // separately still admits 4096 x 4096 from one element; see
    // `MAX_POPULATED_CELLS`. The cursor is unaffected — a document may legally
    // *span* a huge range, it may just not materialise one.
    let rows = row_repeat.clamp(1, MAX_REPEAT);
    let cols = p.repeat.clamp(1, MAX_REPEAT);
    if *cells + (rows as usize).saturating_mul(cols as usize) > MAX_POPULATED_CELLS {
        return Err(OdsError::TooManyCells {
            cells: *cells + (rows as usize).saturating_mul(cols as usize),
            limit: MAX_POPULATED_CELLS,
        });
    }
    *cells += (rows as usize).saturating_mul(cols as usize);
    for dr in 0..rows {
        for dc in 0..cols {
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
    Ok(p.repeat.max(1))
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
        // Written only when the document has something to say about itself, so
        // a workbook that never carried metadata still produces the minimal
        // package it used to — and the manifest is built to match, because a
        // manifest naming a part the package does not hold is as invalid as a
        // part the manifest does not name.
        let meta = (!workbook.properties.is_empty()).then(|| meta_xml(&workbook.properties));

        zip.add_directory("META-INF", deflated)
            .map_err(|e| OdsError::Write(e.to_string()))?;
        zip.start_file("META-INF/manifest.xml", deflated)
            .map_err(|e| OdsError::Write(e.to_string()))?;
        zip.write_all(manifest(meta.is_some()).as_bytes())
            .map_err(|e| OdsError::Write(e.to_string()))?;

        if let Some(meta) = meta {
            zip.start_file("meta.xml", deflated)
                .map_err(|e| OdsError::Write(e.to_string()))?;
            zip.write_all(meta.as_bytes())
                .map_err(|e| OdsError::Write(e.to_string()))?;
        }

        zip.start_file("content.xml", deflated)
            .map_err(|e| OdsError::Write(e.to_string()))?;
        zip.write_all(content_xml(workbook).as_bytes())
            .map_err(|e| OdsError::Write(e.to_string()))?;
        zip.finish().map_err(|e| OdsError::Write(e.to_string()))?;
    }
    Ok(out)
}

/// The package manifest, naming exactly the parts that were written.
fn manifest(with_meta: bool) -> String {
    let mut out = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">"#,
        r#"<manifest:file-entry manifest:full-path="/" manifest:version="1.2" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/>"#,
        r#"<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>"#,
    ));
    if with_meta {
        out.push_str(
            r#"<manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/>"#,
        );
    }
    out.push_str("</manifest:manifest>");
    out
}

/// The document's own account of itself, as ODF's `meta.xml`.
///
/// Written from the model rather than copied from the source bytes, because the
/// source may have been a `.xlsx` — the point of a format-neutral
/// [`DocumentProperties`](casual_calc_model::DocumentProperties) is that the
/// author of a workbook survives the conversion, not only the round trip.
///
/// `meta:generator` goes back out **as the file declared it**. ODF defines it as
/// the application that last modified the document, so stamping this engine's
/// name would be the more literal reading — and it would also throw away the
/// only field that records where the file came from, with nothing to record the
/// loss. Of the two readings, one loses data and one does not.
fn meta_xml(properties: &casual_calc_model::DocumentProperties) -> String {
    let mut out = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<office:document-meta"#,
        r#" xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0""#,
        r#" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0""#,
        r#" xmlns:dc="http://purl.org/dc/elements/1.1/""#,
        r#" office:version="1.2">"#,
        r#"<office:meta>"#,
    ));
    let element = |out: &mut String, name: &str, value: &str| {
        if !value.is_empty() {
            out.push_str(&format!("<{name}>{}</{name}>", escape(value)));
        }
    };
    element(&mut out, "meta:generator", &properties.generator);
    element(&mut out, "dc:title", &properties.title);
    element(&mut out, "dc:description", &properties.description);
    element(&mut out, "dc:subject", &properties.subject);
    for keyword in &properties.keywords {
        element(&mut out, "meta:keyword", keyword);
    }
    element(&mut out, "meta:initial-creator", &properties.creator);
    element(&mut out, "meta:creation-date", &properties.created);
    // The last saver, **not** the author. See `store_meta`.
    element(&mut out, "dc:creator", &properties.last_modified_by);
    element(&mut out, "dc:date", &properties.modified);
    element(&mut out, "dc:language", &properties.language);
    out.push_str("</office:meta></office:document-meta>");
    out
}

fn content_xml(workbook: &Workbook) -> String {
    let mut out = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<office:document-content"#,
        r#" xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0""#,
        r#" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0""#,
        r#" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0""#,
        // **Declared because every formula uses it.** `table:formula` is
        // written as `of:=…`, and a prefix that is used and never bound is not
        // merely untidy — it is not well-formed namespace XML. LibreOffice
        // answers `Err:510` for every formula cell in the document, so a file
        // this engine saved opened with an error in place of each formula.
        //
        // Invisible from inside this crate, and that is the lesson: our own
        // reader matches attributes by *local* name, so it read the file back
        // perfectly and every round-trip test passed. Only a real LibreOffice
        // round trip could see it (`ODS-06`).
        r#" xmlns:of="urn:oasis:names:tc:opendocument:xmlns:of:1.2""#,
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

    /// **Padding is elements, not text** (`ODS-04`).
    ///
    /// ODF collapses whitespace inside `<text:p>`, so LibreOffice writes a
    /// leading, trailing or repeated space as `<text:s text:c="N"/>`. This
    /// reader walked past those elements, so `"  padded  "` came back as
    /// `"padded"` and a cell holding a single space came back empty — a silent
    /// change to the most ordinary content there is.
    ///
    /// The fixture is **written by LibreOffice**, not by the writer below, and
    /// that is the whole point: nothing this repository writes emits
    /// `<text:s>`, so a round trip through our own writer could never have
    /// exercised this. It is the same circularity that hid `ODS-06`.
    #[test]
    fn a_libreoffice_documents_padding_is_not_silently_trimmed() {
        let bytes = include_bytes!("../tests/fixtures/libreoffice-spaces.ods");
        let (wb, _) = import_ods(bytes).expect("read the LibreOffice document");
        let text = |r: u32| {
            wb.sheets[0]
                .cells
                .get(CellRef::new(r, 0))
                .map(|c| match &c.value {
                    CellValue::SharedString(id) | CellValue::InlineString(id) => {
                        wb.strings.get(*id).unwrap_or_default().to_owned()
                    }
                    other => format!("{other:?}"),
                })
                .unwrap_or_default()
        };

        assert_eq!(text(1), "  padded  ", "the padding was trimmed away");
        assert_eq!(text(2), " ", "a cell holding one space came back empty");
        assert_eq!(text(3), "plain", "an ordinary cell was disturbed");
    }

    /// **A few hundred bytes must not become gigabytes.**
    ///
    /// `MAX_REPEAT` bounded each repeat attribute and nothing bounded their
    /// product: one `<table:table-cell>` carrying both
    /// `number-columns-repeated` and `number-rows-repeated` materialised
    /// 4096 x 4096 = 16.7 million cells — measured at 2.0 GB of resident
    /// memory from a 574-byte file, and ten such rows fit in 2 KB.
    ///
    /// This reader is reachable from a WOPI host's upload, so that was a denial
    /// of service behind a friendly extension. Found by the fuzz target, which
    /// was the first thing ever to feed this reader an input nobody wrote.
    ///
    /// The document is **refused**, not truncated: silently keeping the first
    /// eight million cells of a file that claims sixteen would be the silent
    /// loss this project refuses.
    #[test]
    fn a_repeat_product_cannot_amplify_a_tiny_file_into_gigabytes() {
        let xml = format!(
            r#"<?xml version="1.0"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet><table:table table:name="S"><table:table-row table:number-rows-repeated="{r}"><table:table-cell table:number-columns-repeated="{c}" office:value-type="float" office:value="1"/></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"#,
            r = MAX_REPEAT,
            c = MAX_REPEAT,
        );
        let package = package_of(&xml);
        assert!(
            package.len() < 2_000,
            "the reproducer must be tiny, or it proves nothing: {} bytes",
            package.len()
        );

        match import_ods(&package) {
            Err(OdsError::TooManyCells { cells, limit }) => {
                assert!(cells > limit, "refused without exceeding the limit");
                assert_eq!(limit, MAX_POPULATED_CELLS);
            }
            Err(other) => panic!("refused for the wrong reason: {other}"),
            Ok(_) => panic!(
                "{} bytes materialised {} cells unchecked",
                package.len(),
                MAX_REPEAT as u64 * MAX_REPEAT as u64
            ),
        }
    }

    /// **An ordinary wide fill still opens.** The bound must refuse the attack
    /// and not the documents LibreOffice actually writes, which use repeat runs
    /// on nearly every row.
    #[test]
    fn an_ordinary_repeat_run_is_still_admitted() {
        let xml = r#"<?xml version="1.0"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet><table:table table:name="S"><table:table-row table:number-rows-repeated="20"><table:table-cell table:number-columns-repeated="50" office:value-type="float" office:value="7"/></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"#;
        let wb = import_ods(&package_of(xml)).expect("an ordinary wide fill was refused");
        assert_eq!(
            wb.0.sheets[0]
                .cells
                .get(CellRef::new(19, 49))
                .map(|c| c.value.clone()),
            Some(CellValue::Number(7.0)),
            "the fill did not reach its far corner"
        );
    }

    /// **Every namespace prefix the writer uses is declared.**
    ///
    /// `table:formula` is written as `of:=…`, and `of:` was never bound. That is
    /// not untidy XML, it is not well-formed namespace XML — and LibreOffice
    /// answers `Err:510` for every formula cell in a document this engine saved.
    /// Measured, with a real `soffice`: `Err:510` before the declaration, `10`
    /// after.
    ///
    /// **Why this was invisible from inside the crate**, which is the part worth
    /// keeping: `import_ods` matches attributes by *local* name, so it read the
    /// broken file back perfectly and every round-trip test passed. A round trip
    /// through your own reader cannot see a namespace defect, by construction.
    ///
    /// So this asserts the property mechanically rather than re-testing the one
    /// prefix: any prefix that appears on an element or an attribute must appear
    /// in an `xmlns:` declaration. A future prefix is covered without an edit.
    #[test]
    fn every_prefix_the_writer_uses_is_declared() {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(9, 1)), "S");
        sheet
            .cells
            .set(CellRef::new(0, 0), Cell::value(CellValue::Number(2.0)));
        let handle = wb.store_formula(casual_calc_formula::parse("SUM(A1:A1)").unwrap());
        let mut total = Cell::value(CellValue::Number(2.0));
        total.formula = Some(handle);
        sheet.cells.set(CellRef::new(1, 0), total);
        wb.sheets.push(sheet);

        let bytes = export_ods(&wb).expect("write");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
        for part in ["content.xml", "meta.xml"] {
            let Ok(mut entry) = zip.by_name(part) else {
                continue;
            };
            let mut xml = String::new();
            std::io::Read::read_to_string(&mut entry, &mut xml).expect("read");

            // Only a real `xmlns:` **attribute** declares anything. Matching
            // the bare text also hits the URNs, which themselves contain
            // `…:xmlns:office:1.0` — the first version of this did, and its
            // "declared" set held entries like `office:1.0" xmlns:table`.
            // Harmless here by luck, and exactly the loose parsing that hides
            // the next defect.
            let declared: std::collections::BTreeSet<&str> = xml
                .match_indices(" xmlns:")
                .filter_map(|(at, _)| {
                    let name = xml[at + 7..].split('=').next()?;
                    name.chars()
                        .all(|c| c.is_ascii_alphanumeric())
                        .then_some(name)
                })
                .collect();
            // Prefixes actually in use: `<pfx:name` and ` pfx:name=`.
            let mut used: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for (at, _) in xml.match_indices(':') {
                let before = &xml[..at];
                let Some(start) = before.rfind(['<', ' ', '/']) else {
                    continue;
                };
                let prefix = &before[start + 1..];
                if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_lowercase()) {
                    continue;
                }
                let after = &xml[at + 1..];
                // A real qualified name, not a URN or a formula body.
                if after.starts_with(|c: char| c.is_ascii_alphabetic()) && prefix != "xmlns" {
                    let name: String = after
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                        .collect();
                    let rest = &after[name.len()..];
                    if rest.starts_with('=') || rest.starts_with(' ') || rest.starts_with('>') {
                        used.insert(prefix.to_owned());
                    }
                }
            }
            for prefix in &used {
                assert!(
                    declared.contains(prefix.as_str()),
                    "{part} uses the prefix `{prefix}:` and never declares it, so the \
                     document is not well-formed and LibreOffice will not read it; \
                     declared: {declared:?}"
                );
            }
            assert!(
                used.contains("office"),
                "{part}: the scan found no prefixes at all"
            );

            // **A prefix can also live inside an attribute value, and that is the
            // one that bit.** `table:formula="of:=SUM(…)"` binds `of:` in the
            // *value*, which the scan above cannot see: it requires a name after
            // the colon and finds `=`. ODF requires that prefix declared all the
            // same, and the first version of this test passed with `xmlns:of`
            // deleted — proving nothing about the defect it was written for.
            for (attr, prefix) in [("table:formula=\"", "of:")] {
                if let Some(at) = xml.find(attr) {
                    let value = &xml[at + attr.len()..];
                    if value.starts_with(prefix) {
                        assert!(
                            declared.contains(prefix.trim_end_matches(':')),
                            "{part} writes {attr}{prefix}…\" and never declares `{prefix}`, so \
                         LibreOffice answers Err:510 for every formula cell; declared: {declared:?}"
                        );
                    }
                }
            }
        }
    }

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

    /// **Document metadata: the one loss this crate used to make silently**
    /// (`ODS-03`).
    ///
    /// Everything else a `.ods` carries and this does not is met while reading
    /// `content.xml`, and named there. `meta.xml` was never opened, so the
    /// author, the title and the timestamps went missing with an **empty
    /// report** — nothing can count what was never read.
    mod document_metadata {
        use super::*;

        /// Exactly what the fixture's own `meta.xml` declares.
        ///
        /// Asserted against the string LibreOffice wrote rather than against
        /// anything this crate produces: a round trip through our own writer
        /// and reader agrees with itself whether or not either is right.
        const REAL_GENERATOR: &str = "LibreOffice/26.2.4.2$MacOSX_AARCH64 LibreOffice_project/0229ac93fcf0d7cbc6376066c6f35021cef002dc";

        fn part_of(package: &[u8], name: &str) -> Option<String> {
            let mut zip = zip::ZipArchive::new(Cursor::new(package)).expect("a zip");
            let mut xml = String::new();
            zip.by_name(name).ok()?.read_to_string(&mut xml).ok()?;
            Some(xml)
        }

        fn features(report: &CompatibilityReport) -> Vec<String> {
            report
                .entries()
                .into_iter()
                .map(|e| e.feature.to_string())
                .collect()
        }

        /// **A real document's metadata reaches the model.**
        #[test]
        fn a_real_documents_metadata_is_read() {
            let (wb, _) = import_ods(REAL).expect("LibreOffice's own output must open");
            assert_eq!(
                wb.properties.generator, REAL_GENERATOR,
                "meta.xml was not read: the model knows nothing about where this file came from"
            );
        }

        /// **…and survives a round trip, in the file's bytes.**
        ///
        /// Through `export_ods` and back out of the zip, because the question is
        /// what a `.ods` this engine hands back actually contains — a model that
        /// holds the generator and a writer that drops it lose it just as
        /// completely.
        #[test]
        fn a_real_documents_metadata_survives_a_round_trip() {
            let (wb, _) = import_ods(REAL).expect("opens");
            let written = export_ods(&wb).expect("writes");

            let meta = part_of(&written, "meta.xml")
                .expect("the written package has no meta.xml: the metadata was dropped on save");
            assert!(
                meta.contains(REAL_GENERATOR),
                "meta.xml came back without the generator the file declared: {meta}"
            );

            // And it reads back as itself, so the round trip closes.
            let (again, _) = import_ods(&written).expect("re-opens");
            assert_eq!(again.properties, wb.properties);
        }

        /// **The manifest names what the package holds.**
        ///
        /// A manifest that names a part the zip does not carry, or omits one it
        /// does, is an invalid package — and one that still opens in
        /// LibreOffice, which is how it would go unnoticed.
        #[test]
        fn the_manifest_agrees_with_the_parts_written() {
            let (wb, _) = import_ods(REAL).expect("opens");
            let with = export_ods(&wb).expect("writes");
            assert!(
                part_of(&with, "META-INF/manifest.xml")
                    .expect("a manifest")
                    .contains(r#"manifest:full-path="meta.xml""#),
                "meta.xml was written and the manifest does not name it"
            );

            // A workbook with nothing to say about itself writes neither.
            let mut plain = Workbook::new(Id::from_parts(1, 1));
            plain
                .sheets
                .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "S"));
            let without = export_ods(&plain).expect("writes");
            assert!(part_of(&without, "meta.xml").is_none());
            assert!(
                !part_of(&without, "META-INF/manifest.xml")
                    .expect("a manifest")
                    .contains("meta.xml"),
                "the manifest names a meta.xml the package does not hold"
            );
        }

        /// **`mimetype` is still first and still uncompressed**, with `meta.xml`
        /// in the package.
        ///
        /// The ODF rule the writer already kept, re-checked on the path that now
        /// writes an extra part before `content.xml`: `mimetype` second is a
        /// file that opens in LibreOffice and is unidentifiable everywhere else.
        #[test]
        fn a_package_with_metadata_still_leads_with_an_uncompressed_mimetype() {
            let (wb, _) = import_ods(REAL).expect("opens");
            let written = export_ods(&wb).expect("writes");

            let mut zip = zip::ZipArchive::new(Cursor::new(&written[..])).expect("a zip");
            let first = zip.by_index(0).expect("at least one entry");
            assert_eq!(first.name(), "mimetype", "mimetype is not the first entry");
            assert_eq!(first.compression(), zip::CompressionMethod::Stored);
        }

        /// **What `meta.xml` held and this does not model is named.**
        ///
        /// The fixture's `meta.xml` carries a `<meta:document-statistic>` that
        /// nothing here models. Carried or counted — those are the two allowed
        /// outcomes, and before this it was neither.
        #[test]
        fn what_meta_xml_held_and_this_does_not_model_is_named() {
            let (_, report) = import_ods(REAL).expect("opens");
            let named = features(&report);
            assert!(
                named.contains(&"document metadata: document-statistic".to_owned()),
                "a part of meta.xml was dropped without a word: {named:?}"
            );
        }

        /// **The parts this reader never opens are named too.**
        ///
        /// `styles.xml` and `settings.xml` are in the fixture and are not read.
        /// Same defect as the metadata, same shape: a report assembled while
        /// reading `content.xml` cannot mention a file nobody opened.
        #[test]
        fn the_parts_this_reader_never_opens_are_named() {
            let (_, report) = import_ods(REAL).expect("opens");
            let named = features(&report);
            assert!(
                named.contains(&"document styles (styles.xml)".to_owned()),
                "{named:?}"
            );
            assert!(
                named.contains(&"view settings (settings.xml)".to_owned()),
                "{named:?}"
            );

            // …and not on a package that holds neither, or the report warns on
            // every file and is read on none.
            let (_, bare) = import_ods(&package_of(&format!(
                "{}{}",
                r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"><office:body><office:spreadsheet><table:table table:name="S"/>"#,
                "</office:spreadsheet></office:body></office:document-content>"
            )))
            .expect("opens");
            assert!(features(&bare).is_empty(), "{:?}", features(&bare));
        }

        /// A `.ods` holding a `content.xml` and the given `meta.xml`.
        fn package_with_meta(meta: &str) -> Vec<u8> {
            let mut out = Vec::new();
            {
                let mut zip = zip::ZipWriter::new(Cursor::new(&mut out));
                let plain = zip::write::SimpleFileOptions::default();
                zip.start_file("content.xml", plain).unwrap();
                zip.write_all(
                    concat!(
                        r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0""#,
                        r#" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0">"#,
                        r#"<office:body><office:spreadsheet><table:table table:name="S"/>"#,
                        r#"</office:spreadsheet></office:body></office:document-content>"#,
                    )
                    .as_bytes(),
                )
                .unwrap();
                zip.start_file("meta.xml", plain).unwrap();
                zip.write_all(meta.as_bytes()).unwrap();
                zip.finish().unwrap();
            }
            out
        }

        /// **The author and the last saver are not confused for each other.**
        ///
        /// `dc:creator` means opposite things in the two formats this engine
        /// reads: in ODF it is whoever saved last, and the author is
        /// `meta:initial-creator`; in OOXML it is the author. Read one as the
        /// other and every file's author silently becomes whoever last opened
        /// it — a wrong answer that looks like a right one, which is why it gets
        /// its own test rather than a line in a round trip.
        ///
        /// Synthetic, because the real fixture was written by a LibreOffice that
        /// recorded neither, and a trap this specific has to be sprung
        /// deliberately.
        #[test]
        fn the_author_and_the_last_saver_do_not_swap() {
            let package = package_with_meta(concat!(
                r#"<office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0""#,
                r#" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0""#,
                r#" xmlns:dc="http://purl.org/dc/elements/1.1/"><office:meta>"#,
                r#"<meta:initial-creator>Ada Lovelace</meta:initial-creator>"#,
                r#"<dc:creator>Grace Hopper</dc:creator>"#,
                r#"<dc:title>Q3 &amp; Q4</dc:title>"#,
                r#"<meta:creation-date>2026-01-02T03:04:05</meta:creation-date>"#,
                r#"<dc:date>2026-08-17T09:10:11</dc:date>"#,
                // The second one contains the separator a joined form would
                // have used, so a `String` of keywords would come back as three.
                r#"<meta:keyword>budget</meta:keyword><meta:keyword>Q3, Q4</meta:keyword>"#,
                r#"</office:meta></office:document-meta>"#,
            ));

            let (wb, _) = import_ods(&package).expect("opens");
            assert_eq!(wb.properties.creator, "Ada Lovelace", "the author moved");
            assert_eq!(
                wb.properties.last_modified_by, "Grace Hopper",
                "the last saver moved"
            );
            // An entity in a name is a character, not four of them.
            assert_eq!(wb.properties.title, "Q3 & Q4");
            assert_eq!(wb.properties.created, "2026-01-02T03:04:05");
            assert_eq!(wb.properties.modified, "2026-08-17T09:10:11");
            assert_eq!(
                wb.properties.keywords,
                ["budget", "Q3, Q4"],
                "a keyword with a comma in it was split into two"
            );

            // …and the two stay apart through the writer, which is the half a
            // reader test cannot see: a writer that swapped them back would
            // make the reader's correctness invisible.
            let written = export_ods(&wb).expect("writes");
            let meta = part_of(&written, "meta.xml").expect("a meta.xml");
            assert!(
                meta.contains("<meta:initial-creator>Ada Lovelace</meta:initial-creator>"),
                "{meta}"
            );
            assert!(
                meta.contains("<dc:creator>Grace Hopper</dc:creator>"),
                "{meta}"
            );
            assert!(meta.contains("<dc:title>Q3 &amp; Q4</dc:title>"), "{meta}");
            assert!(
                meta.contains("<meta:keyword>Q3, Q4</meta:keyword>"),
                "the keyword was rewritten on the way out: {meta}"
            );

            let (again, _) = import_ods(&written).expect("re-opens");
            assert_eq!(again.properties, wb.properties);
        }

        /// **A broken `meta.xml` costs the metadata, not the document.**
        ///
        /// A spreadsheet whose `content.xml` is perfectly good is still a
        /// spreadsheet. Refusing to open it over its author's name would be a
        /// worse answer than opening it and saying the metadata did not come
        /// through — but saying nothing would be the worst of the three.
        #[test]
        fn a_broken_meta_xml_is_reported_rather_than_fatal() {
            let (wb, report) =
                import_ods(&package_with_meta("<office:meta><unclosed>")).expect("still opens");
            assert_eq!(
                wb.sheets.len(),
                1,
                "the document was lost with its metadata"
            );
            assert!(
                features(&report).contains(&"document metadata (unreadable)".to_owned()),
                "{:?}",
                features(&report)
            );
        }

        /// **A part over the limit is refused before it is decompressed.**
        ///
        /// The bound `content.xml` already had, applied to every part this now
        /// opens: a zip bomb in `meta.xml` is the same attack with a different
        /// file name, and a limit that only guards the part somebody thought of
        /// first is not a limit.
        ///
        /// Exercised at a small limit rather than the real 256 MB, which no test
        /// should have to allocate; it is the same code path.
        #[test]
        fn a_part_over_the_limit_is_refused() {
            let package = package_with_meta(&format!(
                "<office:meta>{}</office:meta>",
                " ".repeat(64 * 1024)
            ));
            let mut zip = zip::ZipArchive::new(Cursor::new(&package[..])).expect("a zip");

            assert!(
                matches!(
                    read_part_bounded(&mut zip, "meta.xml", 1024),
                    Err(OdsError::TooLarge { limit: 1024, .. })
                ),
                "a part over the limit was decompressed"
            );
            // The same part under a limit that admits it, so the refusal above is
            // the bound rather than a part this cannot read at all.
            assert!(
                read_part_bounded(&mut zip, "meta.xml", 1024 * 1024)
                    .unwrap()
                    .is_some()
            );
            // A part the package does not hold is absent, not an error: a `.ods`
            // without a `meta.xml` is an ordinary file.
            assert!(read_part(&mut zip, "nothing.xml").unwrap().is_none());
        }
    }
}
