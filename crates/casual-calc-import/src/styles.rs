//! Parse `xl/styles.xml`: custom number formats and the `cellXfs` records cells
//! reference by index. Font/fill/border are not yet modeled (a later increment).

use std::collections::HashMap;

use casual_calc_ooxml::OoxmlError;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::ImportError;

/// The number-format code for a `cellXfs` entry (by cell `s` index), if any.
#[derive(Debug, Default)]
pub struct StyleSheet {
    /// One entry per `xf` in `cellXfs`, in order: its resolved number-format code.
    pub xf_number_formats: Vec<Option<String>>,
}

fn xml_err(err: quick_xml::Error) -> ImportError {
    ImportError::Ooxml(OoxmlError::MalformedXml(err.to_string()))
}

fn read_attr(e: &BytesStart<'_>, local: &[u8]) -> Result<Option<String>, ImportError> {
    for attr in e.attributes() {
        let attr = attr.map_err(|err| xml_err(err.into()))?;
        if attr.key.local_name().as_ref() == local {
            return Ok(Some(attr.unescape_value().map_err(xml_err)?.into_owned()));
        }
    }
    Ok(None)
}

/// Parse a `styles.xml` part into the per-`cellXfs` number-format codes.
pub fn parse_styles(xml: &[u8]) -> Result<StyleSheet, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();

    let mut custom_formats: HashMap<u32, String> = HashMap::new();
    let mut xf_number_formats: Vec<Option<String>> = Vec::new();
    let mut in_cell_xfs = false;
    let mut depth = 0usize;
    let mut elements = 0usize;

    loop {
        let event = reader.read_event_into(&mut buf).map_err(xml_err)?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                elements += 1;
                if elements > 5_000_000 {
                    return Err(ImportError::Ooxml(OoxmlError::TooManyElements {
                        limit: 5_000_000,
                    }));
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                    if depth > 256 {
                        return Err(ImportError::Ooxml(OoxmlError::TooDeep { limit: 256 }));
                    }
                }
                match e.local_name().as_ref() {
                    b"numFmt" => {
                        if let (Some(id), Some(code)) = (
                            read_attr(e, b"numFmtId")?.and_then(|s| s.parse::<u32>().ok()),
                            read_attr(e, b"formatCode")?,
                        ) {
                            custom_formats.insert(id, code);
                        }
                    }
                    b"cellXfs" => in_cell_xfs = true,
                    b"xf" if in_cell_xfs => {
                        let num_fmt_id = read_attr(e, b"numFmtId")?
                            .and_then(|s| s.parse::<u32>().ok())
                            .unwrap_or(0);
                        let code = custom_formats
                            .get(&num_fmt_id)
                            .cloned()
                            .or_else(|| builtin_number_format(num_fmt_id).map(str::to_owned));
                        // `General` (id 0, empty code) is treated as no format.
                        let code = code.filter(|c| !c.is_empty() && c != "General");
                        xf_number_formats.push(code);
                    }
                    _ => {}
                }
                if matches!(event, Event::Empty(_)) {
                    // no depth change
                }
            }
            Event::End(ref e) => {
                depth = depth.saturating_sub(1);
                if e.local_name().as_ref() == b"cellXfs" {
                    in_cell_xfs = false;
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(StyleSheet { xf_number_formats })
}

/// The code for a built-in `numFmtId` (the common subset of the ECMA-376 table).
fn builtin_number_format(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "mm-dd-yy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yy h:mm",
        37 => "#,##0 ;(#,##0)",
        38 => "#,##0 ;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}
