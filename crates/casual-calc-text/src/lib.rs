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
//! - [`cjk`] — Japanese, Chinese and Korean: the East Asian font names, the
//!   region a run is drawn in, and the slices a host supplies (`XL-FEAT-140`).
//! - [`woff2`] — reading those slices, which are WOFF2.

pub mod cjk;
pub mod faces;
pub mod substitution;
pub mod woff2;

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
    let faces = faces::TextFaces::new(family, bold, italic, text);
    text.chars()
        .map(|ch| char_advance(&faces.resolve(ch), ch, px))
        .sum()
}

/// How far one character advances the pen, in pixels at `px`, in the face it
/// resolved to.
///
/// With no face — a CJK slice that has not loaded yet — the width is the one
/// every CJK face gives it ([`cjk::nominal_advance_em`]): one em for an
/// ideograph, kana or hangul syllable, half for the half-width forms. So a
/// column measured before the face arrives is already the right width for
/// that text, and the renderers leave the same gap a drawn glyph would.
#[must_use]
pub fn char_advance(resolved: &faces::Resolved, ch: char, px: f32) -> f32 {
    let drawn = resolved.face.and_then(|bytes| {
        let font = FontRef::new(bytes).ok()?;
        let g = font.charmap().map(ch)?;
        font.glyph_metrics(Size::new(px), LocationRef::default())
            .advance_width(g)
    });
    drawn
        .or_else(|| resolved.nominal_em.map(|em| em * px))
        .unwrap_or(0.0)
}

#[cfg(test)]
mod cjk_tests;
#[cfg(test)]
mod tests;
