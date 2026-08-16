//! Parse `xl/styles.xml`: number formats, fonts, fills, and the `cellXfs`
//! records cells reference by index. Produces a resolved [`Style`] per `xf`.

use std::collections::HashMap;

use casual_calc_model::{
    BorderEdge, Borders, GradientFill, GradientStop, HAlign, NamedCellStyle, Style, ThemeTint,
    Underline, VAlign, VertAlign, to_micro,
};
use casual_calc_ooxml::OoxmlError;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::ImportError;
use crate::theme::{ThemePalette, indexed_color};

/// The resolved styles, one per `cellXfs` entry (indexed by a cell's `s`).
#[derive(Debug, Default)]
pub struct StyleSheet {
    /// One `Style` per `xf` in `cellXfs`, in order.
    pub xf_styles: Vec<Style>,
    /// `xf/@xfId` per `cellXfs` entry, in the same order — the link to a named
    /// style, kept separate because it is an association, not formatting.
    pub xf_style_refs: Vec<Option<u32>>,
    /// Named cell styles in `cellStyleXfs` order, resolved from `<cellStyles>`.
    pub cell_styles: Vec<NamedCellStyle>,
    /// Differential-format fill color (`RRGGBB`) per `<dxf>`, by dxfId — used by
    /// conditional formatting. `None` if the dxf carries no solid fill.
    pub dxf_fills: Vec<Option<String>>,
    /// The workbook default font (name + half-point size): `<fonts>` entry 0,
    /// which OOXML treats as the default. Shown for cells with no explicit font.
    pub default_font_name: Option<String>,
    pub default_font_size_hp: Option<u32>,
}

#[derive(Debug, Default, Clone)]
struct Font {
    bold: bool,
    italic: bool,
    underline: Option<Underline>,
    outline: bool,
    shadow: bool,
    condense: bool,
    extend: bool,
    vert_align: Option<VertAlign>,
    family: Option<u32>,
    scheme: Option<String>,
    charset: Option<u32>,
    strike: bool,
    color: Option<String>,
    /// The theme slot `color` was resolved from, when it was a theme reference.
    color_theme: Option<ThemeTint>,
    name: Option<String>,
    /// Font size in half-points (`sz val` rounded to the nearest half-point).
    size_hp: Option<u32>,
}

#[derive(Debug, Default, Clone)]
struct FillInfo {
    solid: bool,
    /// The raw `patternType`, kept so a non-solid pattern survives.
    pattern: Option<String>,
    color: Option<String>,
    /// The theme slot `color` was resolved from, when it was a theme reference.
    color_theme: Option<ThemeTint>,
    bg_color: Option<String>,
    bg_theme: Option<ThemeTint>,
    gradient: Option<GradientFill>,
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
    shrink_to_fit: bool,
    reading_order: Option<u8>,
    justify_last_line: bool,
    relative_indent: Option<i16>,
    rotation: u16,
    indent: u8,
    locked: Option<bool>,
    formula_hidden: Option<bool>,
    /// `xf/@quotePrefix` — the value was typed with a leading apostrophe.
    quote_prefix: bool,
    /// `xf/@xfId` — which `cellStyleXfs` entry (and so which named style) this
    /// cell format belongs to. Only meaningful on a `cellXfs` entry.
    xf_id: Option<u32>,
}

/// The border edge currently being parsed, so a nested `<color>` attaches to it.
#[derive(Clone, Copy)]
enum Edge {
    /// The single `<diagonal>` element; which way it runs comes from the
    /// `diagonalUp`/`diagonalDown` flags on `<border>` itself.
    Diagonal,
    Left,
    Right,
    Top,
    Bottom,
    /// The inside rules of a border applied across a range.
    InsideHorizontal,
    /// See [`Edge::InsideHorizontal`].
    InsideVertical,
}

fn edge_field(borders: &mut Borders, edge: Edge) -> &mut Option<BorderEdge> {
    match edge {
        Edge::Left => &mut borders.left,
        Edge::Right => &mut borders.right,
        Edge::Top => &mut borders.top,
        Edge::Bottom => &mut borders.bottom,
        Edge::Diagonal => &mut borders.diagonal,
        Edge::InsideHorizontal => &mut borders.inside_horizontal,
        Edge::InsideVertical => &mut borders.inside_vertical,
    }
}

fn xml_err(err: quick_xml::Error) -> ImportError {
    ImportError::Ooxml(OoxmlError::MalformedXml(err.to_string()))
}

fn attr(e: &BytesStart<'_>, local: &[u8]) -> Result<Option<String>, ImportError> {
    for a in e.attributes() {
        let a = a.map_err(|err| xml_err(err.into()))?;
        if a.key.local_name().as_ref() == local {
            return Ok(Some(
                a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .map_err(xml_err)?
                    .into_owned(),
            ));
        }
    }
    Ok(None)
}

fn attr_u32(e: &BytesStart<'_>, local: &[u8]) -> Result<Option<u32>, ImportError> {
    Ok(attr(e, local)?.and_then(|s| s.parse().ok()))
}

/// An OOXML font toggle (`<b/>`, `<i/>`, `<strike/>`): present means *on*, but
/// a writer may state it explicitly, and `val="0"` / `val="false"` means off.
/// Treating the element's mere presence as on turned every explicitly-disabled
/// property back on.
fn toggle_on(e: &BytesStart<'_>) -> Result<bool, ImportError> {
    Ok(attr(e, b"val")?.is_none_or(|v| v == "1" || v.eq_ignore_ascii_case("true")))
}

/// An `xsd:double` attribute as integer millionths, `0` when absent.
fn micro_attr(e: &BytesStart<'_>, name: &[u8]) -> Result<i32, ImportError> {
    Ok(attr(e, name)?
        .and_then(|v| v.parse::<f64>().ok())
        .map(to_micro)
        .unwrap_or(0))
}

/// The largest font Excel will accept, in half-points: 409 points.
const MAX_FONT_HALF_POINTS: u32 = 818;

/// A `<sz val>` as half-points, or `None` when it is not a usable size.
///
/// `val` is an arbitrary `xsd:double` off the wire, and `(v * 2.0).round() as u32`
/// took it at its word: `as` **saturates** rather than wrapping or failing, so
/// `<sz val="1e300"/>` produced `u32::MAX` half-points — about two billion
/// points of font — and `NaN` produced 0. Neither is a size, and both travel: a
/// font size reaches row autofit, the layout's text measurement and the
/// rasteriser, none of which are prepared to be asked for a glyph two billion
/// points tall.
///
/// Bounded here rather than at each of those, and to Excel's own ceiling rather
/// than to whatever the arithmetic happens to survive, so the value the model
/// holds is one a real producer could have written. A size outside the range is
/// dropped rather than clamped: `None` means "this font states no size", which
/// every caller already handles by falling back to the workbook default — and a
/// silently *resized* font is a worse answer than an unstyled one.
fn font_size_half_points(points: f64) -> Option<u32> {
    if !points.is_finite() || points <= 0.0 {
        return None;
    }
    let half = (points * 2.0).round();
    if half < 1.0 || half > f64::from(MAX_FONT_HALF_POINTS) {
        return None;
    }
    // Finite, positive and inside the ceiling, so the cast cannot saturate.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(half as u32)
}

/// Resolve an OOXML color element to `RRGGBB`, whichever of the three forms it
/// uses: a literal `rgb` (`FFRRGGBB` or `RRGGBB`), a `theme` slot with an
/// optional `tint`, or a legacy `indexed` palette entry. Reading only `rgb` —
/// as this used to — dropped every color Excel's built-in cell styles use,
/// since those are all theme references.
fn color(e: &BytesStart<'_>, theme: &ThemePalette) -> Result<Option<String>, ImportError> {
    Ok(color_ref(e, theme)?.map(|(rgb, _)| rgb))
}

/// As [`color`], but also reporting the theme slot when the colour came from
/// one. The slot is what makes a cell follow the workbook when it is re-themed;
/// resolving to `RRGGBB` and discarding it freezes the cell at today's palette.
fn color_ref(
    e: &BytesStart<'_>,
    theme: &ThemePalette,
) -> Result<Option<(String, Option<ThemeTint>)>, ImportError> {
    if let Some(s) = attr(e, b"rgb")? {
        // `AARRGGBB` drops the alpha pair. Counted in **characters**, and only
        // when they are all hex, because `s` is whatever the file said and
        // `s[2..]` on eight arbitrary *bytes* panics the moment one of them is
        // the middle of a multi-byte character — `<color rgb="aé12345"/>` is
        // eight bytes with `é` spanning 1..3. That is not a caught error: a
        // panic in this crate reaches the browser as a WebAssembly trap, which
        // aborts the module and takes any workbook already open with it, while
        // the editor's `catch` prints a friendly "could not open".
        //
        // A value that is not hex at all is kept verbatim rather than rejected
        // — docs/34 says preserve what we do not model — and every consumer
        // that turns a colour into output validates it there.
        let hex = s.len() == s.chars().count() && s.chars().all(|c| c.is_ascii_hexdigit());
        let rgb = if hex && s.len() == 8 {
            s[2..].to_owned()
        } else {
            s
        };
        return Ok(Some((rgb, None)));
    }
    if let Some(slot) = attr_u32(e, b"theme")? {
        let tint = attr(e, b"tint")?
            .and_then(|t| t.parse::<f64>().ok())
            .unwrap_or(0.0);
        return Ok(theme
            .resolve(slot as usize, tint)
            .map(|rgb| (rgb, Some(ThemeTint::from_tint(slot, tint)))));
    }
    if let Some(index) = attr_u32(e, b"indexed")? {
        return Ok(indexed_color(index as usize).map(|rgb| (rgb, None)));
    }
    Ok(None)
}

/// Parse a `styles.xml` part into the resolved per-`xf` styles, resolving theme
/// references against the workbook's palette.
pub fn parse_styles(xml: &[u8], theme: &ThemePalette) -> Result<StyleSheet, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();

    let mut custom_formats: HashMap<u32, String> = HashMap::new();
    let mut fonts: Vec<Font> = Vec::new();
    let mut fills: Vec<FillInfo> = Vec::new();
    let mut borders: Vec<Borders> = Vec::new();
    let mut xfs: Vec<Xf> = Vec::new();

    let (mut in_fonts, mut in_fills, mut in_cellxfs) = (false, false, false);
    // `cellStyleXfs` holds the *named* styles' formats; `cellStyles` names them
    // and points at those entries. Both were previously ignored, so a file's
    // Good/Bad/Heading styles were dropped and every cell was written back
    // pointing at xfId 0.
    let mut in_style_xfs = false;
    let mut style_xfs: Vec<Xf> = Vec::new();
    let mut style_names: Vec<(String, u32, Option<u32>)> = Vec::new();
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
                    b"cellStyleXfs" => in_style_xfs = true,
                    b"dxfs" => in_dxfs = true,
                    b"dxf" if in_dxfs => dxfs.push(None),
                    b"bgColor" if in_dxfs => {
                        if let Some(hex) = color(e, theme)? {
                            let hex = hex.trim();
                            if hex.len() >= 6
                                && hex.bytes().all(|b| b.is_ascii_hexdigit())
                                && let Some(last) = dxfs.last_mut()
                            {
                                *last = Some(hex[hex.len() - 6..].to_ascii_uppercase());
                            }
                        }
                    }
                    b"border" if in_borders => {
                        // The two diagonal directions are attributes of the
                        // border itself, not of the `<diagonal>` element.
                        borders.push(Borders {
                            diagonal_up: attr(e, b"diagonalUp")?
                                .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")),
                            diagonal_down: attr(e, b"diagonalDown")?
                                .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")),
                            ..Borders::default()
                        });
                    }
                    b"left" | b"right" | b"top" | b"bottom" | b"diagonal" | b"horizontal"
                    | b"vertical"
                        if in_borders =>
                    {
                        // `horizontal` and `vertical` inside `<border>` are the
                        // *inside* rules, not alignment — the format reuses two
                        // names that mean something else on `<alignment>`.
                        let edge = match e.local_name().as_ref() {
                            b"left" => Edge::Left,
                            b"right" => Edge::Right,
                            b"top" => Edge::Top,
                            b"diagonal" => Edge::Diagonal,
                            b"horizontal" => Edge::InsideHorizontal,
                            b"vertical" => Edge::InsideVertical,
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
                        if let (Some(edge), Some(c)) = (cur_edge, color(e, theme)?)
                            && let Some(border) = borders.last_mut()
                            && let Some(be) = edge_field(border, edge).as_mut()
                        {
                            be.color = Some(c);
                        }
                    }
                    b"font" if in_fonts => fonts.push(Font::default()),
                    b"b" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            f.bold = toggle_on(e)?;
                        }
                    }
                    b"i" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            f.italic = toggle_on(e)?;
                        }
                    }
                    // `<u>` carries a style, not a boolean: absent means single,
                    // and `val="none"` is the one value that means no underline.
                    b"u" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            // `<u/>` with no val is single; `val="none"` is the
                            // one spelling that means not underlined at all.
                            f.underline = match attr(e, b"val")?.as_deref() {
                                Some("none") => None,
                                other => Underline::from_ooxml(other.unwrap_or("")),
                            };
                        }
                    }
                    b"strike" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            f.strike = toggle_on(e)?;
                        }
                    }
                    b"color" if in_fonts => {
                        if let (Some(f), Some((c, slot))) = (fonts.last_mut(), color_ref(e, theme)?)
                        {
                            f.color = Some(c);
                            f.color_theme = slot;
                        }
                    }
                    b"outline" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            f.outline = toggle_on(e)?;
                        }
                    }
                    b"shadow" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            f.shadow = toggle_on(e)?;
                        }
                    }
                    b"condense" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            f.condense = toggle_on(e)?;
                        }
                    }
                    b"extend" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            f.extend = toggle_on(e)?;
                        }
                    }
                    b"vertAlign" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            f.vert_align =
                                VertAlign::from_ooxml(&attr(e, b"val")?.unwrap_or_default());
                        }
                    }
                    b"family" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            f.family = attr(e, b"val")?.and_then(|v| v.parse().ok());
                        }
                    }
                    b"scheme" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            f.scheme = attr(e, b"val")?;
                        }
                    }
                    b"charset" if in_fonts => {
                        if let Some(f) = fonts.last_mut() {
                            f.charset = attr(e, b"val")?.and_then(|v| v.parse().ok());
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
                            f.size_hp = font_size_half_points(v);
                        }
                    }
                    b"fill" if in_fills => fills.push(FillInfo::default()),
                    b"patternFill" if in_fills => {
                        if let Some(fill) = fills.last_mut() {
                            let pattern = attr(e, b"patternType")?;
                            fill.solid = pattern.as_deref() == Some("solid");
                            // `none` is the absence of a fill, not a pattern to
                            // carry; keeping it would make every unfilled cell
                            // carry a style.
                            fill.pattern = pattern.filter(|p| p != "none" && p != "solid");
                        }
                    }
                    b"gradientFill" if in_fills => {
                        if let Some(fill) = fills.last_mut() {
                            fill.gradient = Some(GradientFill {
                                kind: attr(e, b"type")?,
                                degree_micro: micro_attr(e, b"degree")?,
                                left_micro: micro_attr(e, b"left")?,
                                right_micro: micro_attr(e, b"right")?,
                                top_micro: micro_attr(e, b"top")?,
                                bottom_micro: micro_attr(e, b"bottom")?,
                                stops: Vec::new(),
                            });
                        }
                    }
                    b"stop" if in_fills => {
                        if let Some(gradient) = fills.last_mut().and_then(|f| f.gradient.as_mut()) {
                            gradient.stops.push(GradientStop {
                                position_micro: micro_attr(e, b"position")?,
                                color: String::new(),
                                color_theme: None,
                            });
                        }
                    }
                    b"bgColor" if in_fills => {
                        if let (Some(fill), Some((c, slot))) =
                            (fills.last_mut(), color_ref(e, theme)?)
                        {
                            fill.bg_color = Some(c);
                            fill.bg_theme = slot;
                        }
                    }
                    // Inside a `<stop>` the `<color>` belongs to the stop, not
                    // to the font that `b"color"` otherwise handles.
                    b"color" if in_fills => {
                        if let (Some(gradient), Some((c, slot))) = (
                            fills.last_mut().and_then(|f| f.gradient.as_mut()),
                            color_ref(e, theme)?,
                        ) && let Some(stop) = gradient.stops.last_mut()
                        {
                            stop.color = c;
                            stop.color_theme = slot;
                        }
                    }
                    b"fgColor" if in_fills => {
                        if let (Some(fill), Some((c, slot))) =
                            (fills.last_mut(), color_ref(e, theme)?)
                        {
                            fill.color = Some(c);
                            fill.color_theme = slot;
                        }
                    }
                    b"xf" if in_cellxfs || in_style_xfs => {
                        let parsed = Xf {
                            num_fmt_id: attr_u32(e, b"numFmtId")?.unwrap_or(0),
                            font_id: attr_u32(e, b"fontId")?.unwrap_or(0) as usize,
                            fill_id: attr_u32(e, b"fillId")?.unwrap_or(0) as usize,
                            border_id: attr_u32(e, b"borderId")?.unwrap_or(0) as usize,
                            align: None,
                            rotation: 0,
                            valign: None,
                            wrap: false,
                            shrink_to_fit: false,
                            reading_order: None,
                            justify_last_line: false,
                            relative_indent: None,
                            indent: 0,
                            locked: None,
                            formula_hidden: None,
                            quote_prefix: attr(e, b"quotePrefix")?
                                .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")),
                            xf_id: attr_u32(e, b"xfId")?,
                        };
                        if in_cellxfs {
                            xfs.push(parsed)
                        } else {
                            style_xfs.push(parsed)
                        }
                    }
                    b"cellStyle" => {
                        if let Some(name) = attr(e, b"name")? {
                            style_names.push((
                                name,
                                attr_u32(e, b"xfId")?.unwrap_or(0),
                                attr_u32(e, b"builtinId")?,
                            ));
                        }
                    }
                    b"protection" if in_cellxfs || in_style_xfs => {
                        let target = if in_cellxfs {
                            xfs.last_mut()
                        } else {
                            style_xfs.last_mut()
                        };
                        if let Some(xf) = target {
                            let flag = |v: Option<String>| {
                                v.map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                            };
                            xf.locked = flag(attr(e, b"locked")?);
                            xf.formula_hidden = flag(attr(e, b"hidden")?);
                        }
                    }
                    b"alignment" if in_cellxfs || in_style_xfs => {
                        let target = if in_cellxfs {
                            xfs.last_mut()
                        } else {
                            style_xfs.last_mut()
                        };
                        if let Some(xf) = target {
                            if let Some(h) = attr(e, b"horizontal")? {
                                xf.align = HAlign::from_ooxml(&h);
                            }
                            if let Some(vtoken) = attr(e, b"vertical")? {
                                xf.valign = VAlign::from_ooxml(&vtoken);
                            }
                            let flag = |name: &[u8]| -> Result<bool, ImportError> {
                                Ok(attr(e, name)?
                                    .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")))
                            };
                            xf.shrink_to_fit = flag(b"shrinkToFit")?;
                            xf.justify_last_line = flag(b"justifyLastLine")?;
                            xf.reading_order =
                                attr(e, b"readingOrder")?.and_then(|v| v.parse().ok());
                            xf.relative_indent =
                                attr(e, b"relativeIndent")?.and_then(|v| v.parse().ok());
                            // `xsd:boolean` — "true" is as valid as "1".
                            if attr(e, b"wrapText")?
                                .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                            {
                                xf.wrap = true;
                            }
                            if let Some(indent) = attr_u32(e, b"indent")? {
                                xf.indent = indent.min(u8::MAX as u32) as u8;
                            }
                            // OOXML's own encoding: 0-90 CCW, 91-180 clockwise,
                            // 255 = stacked. Kept verbatim (see Style::rotation).
                            if let Some(rot) = attr_u32(e, b"textRotation")? {
                                xf.rotation = rot.min(255) as u16;
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
                    b"cellStyleXfs" => in_style_xfs = false,
                    b"left" | b"right" | b"top" | b"bottom" if in_borders => cur_edge = None,
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    // One conversion, used for both `cellXfs` and `cellStyleXfs` — they are the
    // same element shape and must resolve identically.
    let to_style = |xf: &Xf| {
        let font = fonts.get(xf.font_id).cloned().unwrap_or_default();
        let fill = fills.get(xf.fill_id).cloned().unwrap_or_default();
        let border = borders.get(xf.border_id).cloned().unwrap_or_default();
        Style {
            number_format: resolve_format(xf.num_fmt_id, &custom_formats),
            bold: font.bold,
            italic: font.italic,
            underline: font.underline,
            vert_align: font.vert_align,
            font_family: font.family,
            font_scheme: font.scheme.clone(),
            font_charset: font.charset,
            font_outline: font.outline,
            font_shadow: font.shadow,
            font_condense: font.condense,
            font_extend: font.extend,
            strike: font.strike,
            // No SpreadsheetML attribute maps to clip; Excel always spills.
            clip: false,
            rotation: xf.rotation,
            font_name: font.name,
            font_size_hp: font.size_hp,
            font_color: font.color,
            font_theme: font.color_theme,
            // A pattern's foreground is still its colour, so a non-solid
            // pattern keeps `fill_color`; only an empty fill has none.
            fill_color: if fill.solid || fill.pattern.is_some() {
                fill.color.clone()
            } else {
                None
            },
            fill_theme: if fill.solid || fill.pattern.is_some() {
                fill.color_theme
            } else {
                None
            },
            fill_pattern: fill.pattern.clone(),
            fill_bg_color: fill.bg_color.clone(),
            fill_bg_theme: fill.bg_theme,
            fill_gradient: fill.gradient.clone(),
            align: xf.align,
            valign: xf.valign,
            wrap: xf.wrap,
            shrink_to_fit: xf.shrink_to_fit,
            reading_order: xf.reading_order,
            justify_last_line: xf.justify_last_line,
            relative_indent: xf.relative_indent,
            indent: xf.indent,
            border: (!border.is_empty()).then_some(border),
            // `None` means the xf said nothing, which OOXML defines as locked.
            locked: xf.locked,
            formula_hidden: xf.formula_hidden,
            quote_prefix: xf.quote_prefix,
            // A named style's own entry is the definition, not a reference.
            style_ref: None,
        }
    };
    let xf_styles: Vec<Style> = xfs.iter().map(&to_style).collect();
    let xf_style_refs: Vec<Option<u32>> = xfs.iter().map(|xf| xf.xf_id).collect();
    // Pair each `<cellStyle>` name with the `cellStyleXfs` entry it points at.
    // A name pointing past the end is dropped rather than guessed at.
    let cell_styles: Vec<NamedCellStyle> = style_names
        .into_iter()
        .filter_map(|(name, xf_id, builtin_id)| {
            let xf = style_xfs.get(xf_id as usize)?;
            Some(NamedCellStyle {
                name,
                builtin_id,
                style: to_style(xf),
            })
        })
        .collect();

    // `<fonts>` entry 0 is the workbook default font (referenced by the Normal
    // cell style); surface its name/size so the editor can show the real
    // effective font for cells that carry no explicit one.
    let default_font = fonts.first();
    Ok(StyleSheet {
        xf_styles,
        xf_style_refs,
        cell_styles,
        dxf_fills: dxfs,
        default_font_name: default_font.and_then(|f| f.name.clone()),
        default_font_size_hp: default_font.and_then(|f| f.size_hp),
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
    use super::{ThemePalette, builtin_number_format, parse_styles, resolve_format};
    use std::collections::HashMap;

    #[test]
    fn captures_workbook_default_font_from_fonts_entry_0() {
        let xml =
            br#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
            <fonts count="2">
                <font><sz val="12"/><name val="Aptos"/></font>
                <font><b/><sz val="18"/><name val="Arial"/></font>
            </fonts>
            <cellXfs count="1"><xf numFmtId="0" fontId="0"/></cellXfs>
        </styleSheet>"#;
        let ss = parse_styles(xml, &ThemePalette::default()).expect("parse");
        assert_eq!(ss.default_font_name.as_deref(), Some("Aptos"));
        assert_eq!(ss.default_font_size_hp, Some(24)); // 12pt = 24 half-points
    }

    /// **A colour attribute is eight arbitrary bytes, not eight characters.**
    ///
    /// The `AARRGGBB` form is turned into `RRGGBB` by dropping the first two,
    /// and that was written as `s[2..]` guarded by `s.len() == 8` — a byte
    /// length. `<color rgb="aé12345"/>` is eight bytes with `é` spanning 1..3,
    /// so the slice landed inside a character and panicked.
    ///
    /// This is not an error the caller can handle. Compiled to WebAssembly the
    /// panic is a trap: the module aborts, and any workbook already open in
    /// that tab goes with it while the editor's `catch` prints a friendly
    /// "could not open". On the server it takes the request thread.
    /// **A font size arrives as an arbitrary `xsd:double` and reaches the
    /// rasteriser.**
    ///
    /// `(v * 2.0).round() as u32` took the file at its word, and `as` in Rust
    /// **saturates**: `<sz val="1e300"/>` became `u32::MAX` half-points, about
    /// two billion points of font, and `NaN` became 0. A size travels — row
    /// autofit, text measurement, glyph rasterisation — and none of those is
    /// prepared to be asked for a glyph that tall.
    ///
    /// Out of range is dropped rather than clamped: `None` means the font
    /// states no size, which every caller already handles by falling back to
    /// the workbook default. A silently *resized* font would be a worse answer
    /// than an unstyled one.
    #[test]
    fn an_absurd_font_size_is_refused_rather_than_saturating() {
        let with = |val: &str| {
            let xml = format!(
                r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                    <fonts count="1"><font><sz val="{val}"/></font></fonts>
                    <cellXfs count="1"><xf numFmtId="0" fontId="0" applyFont="1"/></cellXfs>
                </styleSheet>"#
            );
            parse_styles(xml.as_bytes(), &ThemePalette::default())
                .unwrap_or_else(|e| {
                    panic!("sz={val:?} must parse or be refused, never panic: {e:?}")
                })
                .xf_styles[0]
                .font_size_hp
        };

        for absurd in [
            "1e300", "1e308", "-1e300", "NaN", "INF", "-INF", "0", "-12", "1e9",
        ] {
            assert_eq!(
                with(absurd),
                None,
                "sz={absurd:?} is not a font size and must not reach the model"
            );
        }

        // Everything a real producer writes still lands, including the halves
        // that are the reason this is stored in half-points at all.
        assert_eq!(with("11"), Some(22));
        assert_eq!(with("10.5"), Some(21));
        assert_eq!(with("409"), Some(818), "Excel's ceiling is admitted");
        assert_eq!(with("409.5"), None, "and just past it is not");
    }

    #[test]
    fn a_colour_that_is_not_hex_does_not_panic_the_parser() {
        for rgb in [
            "aé12345",  // eight bytes, é spanning 1..3 — the reported case
            "ééé",      // six bytes, three characters
            "日本語",   // nine bytes
            "",         // empty
            "FF0000",   // ordinary RRGGBB, must be untouched
            "FFFF0000", // ordinary AARRGGBB, must lose the alpha pair
            "notahex!", // eight ASCII bytes that are not hex
        ] {
            let xml = format!(
                r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                    <fonts count="1"><font><color rgb="{rgb}"/></font></fonts>
                    <cellXfs count="1"><xf numFmtId="0" fontId="0" applyFont="1"/></cellXfs>
                </styleSheet>"#
            );
            let parsed = parse_styles(xml.as_bytes(), &ThemePalette::default());
            assert!(
                parsed.is_ok(),
                "rgb={rgb:?} must parse or be refused, never panic"
            );
        }

        // And the two well-formed shapes still mean what they meant.
        let with = |rgb: &str| {
            let xml = format!(
                r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                    <fonts count="1"><font><color rgb="{rgb}"/></font></fonts>
                    <cellXfs count="1"><xf numFmtId="0" fontId="0" applyFont="1"/></cellXfs>
                </styleSheet>"#
            );
            let ss = parse_styles(xml.as_bytes(), &ThemePalette::default()).expect("parse");
            ss.xf_styles[0].font_color.clone()
        };
        assert_eq!(with("FFFF0000").as_deref(), Some("FF0000"), "alpha dropped");
        assert_eq!(with("FF0000").as_deref(), Some("FF0000"), "left alone");
    }

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
