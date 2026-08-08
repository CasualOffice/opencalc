//! Theme and indexed colour resolution.
//!
//! A `styles.xml` colour is written three ways: literal `rgb="FFRRGGBB"`, a
//! `theme="N"` slot into the workbook theme (optionally shaded by `tint`), or a
//! legacy `indexed="N"` palette entry. Reading only the literal form drops every
//! colour Excel's built-in cell styles use — they are all theme references — so
//! a themed workbook imports as unstyled.
//!
//! See ECMA-376 §18.3.1.15 (`color`) and §20.1.6.2 (`clrScheme`).

use casual_calc_ooxml::OoxmlError;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::ImportError;

/// Element and nesting ceilings for the theme part, matching the limits the
/// other parts are read under.
const MAX_ELEMENTS: usize = 1_000_000;
const MAX_DEPTH: usize = 256;

/// The workbook's twelve theme colours as `RRGGBB`, in the order a `theme="N"`
/// attribute indexes them.
///
/// That order is **not** the order `<a:clrScheme>` lists them in: the first two
/// pairs are swapped, so slot 0 is the light "background 1" while the scheme
/// element is `<a:dk1>`. Getting this wrong silently turns black text white.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemePalette {
    slots: [String; 12],
}

impl Default for ThemePalette {
    /// The stock Office theme, used when the package has no theme part.
    fn default() -> Self {
        Self {
            slots: [
                "FFFFFF", // 0  background 1 (lt1)
                "000000", // 1  text 1       (dk1)
                "E7E6E6", // 2  background 2 (lt2)
                "44546A", // 3  text 2       (dk2)
                "4472C4", // 4  accent 1
                "ED7D31", // 5  accent 2
                "A5A5A5", // 6  accent 3
                "FFC000", // 7  accent 4
                "5B9BD5", // 8  accent 5
                "70AD47", // 9  accent 6
                "0563C1", // 10 hyperlink
                "954F72", // 11 followed hyperlink
            ]
            .map(str::to_owned),
        }
    }
}

impl ThemePalette {
    /// The colour for a `theme="N"` slot, shaded by `tint` when non-zero.
    #[must_use]
    pub fn resolve(&self, slot: usize, tint: f64) -> Option<String> {
        let base = self.slots.get(slot)?;
        Some(if tint == 0.0 {
            base.clone()
        } else {
            apply_tint(base, tint)
        })
    }

    /// The slots in OOXML order, for a host that wants to offer the workbook's
    /// own theme colours.
    #[must_use]
    pub fn slots(&self) -> &[String] {
        &self.slots
    }
}

/// The stock Office theme slots, for a workbook that never came from a package.
#[must_use]
pub fn stock_theme_slots() -> Vec<String> {
    ThemePalette::default().slots().to_vec()
}

/// Parse `xl/theme/theme1.xml` into the palette. Anything it cannot read keeps
/// the stock Office value for that slot rather than dropping the colour.
pub fn parse_theme(xml: &[u8]) -> Result<ThemePalette, ImportError> {
    let mut palette = ThemePalette::default();
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut in_scheme = false;
    // The scheme element currently open, as its index in `<a:clrScheme>` order.
    let mut scheme_slot: Option<usize> = None;
    // Same element/depth ceilings the other parts are read under: a theme part
    // is untrusted input like any other, and this one is small by nature.
    let mut elements = 0usize;
    let mut depth = 0usize;

    loop {
        let event = reader.read_event_into(&mut buf).map_err(xml_err)?;
        if let Event::Start(_) | Event::Empty(_) = event {
            elements += 1;
            if elements > MAX_ELEMENTS {
                return Err(ImportError::Ooxml(OoxmlError::TooManyElements {
                    limit: MAX_ELEMENTS,
                }));
            }
        }
        match event {
            Event::Start(_) => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(ImportError::Ooxml(OoxmlError::TooDeep { limit: MAX_DEPTH }));
                }
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"clrScheme" => in_scheme = true,
                    b"srgbClr" | b"sysClr" if in_scheme => {
                        // `<a:sysClr>` carries the resolved value in `lastClr`.
                        let value = attr(e, b"val")?
                            .filter(|v| is_hex6(v))
                            .or(attr(e, b"lastClr")?.filter(|v| is_hex6(v)));
                        if let (Some(slot), Some(v)) = (scheme_slot, value) {
                            palette.slots[theme_index(slot)] = v.to_ascii_uppercase();
                        }
                    }
                    other if in_scheme => {
                        if let Some(slot) = scheme_order(other) {
                            scheme_slot = Some(slot);
                        }
                    }
                    _ => {}
                }
            }
            Event::End(ref e) => match e.local_name().as_ref() {
                b"clrScheme" => break,
                other => {
                    if scheme_order(other).is_some() {
                        scheme_slot = None;
                    }
                }
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(palette)
}

/// Position of a `<a:clrScheme>` child in document order, or `None` for an
/// element that is not one of the twelve.
fn scheme_order(local: &[u8]) -> Option<usize> {
    Some(match local {
        b"dk1" => 0,
        b"lt1" => 1,
        b"dk2" => 2,
        b"lt2" => 3,
        b"accent1" => 4,
        b"accent2" => 5,
        b"accent3" => 6,
        b"accent4" => 7,
        b"accent5" => 8,
        b"accent6" => 9,
        b"hlink" => 10,
        b"folHlink" => 11,
        _ => return None,
    })
}

/// Map a `<a:clrScheme>` position to the `theme="N"` index: the dark/light
/// pairs are swapped, everything from accent1 on is identical.
fn theme_index(scheme_pos: usize) -> usize {
    match scheme_pos {
        0 => 1, // dk1 → text 1
        1 => 0, // lt1 → background 1
        2 => 3, // dk2 → text 2
        3 => 2, // lt2 → background 2
        n => n,
    }
}

/// Excel's legacy 64-entry indexed palette (ECMA-376 §18.8.27). Index 64/65 are
/// the system foreground/background — "automatic", which the model represents
/// as no explicit colour.
const INDEXED: [&str; 64] = [
    "000000", "FFFFFF", "FF0000", "00FF00", "0000FF", "FFFF00", "FF00FF", "00FFFF", "000000",
    "FFFFFF", "FF0000", "00FF00", "0000FF", "FFFF00", "FF00FF", "00FFFF", "800000", "008000",
    "000080", "808000", "800080", "008080", "C0C0C0", "808080", "9999FF", "993366", "FFFFCC",
    "CCFFFF", "660066", "FF8080", "0066CC", "CCCCFF", "000080", "FF00FF", "FFFF00", "00FFFF",
    "800080", "800000", "008080", "0000FF", "00CCFF", "CCFFFF", "CCFFCC", "FFFF99", "99CCFF",
    "FF99CC", "CC99FF", "FFCC99", "3366FF", "33CCCC", "99CC00", "FFCC00", "FF9900", "FF6600",
    "666699", "969696", "003366", "339966", "003300", "333300", "993300", "993366", "333399",
    "333333",
];

/// The colour for an `indexed="N"` attribute, or `None` for "automatic" and for
/// an index outside the palette.
#[must_use]
pub fn indexed_color(index: usize) -> Option<String> {
    INDEXED.get(index).map(|s| (*s).to_owned())
}

/// Apply an OOXML `tint` to an `RRGGBB` colour: negative darkens, positive
/// lightens, both by scaling luminance in HSL (ECMA-376 §18.8.19).
fn apply_tint(hex: &str, tint: f64) -> String {
    let Some((r, g, b)) = split_rgb(hex) else {
        return hex.to_owned();
    };
    let (h, s, l) = rgb_to_hsl(r, g, b);
    let t = tint.clamp(-1.0, 1.0);
    let l = if t < 0.0 {
        l * (1.0 + t)
    } else {
        l * (1.0 - t) + t
    };
    let (r, g, b) = hsl_to_rgb(h, s, l.clamp(0.0, 1.0));
    format!("{r:02X}{g:02X}{b:02X}")
}

fn split_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    if hex.len() != 6 {
        return None;
    }
    Some((
        u8::from_str_radix(&hex[0..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..6], 16).ok()?,
    ))
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let (r, g, b) = (
        f64::from(r) / 255.0,
        f64::from(g) / 255.0,
        f64::from(b) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, l)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match h {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to_byte = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to_byte(r), to_byte(g), to_byte(b))
}

fn is_hex6(v: &str) -> bool {
    v.len() == 6 && v.bytes().all(|b| b.is_ascii_hexdigit())
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

fn xml_err(err: quick_xml::Error) -> ImportError {
    ImportError::Ooxml(casual_calc_ooxml::OoxmlError::MalformedXml(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const THEME: &[u8] =
        br#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
      <a:themeElements><a:clrScheme name="Office">
        <a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
        <a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
        <a:dk2><a:srgbClr val="44546A"/></a:dk2>
        <a:lt2><a:srgbClr val="E7E6E6"/></a:lt2>
        <a:accent1><a:srgbClr val="4472C4"/></a:accent1>
        <a:accent2><a:srgbClr val="ED7D31"/></a:accent2>
        <a:accent3><a:srgbClr val="A5A5A5"/></a:accent3>
        <a:accent4><a:srgbClr val="FFC000"/></a:accent4>
        <a:accent5><a:srgbClr val="5B9BD5"/></a:accent5>
        <a:accent6><a:srgbClr val="70AD47"/></a:accent6>
        <a:hlink><a:srgbClr val="0563C1"/></a:hlink>
        <a:folHlink><a:srgbClr val="954F72"/></a:folHlink>
      </a:clrScheme></a:themeElements></a:theme>"#;

    #[test]
    fn dark_light_slots_are_swapped_against_scheme_order() {
        let p = parse_theme(THEME).unwrap();
        // theme="0" is the *background*, even though <a:dk1> comes first.
        assert_eq!(p.resolve(0, 0.0).unwrap(), "FFFFFF");
        assert_eq!(p.resolve(1, 0.0).unwrap(), "000000");
        assert_eq!(p.resolve(2, 0.0).unwrap(), "E7E6E6");
        assert_eq!(p.resolve(3, 0.0).unwrap(), "44546A");
        assert_eq!(p.resolve(4, 0.0).unwrap(), "4472C4");
        assert_eq!(p.resolve(11, 0.0).unwrap(), "954F72");
        assert!(p.resolve(12, 0.0).is_none());
    }

    #[test]
    fn a_theme_part_that_omits_slots_keeps_the_office_defaults() {
        let partial = br#"<a:theme xmlns:a="x"><a:clrScheme><a:accent1><a:srgbClr val="112233"/></a:accent1></a:clrScheme></a:theme>"#;
        let p = parse_theme(partial).unwrap();
        assert_eq!(p.resolve(4, 0.0).unwrap(), "112233");
        assert_eq!(p.resolve(1, 0.0).unwrap(), "000000"); // untouched default
    }

    #[test]
    fn tint_lightens_and_darkens() {
        // The classic "white, background 1, darker 15%" of Excel's style gallery.
        let p = ThemePalette::default();
        assert_eq!(p.resolve(0, -0.15).unwrap(), "D9D9D9");
        // A positive tint lightens toward white; zero leaves the colour alone.
        assert_eq!(p.resolve(1, 0.5).unwrap(), "808080");
        assert_eq!(p.resolve(1, 0.0).unwrap(), "000000");
    }

    #[test]
    fn indexed_palette_covers_the_legacy_range_only() {
        assert_eq!(indexed_color(0).unwrap(), "000000");
        assert_eq!(indexed_color(10).unwrap(), "FF0000");
        assert_eq!(indexed_color(63).unwrap(), "333333");
        assert!(indexed_color(64).is_none()); // system foreground = automatic
    }
}
