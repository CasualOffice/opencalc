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
#[cfg(feature = "shaping")]
mod shape;

use casual_calc_layout::visible_range;
use casual_calc_layout::{
    Align, BorderLine, DisplayList, GridGeometry, PaintItem, Pane, Rect as LayoutRect, Viewport,
};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};

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
pub fn render_pixmap(
    display_list: &DisplayList,
    geometry: &GridGeometry,
    viewport: &Viewport,
    dpi: u32,
) -> Result<Pixmap, RenderError> {
    let width = twips_to_px(viewport.width, dpi).ceil() as u32;
    let height = twips_to_px(viewport.height, dpi).ceil() as u32;
    let mut pixmap =
        Pixmap::new(width, height).ok_or(RenderError::InvalidSize { width, height })?;
    pixmap.fill(Color::WHITE);

    draw_gridlines(&mut pixmap, geometry, viewport, dpi);

    // The display list is in deterministic painter's order (fills behind text,
    // borders on top). Execute it in that order.
    for item in &display_list.items {
        draw_item(&mut pixmap, item, viewport, dpi);
    }

    Ok(pixmap)
}

fn draw_item(pixmap: &mut Pixmap, item: &PaintItem, viewport: &Viewport, dpi: u32) {
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

/// The default font size (points) for a Text item that carries no explicit size.
const DEFAULT_FONT_PT: f32 = 11.0;
/// Cell text inset from the left/right edge, in twips (~2px at 96 dpi).
const TEXT_PAD_TWIPS: i64 = 30;

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
    let width = twips_to_px(viewport.width.max(0), dpi).ceil() as u32;
    let height = twips_to_px(viewport.height.max(0), dpi).ceil() as u32;
    let mut pixmap =
        Pixmap::new(width, height).ok_or(RenderError::InvalidSize { width, height })?;
    pixmap.fill(Color::WHITE);

    for paint in panes {
        let rendered = render_pixmap(paint.display_list, geometry, &paint.pane.viewport, dpi)?;
        blit(
            &mut pixmap,
            &rendered,
            twips_to_px(paint.pane.origin.0, dpi).round() as i64,
            twips_to_px(paint.pane.origin.1, dpi).round() as i64,
        );
    }

    draw_freeze_lines(&mut pixmap, panes, dpi);
    Ok(pixmap)
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
