//! `casual-calc-render` — the CPU raster backend.
//!
//! Phase 1D, increment 1: executes a [`DisplayList`] for a viewport onto a
//! `tiny-skia` pixmap and encodes a PNG. It draws the grid — a white ground,
//! light gridlines at the visible row/column boundaries, and a subtle fill for
//! each cell that carries content. **Glyph text is not yet rendered** (that
//! needs a bundled font + `skrifa`, the next increment); content cells are shown
//! as highlighted rectangles so the geometry and virtualization are inspectable.
//!
//! See `docs/42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md`.

use casual_calc_layout::visible_range;
use casual_calc_layout::{DisplayList, GridGeometry, PaintItem, Rect as LayoutRect, Viewport};
use tiny_skia::{Color, Paint, Pixmap, Rect, Transform};

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

    // Content cells: a subtle fill so laid-out cells are visible before glyphs.
    let mut fill = Paint::default();
    fill.set_color(Color::from_rgba8(210, 224, 246, 255));
    fill.anti_alias = false;
    for item in &display_list.items {
        if let PaintItem::Text { rect, .. } = item
            && let Some(screen) = to_screen(rect, viewport, dpi)
        {
            pixmap.fill_rect(screen, &fill, Transform::identity(), None);
        }
    }

    Ok(pixmap)
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
