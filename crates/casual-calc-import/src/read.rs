//! Streaming parsers for `sharedStrings.xml` and worksheet `sheetData`.

use std::collections::{BTreeMap, BTreeSet};

use casual_calc_model::{
    OutlinePr, PrintSetup, RetainedRef, RunFont, SortState, TableColumn, TextRun, Underline,
    VertAlign, WorkbookSettings,
};
use casual_calc_ooxml::OoxmlError;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::ImportError;
use crate::theme::{ThemePalette, indexed_color};

const MAX_XML_ELEMENTS: usize = 50_000_000;
const MAX_XML_DEPTH: usize = 256;

pub(crate) fn xml_err(err: quick_xml::Error) -> ImportError {
    ImportError::Ooxml(OoxmlError::MalformedXml(err.to_string()))
}

/// A raw worksheet cell, before mapping to the model.
#[derive(Debug, Default)]
pub struct RawCell {
    /// The A1 reference (`r` attribute).
    pub reference: String,
    /// The cell type (`t` attribute): `n`/`b`/`s`/`str`/`inlineStr`/`e`.
    pub cell_type: Option<String>,
    /// The style index (`s` attribute) into `cellXfs`.
    pub style_index: Option<u32>,
    /// The `<v>` value text.
    pub value: Option<String>,
    /// The `<is><t>` inline-string text.
    pub inline: Option<String>,
    /// The `<f>` formula text, if present.
    pub formula: Option<String>,
    /// The shared-formula group (`<f t="shared" si="N">`). The group's master
    /// carries the expression text; every follower's `<f>` is empty and must be
    /// rebuilt from the master, shifted by the cell delta.
    pub shared_index: Option<u32>,
}

pub(crate) fn read_attr(
    e: &quick_xml::events::BytesStart<'_>,
    local: &[u8],
) -> Result<Option<String>, ImportError> {
    for attr in e.attributes() {
        let attr = attr.map_err(|err| xml_err(err.into()))?;
        if attr.key.local_name().as_ref() == local {
            let value = attr.unescape_value().map_err(xml_err)?;
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}

/// Guard element count and depth while reading; returns the bounded error.
struct Bounds {
    elements: usize,
    depth: usize,
}

impl Bounds {
    fn new() -> Self {
        Self {
            elements: 0,
            depth: 0,
        }
    }
    fn open(&mut self) -> Result<(), ImportError> {
        self.depth += 1;
        if self.depth > MAX_XML_DEPTH {
            return Err(ImportError::Ooxml(OoxmlError::TooDeep {
                limit: MAX_XML_DEPTH,
            }));
        }
        self.count()
    }
    fn close(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }
    fn count(&mut self) -> Result<(), ImportError> {
        self.elements += 1;
        if self.elements > MAX_XML_ELEMENTS {
            return Err(ImportError::Ooxml(OoxmlError::TooManyElements {
                limit: MAX_XML_ELEMENTS,
            }));
        }
        Ok(())
    }
}

/// Parse `sharedStrings.xml` into the ordered list of strings. Text within each
/// `<si>` (including multiple `<r><t>` runs) is concatenated.
pub fn parse_shared_strings(xml: &[u8]) -> Result<Vec<Vec<TextRun>>, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut bounds = Bounds::new();

    let mut strings: Vec<Vec<TextRun>> = Vec::new();
    let mut current: Option<Vec<TextRun>> = None;
    let mut run_font: Option<RunFont> = None;
    let mut in_rpr = false;
    let mut in_run = false;
    let mut in_text = false;
    let mut text = String::new();

    loop {
        let event = reader.read_event_into(&mut buf).map_err(xml_err)?;
        match event {
            // `<rPr>`'s children are all childless toggles and values, so they
            // arrive as `Empty`; folding the two cases keeps one dispatch.
            Event::Start(ref e) | Event::Empty(ref e) => {
                if matches!(event, Event::Start(_)) {
                    bounds.open()?;
                } else {
                    bounds.count()?;
                }
                match e.local_name().as_ref() {
                    b"si" => {
                        current = Some(Vec::new());
                        if matches!(event, Event::Empty(_)) {
                            strings.push(Vec::new());
                            current = None;
                        }
                    }
                    b"r" => {
                        in_run = true;
                        run_font = None;
                    }
                    b"rPr" => {
                        in_rpr = true;
                        run_font = Some(RunFont::default());
                    }
                    b"t" => {
                        in_text = true;
                        text.clear();
                    }
                    _ if in_rpr => {
                        if let Some(font) = run_font.as_mut() {
                            read_rpr_child(e, font)?;
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(e) => {
                if in_text {
                    text.push_str(&e.unescape().map_err(xml_err)?);
                }
            }
            Event::End(e) => {
                bounds.close();
                match e.local_name().as_ref() {
                    b"rPr" => in_rpr = false,
                    b"t" => {
                        in_text = false;
                        // A `<t>` directly under `<si>` is the whole string; one
                        // inside `<r>` waits for `</r>` so it can carry its font.
                        if !in_run && let Some(runs) = current.as_mut() {
                            runs.push(TextRun {
                                text: std::mem::take(&mut text),
                                font: None,
                            });
                        }
                    }
                    b"r" => {
                        in_run = false;
                        if let Some(runs) = current.as_mut() {
                            let font = run_font.take().filter(|f| !f.is_empty());
                            runs.push(TextRun {
                                text: std::mem::take(&mut text),
                                font,
                            });
                        }
                    }
                    b"si" => {
                        if let Some(runs) = current.take() {
                            strings.push(runs);
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(strings)
}

/// Fold one `<rPr>` child into the run's font.
fn read_rpr_child(e: &BytesStart<'_>, font: &mut RunFont) -> Result<(), ImportError> {
    // `<b/>` means bold; `<b val="0"/>` means explicitly not bold. Treating the
    // element's presence as truth would make the second one bold.
    let on = |e: &BytesStart<'_>| -> Result<bool, ImportError> {
        Ok(read_attr(e, b"val")?.is_none_or(|v| v == "1" || v.eq_ignore_ascii_case("true")))
    };
    match e.local_name().as_ref() {
        b"b" => font.bold = on(e)?,
        b"i" => font.italic = on(e)?,
        b"strike" => font.strike = on(e)?,
        b"outline" => font.outline = on(e)?,
        b"shadow" => font.shadow = on(e)?,
        b"condense" => font.condense = on(e)?,
        b"extend" => font.extend = on(e)?,
        b"u" => font.underline = Underline::from_ooxml(&read_attr(e, b"val")?.unwrap_or_default()),
        b"vertAlign" => {
            font.vert_align = VertAlign::from_ooxml(&read_attr(e, b"val")?.unwrap_or_default())
        }
        b"sz" => {
            font.size_hp = read_attr(e, b"val")?
                .and_then(|v| v.parse::<f64>().ok())
                .map(|pt| (pt * 2.0).round() as u32);
        }
        b"rFont" => font.name = read_attr(e, b"val")?,
        b"family" => font.family = read_attr(e, b"val")?.and_then(|v| v.parse().ok()),
        b"scheme" => font.scheme = read_attr(e, b"val")?,
        b"charset" => font.charset = read_attr(e, b"val")?.and_then(|v| v.parse().ok()),
        b"color" => {
            if let Some(rgb) = read_attr(e, b"rgb")? {
                font.color = Some(if rgb.len() == 8 {
                    rgb[2..].to_owned()
                } else {
                    rgb
                });
            }
        }
        _ => {}
    }
    Ok(())
}

/// A parsed cell comment: `(cell reference, author, text)`.
pub type RawComment = (String, Option<String>, String);

/// Parse an `xl/comments{n}.xml` part into `(ref, author, text)` per note.
pub fn parse_comments(xml: &[u8]) -> Result<Vec<RawComment>, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut bounds = Bounds::new();

    let mut authors: Vec<String> = Vec::new();
    let mut out: Vec<RawComment> = Vec::new();
    let mut in_author = false;
    let mut cur_author = String::new();
    let mut cur_ref = String::new();
    let mut cur_aid: usize = 0;
    let mut in_comment = false;
    let mut cur_text = String::new();
    let mut in_t = false;

    loop {
        match reader.read_event_into(&mut buf).map_err(xml_err)? {
            Event::Start(e) => {
                bounds.open()?;
                match e.local_name().as_ref() {
                    b"author" => {
                        in_author = true;
                        cur_author.clear();
                    }
                    b"comment" => {
                        in_comment = true;
                        cur_ref = read_attr(&e, b"ref")?.unwrap_or_default();
                        cur_aid = read_attr(&e, b"authorId")?
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                        cur_text.clear();
                    }
                    b"t" if in_comment => in_t = true,
                    _ => {}
                }
            }
            Event::Text(e) => {
                if in_author {
                    cur_author.push_str(&e.unescape().map_err(xml_err)?);
                } else if in_t {
                    cur_text.push_str(&e.unescape().map_err(xml_err)?);
                }
            }
            Event::End(e) => {
                bounds.close();
                match e.local_name().as_ref() {
                    b"author" => {
                        in_author = false;
                        authors.push(cur_author.clone());
                    }
                    b"t" => in_t = false,
                    b"comment" => {
                        in_comment = false;
                        // An empty author is the schema's way of saying
                        // "anonymous"; carrying it as `Some("")` would make an
                        // unsigned note look signed by nobody in particular.
                        let author = authors.get(cur_aid).filter(|a| !a.is_empty()).cloned();
                        out.push((cur_ref.clone(), author, cur_text.clone()));
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

/// One `<threadedComment>` element, before threads are assembled.
#[derive(Debug, Clone)]
pub struct RawThreadedComment {
    /// The cell it is anchored to, in A1 form.
    pub reference: String,
    /// This comment's own GUID.
    pub id: String,
    /// The GUID of the comment it replies to, if it is a reply.
    pub parent_id: Option<String>,
    /// The GUID of the person who wrote it, resolved through the persons part.
    pub person_id: Option<String>,
    /// The `dT` timestamp, ISO 8601.
    pub date: Option<String>,
    /// Whether the thread is marked resolved (only meaningful on a root).
    pub done: bool,
    /// The comment body.
    pub text: String,
}

/// Parse an `xl/persons/person{n}.xml` part into `(id, displayName)` pairs.
pub fn parse_persons(xml: &[u8]) -> Result<Vec<(String, String)>, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut bounds = Bounds::new();
    let mut out = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf).map_err(xml_err)?;
        match event {
            // `<person>` is childless, so it arrives as `Empty` from every
            // writer that self-closes it — handling only `Start` would read
            // the file as having no people at all.
            Event::Start(ref e) | Event::Empty(ref e) => {
                if matches!(event, Event::Start(_)) {
                    bounds.open()?;
                }
                if e.local_name().as_ref() == b"person"
                    && let Some(id) = read_attr(e, b"id")?
                {
                    let name = read_attr(e, b"displayName")?.unwrap_or_default();
                    out.push((id, name));
                }
            }
            Event::End(_) => bounds.close(),
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

/// Parse an `xl/threadedComments/threadedComment{n}.xml` part. The schema is
/// flat — replies are siblings of their root, linked by `parentId` — so this
/// returns the elements in document order and leaves threading to the caller.
pub fn parse_threaded_comments(xml: &[u8]) -> Result<Vec<RawThreadedComment>, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut bounds = Bounds::new();
    let mut out: Vec<RawThreadedComment> = Vec::new();
    let mut current: Option<RawThreadedComment> = None;
    let mut in_text = false;

    loop {
        match reader.read_event_into(&mut buf).map_err(xml_err)? {
            Event::Start(e) => {
                bounds.open()?;
                match e.local_name().as_ref() {
                    b"threadedComment" => {
                        current = Some(RawThreadedComment {
                            reference: read_attr(&e, b"ref")?.unwrap_or_default(),
                            id: read_attr(&e, b"id")?.unwrap_or_default(),
                            parent_id: read_attr(&e, b"parentId")?,
                            person_id: read_attr(&e, b"personId")?,
                            date: read_attr(&e, b"dT")?,
                            done: read_attr(&e, b"done")?
                                .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")),
                            text: String::new(),
                        });
                    }
                    b"text" if current.is_some() => in_text = true,
                    _ => {}
                }
            }
            Event::Text(e) => {
                if in_text && let Some(c) = current.as_mut() {
                    c.text.push_str(&e.unescape().map_err(xml_err)?);
                }
            }
            Event::End(e) => {
                bounds.close();
                match e.local_name().as_ref() {
                    b"text" => in_text = false,
                    b"threadedComment" => {
                        if let Some(c) = current.take() {
                            out.push(c);
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

/// One `<hyperlink>` before its `r:id` is resolved against the sheet rels.
#[derive(Debug, Clone)]
pub struct RawHyperlink {
    /// The `ref` range the link covers.
    pub reference: String,
    /// The relationship id naming an external target, when there is one.
    pub rel_id: Option<String>,
    /// An anchor inside the target, or inside this workbook.
    pub location: Option<String>,
    /// Hover text.
    pub tooltip: Option<String>,
    /// The `display` attribute.
    pub display: Option<String>,
}

/// A parsed worksheet: its cells, merged ranges, frozen panes, and axis sizing.
#[derive(Debug, Default)]
pub struct Worksheet {
    /// The raw cells.
    pub cells: Vec<RawCell>,
    /// Merged-range references (`A1:B2`).
    pub merges: Vec<String>,
    /// Hyperlinks, still holding their relationship ids.
    pub hyperlinks: Vec<RawHyperlink>,
    /// Frozen panes as `(frozen_rows, frozen_cols)`, if any.
    pub frozen: Option<(u32, u32)>,
    /// Default column width (twips), from `sheetFormatPr/@defaultColWidth`.
    pub col_default: Option<i64>,
    /// Default row height (twips), from `sheetFormatPr/@defaultRowHeight`.
    pub row_default: Option<i64>,
    /// Explicit column widths (twips), keyed by zero-based column.
    pub col_sizes: BTreeMap<u32, i64>,
    /// Explicit row heights (twips), keyed by zero-based row.
    pub row_sizes: BTreeMap<u32, i64>,
    /// Hidden rows, by zero-based index.
    pub hidden_rows: BTreeSet<u32>,
    /// Hidden columns, by zero-based index.
    pub hidden_cols: BTreeSet<u32>,
    /// Outline nesting level per row (`<row outlineLevel>`), zero-based index.
    pub row_outline_levels: BTreeMap<u32, u8>,
    /// Outline nesting level per column (`<col outlineLevel>`), zero-based index.
    pub col_outline_levels: BTreeMap<u32, u8>,
    /// Rows with a collapsed outline group (`<row collapsed="1">`).
    pub collapsed_rows: BTreeSet<u32>,
    /// Columns with a collapsed outline group (`<col collapsed="1">`).
    pub collapsed_cols: BTreeSet<u32>,
    /// How many `<col>` spans named columns past the end of the grid, wholly or
    /// in part. Counted rather than reported here because this reader has no
    /// compatibility report; `import_package` names them (FID-18).
    pub out_of_grid_cols: u32,
    /// How many `<row>` elements named a row past the end of the grid.
    pub out_of_grid_rows: u32,
    /// Outline summary-position flags from `<sheetPr><outlinePr/>`, if present.
    pub outline: Option<OutlinePr>,
    /// View zoom percentage from `<sheetView zoomScale>`, if set and non-default.
    pub zoom: Option<u16>,
    /// `true` when `<sheetView showGridLines="0">` hides the grid lines.
    pub hide_gridlines: bool,
    /// `true` when `<sheetView showRowColHeaders="0">` hides the headers.
    pub hide_headers: bool,
    /// Tab color as `RRGGBB` (from `sheetPr/tabColor/@rgb`), if any.
    pub tab_color: Option<String>,
    /// Data-validations, mapped to the model in `lib.rs`.
    pub validations: Vec<RawDv>,
    /// Raw conditional-formatting rules, mapped to the model in `lib.rs`.
    pub conditional_formats: Vec<RawCf>,
    /// `<sheetProtection>` attributes exactly as read, or `None` if absent.
    pub protection: Option<BTreeMap<String, String>>,
    /// Print layout, carried through verbatim.
    pub print: PrintSetup,
    /// `<sortState>` with its conditions, carried verbatim.
    pub sort_state: Option<SortState>,
    /// Sheet-level elements carried verbatim, in document order.
    pub carried: Vec<RetainedRef>,
    /// `<sheetFormatPr>` attributes as read.
    pub format_pr: BTreeMap<String, String>,
    /// `<sheetView>` flags beyond grid lines and headers.
    pub right_to_left: bool,
    /// See [`Worksheet::right_to_left`].
    pub show_formulas: bool,
    /// See [`Worksheet::right_to_left`].
    pub hide_zeros: bool,
    /// See [`Worksheet::right_to_left`].
    pub tab_selected: bool,
    /// Elements naming a retained part, in document order.
    pub retained_refs: Vec<RetainedRef>,
    /// The `<autoFilter ref>` range, if the sheet has an autofilter.
    pub auto_filter: Option<String>,
    /// Per-column filter rules inside that `<autoFilter>`.
    pub filter_columns: Vec<RawFilterColumn>,
}

/// A raw `<filterColumn>`, before mapping to the model.
#[derive(Debug, Default, Clone)]
pub struct RawFilterColumn {
    /// The `colId` attribute — an offset from the filter range's first column.
    pub col_id: u32,
    /// `<filter val>` entries from a `<filters>` checklist.
    pub values: Vec<String>,
    /// `<filters blank="1">` — blanks are an attribute, not a `<filter>` entry.
    pub blank: bool,
    /// Whether a `<filters>` element was seen at all. Distinguishes a checklist
    /// that selects only blanks from a column with no checklist.
    pub saw_filters: bool,
    /// `<customFilter>` entries as `(operator, val)`.
    pub custom: Vec<(String, String)>,
    /// `<customFilters and="1">`.
    pub custom_and: bool,
    /// A refinement we carry but do not evaluate, as `(element, attributes)`.
    pub unevaluated: Option<(String, BTreeMap<String, String>)>,
}

/// A raw `<dataValidation>`, before mapping to the model.
#[derive(Debug, Default, Clone)]
pub struct RawDv {
    /// The `sqref` attribute — a space-separated list of areas.
    pub sqref: String,
    /// `errorStyle`, `showDropDown` and `imeMode`, carried through.
    pub error_style: Option<String>,
    /// See [`RawDv::error_style`].
    pub hide_dropdown: bool,
    /// See [`RawDv::error_style`].
    pub ime_mode: Option<String>,
    /// The `type` attribute; absent means `none`.
    pub kind: String,
    /// The `operator` attribute; absent means `between`.
    pub operator: String,
    /// `<formula1>` text.
    pub formula1: String,
    /// `<formula2>` text.
    pub formula2: String,
    /// `allowBlank`; OOXML defaults it to true.
    pub allow_blank: bool,
    /// Author-set message wording.
    pub error_title: String,
    /// Body of the error message.
    pub error_text: String,
    /// Title of the selection hint.
    pub prompt_title: String,
    /// Body of that hint.
    pub prompt_text: String,
}

/// A raw `<cfRule>` with its enclosing `sqref`, before mapping to the model.
#[derive(Debug, Default)]
pub struct RawCf {
    /// The range the rule applies to (`<conditionalFormatting sqref>`).
    pub sqref: String,
    /// The `type` attribute (`cellIs`, `containsText`, …).
    pub kind: String,
    /// The `operator` attribute (`greaterThan`, `between`, …).
    pub operator: String,
    /// The `dxfId` — index into the styles `<dxfs>` for the fill.
    pub dxf_id: Option<usize>,
    /// The `text` attribute (for `containsText`).
    pub text: Option<String>,
    /// The `<formula>` operand texts, in order.
    pub formulas: Vec<String>,
    /// The `<color>` stops inside a `<colorScale>` / `<dataBar>`, low → high,
    /// as `RRGGBB`. Those kinds carry their own presentation instead of a dxf.
    pub colors: Vec<String>,
    /// `rank` for a `top10` rule.
    pub rank: u32,
    /// `bottom="1"` on a `top10` rule.
    pub bottom: bool,
    /// `percent="1"` on a `top10` rule.
    pub percent: bool,
    /// `aboveAverage` — absent defaults to `1` (above), per the schema.
    pub above_average: bool,
    /// `equalAverage="1"`.
    pub equal_average: bool,
    /// Evaluation order; lower wins.
    pub priority: u32,
    /// `stopIfTrue="1"`.
    pub stop_if_true: bool,
}

/// Ceiling on how many columns one `<col>` span may expand into per-line
/// overrides; a wider custom span is treated as the sheet's default width.
const MAX_COL_SPAN: u32 = 4096;

/// Ceiling on an imported column width, in twips: Excel refuses a column wider
/// than 255 characters, which is `255 * 7 + 5 = 1790` px, or 26_850 twips.
const MAX_COL_TWIPS: i64 = 26_850;

/// Ceiling on an imported row height, in twips: Excel refuses a row taller than
/// 409.5 points, or 8_190 twips.
const MAX_ROW_TWIPS: i64 = 8_190;

/// Convert an Excel column width (character units) to twips, matching the
/// pixel rounding Excel uses so the value survives a write→read round-trip.
///
/// `chars` is an arbitrary `xsd:double` off the wire — `<col width>` and
/// `<sheetFormatPr defaultColWidth>` are attacker-controlled — and nothing used
/// to bound it. `width="1e300"` saturated the float→int cast to `i64::MAX` and
/// the `* 15` panicked with "attempt to multiply with overflow" under the dev
/// profile, so a crafted `<cols>` aborted the host process instead of returning
/// `Err`; the shipped release profile wrapped instead, and `width="-1e300"`
/// handed layout a *negative* column width. Clamp before the multiply, not
/// after — after is already too late.
pub(crate) fn col_width_to_twips(chars: f64) -> i64 {
    let px = (chars * 7.0 + 5.0).round();
    // Two layers, and they answer different questions. `read_f64_attr` refuses
    // a non-finite attribute at the boundary, so nothing from a file arrives
    // here as `NaN`; this function takes a plain `f64` from anywhere in the
    // crate, so it still declines to trust one. `NaN` is named rather than left
    // to fall through a comparison it silently fails — it is neither `<` nor
    // `>`, so relying on that would send it to the ceiling below.
    if !px.is_finite() || px <= 0.0 {
        return 0;
    }
    let max_px = (MAX_COL_TWIPS / 15) as f64;
    if px > max_px {
        return MAX_COL_TWIPS;
    }
    (px as i64) * 15
}

/// Convert an Excel row height (points) to twips. Bounded for the same reason
/// as [`col_width_to_twips`]: `ht="1e308"` saturated the cast and yielded
/// `i64::MAX` twips of row.
fn row_height_to_twips(points: f64) -> i64 {
    let twips = (points * 20.0).round();
    if !twips.is_finite() || twips <= 0.0 {
        return 0;
    }
    if twips > MAX_ROW_TWIPS as f64 {
        return MAX_ROW_TWIPS;
    }
    twips as i64
}

/// Build a [`RawCf`] from a `<cfRule>` element. Shared by the Start and Empty
/// dispatches: a rule with `<formula>` children opens an element, while
/// `top10` / `aboveAverage` / `duplicateValues` are self-closing.
fn read_cf_rule(e: &BytesStart<'_>, sqref: &str) -> Result<RawCf, ImportError> {
    Ok(RawCf {
        sqref: sqref.to_owned(),
        kind: read_attr(e, b"type")?.unwrap_or_default(),
        operator: read_attr(e, b"operator")?.unwrap_or_default(),
        dxf_id: read_attr(e, b"dxfId")?.and_then(|s| s.parse().ok()),
        text: read_attr(e, b"text")?,
        formulas: Vec::new(),
        colors: Vec::new(),
        rank: parse_u32_attr(e, b"rank")?,
        bottom: read_bool_attr(e, b"bottom")?.unwrap_or(false),
        percent: read_bool_attr(e, b"percent")?.unwrap_or(false),
        // The schema defaults `aboveAverage` to true, so an absent attribute
        // means "above", not "below".
        above_average: read_bool_attr(e, b"aboveAverage")?.unwrap_or(true),
        equal_average: read_bool_attr(e, b"equalAverage")?.unwrap_or(false),
        priority: parse_u32_attr(e, b"priority")?,
        stop_if_true: read_bool_attr(e, b"stopIfTrue")?.unwrap_or(false),
    })
}

/// Handle one `<autoFilter>` subtree element. Returns `true` if the element was
/// consumed here, so both the Start and Empty dispatches can share this — every
/// one of these elements appears in either form depending on whether it has
/// children.
fn read_filter_element(
    e: &BytesStart<'_>,
    name: &[u8],
    auto_filter_ref: &mut Option<String>,
    cur_fc: &mut Option<RawFilterColumn>,
) -> Result<bool, ImportError> {
    match name {
        b"autoFilter" => {
            *auto_filter_ref = read_attr(e, b"ref")?;
        }
        b"filterColumn" => {
            *cur_fc = Some(RawFilterColumn {
                col_id: parse_u32_attr(e, b"colId")?,
                ..RawFilterColumn::default()
            });
        }
        b"filters" if cur_fc.is_some() => {
            if let Some(fc) = cur_fc.as_mut() {
                fc.saw_filters = true;
                fc.blank = read_bool_attr(e, b"blank")?.unwrap_or(false);
            }
        }
        b"filter" if cur_fc.is_some() => {
            if let Some(val) = read_attr(e, b"val")?
                && let Some(fc) = cur_fc.as_mut()
            {
                fc.values.push(val);
            }
        }
        // Refinements we keep but never apply. Carried so the filter survives
        // the save; the rows they would hide stay visible, which is visibly
        // incomplete rather than silently wrong.
        b"top10" | b"dynamicFilter" | b"colorFilter" | b"iconFilter" | b"dateGroupItem"
            if cur_fc.is_some() =>
        {
            let element = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
            let attrs = read_attrs(e)?;
            if let Some(fc) = cur_fc.as_mut() {
                fc.unevaluated = Some((element, attrs));
            }
        }
        b"customFilters" if cur_fc.is_some() => {
            if let Some(fc) = cur_fc.as_mut() {
                fc.custom_and = read_bool_attr(e, b"and")?.unwrap_or(false);
            }
        }
        b"customFilter" if cur_fc.is_some() => {
            // An absent operator means `equal` per the schema.
            let op = read_attr(e, b"operator")?.unwrap_or_else(|| "equal".to_owned());
            let val = read_attr(e, b"val")?.unwrap_or_default();
            if let Some(fc) = cur_fc.as_mut() {
                fc.custom.push((op, val));
            }
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn parse_u32_attr(e: &BytesStart<'_>, local: &[u8]) -> Result<u32, ImportError> {
    Ok(read_attr(e, local)?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0))
}

/// A numeric attribute, or `None` when it is absent or not a usable number.
///
/// **Non-finite is not a measurement.** `xsd:double` admits `NaN`, `INF` and
/// `-INF`, and Rust's parser accepts all of their spellings, so before this
/// filter every numeric attribute in the format could arrive as a value no
/// arithmetic downstream is prepared for. That is not a column-width problem:
/// thirteen call sites read one of these — widths, heights, zoom, pane splits,
/// tint, chart geometry — and each would have needed its own guard.
///
/// Refused once, at the boundary where the untrusted text becomes a number,
/// because a rule applied at call sites is a rule the fourteenth call site does
/// not get. Absent and unusable collapse to the same `None` the callers already
/// handle, so each falls back to its documented default rather than to a
/// value that will surprise it later.
fn read_f64_attr(e: &BytesStart<'_>, local: &[u8]) -> Result<Option<f64>, ImportError> {
    Ok(read_attr(e, local)?
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|value| value.is_finite()))
}

/// Record a `<col min max width customWidth hidden>` element into the width
/// overrides and the hidden-column set.
fn read_col(e: &BytesStart<'_>, result: &mut Worksheet) -> Result<(), ImportError> {
    let min = parse_u32_attr(e, b"min")?; // 1-based
    let mut max = parse_u32_attr(e, b"max")?;
    if min == 0 || max < min {
        return Ok(());
    }
    // The grid ends at column 16,384 (`docs/21-PARSER-LIMITS.md`). A span that
    // starts past it describes nothing, and one that merely *ends* past it is
    // clipped to the part that exists — clipping the tail off a span is not the
    // clamping this fix refuses, because no value moves: `<col>` states a width
    // for a range of columns, and the columns beyond the grid are not columns.
    // Left unbounded, `<col min="20000" max="20005" width="30"/>` became six
    // per-column widths the writer re-emitted as a `<col min="20000">` Excel
    // will not open (FID-18).
    let last = casual_calc_model::GRID_MAX_COL + 1; // one-based
    if min > last {
        result.out_of_grid_cols = result.out_of_grid_cols.saturating_add(1);
        return Ok(());
    }
    if max > last {
        result.out_of_grid_cols = result.out_of_grid_cols.saturating_add(1);
        max = last;
    }
    // A hidden span is recorded per zero-based column regardless of width. The
    // same span rules apply to the outline level and collapsed flag, which — like
    // hidden — are meaningful even when the column carries no custom width, so
    // they are parsed before the width-driven early return below.
    let narrow = max.saturating_sub(min) < MAX_COL_SPAN;
    if read_bool_attr(e, b"hidden")?.unwrap_or(false) && narrow {
        for col in min..=max {
            result.hidden_cols.insert(col - 1);
        }
    }
    if let Some(level) = read_attr(e, b"outlineLevel")?.and_then(|s| s.parse::<u8>().ok())
        && level != 0
        && narrow
    {
        for col in min..=max {
            result.col_outline_levels.insert(col - 1, level);
        }
    }
    if read_bool_attr(e, b"collapsed")?.unwrap_or(false) && narrow {
        for col in min..=max {
            result.collapsed_cols.insert(col - 1);
        }
    }
    let Some(width) = read_f64_attr(e, b"width")? else {
        return Ok(());
    };
    let twips = col_width_to_twips(width);
    // A span covering (nearly) the whole sheet is the default width, not
    // thousands of per-column overrides — record it as the default to stay
    // compact, and let it win over an earlier `<sheetFormatPr defaultColWidth>`
    // since it is the more specific statement.
    //
    // Everything narrower is an authoritative per-column width. `customWidth`
    // does NOT gate this: it only records whether the user set the width by
    // hand rather than by autofit, and gating on it silently dropped every
    // width in files that write the equally-valid `customWidth="true"`
    // (LibreOffice, Apache POI, ExcelJS) — the widths of an imported workbook
    // simply vanished.
    if !narrow {
        result.col_default = Some(twips);
        return Ok(());
    }
    for col in min..=max {
        result.col_sizes.insert(col - 1, twips);
    }
    Ok(())
}

/// Where a `<c>` sits, for the cells that do not say.
///
/// `r` is **optional** in ECMA-376: a cell without one sits at the ordinal
/// position it occupies within its row, and its row is the enclosing `<row>`.
/// Excel writes it for everything, which is why treating it as required survives
/// a long time — and then a real file arrives with `<c t="inlineStr">` and no
/// `r`, and every cell in it is dropped on the floor.
///
/// `col` is the running position within the current row and is advanced here.
/// A cell that *does* carry `r` resets it, so a row mixing the two forms — legal,
/// and the reason this cannot simply count — stays aligned to what the file says.
fn cell_reference(e: &BytesStart<'_>, row: u32, col: &mut u32) -> Result<String, ImportError> {
    if let Some(reference) = read_attr(e, b"r")?
        && !reference.is_empty()
    {
        if let Some(at) = crate::a1::parse_a1(&reference) {
            *col = at.col.saturating_add(1);
        }
        return Ok(reference);
    }
    let here = *col;
    *col = col.saturating_add(1);
    Ok(format!(
        "{}{}",
        casual_calc_formula::column_to_letters(here),
        row.max(1)
    ))
}

/// Record a `<row r ht hidden>` element's custom height into the height
/// overrides and its hidden flag into the hidden-row set.
fn read_row(e: &BytesStart<'_>, result: &mut Worksheet) -> Result<(), ImportError> {
    let Some(r) = read_attr(e, b"r")?.and_then(|s| s.parse::<u32>().ok()) else {
        return Ok(());
    };
    if r < 1 {
        return Ok(());
    }
    // The grid ends at row 1,048,576. A `<row r="4294967295">` is not a tall row
    // or a hidden row, it is an address that does not exist, and every one of
    // the maps below is keyed by row index and re-emitted by the writer as
    // `<row r="…">` (FID-18).
    if r > casual_calc_model::GRID_MAX_ROW + 1 {
        result.out_of_grid_rows = result.out_of_grid_rows.saturating_add(1);
        return Ok(());
    }
    if let Some(ht) = read_f64_attr(e, b"ht")? {
        result.row_sizes.insert(r - 1, row_height_to_twips(ht));
    }
    if read_bool_attr(e, b"hidden")?.unwrap_or(false) {
        result.hidden_rows.insert(r - 1);
    }
    if let Some(level) = read_attr(e, b"outlineLevel")?.and_then(|s| s.parse::<u8>().ok())
        && level != 0
    {
        result.row_outline_levels.insert(r - 1, level);
    }
    if read_bool_attr(e, b"collapsed")?.unwrap_or(false) {
        result.collapsed_rows.insert(r - 1);
    }
    Ok(())
}

/// Record `<sheetFormatPr>` axis defaults.
fn read_sheet_format(e: &BytesStart<'_>, result: &mut Worksheet) -> Result<(), ImportError> {
    // The two defaults below are interpreted; the rest travel verbatim.
    result.format_pr = read_attrs(e)?;
    result.format_pr.remove("defaultColWidth");
    result.format_pr.remove("defaultRowHeight");
    if let Some(w) = read_f64_attr(e, b"defaultColWidth")? {
        result.col_default = Some(col_width_to_twips(w));
    }
    if let Some(h) = read_f64_attr(e, b"defaultRowHeight")? {
        result.row_default = Some(row_height_to_twips(h));
    }
    Ok(())
}

/// Parse a worksheet part's `sheetData`, `mergeCells`, and `sheetView` pane.
pub fn parse_worksheet(xml: &[u8], theme: &ThemePalette) -> Result<Worksheet, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut bounds = Bounds::new();

    let mut result = Worksheet::default();
    // `<brk>` is identical under `<rowBreaks>` and `<colBreaks>`; only the
    // enclosing element says which list it belongs to.
    let mut in_col_breaks = false;
    // Header/footer strings are element text, not attributes.
    let mut header_footer_tag: Option<String> = None;
    let mut current: Option<RawCell> = None;
    let mut in_value = false;
    let mut in_formula = false;
    let mut in_inline = false;
    let mut in_inline_text = false;
    // Active `<dataValidation>` being parsed: (sqref, accumulated formula1).
    let mut dv: Option<RawDv> = None;
    let mut in_dv_formula1 = false;
    let mut in_dv_formula2 = false;
    // Active `<conditionalFormatting>` sqref + `<cfRule>` being parsed.
    let mut cf_sqref = String::new();
    let mut cur_cf: Option<RawCf> = None;
    let mut cur_fc: Option<RawFilterColumn> = None;
    let mut in_cf_formula = false;
    // Where the reader is, for cells and rows that decline to say. Both
    // attributes are optional; a file using neither is a plain sequence.
    let mut row_now: u32 = 0;
    let mut col_next: u32 = 0;

    loop {
        match reader.read_event_into(&mut buf).map_err(xml_err)? {
            Event::Start(e) => {
                bounds.open()?;
                match e.local_name().as_ref() {
                    b"c" => {
                        current = Some(RawCell {
                            reference: cell_reference(&e, row_now, &mut col_next)?,
                            cell_type: read_attr(&e, b"t")?,
                            style_index: read_attr(&e, b"s")?.and_then(|s| s.parse().ok()),
                            ..RawCell::default()
                        });
                    }
                    b"v" => in_value = true,
                    b"f" => {
                        in_formula = true;
                        let si = read_attr(&e, b"si")?.and_then(|s| s.parse().ok());
                        if let Some(cell) = current.as_mut() {
                            cell.formula.get_or_insert_with(String::new);
                            cell.shared_index = si;
                        }
                    }
                    b"is" => in_inline = true,
                    b"t" if in_inline => in_inline_text = true,
                    // `<hyperlink>` is childless and self-closing in practice; the

                    // worksheet walk folds Start and Empty together, so one arm covers it.
                    b"hyperlink" => {
                        result.hyperlinks.push(RawHyperlink {
                            reference: read_attr(&e, b"ref")?.unwrap_or_default(),

                            rel_id: read_attr(&e, b"id")?,

                            location: read_attr(&e, b"location")?,

                            tooltip: read_attr(&e, b"tooltip")?,

                            display: read_attr(&e, b"display")?,
                        });
                    }
                    // These name a part we keep but do not model. `<drawing>`
                    // is the one that matters most: without it a retained chart
                    // is in the package but on no sheet.
                    b"drawing" | b"picture" | b"oleObjects" | b"controls" => {
                        result.retained_refs.push((
                            String::from_utf8_lossy(e.local_name().as_ref()).into_owned(),
                            read_attrs(&e)?,
                        ));
                    }
                    b"sortState" => {
                        result.sort_state = Some(SortState {
                            attrs: read_attrs(&e)?,
                            conditions: Vec::new(),
                        });
                    }
                    b"sortCondition" => {
                        if let Some(state) = result.sort_state.as_mut() {
                            state.conditions.push(read_attrs(&e)?);
                        }
                    }
                    // Nothing here acts on these, so they travel verbatim.
                    // `<protectedRange>` holds password hashes; regenerating one
                    // would lock the author out of their own range.
                    b"dimension" | b"selection" | b"sheetCalcPr" | b"ignoredError"
                    | b"protectedRange" => {
                        result.carried.push((
                            String::from_utf8_lossy(e.local_name().as_ref()).into_owned(),
                            read_attrs(&e)?,
                        ));
                    }
                    b"mergeCell" => {
                        if let Some(reference) = read_attr(&e, b"ref")? {
                            result.merges.push(reference);
                        }
                    }
                    b"colBreaks" => in_col_breaks = true,
                    b"rowBreaks" => in_col_breaks = false,
                    b"oddHeader" | b"oddFooter" | b"evenHeader" | b"evenFooter"
                    | b"firstHeader" | b"firstFooter" => {
                        header_footer_tag =
                            Some(String::from_utf8_lossy(e.local_name().as_ref()).into_owned());
                    }
                    b"pageMargins" => result.print.margins = read_attrs(&e)?,
                    b"pageSetup" => result.print.page = read_attrs(&e)?,
                    b"printOptions" => result.print.options = read_attrs(&e)?,
                    b"pageSetUpPr" => result.print.setup_pr = read_attrs(&e)?,
                    b"headerFooter" => result.print.header_footer = read_attrs(&e)?,
                    b"brk" => {
                        // `<brk>` appears under both `<rowBreaks>` and
                        // `<colBreaks>`; which list it joins is decided by the
                        // enclosing element, tracked in `in_col_breaks`.
                        let attrs = read_attrs(&e)?;
                        if in_col_breaks {
                            result.print.col_breaks.push(attrs);
                        } else {
                            result.print.row_breaks.push(attrs);
                        }
                    }
                    b"sheetProtection" => {
                        result.protection = Some(read_protection(&e)?);
                    }
                    b"pane" => read_pane(&e, &mut result)?,
                    b"sheetView" => read_sheet_view(&e, &mut result)?,
                    b"row" => {
                        row_now = read_attr(&e, b"r")?
                            .and_then(|r| r.parse::<u32>().ok())
                            .unwrap_or_else(|| row_now.saturating_add(1));
                        col_next = 0;
                        read_row(&e, &mut result)?;
                    }
                    b"col" => read_col(&e, &mut result)?,
                    b"sheetFormatPr" => read_sheet_format(&e, &mut result)?,
                    b"outlinePr" => read_outline_pr(&e, &mut result)?,
                    b"tabColor" => read_tab_color(&e, &mut result, theme)?,
                    b"dataValidation" => {
                        // Every kind is modelled: dropping the non-list ones is
                        // how a file's number and date rules used to disappear.
                        dv = Some(RawDv {
                            sqref: read_attr(&e, b"sqref")?.unwrap_or_default(),
                            error_style: read_attr(&e, b"errorStyle")?,
                            hide_dropdown: read_attr(&e, b"showDropDown")?
                                .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")),
                            ime_mode: read_attr(&e, b"imeMode")?,
                            kind: read_attr(&e, b"type")?.unwrap_or_default(),
                            operator: read_attr(&e, b"operator")?.unwrap_or_default(),
                            allow_blank: read_bool_attr(&e, b"allowBlank")?.unwrap_or(false),
                            error_title: read_attr(&e, b"errorTitle")?.unwrap_or_default(),
                            error_text: read_attr(&e, b"error")?.unwrap_or_default(),
                            prompt_title: read_attr(&e, b"promptTitle")?.unwrap_or_default(),
                            prompt_text: read_attr(&e, b"prompt")?.unwrap_or_default(),
                            ..RawDv::default()
                        });
                    }
                    b"formula1" if dv.is_some() => in_dv_formula1 = true,
                    b"formula2" if dv.is_some() => in_dv_formula2 = true,
                    b"conditionalFormatting" => {
                        cf_sqref = read_attr(&e, b"sqref")?.unwrap_or_default();
                    }
                    n if read_filter_element(&e, n, &mut result.auto_filter, &mut cur_fc)? => {}
                    b"cfRule" => {
                        cur_cf = Some(read_cf_rule(&e, &cf_sqref)?);
                    }
                    b"color" if cur_cf.is_some() => {
                        if let Some(rgb) = read_attr(&e, b"rgb")?
                            && let Some(cf) = cur_cf.as_mut()
                        {
                            let hex = rgb.trim();
                            if hex.len() >= 6 {
                                cf.colors.push(hex[hex.len() - 6..].to_ascii_uppercase());
                            }
                        }
                    }
                    b"formula" if cur_cf.is_some() => {
                        in_cf_formula = true;
                        if let Some(cf) = cur_cf.as_mut() {
                            cf.formulas.push(String::new());
                        }
                    }
                    _ => {}
                }
            }
            Event::Empty(e) => {
                bounds.count()?;
                match e.local_name().as_ref() {
                    b"c" => {
                        result.cells.push(RawCell {
                            reference: cell_reference(&e, row_now, &mut col_next)?,
                            cell_type: read_attr(&e, b"t")?,
                            style_index: read_attr(&e, b"s")?.and_then(|s| s.parse().ok()),
                            ..RawCell::default()
                        });
                    }
                    // A follower of a shared formula: `<f t="shared" si="N"/>`
                    // with no text of its own.
                    b"f" => {
                        let si = read_attr(&e, b"si")?.and_then(|s| s.parse().ok());
                        if let Some(cell) = current.as_mut() {
                            cell.formula.get_or_insert_with(String::new);
                            cell.shared_index = si;
                        }
                    }
                    // `<hyperlink>` is childless, so every writer self-closes
                    // it and it only ever arrives here, never in the `Start`
                    // dispatch. Handling it there alone would read a workbook
                    // full of links as having none.
                    b"hyperlink" => {
                        result.hyperlinks.push(RawHyperlink {
                            reference: read_attr(&e, b"ref")?.unwrap_or_default(),
                            rel_id: read_attr(&e, b"id")?,
                            location: read_attr(&e, b"location")?,
                            tooltip: read_attr(&e, b"tooltip")?,
                            display: read_attr(&e, b"display")?,
                        });
                    }
                    // All childless, so they arrive here rather than in the
                    // `Start` dispatch. Reading them only there is why the
                    // first version of this saw no print settings at all.
                    b"pageMargins" => result.print.margins = read_attrs(&e)?,
                    b"pageSetup" => result.print.page = read_attrs(&e)?,
                    b"printOptions" => result.print.options = read_attrs(&e)?,
                    b"pageSetUpPr" => result.print.setup_pr = read_attrs(&e)?,
                    b"headerFooter" => result.print.header_footer = read_attrs(&e)?,
                    b"brk" => {
                        let attrs = read_attrs(&e)?;
                        if in_col_breaks {
                            result.print.col_breaks.push(attrs);
                        } else {
                            result.print.row_breaks.push(attrs);
                        }
                    }
                    // These name a part we keep but do not model. `<drawing>`
                    // is the one that matters most: without it a retained chart
                    // is in the package but on no sheet.
                    b"drawing" | b"picture" | b"oleObjects" | b"controls" => {
                        result.retained_refs.push((
                            String::from_utf8_lossy(e.local_name().as_ref()).into_owned(),
                            read_attrs(&e)?,
                        ));
                    }
                    b"sortState" => {
                        result.sort_state = Some(SortState {
                            attrs: read_attrs(&e)?,
                            conditions: Vec::new(),
                        });
                    }
                    b"sortCondition" => {
                        if let Some(state) = result.sort_state.as_mut() {
                            state.conditions.push(read_attrs(&e)?);
                        }
                    }
                    // Nothing here acts on these, so they travel verbatim.
                    // `<protectedRange>` holds password hashes; regenerating one
                    // would lock the author out of their own range.
                    b"dimension" | b"selection" | b"sheetCalcPr" | b"ignoredError"
                    | b"protectedRange" => {
                        result.carried.push((
                            String::from_utf8_lossy(e.local_name().as_ref()).into_owned(),
                            read_attrs(&e)?,
                        ));
                    }
                    b"mergeCell" => {
                        if let Some(reference) = read_attr(&e, b"ref")? {
                            result.merges.push(reference);
                        }
                    }
                    b"pane" => read_pane(&e, &mut result)?,
                    b"sheetView" => read_sheet_view(&e, &mut result)?,
                    b"row" => {
                        row_now = read_attr(&e, b"r")?
                            .and_then(|r| r.parse::<u32>().ok())
                            .unwrap_or_else(|| row_now.saturating_add(1));
                        col_next = 0;
                        read_row(&e, &mut result)?;
                    }
                    b"col" => read_col(&e, &mut result)?,
                    b"sheetFormatPr" => read_sheet_format(&e, &mut result)?,
                    b"outlinePr" => read_outline_pr(&e, &mut result)?,
                    b"tabColor" => read_tab_color(&e, &mut result, theme)?,
                    // Self-closing in every file that carries it, so this arm is
                    // the one that actually fires — the Start arm above is for
                    // the form the schema allows but nobody writes.
                    b"sheetProtection" => {
                        result.protection = Some(read_protection(&e)?);
                    }
                    // A rule with no children — top10, aboveAverage,
                    // duplicateValues — arrives self-closing, so it never reaches
                    // the End handler that pushes the others. Complete it here.
                    b"cfRule" => {
                        result
                            .conditional_formats
                            .push(read_cf_rule(&e, &cf_sqref)?);
                    }
                    n if read_filter_element(&e, n, &mut result.auto_filter, &mut cur_fc)? => {}
                    _ => {}
                }
            }
            Event::Text(e) => {
                // Header and footer strings are element text, not attributes.
                if let Some(tag) = header_footer_tag.clone() {
                    let text = e.unescape().map_err(xml_err)?.into_owned();
                    result
                        .print
                        .header_footer_text
                        .entry(tag)
                        .and_modify(|v| v.push_str(&text))
                        .or_insert(text);
                }
                if in_dv_formula1 && let Some(raw) = dv.as_mut() {
                    raw.formula1.push_str(&e.unescape().map_err(xml_err)?);
                } else if in_dv_formula2 && let Some(raw) = dv.as_mut() {
                    raw.formula2.push_str(&e.unescape().map_err(xml_err)?);
                } else if in_cf_formula
                    && let Some(cf) = cur_cf.as_mut()
                    && let Some(last) = cf.formulas.last_mut()
                {
                    last.push_str(&e.unescape().map_err(xml_err)?);
                } else if let Some(cell) = current.as_mut() {
                    if in_value {
                        cell.value
                            .get_or_insert_with(String::new)
                            .push_str(&e.unescape().map_err(xml_err)?);
                    } else if in_formula {
                        cell.formula
                            .get_or_insert_with(String::new)
                            .push_str(&e.unescape().map_err(xml_err)?);
                    } else if in_inline_text {
                        cell.inline
                            .get_or_insert_with(String::new)
                            .push_str(&e.unescape().map_err(xml_err)?);
                    }
                }
            }
            Event::End(e) => {
                bounds.close();
                if matches!(
                    e.local_name().as_ref(),
                    b"oddHeader"
                        | b"oddFooter"
                        | b"evenHeader"
                        | b"evenFooter"
                        | b"firstHeader"
                        | b"firstFooter"
                ) {
                    header_footer_tag = None;
                }
                match e.local_name().as_ref() {
                    b"v" => in_value = false,
                    b"f" => in_formula = false,
                    b"formula1" => in_dv_formula1 = false,
                    b"formula2" => in_dv_formula2 = false,
                    b"dataValidation" => {
                        if let Some(raw) = dv.take() {
                            result.validations.push(raw);
                        }
                    }
                    b"formula" => in_cf_formula = false,
                    b"cfRule" => {
                        if let Some(cf) = cur_cf.take() {
                            result.conditional_formats.push(cf);
                        }
                    }
                    b"conditionalFormatting" => cf_sqref.clear(),
                    b"filterColumn" => {
                        if let Some(fc) = cur_fc.take() {
                            result.filter_columns.push(fc);
                        }
                    }
                    b"t" if in_inline => in_inline_text = false,
                    b"is" => in_inline = false,
                    b"c" => {
                        if let Some(cell) = current.take() {
                            result.cells.push(cell);
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(result)
}

fn read_pane(e: &BytesStart<'_>, result: &mut Worksheet) -> Result<(), ImportError> {
    let state = read_attr(e, b"state")?.unwrap_or_default();
    if state == "frozen" || state == "frozenSplit" {
        let cols = parse_u32_attr(e, b"xSplit")?;
        let rows = parse_u32_attr(e, b"ySplit")?;
        result.frozen = Some((rows, cols));
    }
    Ok(())
}

/// Parse the `zoomScale` attribute of a `<sheetView>`. A zoom of 0 or 100 is the
/// application default and is not retained, so no phantom `zoomScale` is written.
fn read_sheet_view(e: &BytesStart<'_>, result: &mut Worksheet) -> Result<(), ImportError> {
    let flag = |name: &[u8]| -> Result<bool, ImportError> {
        Ok(read_attr(e, name)?.is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")))
    };
    result.right_to_left = flag(b"rightToLeft")?;
    result.show_formulas = flag(b"showFormulas")?;
    result.tab_selected = flag(b"tabSelected")?;
    // `showZeros` defaults to true, so only an explicit "0" hides them.
    result.hide_zeros =
        read_attr(e, b"showZeros")?.is_some_and(|v| v == "0" || v.eq_ignore_ascii_case("false"));
    if let Some(zoom) = read_attr(e, b"zoomScale")?.and_then(|s| s.parse::<u16>().ok())
        && zoom != 0
        && zoom != 100
    {
        result.zoom = Some(zoom);
    }
    // Grid lines and headers show by default; only an explicit "0" hides them.
    if read_bool_attr(e, b"showRowColHeaders")? == Some(false) {
        result.hide_headers = true;
    }
    if read_bool_attr(e, b"showGridLines")? == Some(false) {
        result.hide_gridlines = true;
    }
    Ok(())
}

/// Parse `<sheetPr><outlinePr summaryBelow= summaryRight=/>`. Both flags default
/// to `true` (the OOXML default) when the attribute is absent.
fn read_outline_pr(e: &BytesStart<'_>, result: &mut Worksheet) -> Result<(), ImportError> {
    result.outline = Some(OutlinePr {
        summary_below: read_bool_attr(e, b"summaryBelow")?.unwrap_or(true),
        summary_right: read_bool_attr(e, b"summaryRight")?.unwrap_or(true),
    });
    Ok(())
}

/// Read an OOXML boolean attribute (`"1"`/`"true"` → `true`, `"0"`/`"false"` →
/// `false`), returning `None` when the attribute is absent.
fn read_bool_attr(e: &BytesStart<'_>, local: &[u8]) -> Result<Option<bool>, ImportError> {
    Ok(read_attr(e, local)?.map(|v| v == "1" || v.eq_ignore_ascii_case("true")))
}

/// Parse `<sheetPr><tabColor rgb="AARRGGBB"/>`. Excel stores an 8-hex ARGB
/// value; we keep the last six (`RRGGBB`) and drop the alpha. Indexed/theme
/// colors (no `@rgb`) are ignored — they'd need the theme part to resolve.
fn read_tab_color(
    e: &BytesStart<'_>,
    result: &mut Worksheet,
    theme: &ThemePalette,
) -> Result<(), ImportError> {
    // A tab colour is an OOXML colour like any other: rgb, theme+tint, or
    // indexed. Excel's colour picker writes theme references.
    let resolved = match read_attr(e, b"rgb")? {
        Some(v) => Some(v),
        None => match read_attr(e, b"theme")?.and_then(|s| s.parse::<usize>().ok()) {
            Some(slot) => theme.resolve(
                slot,
                read_attr(e, b"tint")?
                    .and_then(|t| t.parse::<f64>().ok())
                    .unwrap_or(0.0),
            ),
            None => read_attr(e, b"indexed")?
                .and_then(|s| s.parse::<usize>().ok())
                .and_then(indexed_color),
        },
    };
    if let Some(rgb) = resolved {
        let hex = rgb.trim();
        if hex.len() >= 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            result.tab_color = Some(hex[hex.len() - 6..].to_ascii_uppercase());
        }
    }
    Ok(())
}

/// Parse `<definedName>` entries from `workbook.xml`.
///
/// Returns `(name, local_sheet_id, refers_to_text)` per entry; `local_sheet_id`
/// is the 0-based sheet index for sheet-scoped names.
/// Every attribute of `<sheetProtection>`, verbatim. The element mixes
/// permission flags with a password hash, salt and algorithm; inventing or
/// dropping either is worse than not interpreting them, so nothing here is
/// interpreted at all.
fn read_protection(e: &BytesStart<'_>) -> Result<BTreeMap<String, String>, ImportError> {
    read_attrs(e)
}

/// Every attribute of an element, by local name, in a deterministic order.
fn read_attrs(e: &BytesStart<'_>) -> Result<BTreeMap<String, String>, ImportError> {
    let mut attrs = BTreeMap::new();
    for a in e.attributes() {
        let a = a.map_err(|err| xml_err(quick_xml::Error::from(err)))?;
        // Namespace declarations are packaging, not data. Keeping them would
        // also collapse `xmlns:r` to the local name `r`, colliding with a real
        // attribute, and re-emit an `xmlns` the writer already declares.
        if a.key.as_ref().starts_with(b"xmlns") {
            continue;
        }
        let key = String::from_utf8_lossy(a.key.local_name().as_ref()).into_owned();
        let value = a.unescape_value().map_err(xml_err)?.into_owned();
        attrs.insert(key, value);
    }
    Ok(attrs)
}

/// Whether `workbook.xml` declares the 1904 date system.
pub fn parse_date1904(xml: &[u8]) -> Result<bool, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut bounds = Bounds::new();
    loop {
        match reader.read_event_into(&mut buf).map_err(xml_err)? {
            Event::Start(e) | Event::Empty(e) => {
                bounds.count()?;
                if e.local_name().as_ref() == b"workbookPr" {
                    return Ok(read_bool_attr(&e, b"date1904")?.unwrap_or(false));
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(false)
}

/// Read the workbook-level settings that travel verbatim.
///
/// `date1904` is deliberately *not* removed from `workbookPr`: it is interpreted
/// into `Workbook::date1904` for display, and also written back from here, so
/// the two agree. Splitting responsibility for one attribute is how it ends up
/// written twice or not at all.
pub fn parse_workbook_settings(xml: &[u8]) -> Result<WorkbookSettings, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut bounds = Bounds::new();
    let mut out = WorkbookSettings::default();
    loop {
        match reader.read_event_into(&mut buf).map_err(xml_err)? {
            Event::Start(e) | Event::Empty(e) => {
                bounds.count()?;
                match e.local_name().as_ref() {
                    b"calcPr" => out.calc = read_attrs(&e)?,
                    b"fileVersion" => out.file_version = read_attrs(&e)?,
                    b"workbookPr" => out.workbook_pr = read_attrs(&e)?,
                    b"workbookProtection" => out.protection = read_attrs(&e)?,
                    b"fileSharing" => out.file_sharing = read_attrs(&e)?,
                    b"workbookView" => out.views.push(read_attrs(&e)?),
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

/// Elements in `workbook.xml` that name a retained part by `r:id`, as
/// `(element name, attributes)` in document order.
///
/// The part alone is not enough: `<externalReference r:id="rId4"/>` is what ties
/// the workbook to its external-link cache, and a retained part nothing refers
/// to is invisible to Excel.
pub fn parse_retained_refs(xml: &[u8]) -> Result<Vec<RetainedRef>, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut bounds = Bounds::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf).map_err(xml_err)? {
            Event::Start(e) | Event::Empty(e) => {
                bounds.count()?;
                match e.local_name().as_ref() {
                    b"externalReference" => {
                        out.push(("externalReference".to_owned(), read_attrs(&e)?));
                    }
                    // `<pivotCache cacheId r:id>` is what ties a pivot table's
                    // `cacheId` to the cache part. Dropping it left the parts
                    // and the relationship in the package with nothing
                    // declaring them, and Excel reports that as a file needing
                    // repair rather than as a missing pivot.
                    b"pivotCache" => {
                        out.push(("pivotCache".to_owned(), read_attrs(&e)?));
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

pub fn parse_defined_names(xml: &[u8]) -> Result<Vec<(String, Option<u32>, String)>, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut bounds = Bounds::new();

    let mut names = Vec::new();
    let mut current: Option<(String, Option<u32>, String)> = None;

    loop {
        match reader.read_event_into(&mut buf).map_err(xml_err)? {
            Event::Start(e) => {
                bounds.open()?;
                if e.local_name().as_ref() == b"definedName" {
                    let name = read_attr(&e, b"name")?.unwrap_or_default();
                    let local = read_attr(&e, b"localSheetId")?.and_then(|s| s.parse().ok());
                    current = Some((name, local, String::new()));
                }
            }
            Event::Text(e) => {
                if let Some((_, _, text)) = current.as_mut() {
                    text.push_str(&e.unescape().map_err(xml_err)?);
                }
            }
            Event::End(e) => {
                bounds.close();
                if e.local_name().as_ref() == b"definedName"
                    && let Some(entry) = current.take()
                {
                    names.push(entry);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(names)
}

/// Parse an `xl/tables/table{n}.xml` part.
pub fn parse_table(xml: &[u8]) -> Result<Option<RawTable>, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut bounds = Bounds::new();
    let mut table: Option<RawTable> = None;
    let mut column: Option<TableColumn> = None;
    let mut cur_fc: Option<RawFilterColumn> = None;
    let mut in_text: Option<&'static str> = None;
    let mut text = String::new();

    loop {
        let event = reader.read_event_into(&mut buf).map_err(xml_err)?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                if matches!(event, Event::Start(_)) {
                    bounds.open()?;
                } else {
                    bounds.count()?;
                }
                match e.local_name().as_ref() {
                    b"table" => {
                        table = Some(RawTable {
                            attrs: read_attrs(e)?,
                            ..Default::default()
                        })
                    }
                    n if table.is_some()
                        && read_filter_element(
                            e,
                            n,
                            &mut table.as_mut().expect("checked").auto_filter_ref,
                            &mut cur_fc,
                        )? => {}
                    b"tableStyleInfo" => {
                        if let Some(t) = table.as_mut() {
                            t.style = read_attrs(e)?;
                        }
                    }
                    b"tableColumn" => {
                        let attrs = read_attrs(e)?;
                        column = Some(TableColumn {
                            id: attrs.get("id").and_then(|v| v.parse().ok()).unwrap_or(0),
                            name: attrs.get("name").cloned().unwrap_or_default(),
                            totals_row_function: attrs.get("totalsRowFunction").cloned(),
                            totals_row_label: attrs.get("totalsRowLabel").cloned(),
                            calculated_column_formula: None,
                            totals_row_formula: None,
                        });
                    }
                    // Both are element *text*, not attributes: a calculated
                    // column's formula is the body of the element.
                    b"calculatedColumnFormula" => {
                        in_text = Some("calc");
                        text.clear();
                    }
                    b"totalsRowFormula" => {
                        in_text = Some("totals");
                        text.clear();
                    }
                    _ => {}
                }
                // A self-closed <tableColumn/> is complete right here.
                if matches!(event, Event::Empty(_))
                    && e.local_name().as_ref() == b"tableColumn"
                    && let (Some(t), Some(c)) = (table.as_mut(), column.take())
                {
                    t.columns.push(c);
                }
            }
            Event::Text(ref e) => {
                if in_text.is_some() {
                    text.push_str(&e.unescape().map_err(xml_err)?);
                }
            }
            Event::End(ref e) => {
                bounds.close();
                match e.local_name().as_ref() {
                    b"filterColumn" => {
                        if let (Some(t), Some(fc)) = (table.as_mut(), cur_fc.take()) {
                            t.filter_columns.push(fc);
                        }
                    }
                    b"calculatedColumnFormula" | b"totalsRowFormula" => {
                        if let Some(c) = column.as_mut() {
                            match in_text {
                                Some("calc") => {
                                    c.calculated_column_formula = Some(std::mem::take(&mut text))
                                }
                                Some("totals") => {
                                    c.totals_row_formula = Some(std::mem::take(&mut text))
                                }
                                _ => {}
                            }
                        }
                        in_text = None;
                    }
                    b"tableColumn" => {
                        if let (Some(t), Some(c)) = (table.as_mut(), column.take()) {
                            t.columns.push(c);
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(table)
}

/// A `<table>` part before its `ref` is resolved to a range.
#[derive(Debug, Default, Clone)]
pub struct RawTable {
    /// Every `<table>` attribute as read.
    pub attrs: BTreeMap<String, String>,
    /// The columns, left to right.
    pub columns: Vec<TableColumn>,
    /// `<autoFilter ref>`, if present.
    pub auto_filter_ref: Option<String>,
    /// Per-column filter rules inside that `<autoFilter>`.
    pub filter_columns: Vec<RawFilterColumn>,
    /// `<tableStyleInfo>` attributes.
    pub style: BTreeMap<String, String>,
}

/// The `r:id`s of a worksheet's `<tablePart>` children, in document order.
pub fn parse_table_parts(xml: &[u8]) -> Result<Vec<String>, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut bounds = Bounds::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf).map_err(xml_err)? {
            Event::Start(e) | Event::Empty(e) => {
                bounds.count()?;
                if e.local_name().as_ref() == b"tablePart"
                    && let Some(id) = read_attr(&e, b"id")?
                {
                    out.push(id);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}
