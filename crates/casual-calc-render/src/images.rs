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

/// The longest a single side may be, for the decoders that are given a limit
/// rather than asked for dimensions.
#[cfg(feature = "raster")]
///
/// A picture 16 million pixels wide and one tall is inside [`MAX_IMAGE_PIXELS`]
/// and is not a picture. Bounding the side as well as the area refuses it.
pub const MAX_IMAGE_SIDE: u32 = 16_000;

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
    // Raster formats are decoded now (`RND-12`), so they are not "foreign" any
    // more — a workbook's JPEG logo draws here as it does in the browser. With
    // the decoders compiled out they are named as they always were, so the
    // no-silent-loss rule holds in both builds.
    #[cfg(feature = "raster")]
    if raster_format(bytes).is_some() {
        return None;
    }
    // The full name table, in **both** feature configurations. With the
    // decoders compiled out these are reached and reported exactly as they
    // always were; naming a JPEG "an unrecognised format" because a decoder was
    // not built would be a report that got worse without anybody deciding to
    // weaken it.
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

/// The raster format `bytes` look like, or `None` if they are not one this
/// backend decodes.
///
/// **Sniffed here rather than left to the decoder's own guess**, for the reason
/// the PNG path already gave: the part's extension and its content type are the
/// file's claim about itself, and neither is what the decoder will meet. It also
/// keeps the set of formats this accepts a list in one place rather than a
/// property of whichever codecs a dependency happened to compile in.
///
/// Vector formats are **not** here and are refused by name. `emf`, `wmf` and
/// `svg` need a renderer, not a decoder — a drawing to be executed rather than
/// pixels to be unpacked — and half-executing one produces a picture that is
/// wrong rather than a picture that is missing.
#[cfg(feature = "raster")]
fn raster_format(bytes: &[u8]) -> Option<image::ImageFormat> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(image::ImageFormat::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(image::ImageFormat::Gif)
    } else if bytes.starts_with(b"BM") {
        Some(image::ImageFormat::Bmp)
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        Some(image::ImageFormat::Tiff)
    } else if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        Some(image::ImageFormat::WebP)
    } else {
        None
    }
}

/// Decode a raster format into a pixmap, bounded before it allocates.
///
/// The bound is the same one the PNG path enforces and for the same reason:
/// the dimensions are read from the header **first**, so a picture claiming to
/// be 60000x60000 is refused for what it says it is rather than after the
/// allocation it asked for. A picture's bytes come from a workbook somebody
/// else wrote, which makes this the decompression-bomb seam.
///
/// `image` is also given its own allocation limit, because a header can lie:
/// belt and braces on the one path in this crate that takes arbitrary
/// compressed input.
#[cfg(feature = "raster")]
fn decode_raster(
    bytes: &[u8],
    format: image::ImageFormat,
) -> Result<tiny_skia::Pixmap, UndrawnReason> {
    use image::ImageDecoder as _;

    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_IMAGE_PIXELS * 4);

    let reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| UndrawnReason::Undecodable)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(UndrawnReason::TooLarge { width, height });
    }
    decoder
        .set_limits(limits)
        .map_err(|_| UndrawnReason::TooLarge { width, height })?;

    let decoded = image::DynamicImage::from_decoder(decoder)
        .map_err(|_| UndrawnReason::Undecodable)?
        .into_rgba8();

    // Straight into tiny-skia's premultiplied buffer. `from_vec` refuses a
    // length that does not match the dimensions, so a decoder that disagreed
    // with its own header is caught here rather than drawn as garbage.
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or(UndrawnReason::Undecodable)?;
    for (dst, src) in pixmap.pixels_mut().iter_mut().zip(decoded.pixels()) {
        let [r, g, b, a] = src.0;
        *dst = tiny_skia::PremultipliedColorU8::from_rgba(
            ((u16::from(r) * u16::from(a)) / 255) as u8,
            ((u16::from(g) * u16::from(a)) / 255) as u8,
            ((u16::from(b) * u16::from(a)) / 255) as u8,
            a,
        )
        .unwrap_or_else(|| {
            tiny_skia::PremultipliedColorU8::from_rgba(0, 0, 0, 0)
                .expect("transparent is always a valid premultiplied pixel")
        });
    }
    Ok(pixmap)
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
    #[cfg(feature = "raster")]
    if let Some(format) = raster_format(bytes) {
        return decode_raster(bytes, format);
    }
    // Before `decode_png`, not after: the point of the limit is the allocation
    // it prevents, and asking afterwards has already made it.
    let (width, height) = png_dimensions(bytes).ok_or(UndrawnReason::Undecodable)?;
    if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(UndrawnReason::TooLarge { width, height });
    }
    tiny_skia::Pixmap::decode_png(bytes).map_err(|_| UndrawnReason::Undecodable)
}
