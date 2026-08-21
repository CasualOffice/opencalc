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

use crate::substitution::substitute;
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
    // Supplied faces first. A deployment that went to the trouble of providing a
    // font did so because the bundled ones were not enough, and consulting them
    // second would mean a bundled face that half-covers a script wins over the
    // one chosen deliberately.
    registered()
        .iter()
        .copied()
        .find(|&bytes| face_covers(bytes, ch))
        .or_else(|| {
            FAMILIES
                .iter()
                .map(|family| family.face_bytes(bold, italic))
                .find(|&bytes| face_covers(bytes, ch))
        })
}

/// Faces supplied at runtime, in the order they were given.
///
/// # Why fonts are ingested rather than embedded
///
/// The bundled families cover Latin and Hebrew. Arabic, Devanagari, Thai and
/// CJK render as `.notdef` boxes, and the obvious fix — bundle Noto — is the
/// wrong shape twice over. It puts megabytes into a WebAssembly bundle for
/// scripts most deployments never see; and it makes this project the arbiter of
/// which languages are worth carrying, which is not a judgement it should be
/// making on anybody's behalf.
///
/// So a host supplies them. It knows which scripts its documents are in, it
/// already serves static assets, and it can ship one font or twenty without
/// this crate changing. What stays here is Latin, because something has to work
/// with no configuration at all.
fn registered() -> &'static [&'static [u8]] {
    REGISTERED.read().map_or(&[], |guard| {
        // Leaked deliberately: a font lives as long as the process, the list only
        // grows, and handing out `&'static` keeps every call site below free of
        // lifetimes it would otherwise thread through the whole renderer.
        Box::leak(guard.clone().into_boxed_slice())
    })
}

static REGISTERED: std::sync::RwLock<Vec<&'static [u8]>> = std::sync::RwLock::new(Vec::new());

/// Add a face for the renderer to use, ahead of the bundled ones.
///
/// Returns whether the bytes are a face this can read — a caller handing over a
/// 404 page instead of a font should be told, rather than discovering it from a
/// thumbnail full of boxes.
///
/// Idempotent by content: registering the same bytes twice does not search them
/// twice.
pub fn register_face(bytes: Vec<u8>) -> bool {
    if skrifa::FontRef::new(&bytes).is_err() {
        return false;
    }
    let Ok(mut guard) = REGISTERED.write() else {
        return false;
    };
    let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    if guard.contains(&leaked) {
        return true;
    }
    guard.push(leaked);
    true
}

/// How many faces have been supplied. For a host reporting its own setup.
#[must_use]
pub fn registered_count() -> usize {
    REGISTERED.read().map_or(0, |g| g.len())
}

/// A script this build cannot draw, and one character from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingScript {
    /// A name for the block, for saying out loud. `"Arabic"`, `"CJK"`.
    pub script: &'static str,
    /// The first character seen from it, so a caller can show what it means.
    pub sample: char,
}

/// Name the Unicode block a character belongs to, for the scripts a
/// spreadsheet is realistically written in.
///
/// Deliberately coarse. This exists to turn "boxes" into a sentence naming what
/// to install, and "Devanagari" is that sentence; the precise block name is not.
/// Anything unrecognised is reported under its code point instead of being
/// dropped, because a character nobody anticipated is exactly the one worth
/// mentioning.
fn block_of(ch: char) -> &'static str {
    match u32::from(ch) {
        0x0590..=0x05FF | 0xFB1D..=0xFB4F => "Hebrew",
        0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => "Arabic",
        0x0900..=0x097F => "Devanagari",
        0x0980..=0x09FF => "Bengali",
        0x0A80..=0x0AFF => "Gujarati",
        0x0B80..=0x0BFF => "Tamil",
        0x0C00..=0x0C7F => "Telugu",
        0x0D00..=0x0D7F => "Malayalam",
        0x0E00..=0x0E7F => "Thai",
        0x1000..=0x109F => "Burmese",
        0x10A0..=0x10FF => "Georgian",
        0x0530..=0x058F => "Armenian",
        0x1100..=0x11FF | 0xAC00..=0xD7AF => "Korean",
        0x3040..=0x30FF => "Japanese kana",
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF => "CJK",
        0x0400..=0x04FF => "Cyrillic",
        0x0370..=0x03FF => "Greek",
        0x1F300..=0x1FAFF | 0x2600..=0x27BF => "emoji and symbols",
        _ => "other",
    }
}

/// Which scripts appear in `text` that no available face can draw.
///
/// The gap this closes is not that coverage is missing — that is a deliberate
/// decision, recorded in [ADR-018](../../../docs/64-TEXT-SHAPING.md), and a host
/// supplies what it needs with [`register_face`]. The gap is that a document in
/// an unsupplied script renders as a row of boxes **with nothing anywhere saying
/// why**, which reads as a rendering bug rather than as a missing font.
///
/// Answers for the faces registered *now*, so a caller should ask after
/// registering rather than before. Reported in order of first appearance, once
/// per script, with one sample character — enough to write "this sheet contains
/// Arabic and no face covering it is installed" and nothing more, because a list
/// of every uncoverable code point is not a thing anybody acts on.
///
/// Characters with no visible glyph of their own — spaces, newlines, control
/// codes — are skipped: a face is not required to cover them and reporting them
/// would bury the one line that matters.
#[must_use]
pub fn missing_scripts(text: &str) -> Vec<MissingScript> {
    let mut found: Vec<MissingScript> = Vec::new();
    let mut asked: std::collections::HashSet<char> = std::collections::HashSet::new();
    for ch in text.chars() {
        if ch.is_whitespace() || ch.is_control() {
            continue;
        }
        if !asked.insert(ch) {
            continue;
        }
        // Weight is not part of the question: a deployment missing Arabic is
        // missing it in every weight, and asking four times would report the
        // same script four times.
        if coverage_face_bytes(ch, false, false).is_some() {
            continue;
        }
        let script = block_of(ch);
        if !found.iter().any(|m| m.script == script) {
            found.push(MissingScript { script, sample: ch });
        }
    }
    found
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
    fn coverage_answers_for_common_latin_in_the_requested_weight() {
        // This asserted the *first bundled family* until fonts could be supplied
        // at runtime, and that stopped being the contract: a host's face is
        // searched first, deliberately, since it was provided precisely because
        // the bundled ones were not enough. What must still hold is that Latin
        // is answered at all, in the weight that was asked for.
        //
        // Also a note on why it is written this way: the registry is process
        // global, so a test that registers a face changes what every later test
        // sees. Asserting the property rather than the identity is what makes
        // this suite independent of its own order.
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            let bytes = coverage_face_bytes('A', bold, italic).expect("some family must cover 'A'");
            assert!(
                face_covers(bytes, 'A'),
                "the face returned for {bold}/{italic} actually covers the character"
            );
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

#[cfg(test)]
mod ingest_tests {
    use super::*;

    /// A supplied face covers what the bundle does not.
    ///
    /// Uses a bundled face as the stand-in for a supplied one, because the point
    /// is the *mechanism* — a host hands over bytes and they are searched first
    /// — and asserting it with a real Noto would mean carrying a Noto, which is
    /// exactly what this exists to avoid.
    #[test]
    fn a_supplied_face_is_searched_before_the_bundled_ones() {
        let before = registered_count();
        assert!(
            register_face(CARLITO.face_bytes(false, false).to_vec()),
            "a real face is accepted"
        );
        assert_eq!(
            registered_count(),
            before + 1,
            "and is remembered for the next render"
        );
        // Registering it again does not search it twice.
        assert!(register_face(CARLITO.face_bytes(false, false).to_vec()));
        assert_eq!(registered_count(), before + 1, "idempotent by content");
    }

    #[test]
    fn bytes_that_are_not_a_face_are_refused_rather_than_stored() {
        // The realistic failure: a host fetches a font URL and gets an error
        // page. Storing it would produce a renderer that searches an HTML
        // document for glyphs and a thumbnail full of boxes with nothing to
        // explain it.
        assert!(!register_face(
            b"<!doctype html><title>404</title>".to_vec()
        ));
        assert!(!register_face(Vec::new()));
    }
}

#[cfg(test)]
mod coverage_report_tests {
    use super::*;

    /// Latin is always drawable, with no configuration and no registered face —
    /// that is the one promise the bundle makes, so it must never be reported.
    #[test]
    fn latin_is_never_missing() {
        assert_eq!(missing_scripts("Total 1,234.56 (café)"), Vec::new());
    }

    /// And a script with no bundled face is named rather than silently drawn as
    /// boxes. Asserted through `coverage_face_bytes` rather than against a fixed
    /// list, so this follows the build it is compiled into: the `all-fonts`
    /// build covers more, and the test says the same thing about both.
    #[test]
    fn an_uncovered_script_is_named_once_with_a_sample() {
        // U+0915 DEVANAGARI LETTER KA, thrice, so the "once per script" part is
        // actually exercised rather than assumed.
        let text = "क क क";
        if coverage_face_bytes('क', false, false).is_some() {
            assert_eq!(
                missing_scripts(text),
                Vec::new(),
                "this build covers Devanagari, so it must not be reported"
            );
            return;
        }
        assert_eq!(
            missing_scripts(text),
            vec![MissingScript {
                script: "Devanagari",
                sample: 'क',
            }]
        );
    }

    /// Two scripts, both missing, both named — and Latin in the same string is
    /// not, because the report is about what to install and Latin is installed.
    #[test]
    fn several_scripts_are_reported_separately_and_latin_is_not() {
        let text = "Total: العربية / ไทย";
        let missing = missing_scripts(text);
        assert!(
            !missing.iter().any(|m| m.script == "other"),
            "these are known blocks, not unrecognised ones: {missing:?}"
        );
        for name in ["Arabic", "Thai"] {
            if coverage_face_bytes(if name == "Arabic" { 'ا' } else { 'ไ' }, false, false).is_none()
            {
                assert!(
                    missing.iter().any(|m| m.script == name),
                    "{name} is uncovered and must be named: {missing:?}"
                );
            }
        }
    }

    /// The report and the renderer must never disagree: a character is reported
    /// missing exactly when the renderer has no face for it.
    ///
    /// This is what makes the report right *after* a host registers a face,
    /// without this test registering one — `register_face` writes to process-wide
    /// state, and a test that mutates it races the one that counts it. Asserting
    /// the shared path instead gets the property and no flake.
    #[test]
    fn the_report_agrees_with_what_the_renderer_can_draw() {
        for ch in ['A', 'é', 'א', 'ا', 'क', 'ไ', '中', '€', 'Я'] {
            let drawable = coverage_face_bytes(ch, false, false).is_some();
            let reported = missing_scripts(&ch.to_string()).is_empty();
            assert_eq!(
                drawable, reported,
                "{ch:?}: renderer can draw it = {drawable}, report says fine = {reported}"
            );
        }
    }

    /// Whitespace has no glyph to miss.
    #[test]
    fn blanks_and_control_characters_are_not_reported() {
        assert_eq!(missing_scripts(" \t\n\u{7}"), Vec::new());
    }
}
