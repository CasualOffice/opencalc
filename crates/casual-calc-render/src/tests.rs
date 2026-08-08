//! Render tests: the raster produces a valid PNG and paints content cells.

use casual_calc_layout::{GridGeometry, Viewport, layout_full};
use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

use crate::{render_pixmap, render_png};

fn sample() -> Workbook {
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    sheet
        .cells
        .set(CellRef::new(0, 0), Cell::value(CellValue::Number(1.0)));
    wb.sheets.push(sheet);
    wb
}

fn viewport() -> Viewport {
    Viewport {
        x: 0,
        y: 0,
        width: 4 * 960,
        height: 4 * 300,
    }
}

#[test]
fn renders_a_png() {
    let wb = sample();
    let geo = GridGeometry::default();
    let list = layout_full(&wb, 0, &geo);
    let png = render_png(&list, &geo, &viewport(), 96).unwrap();
    assert!(png.len() > 8);
    // PNG magic number.
    assert_eq!(
        &png[0..8],
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
    );
}

/// Whether any pixel in `[x0,x1) x [y0,y1)` satisfies `pred(r,g,b)`.
fn any_pixel(
    pixmap: &tiny_skia::Pixmap,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    pred: impl Fn(u8, u8, u8) -> bool,
) -> bool {
    for y in y0..y1 {
        for x in x0..x1 {
            if let Some(p) = pixmap.pixel(x, y)
                && pred(p.red(), p.green(), p.blue())
            {
                return true;
            }
        }
    }
    false
}

#[test]
fn content_cell_has_glyphs_and_empty_cell_is_white() {
    let wb = sample();
    let geo = GridGeometry::default();
    let list = layout_full(&wb, 0, &geo);
    let pixmap = render_pixmap(&list, &geo, &viewport(), 96).unwrap();

    // Cell A1 (~64x20 px at 96 dpi) holds the number "1"; a real glyph is painted,
    // so some dark (near-black) pixel exists inside the cell.
    assert!(
        any_pixel(&pixmap, 0, 0, 64, 20, |r, g, b| r < 128
            && g < 128
            && b < 128),
        "A1 should contain painted glyph pixels"
    );

    // A pixel deep inside an empty cell (col 2, row 2) stays white.
    let empty = pixmap.pixel(2 * 64 + 20, 2 * 20 + 8).unwrap();
    assert!(
        empty.red() > 240 && empty.green() > 240 && empty.blue() > 240,
        "empty cell should be white, got r{} g{} b{}",
        empty.red(),
        empty.green(),
        empty.blue()
    );
}

fn styled_sample() -> Workbook {
    use casual_calc_model::{BorderEdge, Borders, Style};
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let styled = wb.intern_style(Style {
        fill_color: Some("00FF00".to_owned()),
        font_color: Some("FF0000".to_owned()),
        border: Some(Borders {
            left: Some(BorderEdge {
                style: "thin".to_owned(),
                color: Some("0000FF".to_owned()),
            }),
            ..Borders::default()
        }),
        ..Style::default()
    });
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    let mut cell = Cell::value(CellValue::Number(1.0));
    cell.style = Some(styled);
    sheet.cells.set(CellRef::new(0, 0), cell);
    wb.sheets.push(sheet);
    wb
}

#[test]
fn fill_font_and_border_are_painted() {
    let wb = styled_sample();
    let geo = GridGeometry::default();
    let list = layout_full(&wb, 0, &geo);
    let pixmap = render_pixmap(&list, &geo, &viewport(), 96).unwrap();

    // The cell fill is green: a spot away from the centered text bar and away
    // from the left border edge.
    let fill = pixmap.pixel(40, 3).unwrap();
    assert!(
        fill.green() > fill.red() && fill.green() > fill.blue() && fill.green() > 200,
        "fill should be green, got r{} g{} b{}",
        fill.red(),
        fill.green(),
        fill.blue()
    );

    // The glyph is painted in the font color (red): some reddish pixel exists
    // inside the cell.
    assert!(
        any_pixel(&pixmap, 0, 0, 64, 20, |r, g, b| r > 150
            && g < 120
            && b < 120),
        "A1 should contain red glyph pixels"
    );

    // The left border edge (column 0) is blue.
    let border = pixmap.pixel(0, 10).unwrap();
    assert!(
        border.blue() > border.red() && border.blue() > border.green() && border.blue() > 200,
        "left border should be blue, got r{} g{} b{}",
        border.red(),
        border.green(),
        border.blue()
    );
}

#[test]
fn render_is_deterministic() {
    let wb = sample();
    let geo = GridGeometry::default();
    let list = layout_full(&wb, 0, &geo);
    let vp = viewport();
    assert_eq!(
        render_png(&list, &geo, &vp, 96).unwrap(),
        render_png(&list, &geo, &vp, 96).unwrap()
    );
}
