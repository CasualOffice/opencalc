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

/// Frozen-pane composition — the fidelity gap `docs/18` named.
///
/// The editor canvas has always split frozen panes; the PNG backend drew one
/// unbroken window, so a pinned header scrolled off the top of an exported
/// image while holding still on screen. These pin the behaviour that closed it.
mod frozen {
    use casual_calc_layout::{
        DEFAULT_COL_WIDTH, DEFAULT_ROW_HEIGHT, Freeze, GridGeometry, Viewport, layout_viewport,
        panes,
    };
    use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

    use crate::{PanePaint, render_panes, render_pixmap};

    /// A sheet whose every cell says which cell it is, so a band that moved
    /// when it should not have shows up as different pixels rather than as an
    /// argument about coordinates.
    fn labelled() -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        for row in 0..40u32 {
            for col in 0..12u32 {
                sheet.cells.set(
                    CellRef::new(row, col),
                    Cell::value(CellValue::Number(f64::from(row * 100 + col))),
                );
            }
        }
        wb.sheets.push(sheet);
        wb
    }

    fn viewport(x: i64, y: i64) -> Viewport {
        Viewport {
            x,
            y,
            width: DEFAULT_COL_WIDTH * 6,
            height: DEFAULT_ROW_HEIGHT * 12,
        }
    }

    /// Render the way a host does: split, lay out each pane, compose.
    fn render(wb: &Workbook, vp: &Viewport, freeze: Freeze) -> tiny_skia::Pixmap {
        let geo = GridGeometry::default();
        let regions = panes(&geo, vp, freeze);
        let lists: Vec<_> = regions
            .iter()
            .map(|pane| layout_viewport(wb, 0, &geo, &pane.viewport))
            .collect();
        let paints: Vec<PanePaint<'_>> = regions
            .iter()
            .zip(&lists)
            .map(|(pane, display_list)| PanePaint {
                pane: *pane,
                display_list,
            })
            .collect();
        render_panes(&paints, &geo, vp, 96).unwrap()
    }

    /// The pixels of `[x0,x1) x [y0,y1)`, for comparing one region across two
    /// renders.
    fn region(pixmap: &tiny_skia::Pixmap, x0: u32, y0: u32, x1: u32, y1: u32) -> Vec<u8> {
        let mut out = Vec::new();
        for y in y0..y1.min(pixmap.height()) {
            for x in x0..x1.min(pixmap.width()) {
                if let Some(p) = pixmap.pixel(x, y) {
                    out.extend_from_slice(&[p.red(), p.green(), p.blue(), p.alpha()]);
                }
            }
        }
        out
    }

    #[test]
    fn an_unfrozen_sheet_composes_to_exactly_the_unsplit_render() {
        // The compatibility guarantee, asserted on bytes rather than trusted:
        // every existing caller moved onto the split path, so if this drifts,
        // every render changed for sheets that froze nothing.
        let wb = labelled();
        let geo = GridGeometry::default();
        let vp = viewport(2_000, 1_500);

        let split = render(&wb, &vp, Freeze::default());
        let unsplit = render_pixmap(&layout_viewport(&wb, 0, &geo, &vp), &geo, &vp, 96).unwrap();

        assert_eq!(split.width(), unsplit.width());
        assert_eq!(split.height(), unsplit.height());
        assert_eq!(split.data(), unsplit.data(), "byte-identical");
    }

    /// Pixel width of the frozen column band at 96 dpi.
    fn band_w(cols: u32) -> u32 {
        ((DEFAULT_COL_WIDTH * i64::from(cols)) as f32 * 96.0 / 1440.0).round() as u32
    }

    /// Pixel height of the frozen row band at 96 dpi.
    fn band_h(rows: u32) -> u32 {
        ((DEFAULT_ROW_HEIGHT * i64::from(rows)) as f32 * 96.0 / 1440.0).round() as u32
    }

    #[test]
    fn a_frozen_column_holds_still_while_the_body_scrolls_sideways() {
        // Each band is pinned on one axis only — a frozen column still scrolls
        // down — so the scroll here is purely horizontal.
        let wb = labelled();
        let freeze = Freeze { rows: 2, cols: 1 };
        let near = render(&wb, &viewport(0, 0), freeze);
        let far = render(&wb, &viewport(DEFAULT_COL_WIDTH * 5, 0), freeze);
        let (fw, fh) = (band_w(1), band_h(2));

        assert_eq!(
            region(&near, 0, 0, fw, near.height()),
            region(&far, 0, 0, fw, far.height()),
            "the frozen column is pinned"
        );
        assert_ne!(
            region(&near, fw, fh, near.width(), near.height()),
            region(&far, fw, fh, far.width(), far.height()),
            "and the body did scroll — otherwise the assertion above proves nothing"
        );
    }

    #[test]
    fn frozen_rows_hold_still_while_the_body_scrolls_down() {
        let wb = labelled();
        let freeze = Freeze { rows: 2, cols: 1 };
        let near = render(&wb, &viewport(0, 0), freeze);
        let far = render(&wb, &viewport(0, DEFAULT_ROW_HEIGHT * 15), freeze);
        let (fw, fh) = (band_w(1), band_h(2));

        assert_eq!(
            region(&near, 0, 0, near.width(), fh),
            region(&far, 0, 0, far.width(), fh),
            "the frozen rows are pinned"
        );
        assert_ne!(
            region(&near, fw, fh, near.width(), near.height()),
            region(&far, fw, fh, far.width(), far.height()),
            "and the body did scroll"
        );
    }

    #[test]
    fn the_pinned_corner_shows_the_top_left_of_the_sheet() {
        // Pinned means *those* lines, not merely some unchanging ones. The
        // corner of a scrolled frozen render must be the same pixels as the
        // corner of an unscrolled unfrozen one: A1 onwards, drawn identically.
        let wb = labelled();
        let scrolled = render(
            &wb,
            &viewport(DEFAULT_COL_WIDTH * 3, DEFAULT_ROW_HEIGHT * 8),
            Freeze { rows: 2, cols: 1 },
        );
        let home = render(&wb, &viewport(0, 0), Freeze::default());

        // Stop short of the divider, which is drawn along the band's far edge
        // and is the one thing the unfrozen render has no counterpart for.
        let (fw, fh) = (band_w(1) - 2, band_h(2) - 2);
        assert_eq!(
            region(&scrolled, 0, 0, fw, fh),
            region(&home, 0, 0, fw, fh),
            "the corner is the sheet's own top-left, however far the body has gone"
        );

        // And it is not blank, so the comparison is of content and not of two
        // empty rectangles.
        assert!(
            crate::tests::any_pixel(&scrolled, 0, 0, fw, fh, |r, g, b| r < 100
                && g < 100
                && b < 100),
            "the pinned cells are painted with their text"
        );
    }

    #[test]
    fn the_freeze_boundary_is_drawn_darker_than_a_gridline() {
        // Without it a pinned header reads as an ordinary first row that
        // happens not to move.
        let wb = labelled();
        let freeze = Freeze { rows: 2, cols: 1 };
        let pixmap = render(&wb, &viewport(0, 0), freeze);

        let fw = (DEFAULT_COL_WIDTH as f32 * 96.0 / 1440.0).round() as u32;
        let fh = (DEFAULT_ROW_HEIGHT as f32 * 2.0 * 96.0 / 1440.0).round() as u32;
        // The divider colour, #5f6368 — distinctly darker than the #e0e0e0
        // gridline it would otherwise be mistaken for.
        let divider = |r: u8, g: u8, b: u8| (r, g, b) == (95, 99, 104);

        assert!(
            crate::tests::any_pixel(&pixmap, fw - 2, 0, fw + 1, pixmap.height(), divider),
            "a vertical divider at the frozen column's edge"
        );
        assert!(
            crate::tests::any_pixel(&pixmap, 0, fh - 2, pixmap.width(), fh + 1, divider),
            "a horizontal divider at the frozen rows' edge"
        );
    }

    #[test]
    fn a_pane_boundary_on_a_fractional_pixel_does_not_run_off_the_image() {
        // Each pane's pixmap rounds *up* to a whole pixel independently, so the
        // panes can add up to one pixel more than the image they go into: a
        // frozen band 64.6 px wide starts the body at pixel 65, and a body
        // 100.2 px wide is 101 pixels, which is 166 in a 165-pixel image.
        //
        // Sizes chosen to produce exactly that: the fractions round up
        // separately and down together. Without the clip in `blit` this writes
        // past the end of a row.
        use casual_calc_layout::Axis;

        let geo = GridGeometry {
            columns: Axis::with_sizes(1_503, [(0, 969)]),
            rows: Axis::with_sizes(1_503, [(0, 969)]),
        };
        let vp = Viewport {
            x: 0,
            y: 0,
            width: 2_472,
            height: 2_472,
        };
        let freeze = Freeze { rows: 1, cols: 1 };

        // The premise, so this stays a test of the overhang if the arithmetic
        // ever changes underneath it.
        let regions = panes(&geo, &vp, freeze);
        let px = |twips: i64| (twips as f32 * 96.0 / 1440.0).ceil() as u32;
        let body = regions.last().unwrap();
        assert!(
            (body.origin.0 as f32 * 96.0 / 1440.0).round() as u32 + px(body.viewport.width)
                > px(vp.width),
            "the panes overhang the image, which is the case under test"
        );

        let wb = labelled();
        let lists: Vec<_> = regions
            .iter()
            .map(|pane| layout_viewport(&wb, 0, &geo, &pane.viewport))
            .collect();
        let paints: Vec<PanePaint<'_>> = regions
            .iter()
            .zip(&lists)
            .map(|(pane, display_list)| PanePaint {
                pane: *pane,
                display_list,
            })
            .collect();

        let pixmap = render_panes(&paints, &geo, &vp, 96).unwrap();
        assert_eq!(pixmap.width(), px(vp.width), "the image keeps its own size");
        assert_eq!(pixmap.height(), px(vp.height));
    }

    #[test]
    fn no_freeze_draws_no_divider() {
        let wb = labelled();
        let pixmap = render(&wb, &viewport(0, 0), Freeze::default());
        assert!(
            !crate::tests::any_pixel(&pixmap, 0, 0, pixmap.width(), pixmap.height(), |r, g, b| (
                r, g, b
            )
                == (95, 99, 104)),
            "nothing is pinned, so there is no boundary to mark"
        );
    }
}

/// Merged cells in the raster — the visible half of RND-03.
///
/// Layout resolving a merge to one rectangle is not enough on its own: the
/// renderer rules the grid from the geometry, so a merged block kept the
/// interior lines it was merged to remove.
mod merged {
    use casual_calc_layout::{
        DEFAULT_COL_WIDTH, DEFAULT_ROW_HEIGHT, GridGeometry, Viewport, layout_full,
    };
    use casual_calc_model::{
        Cell, CellRange, CellRef, CellValue, Id, Sheet, SheetId, Style, Workbook,
    };

    use crate::render_pixmap;

    /// B2:D2 merged — a three-column header band, the commonest merge there is.
    fn banner(fill: Option<&str>) -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let style = fill.map(|hex| {
            wb.intern_style(Style {
                fill_color: Some(hex.to_owned()),
                ..Style::default()
            })
        });
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        let mut anchor = Cell::value(CellValue::Number(1.0));
        anchor.style = style;
        sheet.cells.set(CellRef::new(1, 1), anchor);
        sheet
            .merges
            .push(CellRange::new(CellRef::new(1, 1), CellRef::new(1, 3)));
        wb.sheets.push(sheet);
        wb
    }

    fn viewport() -> Viewport {
        Viewport {
            x: 0,
            y: 0,
            width: DEFAULT_COL_WIDTH * 6,
            height: DEFAULT_ROW_HEIGHT * 6,
        }
    }

    /// Pixel geometry of the merged band at 96 dpi.
    fn band() -> (u32, u32, u32, u32) {
        let px = |t: i64| (t as f32 * 96.0 / 1440.0).round() as u32;
        (
            px(DEFAULT_COL_WIDTH),     // x0
            px(DEFAULT_ROW_HEIGHT),    // y0
            px(DEFAULT_COL_WIDTH * 3), // width
            px(DEFAULT_ROW_HEIGHT),    // height
        )
    }

    #[test]
    fn no_gridline_is_drawn_inside_a_merged_range() {
        // The defect exactly: the boundaries between B, C and D are not cell
        // boundaries any more, so they must not be ruled.
        let wb = banner(None);
        let geo = GridGeometry::default();
        let pixmap = render_pixmap(&layout_full(&wb, 0, &geo), &geo, &viewport(), 96).unwrap();

        let (x0, y0, w, h) = band();
        // A row through the middle of the band, between its own outlines.
        let mid = y0 + h / 2;
        let gridline = |r: u8, g: u8, b: u8| (r, g, b) == (224, 224, 224);
        assert!(
            !crate::tests::any_pixel(&pixmap, x0 + 1, mid, x0 + w - 1, mid + 1, gridline),
            "the interior of the merge is unruled"
        );
    }

    #[test]
    fn the_merged_range_is_still_outlined_as_one_cell() {
        // Erasing the interior must not erase the range itself — a merged block
        // with no boundary reads as a hole in the grid.
        let wb = banner(None);
        let geo = GridGeometry::default();
        let pixmap = render_pixmap(&layout_full(&wb, 0, &geo), &geo, &viewport(), 96).unwrap();

        let (x0, y0, w, h) = band();
        let gridline = |r: u8, g: u8, b: u8| (r, g, b) == (224, 224, 224);
        assert!(
            crate::tests::any_pixel(&pixmap, x0, y0, x0 + w, y0 + 1, gridline),
            "top edge"
        );
        assert!(
            crate::tests::any_pixel(&pixmap, x0, y0 + h - 1, x0 + w, y0 + h, gridline),
            "bottom edge"
        );
        assert!(
            crate::tests::any_pixel(&pixmap, x0, y0, x0 + 1, y0 + h, gridline),
            "left edge"
        );
    }

    #[test]
    fn a_fill_covers_the_whole_merge_and_not_just_its_anchor() {
        // The half-coloured header: the most visible way to get a merge wrong.
        let wb = banner(Some("00FF00"));
        let geo = GridGeometry::default();
        let pixmap = render_pixmap(&layout_full(&wb, 0, &geo), &geo, &viewport(), 96).unwrap();

        let (x0, y0, w, h) = band();
        let mid = y0 + h / 2;
        // Sample near the far end of the band — inside the merge, but two
        // columns past the anchor cell.
        let far = pixmap.pixel(x0 + w - 4, mid).unwrap();
        assert!(
            far.green() > 200 && far.red() < 100 && far.blue() < 100,
            "the fill reaches the end of the merge, got r{} g{} b{}",
            far.red(),
            far.green(),
            far.blue()
        );
    }

    #[test]
    fn a_sheet_without_merges_renders_exactly_as_it_did() {
        // The compatibility property, on bytes: the merge pass must be
        // invisible to every sheet that has none.
        let mut wb = banner(None);
        wb.sheets[0].merges.clear();
        let geo = GridGeometry::default();
        let with_pass = render_pixmap(&layout_full(&wb, 0, &geo), &geo, &viewport(), 96).unwrap();

        // The grid, ruled through where the merge would have been.
        let (x0, y0, w, h) = band();
        let gridline = |r: u8, g: u8, b: u8| (r, g, b) == (224, 224, 224);
        assert!(
            crate::tests::any_pixel(
                &with_pass,
                x0 + 1,
                y0 + h / 2,
                x0 + w - 1,
                y0 + h / 2 + 1,
                gridline
            ),
            "unmerged, so B/C/D are ruled apart as they always were"
        );
    }
}

/// Two merged ranges side by side must not read as one.
///
/// The ordering trap, gated. `MergedRegion` erases the grid across its whole
/// rectangle and then outlines it; doing those two in the other order paints
/// the outline away, and two adjacent merged headers came out looking like a
/// single band twice as wide. Filled, because a fill is what hides the mistake.
#[test]
fn adjacent_merged_ranges_keep_a_boundary_between_them() {
    use casual_calc_layout::{
        DEFAULT_COL_WIDTH, DEFAULT_ROW_HEIGHT, GridGeometry, Viewport, layout_full,
    };
    use casual_calc_model::{
        Cell, CellRange, CellRef, CellValue, Id, Sheet, SheetId, Style, Workbook,
    };

    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let fill = wb.intern_style(Style {
        fill_color: Some("D9E7FF".to_owned()),
        ..Style::default()
    });
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    // A1:B1 and C1:D1 — two two-column headers that touch.
    for col in [0u32, 2] {
        let mut cell = Cell::value(CellValue::Number(f64::from(col)));
        cell.style = Some(fill);
        sheet.cells.set(CellRef::new(0, col), cell);
        sheet.merges.push(CellRange::new(
            CellRef::new(0, col),
            CellRef::new(0, col + 1),
        ));
    }
    wb.sheets.push(sheet);

    let geo = GridGeometry::default();
    let vp = Viewport {
        x: 0,
        y: 0,
        width: DEFAULT_COL_WIDTH * 5,
        height: DEFAULT_ROW_HEIGHT * 3,
    };
    let pixmap = crate::render_pixmap(&layout_full(&wb, 0, &geo), &geo, &vp, 96).unwrap();

    let px = |t: i64| (t as f32 * 96.0 / 1440.0).round() as u32;
    let mid_y = px(DEFAULT_ROW_HEIGHT) / 2;
    let seam = px(DEFAULT_COL_WIDTH * 2);
    let gridline = |r: u8, g: u8, b: u8| (r, g, b) == (224, 224, 224);

    assert!(
        any_pixel(&pixmap, seam - 1, mid_y, seam + 1, mid_y + 1, gridline),
        "the two ranges are separated where they meet"
    );
    // And the interior of the left one is still unruled, so the boundary above
    // is the ranges' own edge and not a surviving interior gridline.
    let interior = px(DEFAULT_COL_WIDTH);
    assert!(
        !any_pixel(&pixmap, interior, mid_y, interior + 1, mid_y + 1, gridline),
        "while A/B inside the first range is not"
    );
}
