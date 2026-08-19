//! Pictures: where a backend gets their bytes, and what it says about the ones
//! it could not draw.
//!
//! A [`PaintItem::Image`](casual_calc_layout::PaintItem::Image) carries the
//! package path of a media part, not the part's bytes — a display list is
//! rebuilt every frame and would otherwise copy every megabyte of every picture
//! each time. So a backend needs somewhere to resolve that path, which is
//! [`ImageSource`], and it needs somewhere to say what it could not resolve or
//! could not read, which is [`ImageReport`].
//!
//! **The report is the point, not a nicety.** AGENTS.md's no-silent-data-loss
//! rule is about the model dropping things, and a renderer drops them just as
//! effectively: a picture that decodes to nothing and a picture that was never
//! there produce the same blank rectangle, and the second is a bug while the
//! first is a file this cannot read. A host folds these into the compatibility
//! report it already shows.

use std::collections::BTreeMap;

/// Where a renderer gets the bytes of a media part.
///
/// A host implements this over whatever already holds the media — for a
/// workbook read from a package that is its retained parts, whose entries are
/// already keyed by the same package path the display list carries, so nothing
/// has to be copied to answer. A `BTreeMap<String, Vec<u8>>` implements it too,
/// for a caller that has the bytes loose.
pub trait ImageSource {
    /// The bytes of the part at `path`, or `None` if this source has none.
    ///
    /// `path` is a package path as it appears in the display list, e.g.
    /// `xl/media/image1.png`.
    fn part_bytes(&self, path: &str) -> Option<&[u8]>;
}

/// A source with no media at all: every picture is reported as not supplied.
///
/// What [`render_pixmap`](crate::render_pixmap) uses, because its signature has
/// nowhere to take a source and nowhere to return a report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoImages;

impl ImageSource for NoImages {
    fn part_bytes(&self, _path: &str) -> Option<&[u8]> {
        None
    }
}

impl ImageSource for BTreeMap<String, Vec<u8>> {
    fn part_bytes(&self, path: &str) -> Option<&[u8]> {
        self.get(path).map(Vec::as_slice)
    }
}

impl<T: ImageSource + ?Sized> ImageSource for &T {
    fn part_bytes(&self, path: &str) -> Option<&[u8]> {
        (**self).part_bytes(path)
    }
}

/// Why a picture in the display list did not reach the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UndrawnReason {
    /// The [`ImageSource`] had no bytes under that path. Either the host did
    /// not supply the media, or the workbook names a part it does not contain.
    NotSupplied,
    /// A picture in an image format this backend does not decode, named — `
    /// "jpeg"`, `"gif"`, `"emf"`. Named rather than counted as one lump,
    /// because "this file's pictures are EMF" is a sentence somebody can act on
    /// and "3 pictures were not drawn" is not.
    UnsupportedFormat(&'static str),
    /// A PNG whose bytes this could not read. A truncated or corrupt part —
    /// distinct from an unsupported format, which is intact and simply not
    /// something this decodes.
    Undecodable,
    /// A PNG declaring more pixels than [`MAX_IMAGE_PIXELS`], refused **before**
    /// decoding.
    TooLarge {
        /// The declared width in pixels.
        width: u32,
        /// The declared height in pixels.
        height: u32,
    },
}

impl UndrawnReason {
    /// A stable key for a compatibility report, so a host can aggregate without
    /// formatting a sentence and matching on it later.
    #[must_use]
    pub fn feature(&self) -> &'static str {
        match self {
            UndrawnReason::NotSupplied => "image (media not supplied)",
            UndrawnReason::UnsupportedFormat(_) => "image (format not decoded)",
            UndrawnReason::Undecodable => "image (undecodable)",
            UndrawnReason::TooLarge { .. } => "image (over the pixel limit)",
        }
    }
}

impl core::fmt::Display for UndrawnReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            UndrawnReason::NotSupplied => f.write_str("no bytes were supplied for the part"),
            UndrawnReason::UnsupportedFormat(name) => {
                write!(f, "{name} is not an image format this backend decodes")
            }
            UndrawnReason::Undecodable => f.write_str("the PNG could not be decoded"),
            UndrawnReason::TooLarge { width, height } => write!(
                f,
                "{width}x{height} is over the {MAX_IMAGE_PIXELS}-pixel limit"
            ),
        }
    }
}

/// One picture the renderer could not draw, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndrawnImage {
    /// The package path from the display list, e.g. `xl/media/image1.png`.
    pub part: String,
    /// What stopped it.
    pub reason: UndrawnReason,
}

/// What a render did with the pictures in its display list.
///
/// `drawn` counts paint instructions that put pixels on the surface, so the
/// same picture anchored twice counts twice — it is a count of what was drawn,
/// not of distinct files. `undrawn` is deduplicated by part and reason: a
/// picture that appears in three frozen panes is one thing wrong, said once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageReport {
    /// How many pictures were painted.
    pub drawn: u32,
    /// The ones that were not, named and reasoned, in first-seen order.
    pub undrawn: Vec<UndrawnImage>,
}

impl ImageReport {
    /// Whether every picture in the display list reached the surface.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.undrawn.is_empty()
    }

    /// Record a picture that was drawn.
    pub(crate) fn drew(&mut self) {
        self.drawn = self.drawn.saturating_add(1);
    }

    /// Record a picture that was not, once per part and reason.
    pub(crate) fn missed(&mut self, part: &str, reason: UndrawnReason) {
        if self
            .undrawn
            .iter()
            .any(|u| u.part == part && u.reason == reason)
        {
            return;
        }
        self.undrawn.push(UndrawnImage {
            part: part.to_owned(),
            reason,
        });
    }

    /// Fold another surface's report into this one — the frozen-pane path,
    /// where each pane renders separately over the same media.
    pub(crate) fn absorb(&mut self, other: ImageReport) {
        self.drawn = self.drawn.saturating_add(other.drawn);
        for miss in other.undrawn {
            self.missed(&miss.part, miss.reason);
        }
    }
}

/// The most pixels a picture may declare before this refuses to decode it.
///
/// **A security bound, not a quality one.** The dimensions come out of an
/// untrusted file's IHDR, and a decoded surface is four bytes a pixel whatever
/// the compressed part weighs — a few hundred bytes of PNG can legally declare
/// 65535x65535 and ask for seventeen gigabytes. The limit is checked against
/// the header *before* any allocation, which is the only place checking it
/// helps.
///
/// Generous next to any picture in a spreadsheet: 16 megapixels is a 4000x4000
/// image, about 64 MB decoded.
pub const MAX_IMAGE_PIXELS: u64 = 16_000_000;

/// The image format `bytes` look like, by magic number, or `None` for PNG —
/// the one format this backend decodes.
///
/// Sniffed rather than taken from the part's extension or its content type,
/// because both are the file's own claim about itself and neither is what the
/// decoder will meet.
fn foreign_format(bytes: &[u8]) -> Option<&'static str> {
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.starts_with(PNG) {
        return None;
    }
    let name = if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "gif"
    } else if bytes.starts_with(b"BM") {
        "bmp"
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        "tiff"
    } else if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        "webp"
    } else if bytes.len() >= 44 && bytes.starts_with(&[1, 0, 0, 0]) && &bytes[40..44] == b" EMF" {
        "emf"
    } else if bytes.starts_with(&[0xd7, 0xcd, 0xc6, 0x9a]) || bytes.starts_with(&[1, 0, 9, 0]) {
        "wmf"
    } else if bytes.starts_with(b"<?xml") || bytes.starts_with(b"<svg") {
        "svg"
    } else {
        "an unrecognised format"
    };
    Some(name)
}

/// The width and height a PNG's IHDR declares, without decoding it.
///
/// The signature is 8 bytes, then a 4-byte chunk length, then `IHDR`, then the
/// two big-endian dimensions — so they are at a fixed offset in every valid
/// PNG, and reading them costs nothing next to inflating the image.
fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((width, height))
}

/// Decode the media part at `path` into a pixmap, or say why not.
pub(crate) fn decode(
    path: &str,
    images: &dyn ImageSource,
) -> Result<tiny_skia::Pixmap, UndrawnReason> {
    let Some(bytes) = images.part_bytes(path) else {
        return Err(UndrawnReason::NotSupplied);
    };
    if let Some(format) = foreign_format(bytes) {
        return Err(UndrawnReason::UnsupportedFormat(format));
    }
    // Before `decode_png`, not after: the point of the limit is the allocation
    // it prevents, and asking afterwards has already made it.
    let (width, height) = png_dimensions(bytes).ok_or(UndrawnReason::Undecodable)?;
    if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(UndrawnReason::TooLarge { width, height });
    }
    tiny_skia::Pixmap::decode_png(bytes).map_err(|_| UndrawnReason::Undecodable)
}
