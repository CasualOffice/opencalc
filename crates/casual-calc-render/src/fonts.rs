//! Bundled font faces for the raster renderer (a port of opendoc's font set).
//!
//! The editor canvas draws text with the browser's own fonts; this module exists
//! for the deterministic PNG/raster path, which must outline glyphs from bundled
//! bytes so output is identical on every machine. Each bundled family carries its
//! four faces (regular, bold, italic, bold-italic). Requested families are mapped
//! to a bundled family by [`casual_calc_layout::substitute`] (the shared single
//! source of truth); this module turns the chosen family + bold/italic into the
//! concrete face bytes skrifa outlines.
//!
//! Provenance and licenses (Apache-2.0 / SIL OFL-1.1) are in `fonts/README.md`
//! and `fonts/LICENSES/`. Bytes are embedded with `include_bytes!` (not crate
//! deps), so `cargo-deny` does not scan them.

use casual_calc_layout::substitute;

macro_rules! face {
    ($path:literal) => {
        include_bytes!(concat!("../fonts/", $path))
    };
}

/// A bundled family: four faces indexed by `bold | italic << 1`.
#[derive(Clone, Copy, Debug)]
pub struct BundledFamily {
    /// Canonical family name (matches the shared substitution table).
    pub name: &'static str,
    faces: [&'static [u8]; 4],
}

impl BundledFamily {
    /// The bytes for the given bold/italic combination.
    #[must_use]
    pub fn face_bytes(&self, bold: bool, italic: bool) -> &'static [u8] {
        self.faces[(bold as usize) | ((italic as usize) << 1)]
    }
}

/// Roboto — the default family / ultimate fallback.
pub const ROBOTO: BundledFamily = BundledFamily {
    name: "Roboto",
    faces: [
        face!("Roboto-Regular.ttf"),
        face!("Roboto-Bold.ttf"),
        face!("Roboto-Italic.ttf"),
        face!("Roboto-BoldItalic.ttf"),
    ],
};
/// Caladea — metric-compatible with Cambria.
pub const CALADEA: BundledFamily = BundledFamily {
    name: "Caladea",
    faces: [
        face!("Caladea-Regular.ttf"),
        face!("Caladea-Bold.ttf"),
        face!("Caladea-Italic.ttf"),
        face!("Caladea-BoldItalic.ttf"),
    ],
};
/// Carlito — metric-compatible with Calibri.
pub const CARLITO: BundledFamily = BundledFamily {
    name: "Carlito",
    faces: [
        face!("Carlito-Regular.ttf"),
        face!("Carlito-Bold.ttf"),
        face!("Carlito-Italic.ttf"),
        face!("Carlito-BoldItalic.ttf"),
    ],
};
/// Liberation Sans — metric-compatible with Arial/Helvetica.
pub const LIBERATION_SANS: BundledFamily = BundledFamily {
    name: "Liberation Sans",
    faces: [
        face!("liberation/LiberationSans-Regular.ttf"),
        face!("liberation/LiberationSans-Bold.ttf"),
        face!("liberation/LiberationSans-Italic.ttf"),
        face!("liberation/LiberationSans-BoldItalic.ttf"),
    ],
};
/// Liberation Serif — metric-compatible with Times New Roman.
pub const LIBERATION_SERIF: BundledFamily = BundledFamily {
    name: "Liberation Serif",
    faces: [
        face!("liberation/LiberationSerif-Regular.ttf"),
        face!("liberation/LiberationSerif-Bold.ttf"),
        face!("liberation/LiberationSerif-Italic.ttf"),
        face!("liberation/LiberationSerif-BoldItalic.ttf"),
    ],
};
/// Liberation Mono — metric-compatible with Courier New.
pub const LIBERATION_MONO: BundledFamily = BundledFamily {
    name: "Liberation Mono",
    faces: [
        face!("liberation/LiberationMono-Regular.ttf"),
        face!("liberation/LiberationMono-Bold.ttf"),
        face!("liberation/LiberationMono-Italic.ttf"),
        face!("liberation/LiberationMono-BoldItalic.ttf"),
    ],
};

/// The default family / ultimate fallback.
pub const DEFAULT_FAMILY: &BundledFamily = &ROBOTO;

/// Every bundled family — the coverage fallback chain.
pub const FAMILIES: [&BundledFamily; 6] = [
    &ROBOTO,
    &CALADEA,
    &CARLITO,
    &LIBERATION_SANS,
    &LIBERATION_SERIF,
    &LIBERATION_MONO,
];

/// Map a bundled family name (from the substitution table) to its `BundledFamily`.
fn family_by_name(name: &str) -> &'static BundledFamily {
    FAMILIES
        .iter()
        .copied()
        .find(|f| f.name == name)
        .unwrap_or(DEFAULT_FAMILY)
}

/// Resolve a requested font family (the cell's, possibly `None`) + bold/italic to
/// the concrete bundled face bytes skrifa outlines, via the shared substitution
/// table. A blank/`None` request uses the default family.
#[must_use]
pub fn face_bytes_for(family: Option<&str>, bold: bool, italic: bool) -> &'static [u8] {
    family
        .and_then(substitute)
        .map_or(DEFAULT_FAMILY, |s| family_by_name(s.family.name))
        .face_bytes(bold, italic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_are_valid_truetype() {
        for family in FAMILIES {
            for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                let bytes = family.face_bytes(bold, italic);
                assert!(bytes.len() > 10_000, "{} face truncated", family.name);
                assert_eq!(
                    &bytes[0..4],
                    &[0x00, 0x01, 0x00, 0x00],
                    "{} not sfnt",
                    family.name
                );
            }
        }
    }

    #[test]
    fn resolves_requested_family_via_substitution() {
        assert_eq!(
            face_bytes_for(Some("Calibri"), false, false),
            CARLITO.face_bytes(false, false)
        );
        assert_eq!(
            face_bytes_for(Some("Arial"), true, false),
            LIBERATION_SANS.face_bytes(true, false)
        );
        assert_eq!(
            face_bytes_for(Some("Times New Roman"), false, true),
            LIBERATION_SERIF.face_bytes(false, true)
        );
        assert_eq!(
            face_bytes_for(None, false, false),
            ROBOTO.face_bytes(false, false)
        );
    }
}
