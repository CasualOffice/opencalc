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

#[test]
fn content_cell_is_filled_and_empty_cell_is_white() {
    let wb = sample();
    let geo = GridGeometry::default();
    let list = layout_full(&wb, 0, &geo);
    let pixmap = render_pixmap(&list, &geo, &viewport(), 96).unwrap();

    // A pixel near the middle of cell A1 (col 0, row 0) should carry the content fill.
    let a1 = pixmap.pixel(20, 8).unwrap();
    assert!(
        a1.blue() > a1.red() && a1.blue() > 200,
        "A1 should be filled bluish, got r{} g{} b{}",
        a1.red(),
        a1.green(),
        a1.blue()
    );

    // A pixel deep inside an empty cell (col 2, row 2) should stay white.
    let empty = pixmap.pixel(2 * 64 + 20, 2 * 20 + 8).unwrap();
    assert!(
        empty.red() > 240 && empty.green() > 240 && empty.blue() > 240,
        "empty cell should be white, got r{} g{} b{}",
        empty.red(),
        empty.green(),
        empty.blue()
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
