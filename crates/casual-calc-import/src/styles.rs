//! Parse `xl/styles.xml`: number formats, fonts, fills, and the `cellXfs`
//! records cells reference by index. Produces a resolved [`Style`] per `xf`.

use std::collections::HashMap;

use casual_calc_model::{BorderEdge, Borders, HAlign, Style, VAlign};
use casual_calc_ooxml::OoxmlError;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::ImportError;

/// The resolved styles, one per `cellXfs` entry (indexed by a cell's `s`).
#[derive(Debug, Default)]
pub struct StyleSheet {
    /// One `Style` per `xf` in `cellXfs`, in order.
    pub xf_styles: Vec<Style>,
    /// Differential-format fill color (`RRGGBB`) per `<dxf>`, by dxfId — used by
    /// conditional formatting. `None` if the dxf carries no solid fill.
    pub dxf_fills: Vec<Option<String>>,
}

#[derive(Debug, Default, Clone)]
struct Font {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    color: Option<String>,
    name: Option<String>,
    /// Font size in half-points (`sz val` rounded to the nearest half-point).
    size_hp: Option<u32>,
}

#[derive(Debug, Default, Clone)]
struct FillInfo {
    solid: bool,
    color: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct Xf {
    num_fmt_id: u32,
    font_id: usize,
    fill_id: usize,
    border_id: usize,
    align: Option<HAlign>,
    valign: Option<VAlign>,
    wrap: bool,
    indent: u8,
}

/// The border edge currently being parsed, so a nested `<color>` attaches to it.
#[derive(Clone, Copy)]
enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

fn edge_field(borders: &mut Borders, edge: Edge) -> &mut Option<BorderEdge> {
    match edge {
        Edge::Left => &mut borders.left,
        Edge::Right => &mut borders.right,
        Edge::Top => &mut borders.top,
        Edge::Bottom => &mut borders.bottom,
    }
}

fn xml_err(err: quick_xml::Error) -> ImportError {
    ImportError::Ooxml(OoxmlError::MalformedXml(err.to_string()))
}

fn attr(e: &BytesStart<'_>, local: &[u8]) -> Result<Option<String>, ImportError> {
    for a in e.attributes() {
        let a = a.map_err(|err| xml_err(err.into()))?;
        if a.key.local_name().as_ref() == local {
            return Ok(Some(a.unescape_value().map_err(xml_err)?.into_owned()));
        }
    }
    Ok(None)
}

fn attr_u32(e: &BytesStart<'_>, local: &[u8]) -> Result<Option<u32>, ImportError> {
    Ok(attr(e, local)?.and_then(|s| s.parse().ok()))
}

/// Normalize an OOXML `rgb` color (`FFRRGGBB` or `RRGGBB`) to `RRGGBB`.
fn rgb(e: &BytesStart<'_>) -> Result<Option<String>, ImportError> {
    Ok(attr(e, b"rgb")?.map(|s| if s.len() == 8 { s[2..].to_owned() } else { s }))
}

/// Parse a `styles.xml` part into the resolved per-`xf` styles.
pub fn parse_styles(xml: &[u8]) -> Result<StyleSheet, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();

    let mut custom_formats: HashMap<u32, String> = HashMap::new();
    let mut fonts: Vec<Font> = Vec::new();
    let mut fills: Vec<FillInfo> = Vec::new();
    let mut borders: Vec<Borders> = Vec::new();
    let mut xfs: Vec<Xf> = Vec::new();

    let (mut in_fonts, mut in_fills, mut in_cellxfs) = (false, false, false);
    let mut in_borders = false;
    let mut in_dxfs = false;
    let mut dxfs: Vec<Option<String>> = Vec::new();
    let mut cur_edge: Option<Edge> = None;
    let mut depth = 0usize;
    let mut elements = 0usize;

    loop {
        let event = reader.read_event_into(&mut buf).map_err(xml_err)?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
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
                        if let (Some(id), Some(code)) =
                            (attr_u32(e, b"numFmtId")?, attr(e, b"formatCode")?)
                        {
                            custom_formats.insert(id, code);
                        }
                    }
                    b"fonts" => in_fonts = true,
                    b"fills" => in_fills = true,
                    b"borders" => in_borders = true,
                    b"cellXfs" => in_cellxfs = true,
                    b"dxfs" => in_dxfs = true,
                    b"dxf" if in_dxfs => dxfs.push(None),
                    b"bgColor" if in_dxfs => {
                        if let Some(rgb) = attr(e, b"rgb")? {
                            let hex = rgb.trim();
                            if hex.len() >= 6
                                && hex.bytes().all(|b| b.is_ascii_hexdigit())
                                && let Some(last) = dxfs.last_mut()
                            {
                                *last = Some(hex[hex.len() - 6..].to_ascii_uppercase());
                            }
                        }
                    }
                    b"border" if in_borders => borders.push(Borders::default()),
                    b"left" | b"right" | b"top" | b"bottom" if in_borders => {
                        let edge = match e.local_name().as_ref() {
                            b"left" => Edge::Left,
                            b"right" => Edge::Right,
                            b"top" => Edge::Top,
                            _ => Edge::Bottom,
                        };
                        cur_edge = Some(edge);
                        // A `style` attribute (other than "none") means a line.
                        if let Some(style) = attr(e, b"style")?
                            && style != "none"
                            && let Some(border) = borders.last_mut()
                        {
                            *edge_field(border, edge) = Some(BorderEdge { style, color: None });
                        }
                    }
                    b"color" if in_borders => {
                        if let (Some(edge), Some(c)) = (cur_edge, rgb(e)?)
                            && let Some(border) = borders.last_mut()
                            && let Some(be) = edge_field(border, edge).as_mut()
                        {
                            be.color = Some(c);
                        }
                    }
                    b"font" if in_fonts => fonts.push(Font::default()),
                    b"b" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            f.bold = true;
                        }
                    }
                    b"i" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            f.italic = true;
                        }
                    }
                    b"u" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            f.underline = true;
                        }
                    }
                    b"strike" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            f.strike = true;
                        }
                    }
                    b"color" if in_fonts => {
                        if let (Some(f), Some(c)) = (fonts.last_mut(), rgb(e)?) {
                            f.color = Some(c);
                        }
                    }
                    b"name" if in_fonts => {
                        if let (Some(f), Some(n)) = (fonts.last_mut(), attr(e, b"val")?) {
                            f.name = Some(n);
                        }
                    }
                    b"sz" if in_fonts => {
                        if let (Some(f), Some(v)) = (
                            fonts.last_mut(),
                            attr(e, b"val")?.and_then(|s| s.parse::<f64>().ok()),
                        ) {
                            f.size_hp = Some((v * 2.0).round() as u32);
                        }
                    }
                    b"fill" if in_fills => fills.push(FillInfo::default()),
                    b"patternFill" if in_fills => {
                        if let Some(fill) = fills.last_mut() {
                            fill.solid = attr(e, b"patternType")?.as_deref() == Some("solid");
                        }
                    }
                    b"fgColor" if in_fills => {
                        if let (Some(fill), Some(c)) = (fills.last_mut(), rgb(e)?) {
                            fill.color = Some(c);
                        }
                    }
                    b"xf" if in_cellxfs => {
                        xfs.push(Xf {
                            num_fmt_id: attr_u32(e, b"numFmtId")?.unwrap_or(0),
                            font_id: attr_u32(e, b"fontId")?.unwrap_or(0) as usize,
                            fill_id: attr_u32(e, b"fillId")?.unwrap_or(0) as usize,
                            border_id: attr_u32(e, b"borderId")?.unwrap_or(0) as usize,
                            align: None,
                            valign: None,
                            wrap: false,
                            indent: 0,
                        });
                    }
                    b"alignment" if in_cellxfs => {
                        if let Some(xf) = xfs.last_mut() {
                            if let Some(h) = attr(e, b"horizontal")? {
                                xf.align = HAlign::from_ooxml(&h);
                            }
                            if let Some(vtoken) = attr(e, b"vertical")? {
                                xf.valign = VAlign::from_ooxml(&vtoken);
                            }
                            if attr(e, b"wrapText")?.as_deref() == Some("1") {
                                xf.wrap = true;
                            }
                            if let Some(indent) = attr_u32(e, b"indent")? {
                                xf.indent = indent.min(u8::MAX as u32) as u8;
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                match e.local_name().as_ref() {
                    b"fonts" => in_fonts = false,
                    b"fills" => in_fills = false,
                    b"dxfs" => in_dxfs = false,
                    b"borders" => in_borders = false,
                    b"cellXfs" => in_cellxfs = false,
                    b"left" | b"right" | b"top" | b"bottom" if in_borders => cur_edge = None,
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    let xf_styles = xfs
        .into_iter()
        .map(|xf| {
            let font = fonts.get(xf.font_id).cloned().unwrap_or_default();
            let fill = fills.get(xf.fill_id).cloned().unwrap_or_default();
            let border = borders.get(xf.border_id).cloned().unwrap_or_default();
            Style {
                number_format: resolve_format(xf.num_fmt_id, &custom_formats),
                bold: font.bold,
                italic: font.italic,
                underline: font.underline,
                strike: font.strike,
                font_name: font.name,
                font_size_hp: font.size_hp,
                font_color: font.color,
                fill_color: if fill.solid { fill.color } else { None },
                align: xf.align,
                valign: xf.valign,
                wrap: xf.wrap,
                indent: xf.indent,
                border: (!border.is_empty()).then_some(border),
            }
        })
        .collect();

    Ok(StyleSheet {
        xf_styles,
        dxf_fills: dxfs,
    })
}

fn resolve_format(id: u32, custom: &HashMap<u32, String>) -> Option<String> {
    custom
        .get(&id)
        .cloned()
        .or_else(|| builtin_number_format(id).map(str::to_owned))
        .filter(|c| !c.is_empty() && c != "General")
}

/// The code for a built-in `numFmtId` (the common subset of the ECMA-376 table).
fn builtin_number_format(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        5 => "$#,##0_);($#,##0)",
        6 => "$#,##0_);[Red]($#,##0)",
        7 => "$#,##0.00_);($#,##0.00)",
        8 => "$#,##0.00_);[Red]($#,##0.00)",
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
        37 => "#,##0_);(#,##0)",
        38 => "#,##0_);[Red](#,##0)",
        39 => "#,##0.00_);(#,##0.00)",
        40 => "#,##0.00_);[Red](#,##0.00)",
        41 => "_(* #,##0_);_(* (#,##0);_(* \"-\"_);_(@_)",
        42 => "_($* #,##0_);_($* (#,##0);_($* \"-\"_);_(@_)",
        43 => "_(* #,##0.00_);_(* (#,##0.00);_(* \"-\"??_);_(@_)",
        44 => "_($* #,##0.00_);_($* (#,##0.00);_($* \"-\"??_);_(@_)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

#[cfg(test)]
mod builtin_fmt_tests {
    use super::{builtin_number_format, resolve_format};
    use std::collections::HashMap;

    #[test]
    fn currency_accounting_and_exponential_ids_resolve() {
        // Previously-missing builtins that silently reverted to General.
        assert_eq!(builtin_number_format(7), Some("$#,##0.00_);($#,##0.00)"));
        assert_eq!(
            builtin_number_format(44),
            Some("_($* #,##0.00_);_($* (#,##0.00);_($* \"-\"??_);_(@_)")
        );
        assert_eq!(builtin_number_format(37), Some("#,##0_);(#,##0)"));
        assert_eq!(builtin_number_format(48), Some("##0.0E+0"));
        assert_eq!(builtin_number_format(12), Some("# ?/?"));
        // Unknown ids still fall through.
        assert_eq!(builtin_number_format(999), None);
    }

    #[test]
    fn resolve_prefers_custom_then_builtin_and_drops_general() {
        let custom = HashMap::from([(164, "0.000".to_owned())]);
        assert_eq!(resolve_format(164, &custom), Some("0.000".to_owned()));
        assert_eq!(
            resolve_format(44, &custom),
            Some("_($* #,##0.00_);_($* (#,##0.00);_($* \"-\"??_);_(@_)".to_owned())
        );
        // General / empty resolve to None (no explicit format needed).
        assert_eq!(resolve_format(0, &custom), None);
    }
}
