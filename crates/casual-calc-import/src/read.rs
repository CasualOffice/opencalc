//! Streaming parsers for `sharedStrings.xml` and worksheet `sheetData`.

use std::collections::{BTreeMap, BTreeSet};

use casual_calc_model::OutlinePr;
use casual_calc_ooxml::OoxmlError;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::ImportError;

const MAX_XML_ELEMENTS: usize = 50_000_000;
const MAX_XML_DEPTH: usize = 256;

fn xml_err(err: quick_xml::Error) -> ImportError {
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
}

fn read_attr(
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
pub fn parse_shared_strings(xml: &[u8]) -> Result<Vec<String>, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut bounds = Bounds::new();

    let mut strings = Vec::new();
    let mut current: Option<String> = None;
    let mut in_text = false;

    loop {
        match reader.read_event_into(&mut buf).map_err(xml_err)? {
            Event::Start(e) => {
                bounds.open()?;
                match e.local_name().as_ref() {
                    b"si" => current = Some(String::new()),
                    b"t" => in_text = true,
                    _ => {}
                }
            }
            Event::Empty(e) => {
                bounds.count()?;
                if e.local_name().as_ref() == b"si" {
                    strings.push(String::new());
                }
            }
            Event::Text(e) => {
                if in_text && let Some(current) = current.as_mut() {
                    current.push_str(&e.unescape().map_err(xml_err)?);
                }
            }
            Event::End(e) => {
                bounds.close();
                match e.local_name().as_ref() {
                    b"t" => in_text = false,
                    b"si" => {
                        if let Some(text) = current.take() {
                            strings.push(text);
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

/// A parsed worksheet: its cells, merged ranges, frozen panes, and axis sizing.
#[derive(Debug, Default)]
pub struct Worksheet {
    /// The raw cells.
    pub cells: Vec<RawCell>,
    /// Merged-range references (`A1:B2`).
    pub merges: Vec<String>,
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
    /// Outline summary-position flags from `<sheetPr><outlinePr/>`, if present.
    pub outline: Option<OutlinePr>,
    /// View zoom percentage from `<sheetView zoomScale>`, if set and non-default.
    pub zoom: Option<u16>,
    /// Tab color as `RRGGBB` (from `sheetPr/tabColor/@rgb`), if any.
    pub tab_color: Option<String>,
    /// List data-validations as `(sqref, formula1)`, where `formula1` is the
    /// raw inline list text (e.g. `"a,b,c"`) or a range reference (skipped).
    pub validations: Vec<(String, String)>,
}

/// Ceiling on how many columns one `<col>` span may expand into per-line
/// overrides; a wider custom span is treated as the sheet's default width.
const MAX_COL_SPAN: u32 = 4096;

/// Convert an Excel column width (character units) to twips, matching the
/// pixel rounding Excel uses so the value survives a write→read round-trip.
fn col_width_to_twips(chars: f64) -> i64 {
    let px = (chars * 7.0 + 5.0).round();
    (px as i64) * 15
}

/// Convert an Excel row height (points) to twips.
fn row_height_to_twips(points: f64) -> i64 {
    (points * 20.0).round() as i64
}

fn parse_u32_attr(e: &BytesStart<'_>, local: &[u8]) -> Result<u32, ImportError> {
    Ok(read_attr(e, local)?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0))
}

fn read_f64_attr(e: &BytesStart<'_>, local: &[u8]) -> Result<Option<f64>, ImportError> {
    Ok(read_attr(e, local)?.and_then(|s| s.parse().ok()))
}

/// Record a `<col min max width customWidth hidden>` element into the width
/// overrides and the hidden-column set.
fn read_col(e: &BytesStart<'_>, result: &mut Worksheet) -> Result<(), ImportError> {
    let min = parse_u32_attr(e, b"min")?; // 1-based
    let max = parse_u32_attr(e, b"max")?;
    if min == 0 || max < min {
        return Ok(());
    }
    // A hidden span is recorded per zero-based column regardless of width. The
    // same span rules apply to the outline level and collapsed flag, which — like
    // hidden — are meaningful even when the column carries no custom width, so
    // they are parsed before the width-driven early return below.
    let narrow = max.saturating_sub(min) < MAX_COL_SPAN;
    if read_attr(e, b"hidden")?.as_deref() == Some("1") && narrow {
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
    if read_attr(e, b"collapsed")?.as_deref() == Some("1") && narrow {
        for col in min..=max {
            result.collapsed_cols.insert(col - 1);
        }
    }
    let Some(width) = read_f64_attr(e, b"width")? else {
        return Ok(());
    };
    let twips = col_width_to_twips(width);
    let custom = read_attr(e, b"customWidth")?.as_deref() == Some("1");
    // A wide span without an explicit custom flag is the sheet's default width,
    // not a per-column override — record it as the default to stay compact.
    if !custom || max.saturating_sub(min) >= MAX_COL_SPAN {
        result.col_default.get_or_insert(twips);
        return Ok(());
    }
    for col in min..=max {
        result.col_sizes.insert(col - 1, twips);
    }
    Ok(())
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
    if let Some(ht) = read_f64_attr(e, b"ht")? {
        result.row_sizes.insert(r - 1, row_height_to_twips(ht));
    }
    if read_attr(e, b"hidden")?.as_deref() == Some("1") {
        result.hidden_rows.insert(r - 1);
    }
    if let Some(level) = read_attr(e, b"outlineLevel")?.and_then(|s| s.parse::<u8>().ok())
        && level != 0
    {
        result.row_outline_levels.insert(r - 1, level);
    }
    if read_attr(e, b"collapsed")?.as_deref() == Some("1") {
        result.collapsed_rows.insert(r - 1);
    }
    Ok(())
}

/// Record `<sheetFormatPr>` axis defaults.
fn read_sheet_format(e: &BytesStart<'_>, result: &mut Worksheet) -> Result<(), ImportError> {
    if let Some(w) = read_f64_attr(e, b"defaultColWidth")? {
        result.col_default = Some(col_width_to_twips(w));
    }
    if let Some(h) = read_f64_attr(e, b"defaultRowHeight")? {
        result.row_default = Some(row_height_to_twips(h));
    }
    Ok(())
}

/// Parse a worksheet part's `sheetData`, `mergeCells`, and `sheetView` pane.
pub fn parse_worksheet(xml: &[u8]) -> Result<Worksheet, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut bounds = Bounds::new();

    let mut result = Worksheet::default();
    let mut current: Option<RawCell> = None;
    let mut in_value = false;
    let mut in_formula = false;
    let mut in_inline = false;
    let mut in_inline_text = false;
    // Active `<dataValidation>` being parsed: (sqref, accumulated formula1).
    let mut dv: Option<(String, String)> = None;
    let mut in_dv_formula1 = false;

    loop {
        match reader.read_event_into(&mut buf).map_err(xml_err)? {
            Event::Start(e) => {
                bounds.open()?;
                match e.local_name().as_ref() {
                    b"c" => {
                        current = Some(RawCell {
                            reference: read_attr(&e, b"r")?.unwrap_or_default(),
                            cell_type: read_attr(&e, b"t")?,
                            style_index: read_attr(&e, b"s")?.and_then(|s| s.parse().ok()),
                            ..RawCell::default()
                        });
                    }
                    b"v" => in_value = true,
                    b"f" => {
                        in_formula = true;
                        if let Some(cell) = current.as_mut() {
                            cell.formula.get_or_insert_with(String::new);
                        }
                    }
                    b"is" => in_inline = true,
                    b"t" if in_inline => in_inline_text = true,
                    b"mergeCell" => {
                        if let Some(reference) = read_attr(&e, b"ref")? {
                            result.merges.push(reference);
                        }
                    }
                    b"pane" => read_pane(&e, &mut result)?,
                    b"sheetView" => read_sheet_view(&e, &mut result)?,
                    b"row" => read_row(&e, &mut result)?,
                    b"col" => read_col(&e, &mut result)?,
                    b"sheetFormatPr" => read_sheet_format(&e, &mut result)?,
                    b"outlinePr" => read_outline_pr(&e, &mut result)?,
                    b"tabColor" => read_tab_color(&e, &mut result)?,
                    b"dataValidation" => {
                        // Only explicit-list dropdowns are modeled.
                        if read_attr(&e, b"type")?.as_deref() == Some("list") {
                            let sqref = read_attr(&e, b"sqref")?.unwrap_or_default();
                            dv = Some((sqref, String::new()));
                        }
                    }
                    b"formula1" if dv.is_some() => in_dv_formula1 = true,
                    _ => {}
                }
            }
            Event::Empty(e) => {
                bounds.count()?;
                match e.local_name().as_ref() {
                    b"c" => {
                        result.cells.push(RawCell {
                            reference: read_attr(&e, b"r")?.unwrap_or_default(),
                            cell_type: read_attr(&e, b"t")?,
                            style_index: read_attr(&e, b"s")?.and_then(|s| s.parse().ok()),
                            ..RawCell::default()
                        });
                    }
                    b"f" => {
                        if let Some(cell) = current.as_mut() {
                            cell.formula.get_or_insert_with(String::new);
                        }
                    }
                    b"mergeCell" => {
                        if let Some(reference) = read_attr(&e, b"ref")? {
                            result.merges.push(reference);
                        }
                    }
                    b"pane" => read_pane(&e, &mut result)?,
                    b"sheetView" => read_sheet_view(&e, &mut result)?,
                    b"row" => read_row(&e, &mut result)?,
                    b"col" => read_col(&e, &mut result)?,
                    b"sheetFormatPr" => read_sheet_format(&e, &mut result)?,
                    b"outlinePr" => read_outline_pr(&e, &mut result)?,
                    b"tabColor" => read_tab_color(&e, &mut result)?,
                    _ => {}
                }
            }
            Event::Text(e) => {
                if in_dv_formula1 && let Some((_, f1)) = dv.as_mut() {
                    f1.push_str(&e.unescape().map_err(xml_err)?);
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
                match e.local_name().as_ref() {
                    b"v" => in_value = false,
                    b"f" => in_formula = false,
                    b"formula1" => in_dv_formula1 = false,
                    b"dataValidation" => {
                        if let Some((sqref, f1)) = dv.take() {
                            result.validations.push((sqref, f1));
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
    if let Some(zoom) = read_attr(e, b"zoomScale")?.and_then(|s| s.parse::<u16>().ok())
        && zoom != 0
        && zoom != 100
    {
        result.zoom = Some(zoom);
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
fn read_tab_color(e: &BytesStart<'_>, result: &mut Worksheet) -> Result<(), ImportError> {
    if let Some(rgb) = read_attr(e, b"rgb")? {
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
