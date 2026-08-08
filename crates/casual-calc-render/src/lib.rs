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

use casual_calc_layout::visible_range;
use casual_calc_layout::{
    Align, BorderLine, DisplayList, GridGeometry, PaintItem, Rect as LayoutRect, Viewport,
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

    // First pass: total advance width, to place the run for the requested align.
    let advance = |ch: char| -> f32 {
        charmap
            .map(ch)
            .and_then(|g| glyph_metrics.advance_width(g))
            .unwrap_or(0.0)
    };
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
    for ch in content.chars() {
        if let Some(gid) = charmap.map(ch)
            && let Some(glyph) = outlines.get(gid)
        {
            let mut pen = GlyphPen {
                builder: &mut builder,
                origin_x: pen_x,
                baseline_y,
            };
            let settings = DrawSettings::unhinted(size, loc);
            let _ = glyph.draw(settings, &mut pen);
        }
        pen_x += advance(ch);
    }
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
