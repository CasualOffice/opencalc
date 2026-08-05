//! `casual-calc-render` — the CPU raster backend.
//!
//! Phase 1D, increment 1: executes a [`DisplayList`] for a viewport onto a
//! `tiny-skia` pixmap and encodes a PNG. It draws the grid — a white ground,
//! light gridlines at the visible row/column boundaries, solid cell fills, and
//! cell border edges — from the display list's paint items, in painter's order.
//! **Glyph text is not yet rendered** (that needs a bundled font + `skrifa`, the
//! next increment); each text cell is marked by an inset bar painted in its font
//! color so geometry, color, and virtualization stay inspectable.
//!
//! See `docs/42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md`.

use casual_calc_layout::visible_range;
use casual_calc_layout::{
    BorderLine, DisplayList, GridGeometry, PaintItem, Rect as LayoutRect, Viewport,
};
use tiny_skia::{Color, Paint, Pixmap, Rect, Transform};

/// The neutral color used for a text placeholder when the cell sets no font
/// color (glyphs are not yet rendered; a bar marks laid-out text).
fn neutral_text() -> Color {
    Color::from_rgba8(210, 224, 246, 255)
}
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
        PaintItem::Text { rect, color, .. } => {
            // Glyphs are not yet rendered; paint an inset bar in the font color
            // (neutral when unset) to mark laid-out text without hiding fills.
            let color = color
                .as_deref()
                .and_then(parse_hex_color)
                .unwrap_or_else(neutral_text);
            draw_text_marker(pixmap, rect, viewport, dpi, color);
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

/// Paint the inset bar that stands in for a line of cell text.
fn draw_text_marker(
    pixmap: &mut Pixmap,
    rect: &LayoutRect,
    viewport: &Viewport,
    dpi: u32,
    color: Color,
) {
    let x = twips_to_px(rect.x - viewport.x, dpi);
    let y = twips_to_px(rect.y - viewport.y, dpi);
    let w = twips_to_px(rect.w, dpi);
    let h = twips_to_px(rect.h, dpi);
    let bar_w = w * 0.8;
    let bar_h = (h * 0.25).max(2.0);
    let bar_x = x + (w - bar_w) / 2.0;
    let bar_y = y + (h - bar_h) / 2.0;

    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = false;
    fill_thin(pixmap, &paint, bar_x, bar_y, bar_w, bar_h);
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
