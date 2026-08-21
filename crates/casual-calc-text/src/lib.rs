//! Font substitution, the bundled faces, and the advance widths they imply.
//!
//! # Why this is a crate, beneath layout
//!
//! Layout owns geometry, and some geometry is decided by how wide a string is.
//! A chart legend is the case that forced this (`RND-11`): the box takes its
//! width from the widest series name, and the plot rectangle is what is left
//! over — so a layout that cannot measure text cannot place the plot. It had
//! been leaving the legend out altogether rather than guess, which made every
//! chart with a legend render with a plot the width of the legend too wide.
//!
//! Measurement used to live in `casual-calc-render`, which sits *above* layout
//! and therefore cannot be called from it. The alternative — passing layout a
//! measurement interface to call back through — was considered and **rejected**
//! (ADR-019). So the measuring moved down here instead, below both, and the two
//! now share one answer rather than each having their own.
//!
//! Nothing was added to the build by doing it: `casual-calc-wasm` already
//! depends on `casual-calc-render`, so these faces and `skrifa` were in the
//! WebAssembly bundle before this crate existed.
//!
//! # What is here
//!
//! - [`substitution`] — which bundled family stands in for a requested one.
//!   Moved from `casual-calc-layout`, and still re-exported from there, so it
//!   remains the single source of truth it always was.
//! - [`faces`] — the bundled bytes, moved from `casual-calc-render`.
//! - [`advance_width`] — how wide a string is in those bytes.

pub mod faces;
pub mod substitution;

use skrifa::{FontRef, MetadataProvider, instance::LocationRef, prelude::Size};

/// How wide `text` is, in pixels, in the face a request resolves to.
///
/// The sum of per-glyph advances, with the **same coverage fallback the
/// renderer draws with**: a character the chosen face has no glyph for is
/// measured in whichever face will actually be used for it. Measuring in one
/// face and drawing in another is how a string comes out a different width than
/// the box reserved for it.
///
/// Zero for an empty string, a non-positive size, or bytes that will not parse
/// as a font — every caller is sizing a box, and a box of nothing is a better
/// answer than a panic.
///
/// # A note on what this is *not*
///
/// This is not what a browser would measure. The editor canvas draws with the
/// system UI face and `measureText`; this measures the bundled face. The two
/// differ, and the headless render has always differed from the canvas in
/// exactly that way for cell text. What matters is that a string is measured in
/// the face it is drawn in, which is what makes a rectangle fit its contents.
#[must_use]
pub fn advance_width(text: &str, px: f32, bold: bool, italic: bool, family: Option<&str>) -> f32 {
    if text.is_empty() || !px.is_finite() || px <= 0.0 {
        return 0.0;
    }
    let bytes = faces::face_bytes_for(family, bold, italic);
    let Ok(font) = FontRef::new(bytes) else {
        return 0.0;
    };
    let size = Size::new(px);
    let loc = LocationRef::default();
    let charmap = font.charmap();
    let metrics = font.glyph_metrics(size, loc);

    text.chars()
        .map(|ch| {
            if let Some(g) = charmap.map(ch) {
                return metrics.advance_width(g).unwrap_or(0.0);
            }
            faces::coverage_face_bytes(ch, bold, italic)
                .and_then(|bytes| FontRef::new(bytes).ok())
                .and_then(|fb| {
                    let g = fb.charmap().map(ch)?;
                    fb.glyph_metrics(size, loc).advance_width(g)
                })
                .unwrap_or(0.0)
        })
        .sum()
}

#[cfg(test)]
mod tests;
