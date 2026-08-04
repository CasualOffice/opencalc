//! Layout tests: geometry, the visible-range query, and the virtualization
//! invariant (viewport output == full output restricted to the window).

use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

use crate::{
    DEFAULT_COL_WIDTH, DEFAULT_ROW_HEIGHT, GridGeometry, Viewport, layout_full, layout_viewport,
    visible_range,
};

fn sample() -> Workbook {
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    sheet
        .cells
        .set(CellRef::new(0, 0), Cell::value(CellValue::Number(1.0)));
    sheet
        .cells
        .set(CellRef::new(0, 5), Cell::value(CellValue::Number(2.0)));
    sheet
        .cells
        .set(CellRef::new(10, 0), Cell::value(CellValue::Number(3.0)));
    sheet
        .cells
        .set(CellRef::new(10, 5), Cell::value(CellValue::Number(4.0)));
    wb.sheets.push(sheet);
    wb
}

#[test]
fn visible_range_is_computed_from_geometry() {
    let geo = GridGeometry::default();
    let vp = Viewport {
        x: 0,
        y: 0,
        width: 2 * DEFAULT_COL_WIDTH + 10,
        height: 3 * DEFAULT_ROW_HEIGHT + 10,
    };
    let range = visible_range(&geo, &vp);
    assert_eq!(range.cols, (0, 2));
    assert_eq!(range.rows, (0, 3));
}

#[test]
fn full_covering_viewport_equals_full_layout() {
    let wb = sample();
    let geo = GridGeometry::default();
    let full = layout_full(&wb, 0, &geo);
    let vp = Viewport {
        x: 0,
        y: 0,
        width: 1_000_000,
        height: 1_000_000,
    };
    let viewport = layout_viewport(&wb, 0, &geo, &vp);
    assert_eq!(
        full, viewport,
        "a viewport covering everything must equal the full layout"
    );
    assert_eq!(full.items.len(), 4);
}

#[test]
fn partial_viewport_is_a_subset_of_full() {
    let wb = sample();
    let geo = GridGeometry::default();
    let full = layout_full(&wb, 0, &geo);

    // A window over roughly the top-left cell only.
    let vp = Viewport {
        x: 0,
        y: 0,
        width: DEFAULT_COL_WIDTH,
        height: DEFAULT_ROW_HEIGHT,
    };
    let viewport = layout_viewport(&wb, 0, &geo, &vp);

    assert_eq!(viewport.items.len(), 1, "only the top-left cell is visible");
    for item in &viewport.items {
        assert!(
            full.items.contains(item),
            "every viewport item must appear in the full layout"
        );
    }
}

#[test]
fn cell_positions_follow_the_offset_index() {
    use crate::{Align, PaintItem};
    let wb = sample();
    let geo = GridGeometry::default();
    let list = layout_full(&wb, 0, &geo);

    // The cell at (row 10, col 5) sits at (5*width, 10*height).
    let target = list
        .items
        .iter()
        .find_map(|i| match i {
            PaintItem::Text { rect, content, .. } if content == "4" => Some(*rect),
            _ => None,
        })
        .expect("cell (10,5) laid out");
    assert_eq!(target.x, 5 * DEFAULT_COL_WIDTH);
    assert_eq!(target.y, 10 * DEFAULT_ROW_HEIGHT);
    assert_eq!(target.w, DEFAULT_COL_WIDTH);
    assert_eq!(target.h, DEFAULT_ROW_HEIGHT);

    // Numbers are right-aligned.
    assert!(matches!(
        list.items[0],
        PaintItem::Text {
            align: Align::Right,
            ..
        }
    ));
}

#[test]
fn display_text_applies_the_cell_number_format() {
    use crate::{PaintItem, display_text};
    use casual_calc_model::Style;

    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let percent = wb.intern_style(Style {
        number_format: Some("0%".to_owned()),
    });
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    let mut cell = Cell::value(CellValue::Number(0.5));
    cell.style = Some(percent);
    sheet.cells.set(CellRef::new(0, 0), cell.clone());
    wb.sheets.push(sheet);

    assert_eq!(display_text(&wb, &cell), "50%");

    let list = layout_full(&wb, 0, &GridGeometry::default());
    assert!(matches!(
        &list.items[0],
        PaintItem::Text { content, .. } if content == "50%"
    ));
}

#[test]
fn layout_is_deterministic() {
    let wb = sample();
    let geo = GridGeometry::default();
    assert_eq!(layout_full(&wb, 0, &geo), layout_full(&wb, 0, &geo));
}
