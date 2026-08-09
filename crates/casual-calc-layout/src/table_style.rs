//! Resolving a table's style name to the colours a host actually paints.
//!
//! Excel does not write a table's banding into the cells. `<tableStyleInfo
//! name="TableStyleMedium2"/>` names a *style*, and the reader derives every
//! colour from that name and the workbook's theme. A host that paints two
//! hardcoded greys instead is not rendering the file: every table looks the
//! same, and a workbook whose author chose a green style opens blue.
//!
//! The built-in names are `TableStyle{Light|Medium|Dark}{n}`. Within each
//! family the accents cycle in groups of seven — position 0 is the neutral
//! (text) colour and positions 1..=6 are `accent1..accent6` — which is why
//! `TableStyleMedium2` is the familiar blue and `Medium9` is blue again.
//!
//! The tints are an approximation. Excel's real definitions live in a `dxf`
//! table inside its own resources, not in the file, so no reader can reproduce
//! them exactly from the package alone. Approximating means a green style looks
//! green and a dark style looks dark, which is far closer than one grey for
//! everything; the alternative is to claim fidelity we cannot have.

use casual_calc_model::Workbook;

/// The colours a table's style resolves to, as `RRGGBB`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableStyleColors {
    /// The header row's fill.
    pub header_fill: String,
    /// Text on the header row — chosen for contrast against `header_fill`.
    pub header_text: String,
    /// The fill on a plain data row.
    ///
    /// A table is a light block in Excel whatever the application theme is, so
    /// this is real: without it a dark-themed grid shows light bands striping a
    /// dark body, and the banded rows' text is unreadable against them.
    pub body_fill: String,
    /// Text on `body_fill` and `band_fill`.
    pub body_text: String,
    /// The fill on every second data row, when row stripes are on.
    pub band_fill: String,
    /// The rule under the header and around the table.
    pub border: String,
}

/// The theme slot a built-in table style name draws its colour from.
///
/// Returns `None` for the neutral positions and for any name that is not a
/// built-in — a custom style defined in the file has no name to decode, and
/// guessing a colour for it would be worse than falling back to the neutral.
fn accent_slot(name: &str) -> Option<usize> {
    let split = name.len() - name.chars().rev().take_while(char::is_ascii_digit).count();
    let n: usize = name.get(split..)?.parse().ok()?;
    if n == 0 {
        return None;
    }
    // Slots 4..=9 are accent1..=accent6 in `theme` attribute order.
    match (n - 1) % 7 {
        0 => None,
        p => Some(3 + p),
    }
}

/// `RRGGBB` -> (r, g, b).
fn rgb(hex: &str) -> (f64, f64, f64) {
    let h = hex.trim_start_matches('#');
    // An 8-digit value is `AARRGGBB`; the alpha is not a colour channel.
    let h = if h.len() == 8 { &h[2..] } else { h };
    let at = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f64;
    if h.len() < 6 {
        return (0.0, 0.0, 0.0);
    }
    (at(0), at(2), at(4))
}

/// Mix towards white (`t > 0`) or black (`t < 0`), as OOXML's own tint does.
fn tint(hex: &str, t: f64) -> String {
    let (r, g, b) = rgb(hex);
    let mix = |c: f64| {
        let v = if t >= 0.0 {
            c + (255.0 - c) * t
        } else {
            c * (1.0 + t)
        };
        v.round().clamp(0.0, 255.0) as u8
    };
    format!("{:02X}{:02X}{:02X}", mix(r), mix(g), mix(b))
}

/// Whether white text reads better than black on this fill.
fn wants_light_text(hex: &str) -> bool {
    let (r, g, b) = rgb(hex);
    // Relative luminance, the sRGB weights. A mid-blue accent is around 0.35,
    // so the threshold sits above it and header text stays white where Excel
    // puts it white.
    (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0 < 0.6
}

/// Resolve a `<tableStyleInfo name>` against a workbook's theme.
///
/// An unknown or empty name falls back to the neutral treatment, which is what
/// `TableStyleLight1` — Excel's own "no colour" style — looks like.
#[must_use]
pub fn table_style_colors(workbook: &Workbook, name: &str) -> TableStyleColors {
    // Slot 1 is `dk1`, the text colour: the neutral styles are built from it.
    let base = accent_slot(name)
        .map(|slot| workbook.theme_slot(slot))
        .unwrap_or_else(|| workbook.theme_slot(1))
        .to_owned();

    let family = if name.contains("Dark") {
        Family::Dark
    } else if name.contains("Medium") {
        Family::Medium
    } else {
        Family::Light
    };

    match family {
        // A solid accent header with white text, and a light wash on every
        // second row. This is the default and by far the most common.
        Family::Medium => TableStyleColors {
            header_text: if wants_light_text(&base) {
                "FFFFFF".to_owned()
            } else {
                "000000".to_owned()
            },
            body_fill: "FFFFFF".to_owned(),
            body_text: "000000".to_owned(),
            band_fill: tint(&base, 0.80),
            border: tint(&base, 0.35),
            header_fill: base,
        },
        // No header fill at all — the header is marked by a rule under it, so
        // the fill matches the sheet and the text keeps the accent.
        Family::Light => TableStyleColors {
            header_fill: "FFFFFF".to_owned(),
            header_text: tint(&base, -0.25),
            body_fill: "FFFFFF".to_owned(),
            body_text: "000000".to_owned(),
            band_fill: tint(&base, 0.88),
            border: base,
        },
        // Darker than the accent, with a stronger band, so the table reads as
        // a block rather than as rows on a sheet.
        Family::Dark => TableStyleColors {
            header_text: "FFFFFF".to_owned(),
            body_fill: tint(&base, 0.75),
            body_text: "000000".to_owned(),
            band_fill: tint(&base, 0.55),
            border: tint(&base, -0.55),
            header_fill: tint(&base, -0.35),
        },
    }
}

enum Family {
    Light,
    Medium,
    Dark,
}

#[cfg(test)]
mod tests {
    use super::*;
    use casual_calc_model::Id;

    fn wb() -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        // Stock Office slots, `lt1 dk1 lt2 dk2 accent1..6 hlink folHlink`.
        wb.theme_colors = [
            "FFFFFF", "000000", "E7E6E6", "44546A", "4472C4", "ED7D31", "A5A5A5", "FFC000",
            "5B9BD5", "70AD47", "0563C1", "954F72",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        wb
    }

    #[test]
    fn medium2_is_accent1_and_the_accents_cycle_every_seven() {
        // The default style. If this drifts, every new table changes colour.
        assert_eq!(accent_slot("TableStyleMedium2"), Some(4));
        assert_eq!(accent_slot("TableStyleMedium7"), Some(9));
        // Position 0 of each group is the neutral, not an accent.
        assert_eq!(accent_slot("TableStyleMedium1"), None);
        assert_eq!(accent_slot("TableStyleMedium8"), None);
        // ...and the next group repeats the accents.
        assert_eq!(accent_slot("TableStyleMedium9"), Some(4));
        assert_eq!(accent_slot("TableStyleLight2"), Some(4));
    }

    #[test]
    fn a_medium_style_paints_its_accent_and_a_wash_of_it() {
        let c = table_style_colors(&wb(), "TableStyleMedium2");
        assert_eq!(c.header_fill, "4472C4", "accent1 verbatim");
        assert_eq!(c.header_text, "FFFFFF", "white on a mid blue");
        // The band is the same hue, not a neutral grey — that is the whole
        // point of resolving the style rather than hardcoding two colours.
        let (r, g, b) = rgb(&c.band_fill);
        assert!(b > g && g > r, "still blue-ish: {}", c.band_fill);
        assert!(b > 200.0, "and light: {}", c.band_fill);
    }

    #[test]
    fn a_different_style_number_gives_a_different_colour() {
        let green = table_style_colors(&wb(), "TableStyleMedium7");
        let blue = table_style_colors(&wb(), "TableStyleMedium2");
        assert_ne!(green.header_fill, blue.header_fill);
        assert_eq!(green.header_fill, "70AD47");
    }

    #[test]
    fn an_unknown_name_falls_back_to_the_neutral_rather_than_guessing() {
        let c = table_style_colors(&wb(), "SomeCustomStyle");
        assert_eq!(c.header_fill, "FFFFFF", "Light family, neutral base");
        let n = table_style_colors(&wb(), "");
        assert_eq!(n.header_fill, "FFFFFF");
    }

    #[test]
    fn the_body_is_light_so_a_dark_theme_does_not_stripe_a_dark_block() {
        // Without a body fill the bands paint light rows over a dark grid and
        // the text on them, still light, disappears.
        for name in ["TableStyleMedium2", "TableStyleLight2", "TableStyleDark2"] {
            let c = table_style_colors(&wb(), name);
            assert!(!wants_light_text(&c.body_fill), "{name} body must be light");
            assert!(!wants_light_text(&c.band_fill), "{name} band must be light");
            assert_eq!(c.body_text, "000000", "{name}");
        }
    }

    #[test]
    fn a_light_header_takes_dark_text() {
        let mut w = wb();
        w.theme_colors[4] = "FFF2CC".to_owned();
        let c = table_style_colors(&w, "TableStyleMedium2");
        assert_eq!(c.header_text, "000000", "black on a pale fill");
    }
}
