//! `casual-calc-render` — the CPU raster backend.
//!
//! Phase 1D, increment 1: executes a [`DisplayList`] for a viewport onto a
//! `tiny-skia` pixmap and encodes a PNG. It draws the grid — a white ground,
//! light gridlines at the visible row/column boundaries, solid cell fills, cell
//! border edges, and **cell text as real glyph outlines** — from the display
//! list's paint items, in painter's order. Text is drawn by resolving each
//! cell's font (via the shared substitution table) to a bundled face and
//! outlining its glyphs with `skrifa` into a `tiny-skia` path.
//!
//! See `docs/42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md`.

mod fonts;
mod images;

pub use fonts::{MissingScript, missing_scripts, register_face, registered_count};
pub use images::{
    ImageReport, ImageSource, MAX_IMAGE_PIXELS, NoImages, UndrawnImage, UndrawnReason,
};
#[cfg(feature = "shaping")]
mod shape;

use casual_calc_layout::visible_range;
use casual_calc_layout::{
    Align, BorderLine, DisplayList, GridGeometry, PaintItem, Pane, Rect as LayoutRect, Viewport,
};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};
use tiny_skia::{
    Color, FillRule, FilterQuality, Paint, PathBuilder, Pattern, Pixmap, Rect, SpreadMode,
    Transform,
};

/// The default border color when an edge specifies none.
fn default_border() -> Color {
    Color::BLACK
}

/// An error rendering to pixels.
#[derive(Debug)]
#[non_exhaustive]
pub enum RenderError {
    /// The requested surface size is zero or too large.
    InvalidSize {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
    },
    /// PNG encoding failed.
    Encode,
}

impl RenderError {
    /// The stable diagnostic code (`docs/20`).
    pub fn code(&self) -> &'static str {
        "OC-RND-0001"
    }
}

impl core::fmt::Display for RenderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RenderError::InvalidSize { width, height } => {
                write!(f, "[{}] invalid surface size {width}x{height}", self.code())
            }
            RenderError::Encode => write!(f, "[{}] PNG encoding failed", self.code()),
        }
    }
}

impl std::error::Error for RenderError {}

fn twips_to_px(twips: i64, dpi: u32) -> f32 {
    twips as f32 * dpi as f32 / 1440.0
}

fn to_screen(rect: &LayoutRect, viewport: &Viewport, dpi: u32) -> Option<Rect> {
    Rect::from_xywh(
        twips_to_px(rect.x - viewport.x, dpi),
        twips_to_px(rect.y - viewport.y, dpi),
        twips_to_px(rect.w, dpi),
        twips_to_px(rect.h, dpi),
    )
}

/// Render a viewport's display list to a `tiny-skia` pixmap.
///
/// **Draws no pictures.** There is nowhere in this signature to hand over the
/// media a [`PaintItem::Image`] refers to, and nowhere to return what could not
/// be drawn, so a sheet with pictures renders without them and says nothing —
/// which is why [`render_pixmap_with_images`] exists and is what a host that
/// cares about fidelity should call.
pub fn render_pixmap(
    display_list: &DisplayList,
    geometry: &GridGeometry,
    viewport: &Viewport,
    dpi: u32,
) -> Result<Pixmap, RenderError> {
    render_pixmap_with_images(display_list, geometry, viewport, dpi, &NoImages).map(|(p, _)| p)
}

/// Render a viewport's display list, drawing the pictures `images` supplies.
///
/// Returns the surface and an [`ImageReport`]: what was drawn, and what was
/// not, named with the reason. A picture that cannot be drawn is **reported**
/// rather than skipped — a blank frame where a logo should be is
/// indistinguishable from a file that never had one.
pub fn render_pixmap_with_images(
    display_list: &DisplayList,
    geometry: &GridGeometry,
    viewport: &Viewport,
    dpi: u32,
    images: &dyn ImageSource,
) -> Result<(Pixmap, ImageReport), RenderError> {
    let width = twips_to_px(viewport.width, dpi).ceil() as u32;
    let height = twips_to_px(viewport.height, dpi).ceil() as u32;
    let mut pixmap =
        Pixmap::new(width, height).ok_or(RenderError::InvalidSize { width, height })?;
    pixmap.fill(Color::WHITE);

    draw_gridlines(&mut pixmap, geometry, viewport, dpi);

    // The display list is in deterministic painter's order (fills behind text,
    // borders on top, pictures over the lot). Execute it in that order.
    let mut report = ImageReport::default();
    for item in &display_list.items {
        draw_item(&mut pixmap, item, viewport, dpi, images, &mut report);
    }

    Ok((pixmap, report))
}

fn draw_item(
    pixmap: &mut Pixmap,
    item: &PaintItem,
    viewport: &Viewport,
    dpi: u32,
    images: &dyn ImageSource,
    report: &mut ImageReport,
) {
    match item {
        PaintItem::CellBackground { rect, fill } => {
            let Some(color) = fill.as_deref().and_then(parse_hex_color) else {
                return;
            };
            if let Some(screen) = to_screen(rect, viewport, dpi) {
                let mut paint = Paint::default();
                paint.set_color(color);
                paint.anti_alias = false;
                pixmap.fill_rect(screen, &paint, Transform::identity(), None);
            }
        }
        PaintItem::GridLine { rect } => {
            if let Some(screen) = to_screen(rect, viewport, dpi) {
                let mut paint = Paint::default();
                paint.set_color(Color::from_rgba8(224, 224, 224, 255));
                paint.anti_alias = false;
                pixmap.fill_rect(screen, &paint, Transform::identity(), None);
            }
        }
        PaintItem::MergedRegion { rect, fill } => {
            let Some(screen) = to_screen(rect, viewport, dpi) else {
                return;
            };
            // The anchor's fill if it has one, the ground otherwise, across the
            // whole range — which erases the gridlines `draw_gridlines` laid
            // down between the cells it covers, since they are not cell
            // boundaries any more. A merged header still ruled into three is
            // the visible half of RND-03.
            let mut ground = Paint::default();
            ground.set_color(
                fill.as_deref()
                    .and_then(parse_hex_color)
                    .unwrap_or(Color::WHITE),
            );
            ground.anti_alias = false;
            pixmap.fill_rect(screen, &ground, Transform::identity(), None);

            // Then one outline for the range itself, in the gridline colour, so
            // it reads as the single cell it is rather than as a gap in the grid.
            let mut line = Paint::default();
            line.set_color(Color::from_rgba8(224, 224, 224, 255));
            line.anti_alias = false;
            let (x, y, w, h) = (screen.x(), screen.y(), screen.width(), screen.height());
            fill_thin(pixmap, &line, x, y, w, 1.0);
            fill_thin(pixmap, &line, x, y + h - 1.0, w, 1.0);
            fill_thin(pixmap, &line, x, y, 1.0, h);
            fill_thin(pixmap, &line, x + w - 1.0, y, 1.0, h);
        }
        PaintItem::DataBar {
            rect,
            fraction,
            color,
        } => {
            draw_data_bar(pixmap, rect, *fraction, color, viewport, dpi);
        }
        PaintItem::Text {
            rect,
            content,
            align,
            color,
            bold,
            italic,
            font_name,
            font_pt,
        } => {
            let color = color
                .as_deref()
                .and_then(parse_hex_color)
                .unwrap_or(Color::BLACK);
            draw_glyphs(
                pixmap,
                rect,
                content,
                *align,
                color,
                *bold,
                *italic,
                font_name.as_deref(),
                font_pt.unwrap_or(DEFAULT_FONT_PT),
                viewport,
                dpi,
            );
        }
        PaintItem::Image { rect, part } => {
            draw_image(pixmap, rect, part, viewport, dpi, images, report);
        }
        PaintItem::CellBorder {
            rect,
            left,
            right,
            top,
            bottom,
        } => {
            draw_borders(pixmap, rect, viewport, dpi, left, right, top, bottom);
        }
    }
}

/// Paint one picture into its frame, or record why it could not be.
///
/// The frame is filled with the decoded picture as a **pattern** whose
/// transform maps the source's own pixel rectangle onto the frame exactly,
/// rather than blitted a pixel at a time. That scale is what fits the picture
/// to its frame — resampled rather than point-sampled, so a photograph shrunk
/// into a few cells is not aliased into noise — and it is also what bounds the
/// picture: the pattern's extent *is* the frame, so the fill and the picture
/// stop in the same place whatever rectangle is filled.
///
/// `SpreadMode::Pad` decides what the sampler sees at that boundary: the
/// picture's own edge colour rather than transparent black, so a frame whose
/// pixel size is not an exact multiple of the source's has no fringe along its
/// edges.
///
/// **The picture fills its frame; it is not letterboxed inside it.** The anchor
/// *is* the extent in OOXML — dragging a corner handle in Excel distorts the
/// picture, and a `twoCellAnchor` records the result — so fitting it inside and
/// centring it would put the picture somewhere the file does not say it is.
/// Note that the editor canvas does letterbox (`drawImages` in
/// `webapp/editor.js`), so the two disagree today; the canvas is the one that
/// does not match Excel, and that is tracked rather than silently copied here.
fn draw_image(
    pixmap: &mut Pixmap,
    rect: &LayoutRect,
    part: &str,
    viewport: &Viewport,
    dpi: u32,
    images: &dyn ImageSource,
    report: &mut ImageReport,
) {
    // Layout does not emit a frame with no area, so this is unreachable for a
    // display list this crate was given rather than handed synthetically. There
    // is nothing lost to report either way: a frame of no size shows nothing.
    let Some(screen) = to_screen(rect, viewport, dpi) else {
        return;
    };
    // Decoded here and dropped at the end of this call, rather than cached
    // across the render. One picture is in memory at a time, so a sheet of
    // twenty photographs costs the largest of them and not the sum — which is
    // the bound worth keeping, since `MAX_IMAGE_PIXELS` alone allows 64 MB
    // apiece. The cost is that a picture anchored across a freeze boundary is
    // decoded once per pane it appears in.
    let source = match images::decode(part, images) {
        Ok(pixmap) => pixmap,
        Err(reason) => {
            report.missed(part, reason);
            return;
        }
    };
    // A zero-dimension pixmap cannot be constructed, so both ratios are finite.
    let scale_x = screen.width() / source.width() as f32;
    let scale_y = screen.height() / source.height() as f32;
    let paint = Paint {
        shader: Pattern::new(
            source.as_ref(),
            SpreadMode::Pad,
            FilterQuality::Bilinear,
            1.0,
            Transform::from_row(scale_x, 0.0, 0.0, scale_y, screen.x(), screen.y()),
        ),
        // The picture's own edges are wherever its pixels stop; anti-aliasing
        // the frame would fringe them against whatever the cells underneath
        // happen to be, differently at every scroll position.
        anti_alias: false,
        ..Paint::default()
    };
    pixmap.fill_rect(screen, &paint, Transform::identity(), None);
    report.drew();
}

/// The default font size (points) for a Text item that carries no explicit size.
const DEFAULT_FONT_PT: f32 = 11.0;
/// A data bar's inset from the cell's left/right edge, in pixels, so the bar
/// reads as sitting inside the cell rather than as the cell's own fill.
/// Whether this build can shape text.
///
/// `ADR-018` gates shaping behind a Cargo feature — on for native, off for
/// WebAssembly, where the browser has a shaper already and the bundle does not
/// need a second one. [`64`](../../../docs/64-TEXT-SHAPING.md) promises that a
/// build without it **says so** "rather than silently producing wrong output …
/// a caller rendering a thumbnail can then decide, rather than discovering it
/// from a customer".
///
/// Nothing could be asked until now (`DOC-031`). A caller that renders Arabic,
/// Hebrew, Devanagari or Thai on an unshaped build gets a picture with the
/// glyphs in the wrong order and no indication of it, which is the one outcome
/// that promise rules out.
///
/// `const`, so a host can branch on it without a call and a test can assert it
/// against the feature it was built with.
#[must_use]
pub const fn shaping_available() -> bool {
    cfg!(feature = "shaping")
}

/// Whether `text` is written in a script that needs shaping to be correct.
///
/// Latin, Greek, Cyrillic and CJK render acceptably glyph-by-glyph. Arabic and
/// Hebrew are cursive or right-to-left; Devanagari, Bengali, Tamil, Thai and
/// Khmer reorder and combine. Drawing those per `char` does not produce
/// slightly-worse text — it produces text a reader of that script cannot read.
///
/// Deliberately a *script* question and not a font question: it is answerable
/// from the string alone, so a caller can ask it before rendering anything.
#[must_use]
pub fn needs_shaping(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(u32::from(c),
            // Arabic, Hebrew, Syriac, Thaana, N'Ko.
            0x0590..=0x08FF
            // Devanagari through Malayalam, Sinhala, Thai, Lao, Tibetan.
            | 0x0900..=0x0FFF
            // Khmer, Myanmar.
            | 0x1000..=0x109F | 0x1780..=0x17FF
            // Arabic Presentation Forms.
            | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
    })
}

pub const DATA_BAR_PAD_X: f32 = 1.0;
/// A data bar's inset from the cell's top/bottom edge, in pixels.
pub const DATA_BAR_PAD_Y: f32 = 2.0;
/// How opaque a data bar is. The number it annotates is drawn on top of it and
/// has to stay readable, so the bar is a wash rather than a block — the same
/// value the editor canvas uses, so the two backends agree.
pub const DATA_BAR_ALPHA: f32 = 0.45;
/// The bar colour for a rule that names none (Excel's default data-bar blue).
/// The colour a data bar uses when its rule names none, as `RRGGBB`.
///
/// Excel's default data-bar blue. A string rather than a `Color` because this
/// is what leaves the engine — the browser canvas needs the same value and
/// cannot take a `tiny_skia::Color`.
pub const DEFAULT_DATA_BAR: &str = "638EC6";

/// Everything needed to draw a data bar, for a renderer that is not this one.
///
/// **The canvas in the editor draws its own bars**, because there is no display
/// list on that side of the WebAssembly boundary — building one is a far larger
/// change than `RND-08` reads as. What that row is actually about is the two
/// renderers being able to disagree, and they could: the inset, the alpha and
/// the default colour were written out twice, in Rust and in JavaScript, and
/// agreed only because somebody had copied them across.
///
/// Exporting them makes this file the single place they are decided. The canvas
/// still does its own painting; it no longer does its own *deciding*.
#[must_use]
pub const fn data_bar_style() -> DataBarStyle {
    DataBarStyle {
        pad_x: DATA_BAR_PAD_X,
        pad_y: DATA_BAR_PAD_Y,
        alpha: DATA_BAR_ALPHA,
        default_color: DEFAULT_DATA_BAR,
    }
}

/// The geometry and colour of a data bar. See [`data_bar_style`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DataBarStyle {
    /// Inset from the cell's left and right edges, in pixels.
    pub pad_x: f32,
    /// Inset from the cell's top and bottom edges, in pixels.
    pub pad_y: f32,
    /// How opaque the bar is, `0.0`–`1.0`.
    pub alpha: f32,
    /// The bar colour when the rule names none, as `RRGGBB`.
    pub default_color: &'static str,
}

fn default_data_bar() -> Color {
    // Parsed from the exported constant rather than written out again, so this
    // renderer and the browser canvas cannot end up with different blues — the
    // whole point of `data_bar_style` (`RND-08`).
    parse_hex_color(DEFAULT_DATA_BAR).unwrap_or(Color::BLACK)
}

/// Cell text inset from the left/right edge, in twips (~2px at 96 dpi).
const TEXT_PAD_TWIPS: i64 = 30;

/// Paint a conditional-formatting data bar: `fraction` of the cell's inset
/// width, from the left, in a translucent `color`.
///
/// The item carries the *cell* rectangle and the fraction, not a pre-measured
/// bar, so the inset is applied here where pixels are known.
fn draw_data_bar(
    pixmap: &mut Pixmap,
    rect: &LayoutRect,
    fraction: f64,
    color: &str,
    viewport: &Viewport,
    dpi: u32,
) {
    let Some(screen) = to_screen(rect, viewport, dpi) else {
        return;
    };
    let inner_w = (screen.width() - 2.0 * DATA_BAR_PAD_X).max(0.0);
    let inner_h = (screen.height() - 2.0 * DATA_BAR_PAD_Y).max(0.0);
    // A fraction outside 0..1 is a resolver bug, not a licence to paint outside
    // the cell: clamp rather than overflow into the neighbour.
    let width = inner_w * fraction.clamp(0.0, 1.0) as f32;
    if width <= 0.0 || inner_h <= 0.0 {
        return;
    }
    let mut fill = parse_hex_color(color).unwrap_or_else(default_data_bar);
    fill.set_alpha(DATA_BAR_ALPHA);
    let mut paint = Paint::default();
    paint.set_color(fill);
    paint.anti_alias = false;
    fill_thin(
        pixmap,
        &paint,
        screen.x() + DATA_BAR_PAD_X,
        screen.y() + DATA_BAR_PAD_Y,
        width,
        inner_h,
    );
}

/// Render a cell's text by outlining each glyph from the resolved bundled face
/// into a single `tiny-skia` path, then filling it in the font color. Glyphs are
/// laid out left-to-right using the face's own advances, vertically centered on
/// the cell, and horizontally aligned per `align`.
#[allow(clippy::too_many_arguments)]
fn draw_glyphs(
    pixmap: &mut Pixmap,
    rect: &LayoutRect,
    content: &str,
    align: Align,
    color: Color,
    bold: bool,
    italic: bool,
    font_name: Option<&str>,
    font_pt: f32,
    viewport: &Viewport,
    dpi: u32,
) {
    let bytes = fonts::face_bytes_for(font_name, bold, italic);
    let Ok(font) = FontRef::new(bytes) else {
        return;
    };
    let size_px = font_pt * dpi as f32 / 72.0;
    if size_px <= 0.0 {
        return;
    }
    let size = Size::new(size_px);
    let loc = LocationRef::default();
    let charmap = font.charmap();
    let glyph_metrics = font.glyph_metrics(size, loc);
    let metrics = font.metrics(size, loc);
    let outlines = font.outline_glyphs();

    // Per-glyph advance: the primary face when it covers `ch`, otherwise the
    // coverage-fallback face's advance for that glyph (so the run stays metric-
    // correct across faces), or 0.0 if nothing covers it.
    let advance = |ch: char| -> f32 {
        if let Some(g) = charmap.map(ch) {
            return glyph_metrics.advance_width(g).unwrap_or(0.0);
        }
        fonts::coverage_face_bytes(ch, bold, italic)
            .and_then(|bytes| FontRef::new(bytes).ok())
            .and_then(|fb| {
                let fb_charmap = fb.charmap();
                let g = fb_charmap.map(ch)?;
                fb.glyph_metrics(size, loc).advance_width(g)
            })
            .unwrap_or(0.0)
    };
    // First pass: total advance width, to place the run for the requested align.
    let total: f32 = content.chars().map(advance).sum();

    let x0 = twips_to_px(rect.x - viewport.x, dpi);
    let y0 = twips_to_px(rect.y - viewport.y, dpi);
    let w = twips_to_px(rect.w, dpi);
    let h = twips_to_px(rect.h, dpi);
    let pad = twips_to_px(TEXT_PAD_TWIPS, dpi);
    let mut pen_x = match align {
        Align::Left => x0 + pad,
        Align::Right => x0 + w - pad - total,
    };
    // Vertically center the ascent/descent band within the cell.
    let text_h = metrics.ascent - metrics.descent;
    let baseline_y = y0 + ((h - text_h) / 2.0).max(0.0) + metrics.ascent;

    let mut builder = PathBuilder::new();

    // Shaped path first, when the build has a shaper and one face covers the
    // whole run. A run spanning two faces falls through to the per-`char` loop
    // below: shaping is defined against a single face, and shaping each part
    // separately would place them by two different sets of rules.
    #[cfg(feature = "shaping")]
    if let Some(face_bytes) = single_face_for(content, font_name, bold, italic)
        && let Some(shaped) = shape::run(face_bytes, content, size_px)
        && let Ok(shaped_font) = FontRef::new(face_bytes)
    {
        let shaped_outlines = shaped_font.outline_glyphs();
        let total_shaped: f32 = shaped.iter().map(|g| g.advance).sum();
        let mut x = match align {
            Align::Left => x0 + pad,
            Align::Right => x0 + w - pad - total_shaped,
        };
        for glyph in &shaped {
            if let Some(outline) = shaped_outlines.get(skrifa::GlyphId::from(glyph.id)) {
                let mut pen = GlyphPen {
                    builder: &mut builder,
                    origin_x: x + glyph.x_offset,
                    baseline_y: baseline_y - glyph.y_offset,
                };
                let _ = outline.draw(DrawSettings::unhinted(size, loc), &mut pen);
            }
            x += glyph.advance;
        }
        finish_text(pixmap, builder, color);
        return;
    }

    for ch in content.chars() {
        if let Some(gid) = charmap.map(ch) {
            // Primary face covers this char: outline from it.
            if let Some(glyph) = outlines.get(gid) {
                let mut pen = GlyphPen {
                    builder: &mut builder,
                    origin_x: pen_x,
                    baseline_y,
                };
                let settings = DrawSettings::unhinted(size, loc);
                let _ = glyph.draw(settings, &mut pen);
            }
        } else if let Some(bytes) = fonts::coverage_face_bytes(ch, bold, italic)
            && let Ok(fb) = FontRef::new(bytes)
        {
            // Coverage fallback: outline from the first family that covers `ch`,
            // accumulating into the same builder (uniform fill color) at the same
            // pen position, size, and baseline.
            let fb_charmap = fb.charmap();
            let fb_outlines = fb.outline_glyphs();
            if let Some(gid) = fb_charmap.map(ch)
                && let Some(glyph) = fb_outlines.get(gid)
            {
                let mut pen = GlyphPen {
                    builder: &mut builder,
                    origin_x: pen_x,
                    baseline_y,
                };
                let settings = DrawSettings::unhinted(size, loc);
                let _ = glyph.draw(settings, &mut pen);
            }
        }
        pen_x += advance(ch);
    }
    finish_text(pixmap, builder, color);
}

/// The one face that covers every character of `content`, if there is one.
///
/// Shaping is defined against a single face. A run spanning two of them — a
/// Latin word beside a Hebrew one, where the bundled families split the
/// coverage — has no single set of rules to shape by, so the caller keeps the
/// per-`char` path for it. That path is already correct for exactly the case it
/// is kept for: unshaped, left to right, one glyph per character.
#[cfg(feature = "shaping")]
fn single_face_for(
    content: &str,
    font_name: Option<&str>,
    bold: bool,
    italic: bool,
) -> Option<&'static [u8]> {
    let primary = fonts::face_bytes_for(font_name, bold, italic);
    if let Ok(font) = FontRef::new(primary) {
        let charmap = font.charmap();
        if content.chars().all(|ch| charmap.map(ch).is_some()) {
            return Some(primary);
        }
    }
    // Otherwise the first fallback that covers the whole run, which is what a
    // single-script cell in a script the primary lacks looks like.
    let first = content.chars().next()?;
    let candidate = fonts::coverage_face_bytes(first, bold, italic)?;
    let font = FontRef::new(candidate).ok()?;
    let charmap = font.charmap();
    content
        .chars()
        .all(|ch| charmap.map(ch).is_some())
        .then_some(candidate)
}

/// Fill an accumulated glyph path.
///
/// Extracted so the shaped and per-`char` paths end the same way rather than
/// each growing their own copy of it.
fn finish_text(pixmap: &mut Pixmap, builder: PathBuilder, color: Color) {
    let Some(path) = builder.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

/// A `skrifa` outline pen that appends glyph contours to a `tiny-skia` path,
/// translating font space (origin at the glyph, y-up) to device space
/// (`origin_x`, `baseline_y`, y-down).
struct GlyphPen<'a> {
    builder: &'a mut PathBuilder,
    origin_x: f32,
    baseline_y: f32,
}

impl GlyphPen<'_> {
    fn map(&self, x: f32, y: f32) -> (f32, f32) {
        (self.origin_x + x, self.baseline_y - y)
    }
}

impl OutlinePen for GlyphPen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (px, py) = self.map(x, y);
        self.builder.move_to(px, py);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (px, py) = self.map(x, y);
        self.builder.line_to(px, py);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (cx, cy) = self.map(cx0, cy0);
        let (px, py) = self.map(x, y);
        self.builder.quad_to(cx, cy, px, py);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (c0x, c0y) = self.map(cx0, cy0);
        let (c1x, c1y) = self.map(cx1, cy1);
        let (px, py) = self.map(x, y);
        self.builder.cubic_to(c0x, c0y, c1x, c1y, px, py);
    }
    fn close(&mut self) {
        self.builder.close();
    }
}

/// Paint the present border edges as thin rects along the cell rectangle.
#[allow(clippy::too_many_arguments)]
fn draw_borders(
    pixmap: &mut Pixmap,
    rect: &LayoutRect,
    viewport: &Viewport,
    dpi: u32,
    left: &Option<BorderLine>,
    right: &Option<BorderLine>,
    top: &Option<BorderLine>,
    bottom: &Option<BorderLine>,
) {
    let x = twips_to_px(rect.x - viewport.x, dpi);
    let y = twips_to_px(rect.y - viewport.y, dpi);
    let w = twips_to_px(rect.w, dpi);
    let h = twips_to_px(rect.h, dpi);

    let mut stroke = |bx: f32, by: f32, bw: f32, bh: f32, line: &BorderLine| {
        let color = line
            .color
            .as_deref()
            .and_then(parse_hex_color)
            .unwrap_or_else(default_border);
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = false;
        fill_thin(pixmap, &paint, bx, by, bw, bh);
    };

    if let Some(line) = left {
        stroke(x, y, line.width as f32, h, line);
    }
    if let Some(line) = right {
        let px = line.width as f32;
        stroke(x + w - px, y, px, h, line);
    }
    if let Some(line) = top {
        stroke(x, y, w, line.width as f32, line);
    }
    if let Some(line) = bottom {
        let px = line.width as f32;
        stroke(x, y + h - px, w, px, line);
    }
}

/// Parse an `RRGGBB` or `AARRGGBB` (or `#`-prefixed) hex color, or `None` if it
/// is not valid hex.
fn parse_hex_color(hex: &str) -> Option<Color> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    let (r, g, b, a) = match hex.len() {
        6 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            255,
        ),
        8 => (
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            u8::from_str_radix(&hex[6..8], 16).ok()?,
            u8::from_str_radix(&hex[0..2], 16).ok()?,
        ),
        _ => return None,
    };
    Some(Color::from_rgba8(r, g, b, a))
}

/// Render a viewport to a PNG byte buffer.
///
/// Draws no pictures, for the reason [`render_pixmap`] gives.
pub fn render_png(
    display_list: &DisplayList,
    geometry: &GridGeometry,
    viewport: &Viewport,
    dpi: u32,
) -> Result<Vec<u8>, RenderError> {
    render_pixmap(display_list, geometry, viewport, dpi)?
        .encode_png()
        .map_err(|_| RenderError::Encode)
}

/// Render a viewport to a PNG, drawing the pictures `images` supplies.
pub fn render_png_with_images(
    display_list: &DisplayList,
    geometry: &GridGeometry,
    viewport: &Viewport,
    dpi: u32,
    images: &dyn ImageSource,
) -> Result<(Vec<u8>, ImageReport), RenderError> {
    let (pixmap, report) =
        render_pixmap_with_images(display_list, geometry, viewport, dpi, images)?;
    let png = pixmap.encode_png().map_err(|_| RenderError::Encode)?;
    Ok((png, report))
}

/// A pane and the display list laid out for it.
///
/// The two are separate arguments in every other signature here; a frozen
/// sheet has several of each and they must not be paired up by index at the
/// call site, where getting it wrong renders the body's cells in the corner.
#[derive(Debug, Clone, Copy)]
pub struct PanePaint<'a> {
    /// Which region of the image, and what it looks at.
    pub pane: Pane,
    /// That region's display list, from
    /// [`layout_viewport`](casual_calc_layout::layout_viewport) on
    /// [`pane.viewport`](Pane::viewport).
    pub display_list: &'a DisplayList,
}

/// Render a split viewport — the frozen-pane composition.
///
/// Each pane is rendered as its own image and copied into place, which is what
/// makes the frozen bands hold still: they are looking at a different part of
/// the sheet from the body, not at the same part drawn twice. Panes are clipped
/// at the image edges, so a pane larger than the room left for it loses its
/// overhang rather than the picture.
///
/// `viewport` is the whole image, the same one the panes were split from.
/// Passing [`panes`](casual_calc_layout::panes) output for a sheet with no
/// freeze produces exactly what [`render_pixmap`] would.
pub fn render_panes(
    panes: &[PanePaint<'_>],
    geometry: &GridGeometry,
    viewport: &Viewport,
    dpi: u32,
) -> Result<Pixmap, RenderError> {
    render_panes_with_images(panes, geometry, viewport, dpi, &NoImages).map(|(p, _)| p)
}

/// Render a split viewport, drawing the pictures `images` supplies.
///
/// A picture straddling a freeze boundary is laid out into more than one pane
/// and therefore drawn more than once, which is what makes it hold still in the
/// frozen band and scroll in the body. The reports are folded together, so a
/// picture that could not be drawn in three panes is one entry, not three.
pub fn render_panes_with_images(
    panes: &[PanePaint<'_>],
    geometry: &GridGeometry,
    viewport: &Viewport,
    dpi: u32,
    images: &dyn ImageSource,
) -> Result<(Pixmap, ImageReport), RenderError> {
    let width = twips_to_px(viewport.width.max(0), dpi).ceil() as u32;
    let height = twips_to_px(viewport.height.max(0), dpi).ceil() as u32;
    let mut pixmap =
        Pixmap::new(width, height).ok_or(RenderError::InvalidSize { width, height })?;
    pixmap.fill(Color::WHITE);

    let mut report = ImageReport::default();
    for paint in panes {
        let (rendered, pane_report) = render_pixmap_with_images(
            paint.display_list,
            geometry,
            &paint.pane.viewport,
            dpi,
            images,
        )?;
        report.absorb(pane_report);
        blit(
            &mut pixmap,
            &rendered,
            twips_to_px(paint.pane.origin.0, dpi).round() as i64,
            twips_to_px(paint.pane.origin.1, dpi).round() as i64,
        );
    }

    draw_freeze_lines(&mut pixmap, panes, dpi);
    Ok((pixmap, report))
}

/// Render a split viewport to a PNG byte buffer.
pub fn render_panes_png(
    panes: &[PanePaint<'_>],
    geometry: &GridGeometry,
    viewport: &Viewport,
    dpi: u32,
) -> Result<Vec<u8>, RenderError> {
    render_panes(panes, geometry, viewport, dpi)?
        .encode_png()
        .map_err(|_| RenderError::Encode)
}

/// Render a split viewport to a PNG, drawing the pictures `images` supplies.
pub fn render_panes_png_with_images(
    panes: &[PanePaint<'_>],
    geometry: &GridGeometry,
    viewport: &Viewport,
    dpi: u32,
    images: &dyn ImageSource,
) -> Result<(Vec<u8>, ImageReport), RenderError> {
    let (pixmap, report) = render_panes_with_images(panes, geometry, viewport, dpi, images)?;
    let png = pixmap.encode_png().map_err(|_| RenderError::Encode)?;
    Ok((png, report))
}

/// Copy `src` into `dst` at `(x, y)`, clipped at the destination's edges.
///
/// A row copy rather than `draw_pixmap`: the panes are opaque and land on whole
/// pixels, so there is nothing to blend or resample, and a copy is exact where
/// a shader is only very close — which matters because the render goldens
/// compare bytes.
fn blit(dst: &mut Pixmap, src: &Pixmap, x: i64, y: i64) {
    let (dst_w, dst_h) = (i64::from(dst.width()), i64::from(dst.height()));
    let (src_w, src_h) = (i64::from(src.width()), i64::from(src.height()));

    // Where the copy starts in each image once the part off the top or left of
    // the destination is dropped.
    let (dx, dy) = (x.max(0), y.max(0));
    let (sx, sy) = (dx - x, dy - y);
    let w = (src_w - sx).min(dst_w - dx);
    let h = (src_h - sy).min(dst_h - dy);
    if w <= 0 || h <= 0 {
        return;
    }

    let src_data = src.data();
    let dst_data = dst.data_mut();
    let bytes = (w * 4) as usize;
    for row in 0..h {
        let from = (((sy + row) * src_w + sx) * 4) as usize;
        let to = (((dy + row) * dst_w + dx) * 4) as usize;
        dst_data[to..to + bytes].copy_from_slice(&src_data[from..from + bytes]);
    }
}

/// Draw the boundary between a frozen band and what scrolls under it.
///
/// Darker than a gridline because it says something a gridline does not: that
/// the two sides of it move independently. Without it a frozen header reads as
/// an ordinary first row that happens to be still. The editor canvas draws the
/// same boundary in the same colour.
fn draw_freeze_lines(pixmap: &mut Pixmap, panes: &[PanePaint<'_>], dpi: u32) {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(95, 99, 104, 255));
    paint.anti_alias = false;

    let width = pixmap.width() as f32;
    let height = pixmap.height() as f32;

    // A boundary is wherever a pane starts that does not start at the edge, so
    // this needs no separate account of the freeze — the split already said it.
    let mut xs: Vec<i64> = panes
        .iter()
        .map(|p| p.pane.origin.0)
        .filter(|x| *x > 0)
        .collect();
    let mut ys: Vec<i64> = panes
        .iter()
        .map(|p| p.pane.origin.1)
        .filter(|y| *y > 0)
        .collect();
    xs.sort_unstable();
    xs.dedup();
    ys.sort_unstable();
    ys.dedup();

    for x in xs {
        fill_thin(
            pixmap,
            &paint,
            twips_to_px(x, dpi).round() - 1.0,
            0.0,
            1.0,
            height.max(1.0),
        );
    }
    for y in ys {
        fill_thin(
            pixmap,
            &paint,
            0.0,
            twips_to_px(y, dpi).round() - 1.0,
            width.max(1.0),
            1.0,
        );
    }
}

fn draw_gridlines(pixmap: &mut Pixmap, geometry: &GridGeometry, viewport: &Viewport, dpi: u32) {
    let range = visible_range(geometry, viewport);
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(224, 224, 224, 255));
    paint.anti_alias = false;

    let width = twips_to_px(viewport.width, dpi);
    let height = twips_to_px(viewport.height, dpi);

    // Vertical lines at column boundaries.
    for col in range.cols.0..=range.cols.1.saturating_add(1) {
        let x = twips_to_px(geometry.columns.offset(col) - viewport.x, dpi);
        fill_thin(pixmap, &paint, x, 0.0, 1.0, height.max(1.0));
    }
    // Horizontal lines at row boundaries.
    for row in range.rows.0..=range.rows.1.saturating_add(1) {
        let y = twips_to_px(geometry.rows.offset(row) - viewport.y, dpi);
        fill_thin(pixmap, &paint, 0.0, y, width.max(1.0), 1.0);
    }
}

fn fill_thin(pixmap: &mut Pixmap, paint: &Paint, x: f32, y: f32, w: f32, h: f32) {
    if let Some(rect) = Rect::from_xywh(x, y, w, h) {
        pixmap.fill_rect(rect, paint, Transform::identity(), None);
    }
}

#[cfg(test)]
mod tests;
