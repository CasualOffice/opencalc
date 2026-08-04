//! Streaming parsers for `sharedStrings.xml` and worksheet `sheetData`.

use casual_calc_ooxml::OoxmlError;
use quick_xml::Reader;
use quick_xml::events::Event;

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

/// Parse a worksheet part's `sheetData` into raw cells.
pub fn parse_worksheet(xml: &[u8]) -> Result<Vec<RawCell>, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut bounds = Bounds::new();

    let mut cells = Vec::new();
    let mut current: Option<RawCell> = None;
    let mut in_value = false;
    let mut in_formula = false;
    let mut in_inline = false;
    let mut in_inline_text = false;

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
                    _ => {}
                }
            }
            Event::Empty(e) => {
                bounds.count()?;
                match e.local_name().as_ref() {
                    b"c" => {
                        cells.push(RawCell {
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
                    _ => {}
                }
            }
            Event::Text(e) => {
                if let Some(cell) = current.as_mut() {
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
                    b"t" if in_inline => in_inline_text = false,
                    b"is" => in_inline = false,
                    b"c" => {
                        if let Some(cell) = current.take() {
                            cells.push(cell);
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
    Ok(cells)
}
