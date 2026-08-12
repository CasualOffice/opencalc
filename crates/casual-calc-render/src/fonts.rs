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
use skrifa::{FontRef, MetadataProvider};

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
#[cfg(feature = "all-fonts")]
pub const DEFAULT_FAMILY: &BundledFamily = &ROBOTO;

/// The default when only one family is embedded.
///
/// It must be the family that *is* embedded, or every fallback reaches for
/// bytes the build dropped — and because the constant is what keeps a blob
/// alive, naming an absent family would quietly pull it back in and undo the
/// saving. That is exactly what happened first time round: gating `FAMILIES`
/// alone left Roboto referenced here, so 2 MB stayed in the bundle and every
/// substitution still landed on it.
#[cfg(not(feature = "all-fonts"))]
const DEFAULT_FAMILY: &BundledFamily = &CARLITO;

/// Every bundled family — the coverage fallback chain.
/// Every bundled family, for a build that embeds them all.
///
/// Native: the server, the CLI and the fidelity tools render PNGs where the
/// point *is* fidelity, and a document asking for Times should not be drawn in
/// Roboto because the metric-compatible face was left out to save a few
/// megabytes on a machine with a disk.
#[cfg(feature = "all-fonts")]
pub const FAMILIES: [&BundledFamily; 6] = [
    &ROBOTO,
    &CALADEA,
    &CARLITO,
    &LIBERATION_SANS,
    &LIBERATION_SERIF,
    &LIBERATION_MONO,
];

/// One family, for WebAssembly.
///
/// The bundled faces were **9.1 MB of a 12.9 MB WebAssembly bundle** — 72% of
/// what every visitor downloads to open the editor — and the editor does not use
/// a single byte of them. It draws text with `ctx.fillText`, so the browser's
/// own fonts are what a user sees; these are only reached by `render_sheet_png`,
/// which produces a thumbnail.
///
/// So the browser gets Carlito alone: metric-compatible with Calibri, which is
/// what an `.xlsx` asks for more often than everything else combined, and enough
/// that a thumbnail has text in it rather than nothing. Anything else falls back
/// to it — visibly the wrong face, which is the honest failure for a preview and
/// far better than a blank one or a nine-megabyte download.
///
/// A build wanting full fidelity in the browser can turn `all-fonts` back on and
/// pay for it deliberately.
#[cfg(not(feature = "all-fonts"))]
pub const FAMILIES: [&BundledFamily; 1] = [&CARLITO];

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

/// Whether the given face bytes cover `ch` (its `cmap` maps the code point to a
/// glyph). Bytes that do not parse as a font are treated as covering nothing.
fn face_covers(bytes: &'static [u8], ch: char) -> bool {
    FontRef::new(bytes).is_ok_and(|font| font.charmap().map(ch).is_some())
}

/// The bytes of the first family in [`FAMILIES`] (in order) whose face — for the
/// given bold/italic combination — covers `ch`, or `None` if no bundled family
/// covers it. This is the per-glyph coverage fallback (a port of opendoc's
/// `resolve::cover_fallback`): the bold/italic face is preserved, only the family
/// changes. Deterministic and pure — coverage is decided solely by the embedded
/// font bytes.
#[must_use]
pub fn coverage_face_bytes(ch: char, bold: bool, italic: bool) -> Option<&'static [u8]> {
    FAMILIES
        .iter()
        .map(|family| family.face_bytes(bold, italic))
        .find(|&bytes| face_covers(bytes, ch))
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

    /// Metric-compatible substitution, which needs the families to substitute.
    ///
    /// Gated, because the WebAssembly build embeds Carlito alone — the faces
    /// were 72% of that bundle and the editor never touches them. Without the
    /// gate this asserted the multi-family behaviour against a single-family
    /// build and failed for the right reason, which is worth keeping as two
    /// tests rather than weakening into one that passes either way.
    #[cfg(feature = "all-fonts")]
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

    /// What the single-family build must still do: put text on the page.
    ///
    /// A thumbnail in visibly the wrong face is an honest failure; a blank one
    /// is a bug report, and a nine-megabyte download to avoid it is a worse
    /// trade for every visitor who never renders a PNG.
    #[cfg(not(feature = "all-fonts"))]
    #[test]
    fn a_single_family_build_still_answers_for_every_request() {
        for family in [
            Some("Calibri"),
            Some("Arial"),
            Some("Times New Roman"),
            None,
        ] {
            assert_eq!(
                face_bytes_for(family, false, false),
                CARLITO.face_bytes(false, false),
                "{family:?} falls back to the one bundled family"
            );
        }
        assert!(
            coverage_face_bytes('A', false, false).is_some(),
            "and Latin is still covered, so a thumbnail has text in it"
        );
    }

    #[cfg(feature = "all-fonts")]
    #[test]
    fn coverage_returns_first_family_for_common_latin() {
        // A common Latin char is covered by the first family (Roboto), so the
        // fallback resolves to it and preserves the requested bold/italic face.
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            let bytes =
                coverage_face_bytes('A', bold, italic).expect("some bundled family must cover 'A'");
            // First covering family for a common Latin char is the default (Roboto),
            // with the requested bold/italic face preserved.
            assert_eq!(bytes, ROBOTO.face_bytes(bold, italic));
        }
    }

    #[test]
    fn coverage_falls_back_when_default_family_lacks_glyph() {
        // U+03E2 (Coptic capital letter Shei) is absent from the default family
        // (Roboto) but present in a later bundled family, so coverage fallback
        // must still find a covering face — and it must not be the default.
        let ch = '\u{03E2}';
        assert!(
            !face_covers(ROBOTO.face_bytes(false, false), ch),
            "test assumes Roboto lacks U+03E2"
        );
        let bytes =
            coverage_face_bytes(ch, false, false).expect("a bundled family must cover U+03E2");
        assert!(face_covers(bytes, ch));
        assert_ne!(bytes, ROBOTO.face_bytes(false, false));
    }
}
