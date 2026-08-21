//! Render tests: the raster produces a valid PNG and paints content cells.

use casual_calc_layout::{GridGeometry, Viewport, layout_full};
use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

use crate::{needs_shaping, render_pixmap, render_png, shaping_available};

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

// --- Data bars reach the pixels (RND-07) -------------------------------------

/// A one-column sheet holding 1, 50 and 100, optionally under a data-bar rule.
///
/// The rule's range is the three cells, so the extremes are 1 and 100 and the
/// middle value lands just under halfway — which is what makes the bar's width
/// a fact about the data rather than a constant.
fn data_bar_sheet(with_rule: bool) -> Workbook {
    use casual_calc_model::{CellRange, CfRule, ConditionalFormat};

    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    for (row, value) in [(0u32, 1.0), (1, 50.0), (2, 100.0)] {
        sheet
            .cells
            .set(CellRef::new(row, 0), Cell::value(CellValue::Number(value)));
    }
    if with_rule {
        sheet.conditional_formats.push(ConditionalFormat {
            range: CellRange::new(CellRef::new(0, 0), CellRef::new(2, 0)),
            // Red, so a bar pixel cannot be confused with the gridlines, the
            // ground, or the black glyphs.
            rule: CfRule::DataBar("FF0000".to_owned()),
            fill: String::new(),
            font_color: None,
            bold: false,
            priority: 0,
            stop_if_true: false,
        });
    }
    wb.sheets.push(sheet);
    wb
}

fn rendered(wb: &Workbook) -> tiny_skia::Pixmap {
    let geo = GridGeometry::default();
    render_pixmap(&layout_full(wb, 0, &geo), &geo, &viewport(), 96).unwrap()
}

/// **A data bar is actually painted, at the width the value earns.**
///
/// `RND-07`: the fraction and the colour were resolved for every renderer and
/// consumed by exactly one of them — the browser canvas — because the display
/// list had no primitive for a partial-width rectangle inside a cell. Asserted
/// in pixels rather than in paint items, since a display-list assertion is
/// satisfied by an item no backend draws, which is the defect itself.
#[test]
fn a_data_bar_is_painted_at_the_fraction_of_the_cell_the_value_earns() {
    let with = rendered(&data_bar_sheet(true));
    let without = rendered(&data_bar_sheet(false));

    // Counted rather than compared with `assert_ne!`, whose failure message
    // would be both pixmaps in full.
    let changed = with
        .data()
        .iter()
        .zip(without.data())
        .filter(|(a, b)| a != b)
        .count();
    assert!(changed > 0, "the data bar changed no pixel at all");

    // A1 = 1 is the range minimum, and it draws a **short** bar rather than
    // none. It used to draw nothing -- a raw fraction of zero -- which made
    // the one value a reader most wants to pick out of the range the only one
    // with no mark on it, and indistinguishable from a cell the rule does not
    // cover (`RND-09`). ECMA-376's `minLength` default is 10% of the cell, so
    // across a 62px inner width the bar runs to about x=7.
    let short = with.pixel(3, 8).unwrap();
    assert!(
        short.red() > 200 && short.green() < 200,
        "the minimum drew no bar at all, got r{} g{} b{}",
        short.red(),
        short.green(),
        short.blue()
    );
    // And it is short: plain ground well before the middle value's bar ends.
    let beyond = with.pixel(20, 8).unwrap();
    assert!(
        beyond.red() > 240 && beyond.green() > 240 && beyond.blue() > 240,
        "the minimum's bar should stop near x=7, but x=20 is painted: r{} g{} b{}",
        beyond.red(),
        beyond.green(),
        beyond.blue()
    );

    // A3 = 100 is the maximum: a full-width bar, so a red wash near both the
    // left edge and the right of the ~64px cell. Red-over-white at the bar's
    // alpha, not opaque red — the number is drawn on top and must stay legible.
    // Sampled just under the top of the bar rather than through the middle,
    // because the right-aligned "100" is painted over it there — which is the
    // ordering working, and is asserted on its own below.
    for x in [6u32, 55] {
        let px = with.pixel(x, 2 * 20 + 3).unwrap();
        assert!(
            px.red() > 200 && px.green() < 200 && px.green() > 80,
            "the maximum's bar should be a red wash at x={x}, got r{} g{} b{}",
            px.red(),
            px.green(),
            px.blue()
        );
    }

    // A2 = 50 sits at 49/99 of the way, so its bar ends around x = 1 + 62*0.495
    // ≈ 31: red to the left of that and plain ground well to the right of it.
    let inside = with.pixel(20, 20 + 10).unwrap();
    assert!(
        inside.red() > 200 && inside.green() < 200,
        "the middle value's bar should cover x=20, got r{} g{} b{}",
        inside.red(),
        inside.green(),
        inside.blue()
    );
    let past = with.pixel(40, 20 + 10).unwrap();
    assert!(
        past.red() > 240 && past.green() > 240 && past.blue() > 240,
        "the middle value's bar should stop well before x=40, got r{} g{} b{}",
        past.red(),
        past.green(),
        past.blue()
    );

    // And the number is still there on top of the full-width bar: a bar drawn
    // after the text, or drawn opaque, swallows the value it annotates.
    assert!(
        any_pixel(&with, 0, 40, 64, 60, |r, g, b| r < 100
            && g < 100
            && b < 100),
        "the maximum's digits should survive on top of its bar"
    );
}

/// **The exported default colour is the colour this renderer actually draws.**
///
/// `data_bar_style` hands a hex string across the WebAssembly boundary for the
/// browser canvas to use, while this renderer parses its own. A typo in one of
/// them gives two renderers two different blues — which is the whole failure
/// `RND-08` is about, in the one place the export cannot prevent by
/// construction.
#[test]
fn the_exported_default_bar_colour_is_the_one_drawn() {
    let exported = crate::data_bar_style().default_color;
    let parsed = crate::parse_hex_color(exported).expect("the exported colour must be a colour");
    assert_eq!(
        parsed,
        crate::default_data_bar(),
        "the colour handed to the canvas is not the colour this renderer draws"
    );
}

/// **The exported geometry is the geometry this renderer insets by.**
///
/// Not a tautology despite reading like one: the export could be given its own
/// literals, which is exactly how the canvas came to have a second copy in the
/// first place.
#[test]
fn the_exported_geometry_matches_the_constants_used_to_draw() {
    let style = crate::data_bar_style();
    assert_eq!(style.pad_x, crate::DATA_BAR_PAD_X);
    assert_eq!(style.pad_y, crate::DATA_BAR_PAD_Y);
    assert_eq!(style.alpha, crate::DATA_BAR_ALPHA);
}

/// Pictures reaching the raster, and the ones that cannot saying so (`RND-06`).
mod images {
    use std::collections::BTreeMap;

    use casual_calc_layout::{GridGeometry, Viewport, layout_full};
    use casual_calc_model::{
        Cell, CellRange, CellRef, CellValue, Emu, Id, ImageView, Sheet, SheetId, Workbook,
    };

    use crate::{
        ImageReport, MAX_IMAGE_PIXELS, NoImages, UndrawnImage, UndrawnReason,
        render_pixmap_with_images,
    };

    const PART: &str = "xl/media/image1.png";

    /// The four quadrant colours of the test picture, clockwise from top-left.
    const RED: (u8, u8, u8) = (220, 20, 20);
    const GREEN: (u8, u8, u8) = (20, 200, 20);
    const BLUE: (u8, u8, u8) = (20, 20, 220);
    const YELLOW: (u8, u8, u8) = (230, 230, 20);

    /// A 40x40 PNG in four solid quadrants.
    ///
    /// Big enough that the centre of each quadrant is far from every edge, so
    /// the assertions below are about *where the picture landed* rather than
    /// about the resampling filter — a 2x2 source scaled to the frame would be
    /// one interpolated smear and could not tell a correct blit from a
    /// mirrored one.
    fn quadrant_png() -> Vec<u8> {
        let mut src = tiny_skia::Pixmap::new(40, 40).unwrap();
        for y in 0..40u32 {
            for x in 0..40u32 {
                let (r, g, b) = match (x < 20, y < 20) {
                    (true, true) => RED,
                    (false, true) => GREEN,
                    (true, false) => BLUE,
                    (false, false) => YELLOW,
                };
                let mut one = tiny_skia::Paint::default();
                one.set_color(tiny_skia::Color::from_rgba8(r, g, b, 255));
                one.anti_alias = false;
                src.fill_rect(
                    tiny_skia::Rect::from_xywh(x as f32, y as f32, 1.0, 1.0).unwrap(),
                    &one,
                    tiny_skia::Transform::identity(),
                    None,
                );
            }
        }
        src.encode_png().unwrap()
    }

    /// A sheet whose picture covers `A1:C3` exactly — 192x60 device pixels at
    /// 96 dpi with the default geometry — over a cell that holds a number, so
    /// the picture has something to be drawn *over*.
    fn sheet_with_picture() -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        sheet
            .cells
            .set(CellRef::new(0, 0), Cell::value(CellValue::Number(8.0)));
        sheet.images.push(ImageView {
            anchor: CellRange::new(CellRef::new(0, 0), CellRef::new(2, 2)),
            from_offset: Emu::default(),
            to_offset: Emu::default(),
            part: PART.to_owned(),
            extent: None,
        });
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

    fn media(bytes: Vec<u8>) -> BTreeMap<String, Vec<u8>> {
        BTreeMap::from([(PART.to_owned(), bytes)])
    }

    fn render(
        wb: &Workbook,
        source: &BTreeMap<String, Vec<u8>>,
    ) -> (tiny_skia::Pixmap, ImageReport) {
        let geo = GridGeometry::default();
        let list = layout_full(wb, 0, &geo);
        render_pixmap_with_images(&list, &geo, &viewport(), 96, source).unwrap()
    }

    /// Assert the pixel at `(x, y)` is `expect`, within a tolerance for the
    /// resampler's last bit.
    #[track_caller]
    fn assert_pixel(pixmap: &tiny_skia::Pixmap, x: u32, y: u32, expect: (u8, u8, u8), what: &str) {
        let p = pixmap.pixel(x, y).expect("pixel out of surface");
        let got = (p.red(), p.green(), p.blue());
        let close = |a: u8, b: u8| a.abs_diff(b) <= 3;
        assert!(
            close(got.0, expect.0) && close(got.1, expect.1) && close(got.2, expect.2),
            "{what}: pixel ({x},{y}) is {got:?}, expected {expect:?}"
        );
    }

    /// The picture lands in its frame, the right way up and the right way
    /// round: each quadrant's colour at the point of the surface its quadrant
    /// maps to. A mirrored, rotated or offset blit fails here; "the PNG is not
    /// blank" would pass against any of them.
    #[test]
    fn a_picture_is_drawn_into_its_frame_the_right_way_round() {
        let (pixmap, report) = render(&sheet_with_picture(), &media(quadrant_png()));

        // The frame is 192x60 device pixels, so the quadrants split at x=96 and
        // y=30; these are the four quadrant centres.
        assert_pixel(&pixmap, 48, 15, RED, "top-left quadrant");
        assert_pixel(&pixmap, 144, 15, GREEN, "top-right quadrant");
        assert_pixel(&pixmap, 48, 45, BLUE, "bottom-left quadrant");
        assert_pixel(&pixmap, 144, 45, YELLOW, "bottom-right quadrant");

        assert_eq!(report.drawn, 1);
        assert!(report.is_complete(), "unexpected misses: {report:?}");
    }

    /// The picture stops at its frame: the grid beyond it is untouched.
    #[test]
    fn a_picture_does_not_paint_outside_its_frame() {
        let (pixmap, _) = render(&sheet_with_picture(), &media(quadrant_png()));
        let white = (255, 255, 255);
        // Just past the frame's right edge (192) and below its bottom (60),
        // and not on a gridline.
        assert_pixel(&pixmap, 200, 30, white, "right of the frame");
        assert_pixel(&pixmap, 100, 70, white, "below the frame");
    }

    /// A picture bigger than the frame it is anchored in is **scaled into it**,
    /// not drawn at its own size and cropped. The 40x40 source goes into `A1`
    /// alone — 64x20 device pixels — so all four quadrants have to be inside
    /// those 64 pixels; drawn unscaled, the right half of the picture would be
    /// past the frame's edge and its top-right quadrant would never appear.
    #[test]
    fn a_picture_larger_than_its_frame_is_scaled_into_it() {
        let mut wb = sheet_with_picture();
        wb.sheets[0].images[0].anchor = CellRange::new(CellRef::new(0, 0), CellRef::new(0, 0));
        let (pixmap, report) = render(&wb, &media(quadrant_png()));
        assert_eq!(report.drawn, 1);

        // A1 is 64x20 px at 96 dpi, so the quadrants split at x=32 and y=10.
        assert_pixel(&pixmap, 16, 5, RED, "top-left quadrant");
        assert_pixel(&pixmap, 48, 5, GREEN, "top-right quadrant");
        assert_pixel(&pixmap, 16, 15, BLUE, "bottom-left quadrant");
        assert_pixel(&pixmap, 48, 15, YELLOW, "bottom-right quadrant");
        // And nothing below the frame, which an unscaled draw would reach.
        assert_pixel(&pixmap, 16, 25, (255, 255, 255), "below the frame");
    }

    /// A picture floats over the cells it covers. `A1` holds a number, and the
    /// glyph the renderer drew for it must be underneath: no dark pixel is left
    /// anywhere in `A1`.
    #[test]
    fn a_picture_covers_the_cell_text_beneath_it() {
        let (pixmap, _) = render(&sheet_with_picture(), &media(quadrant_png()));
        assert!(
            !super::any_pixel(&pixmap, 0, 0, 64, 20, |r, g, b| r < 128
                && g < 128
                && b < 128),
            "the glyph in A1 is still visible through the picture"
        );
    }

    /// The no-media form still renders, and still says nothing — which is why
    /// it is not the one a host should call.
    #[test]
    fn the_media_less_form_draws_no_picture() {
        let wb = sheet_with_picture();
        let geo = GridGeometry::default();
        let list = layout_full(&wb, 0, &geo);
        let (pixmap, report) =
            render_pixmap_with_images(&list, &geo, &viewport(), 96, &NoImages).unwrap();
        assert_pixel(&pixmap, 48, 15, (255, 255, 255), "no picture drawn");
        assert_eq!(report.drawn, 0);
        assert_eq!(
            report.undrawn,
            vec![UndrawnImage {
                part: PART.to_owned(),
                reason: UndrawnReason::NotSupplied,
            }]
        );
    }

    /// No silent loss: media the host did not supply is named, not skipped.
    #[test]
    fn a_picture_with_no_media_is_named_not_skipped() {
        let (_, report) = render(&sheet_with_picture(), &BTreeMap::new());
        assert_eq!(report.drawn, 0);
        assert_eq!(
            report.undrawn,
            vec![UndrawnImage {
                part: PART.to_owned(),
                reason: UndrawnReason::NotSupplied,
            }]
        );
    }

    // --- The raster formats (RND-12) ------------------------------------
    //
    // Only PNG was decoded, because tiny-skia already could. Every other format
    // was sniffed and named — which honoured the no-silent-loss rule and still
    // left the canvas and the headless render disagreeing about the same
    // document, since the browser draws all of them. A thumbnail is exactly
    // where that shows.

    /// The fixtures' quadrant colours — pure, unlike `quadrant_png`'s, so a
    /// lossy codec's drift is measured against something unambiguous.
    #[cfg(feature = "raster")]
    const FIX_RED: (u8, u8, u8) = (255, 0, 0);
    #[cfg(feature = "raster")]
    const FIX_GREEN: (u8, u8, u8) = (0, 255, 0);
    #[cfg(feature = "raster")]
    const FIX_BLUE: (u8, u8, u8) = (0, 0, 255);
    #[cfg(feature = "raster")]
    const FIX_YELLOW: (u8, u8, u8) = (255, 255, 0);

    /// Each quadrant's colour at the point of the surface it maps to.
    ///
    /// The assertion that matters. "It decoded without erroring" passes against
    /// a decoder returning garbage; "it is not blank" passes against one that
    /// flipped the image vertically, which is the classic BMP bug because BMP
    /// stores its rows bottom-up.
    #[cfg(feature = "raster")]
    #[track_caller]
    fn assert_quadrants(pixmap: &tiny_skia::Pixmap, tolerance: u8, what: &str) {
        let check = |x: u32, y: u32, expect: (u8, u8, u8), corner: &str| {
            let p = pixmap.pixel(x, y).expect("pixel out of surface");
            let got = (p.red(), p.green(), p.blue());
            let close = |a: u8, b: u8| a.abs_diff(b) <= tolerance;
            assert!(
                close(got.0, expect.0) && close(got.1, expect.1) && close(got.2, expect.2),
                "{what}, {corner}: pixel ({x},{y}) is {got:?}, expected {expect:?} \
                 within {tolerance}"
            );
        };
        check(48, 15, FIX_RED, "top-left");
        check(144, 15, FIX_GREEN, "top-right");
        check(48, 45, FIX_BLUE, "bottom-left");
        check(144, 45, FIX_YELLOW, "bottom-right");
    }

    /// **Every lossless raster format decodes, and lands the right way up.**
    ///
    /// The fixtures were encoded by `sips` (Apple's ImageIO) and `cwebp`
    /// (Google's reference encoder) — not by the crate that decodes them, so
    /// this cannot pass by a codec agreeing with itself.
    #[cfg(feature = "raster")]
    #[test]
    fn the_lossless_raster_formats_decode_and_land_the_right_way_up() {
        for (name, bytes) in [
            ("gif", &include_bytes!("../fixtures/quad.gif")[..]),
            ("bmp", &include_bytes!("../fixtures/quad.bmp")[..]),
            ("tiff", &include_bytes!("../fixtures/quad.tiff")[..]),
            ("webp", &include_bytes!("../fixtures/quad.webp")[..]),
        ] {
            let (pixmap, report) = render(&sheet_with_picture(), &media(bytes.to_vec()));
            assert_eq!(report.drawn, 1, "{name} was not drawn: {report:?}");
            assert!(report.is_complete(), "{name}: {report:?}");
            assert_quadrants(&pixmap, 3, name);
        }
    }

    /// **A JPEG in a workbook renders headlessly** — the row's acceptance,
    /// stated as itself.
    ///
    /// A wide tolerance because JPEG is lossy and these quadrants meet at hard
    /// edges, which is the worst case for it. Wide enough to absorb the codec,
    /// nowhere near wide enough to confuse red with green.
    #[cfg(feature = "raster")]
    #[test]
    fn a_jpeg_in_a_workbook_renders() {
        let jpeg = include_bytes!("../fixtures/quad.jpeg").to_vec();
        let (pixmap, report) = render(&sheet_with_picture(), &media(jpeg));
        assert_eq!(report.drawn, 1, "the jpeg was not drawn: {report:?}");
        assert!(report.is_complete(), "{report:?}");
        assert_quadrants(&pixmap, 40, "jpeg");
    }

    /// **Without the decoders, a JPEG is still called a JPEG.**
    ///
    /// The build the WebAssembly bundle uses. Gating the decoders out is a size
    /// decision and must not be a *reporting* decision: the first draft of
    /// `RND-12` moved the format names inside the `raster` feature along with
    /// the decoding, so this build called a JPEG "an unrecognised format" — a
    /// no-silent-loss report that got quietly worse because a codec was not
    /// compiled in. Caught by running both configurations, which is the only
    /// thing that can catch it.
    #[cfg(not(feature = "raster"))]
    #[test]
    fn a_jpeg_is_still_named_when_the_decoders_are_compiled_out() {
        let jpeg = include_bytes!("../fixtures/quad.jpeg").to_vec();
        let (_, report) = render(&sheet_with_picture(), &media(jpeg));
        assert_eq!(
            report.undrawn,
            vec![UndrawnImage {
                part: PART.to_owned(),
                reason: UndrawnReason::UnsupportedFormat("jpeg"),
            }],
            "a build with no decoders must still say which format it declined"
        );
    }

    /// **Vector formats are still refused, by name.**
    ///
    /// Not an oversight and not a gap to be closed later with the same trick:
    /// `emf`, `wmf` and `svg` are drawings to be *executed*, not pixels to be
    /// unpacked, and half-executing one produces a picture that is wrong rather
    /// than a picture that is missing. Named, so the report says something a
    /// person can act on.
    #[test]
    fn vector_formats_are_named_rather_than_half_drawn() {
        let cases: [(&str, Vec<u8>); 3] = [
            ("emf", {
                let mut b = vec![1u8, 0, 0, 0];
                b.resize(40, 0);
                b.extend_from_slice(b" EMF");
                b
            }),
            ("wmf", vec![0xd7, 0xcd, 0xc6, 0x9a, 0, 0, 0, 0]),
            (
                "svg",
                b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".to_vec(),
            ),
        ];
        for (name, bytes) in cases {
            let (_, report) = render(&sheet_with_picture(), &media(bytes));
            assert_eq!(
                report.undrawn,
                vec![UndrawnImage {
                    part: PART.to_owned(),
                    reason: UndrawnReason::UnsupportedFormat(name),
                }],
                "{name} should be named, not drawn"
            );
        }
    }

    /// A raster format this *does* decode, over bytes that are not one, is
    /// **undecodable** rather than "unsupported" — the same distinction the
    /// corrupt-PNG test draws. It tells a host whether the file is damaged or
    /// merely in a format this does not read, and the answer changed for JPEG
    /// when JPEG started decoding.
    #[cfg(feature = "raster")]
    #[test]
    fn a_truncated_jpeg_is_undecodable_rather_than_unsupported() {
        let mut bytes = include_bytes!("../fixtures/quad.jpeg").to_vec();
        bytes.truncate(20);
        let (_, report) = render(&sheet_with_picture(), &media(bytes));
        assert_eq!(
            report.undrawn,
            vec![UndrawnImage {
                part: PART.to_owned(),
                reason: UndrawnReason::Undecodable,
            }]
        );
    }

    /// A PNG signature over bytes that are not a PNG is *undecodable*, not
    /// "unsupported": the distinction is what tells a host whether the file is
    /// damaged or merely uses a format this does not read.
    #[test]
    fn a_corrupt_png_is_undecodable_rather_than_unsupported() {
        let mut bytes = quadrant_png();
        bytes.truncate(40);
        let (_, report) = render(&sheet_with_picture(), &media(bytes));
        assert_eq!(
            report.undrawn,
            vec![UndrawnImage {
                part: PART.to_owned(),
                reason: UndrawnReason::Undecodable,
            }]
        );
    }

    /// The pixel bound is checked against the header, so a few hundred bytes
    /// declaring a gigantic image are refused rather than allocated.
    #[test]
    fn a_png_over_the_pixel_limit_is_refused_from_its_header() {
        // A valid signature and IHDR chunk header, then dimensions well past
        // the cap. Nothing after them matters: the refusal happens first.
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&30_000u32.to_be_bytes());
        bytes.extend_from_slice(&30_000u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        const { assert!(30_000u64 * 30_000 > MAX_IMAGE_PIXELS) };

        let (_, report) = render(&sheet_with_picture(), &media(bytes));
        assert_eq!(
            report.undrawn,
            vec![UndrawnImage {
                part: PART.to_owned(),
                reason: UndrawnReason::TooLarge {
                    width: 30_000,
                    height: 30_000,
                },
            }]
        );
    }

    /// The same picture failing in several panes is one entry, not one per
    /// pane — a report that repeats itself per surface is a report nobody
    /// reads.
    #[test]
    fn a_repeated_failure_is_reported_once() {
        use casual_calc_layout::{Freeze, layout_viewport, panes};

        let wb = sheet_with_picture();
        let geo = GridGeometry::default();
        let vp = viewport();
        let split = panes(&geo, &vp, Freeze { rows: 1, cols: 1 });
        assert!(split.len() > 1, "expected a split viewport");
        let lists: Vec<_> = split
            .iter()
            .map(|p| layout_viewport(&wb, 0, &geo, &p.viewport))
            .collect();
        let paints: Vec<crate::PanePaint<'_>> = split
            .iter()
            .zip(&lists)
            .map(|(pane, display_list)| crate::PanePaint {
                pane: *pane,
                display_list,
            })
            .collect();
        let (_, report) =
            crate::render_panes_with_images(&paints, &geo, &vp, 96, &BTreeMap::new()).unwrap();
        assert_eq!(
            report.undrawn,
            vec![UndrawnImage {
                part: PART.to_owned(),
                reason: UndrawnReason::NotSupplied,
            }]
        );
    }
}

/// **A build says whether it can shape.**
///
/// `docs/64` promises that a build without shaping "reports that it lacks it
/// rather than silently producing wrong output", so that "a caller rendering a
/// thumbnail can then decide, rather than discovering it from a customer".
/// Nothing could be asked until `DOC-031` — the render surface exposed the
/// entry points, the data-bar style and its constants, and no way to put the
/// question.
///
/// Asserted against the feature this build was compiled with rather than
/// against a literal, so it is the *answer* being checked and not a constant
/// agreeing with itself.
#[test]
fn a_build_reports_whether_it_can_shape() {
    assert_eq!(
        shaping_available(),
        cfg!(feature = "shaping"),
        "the reported capability disagrees with the build"
    );
}

/// **The scripts that need shaping are the ones that say so.**
///
/// Not a font question — answerable from the string alone, so a caller can ask
/// before rendering anything.
#[test]
fn the_scripts_that_need_shaping_are_named() {
    // Cursive, right-to-left, reordering: unreadable drawn per `char`.
    for text in ["مرحبا", "שלום", "नमस्ते", "สวัสดี", "ជំរាបសួរ"]
    {
        assert!(
            needs_shaping(text),
            "{text:?} needs shaping and was not named"
        );
    }
    // Rendered acceptably glyph-by-glyph.
    for text in ["Total", "Итого", "合計", "Σύνολο", "1 234,56 €", ""] {
        assert!(
            !needs_shaping(text),
            "{text:?} does not need shaping and was named"
        );
    }
}

/// **The two answers together are what a caller acts on.**
///
/// Neither alone decides anything: a build without shaping is fine until the
/// document contains Arabic, and Arabic is fine on a build that shapes. The
/// combination is the thumbnail that is silently wrong.
#[test]
fn the_pair_identifies_a_picture_that_would_be_wrong() {
    let arabic = "مرحبا";
    let would_be_wrong = needs_shaping(arabic) && !shaping_available();
    assert_eq!(
        would_be_wrong,
        !cfg!(feature = "shaping"),
        "the two answers do not compose into the decision they exist for"
    );
}

/// Charts reaching the raster (`RND-11`).
///
/// Every assertion here is a **colour at a named coordinate**, for the reason
/// `RND-06` had to learn twice: "the PNG is not blank" passes against a
/// mirrored plot, a plot with no scale, and a pie that goes round the wrong
/// way. Each fixture is built so that the wrong picture puts a different
/// colour under at least one of these points.
mod charts {
    use casual_calc_layout::{GridGeometry, Viewport, layout_full};
    use casual_calc_model::{
        Cell, CellRange, CellRef, CellValue, ChartKind, ChartSeries, ChartView, Id, Sheet, SheetId,
        Workbook,
    };

    use crate::render_pixmap;

    /// The workbook's first four theme accents, which is where a chart's series
    /// colours come from. Stock Office, since these fixtures carry no theme.
    const ACCENT1: (u8, u8, u8) = (0x44, 0x72, 0xC4);
    const ACCENT2: (u8, u8, u8) = (0xED, 0x7D, 0x31);
    const ACCENT3: (u8, u8, u8) = (0xA5, 0xA5, 0xA5);
    const ACCENT4: (u8, u8, u8) = (0xFF, 0xC0, 0x00);
    /// The chart frame's ground.
    const GROUND: (u8, u8, u8) = (255, 255, 255);

    /// A sheet whose column A holds `values`, with one chart of `kind` over
    /// `A1:F10` — 384x200 device pixels at 96 dpi with the default geometry.
    fn sheet_with_chart(kind: ChartKind, values: &[f64]) -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        for (i, v) in values.iter().enumerate() {
            sheet.cells.set(
                CellRef::new(i as u32, 0),
                Cell::value(CellValue::Number(*v)),
            );
        }
        let mut chart =
            ChartView::new(CellRange::new(CellRef::new(0, 0), CellRef::new(9, 5)), kind);
        chart.series = vec![ChartSeries {
            name: String::new(),
            categories: None,
            values: format!("$A$1:$A${}", values.len()),
        }];
        sheet.charts.push(chart);
        wb.sheets.push(sheet);
        wb
    }

    fn render(wb: &Workbook) -> tiny_skia::Pixmap {
        let geo = GridGeometry::default();
        let list = layout_full(wb, 0, &geo);
        let vp = Viewport {
            x: 0,
            y: 0,
            width: 10 * 960,
            height: 12 * 300,
        };
        render_pixmap(&list, &geo, &vp, 96).unwrap()
    }

    #[track_caller]
    fn assert_pixel(pixmap: &tiny_skia::Pixmap, x: u32, y: u32, expect: (u8, u8, u8), what: &str) {
        let p = pixmap.pixel(x, y).expect("pixel out of surface");
        let got = (p.red(), p.green(), p.blue());
        let close = |a: u8, b: u8| a.abs_diff(b) <= 3;
        assert!(
            close(got.0, expect.0) && close(got.1, expect.1) && close(got.2, expect.2),
            "{what}: pixel ({x},{y}) is {got:?}, expected {expect:?}"
        );
    }

    /// **A column chart's bars are where their values put them.**
    ///
    /// Values 1 and 2 over an extent of 0..2, so the first bar fills the lower
    /// half of the plot and the second fills all of it. The plot is x = 34px,
    /// y = 6px, 340x176px, with the zero line at its foot (y = 182px); the
    /// bars are x 59.5..177.5 and x 229.5..347.5.
    ///
    /// The four points are chosen so that the pictures which "not blank" would
    /// accept all fail: an upside-down plot fills (100, 50) and empties
    /// (100, 140); a plot with no scale fills both bars to the same height and
    /// empties (280, 50) or fills (100, 50).
    #[test]
    fn a_column_charts_bars_stand_at_the_height_of_their_values() {
        let pixmap = render(&sheet_with_chart(ChartKind::Column, &[1.0, 2.0]));

        assert_pixel(&pixmap, 100, 140, ACCENT1, "inside the first bar");
        assert_pixel(&pixmap, 100, 50, GROUND, "above the first bar");
        assert_pixel(&pixmap, 280, 140, ACCENT1, "inside the second bar");
        assert_pixel(
            &pixmap,
            280,
            50,
            ACCENT1,
            "the second bar is twice the first, so it reaches here",
        );
        assert_pixel(&pixmap, 360, 100, GROUND, "right of the last bar");
    }

    /// Whether anything at all was painted at `(x, y)`.
    fn ink(pixmap: &tiny_skia::Pixmap, x: u32, y: u32) -> bool {
        let p = pixmap.pixel(x, y).expect("pixel out of surface");
        (p.red(), p.green(), p.blue()) != GROUND
    }

    /// **The zero line is stroked, and it is at zero.**
    ///
    /// A [`PaintItem::Polyline`] one pixel wide lands on two half-covered rows
    /// whatever colour it is, so this asserts *position* rather than a colour:
    /// there is ink where the axis belongs and none twelve pixels above it,
    /// inside the plot and between the two bars where nothing else is drawn.
    /// An axis that was never stroked, or one placed at the top of the plot,
    /// fails.
    #[test]
    fn the_zero_line_is_stroked_where_the_extent_puts_it() {
        let pixmap = render(&sheet_with_chart(ChartKind::Column, &[1.0, 2.0]));

        // x = 200px is the gap between the bars (they end at 177.5 and start
        // at 229.5), so only the axis can put ink here.
        assert!(
            ink(&pixmap, 200, 181) || ink(&pixmap, 200, 182),
            "no ink on the zero line at y = 182"
        );
        assert!(!ink(&pixmap, 200, 170), "ink above the zero line");
        // And the value axis down the plot's left edge, at x = 34px.
        assert!(
            ink(&pixmap, 33, 100) || ink(&pixmap, 34, 100),
            "no ink on the value axis at x = 34"
        );
        assert!(!ink(&pixmap, 45, 100), "ink right of the value axis");
    }

    /// **A pie starts at twelve o'clock and runs clockwise**, in four equal
    /// quarters coloured by the workbook's first four accents.
    ///
    /// The plot is 340x176px at (34, 6), so the pie is centred at (204, 94)
    /// with radius 84px. Each assertion is 46px diagonally from the centre —
    /// 65px out, inside the pie and well away from every slice edge.
    ///
    /// This is the four-quadrant discipline `RND-06` used for pictures: a pie
    /// swept counter-clockwise swaps the two off-diagonal quadrants, one
    /// started at three o'clock rotates all four, and either passes a
    /// non-blank check.
    #[test]
    fn a_pies_slices_are_where_their_order_puts_them() {
        let pixmap = render(&sheet_with_chart(ChartKind::Pie, &[1.0, 1.0, 1.0, 1.0]));

        assert_pixel(&pixmap, 250, 48, ACCENT1, "first slice, top-right");
        assert_pixel(&pixmap, 250, 140, ACCENT2, "second slice, bottom-right");
        assert_pixel(&pixmap, 158, 140, ACCENT3, "third slice, bottom-left");
        assert_pixel(&pixmap, 158, 48, ACCENT4, "fourth slice, top-left");
        // Solid to the middle: a pie has no hole, and this is the point the
        // doughnut below proves is empty.
        assert_pixel(&pixmap, 214, 84, ACCENT1, "a pie is filled to its centre");
    }

    /// **A doughnut is the same picture with the middle cut out** — and the
    /// hole is a hole in the geometry, so the frame's ground shows through it.
    #[test]
    fn a_doughnut_has_a_hole_and_keeps_its_slices() {
        let pixmap = render(&sheet_with_chart(
            ChartKind::Doughnut,
            &[1.0, 1.0, 1.0, 1.0],
        ));

        // Radius 65px: outside the hole (46px) and inside the ring (84px).
        assert_pixel(&pixmap, 250, 48, ACCENT1, "first slice, top-right");
        assert_pixel(&pixmap, 250, 140, ACCENT2, "second slice, bottom-right");
        assert_pixel(&pixmap, 158, 140, ACCENT3, "third slice, bottom-left");
        assert_pixel(&pixmap, 158, 48, ACCENT4, "fourth slice, top-left");
        // The same point that is solid in the pie above, 14px from the centre.
        assert_pixel(&pixmap, 214, 84, GROUND, "inside the hole");
        assert_pixel(&pixmap, 204, 94, GROUND, "the centre itself");
    }

    /// **A legend reaches the PNG, and takes its side out of the plot.**
    ///
    /// `RND-11` shipped every chart kind but this: layout had no text advances,
    /// so it could not size the legend box — and the plot is what is *left over*
    /// from that box, so it gave the plot the whole frame and drew no legend at
    /// all. Every chart with one rendered with a plot the width of the legend
    /// too wide.
    ///
    /// Proved against the same chart with the legend off, so it cannot pass on
    /// the pie being any particular size — only on its being smaller with a
    /// legend, and on there being a swatch out where the plot no longer reaches.
    #[test]
    fn a_legend_is_drawn_and_narrows_the_plot() {
        let plain = sheet_with_chart(ChartKind::Pie, &[1.0, 1.0, 1.0, 1.0]);
        let without = render(&plain);
        // The control: this point is the first slice with no legend.
        assert_pixel(&without, 250, 48, ACCENT1, "first slice, no legend");

        let mut legended = plain.clone();
        legended.sheets[0].charts[0].legend = Some("r".to_owned());
        let with = render(&legended);

        // The plot narrowed, so the point that was inside the pie is not any
        // more. Asserted as "not the slice colour" rather than as a specific
        // colour, because what is there now is the legend's business.
        let p = with.pixel(250, 48).expect("pixel");
        assert_ne!(
            (p.red(), p.green(), p.blue()),
            ACCENT1,
            "the pie still reaches where it did without a legend: the legend took \
             nothing out of the plot"
        );

        // And the legend itself is there: a swatch of the first series' colour,
        // out beyond where the narrowed plot ends.
        let swatch = (300..380).any(|x| {
            (10..60).any(|y| {
                with.pixel(x, y).is_some_and(|p| {
                    let got = (p.red(), p.green(), p.blue());
                    let close = |a: u8, b: u8| a.abs_diff(b) <= 3;
                    close(got.0, ACCENT1.0) && close(got.1, ACCENT1.1) && close(got.2, ACCENT1.2)
                })
            })
        });
        assert!(
            swatch,
            "no first-series swatch anywhere in the legend column"
        );
    }

    /// A chart is drawn **over** the cells it is anchored across, not behind
    /// them.
    ///
    /// Proved against a control rather than against white: A1 is given a red
    /// fill, so the same point is red with the chart removed and the frame's
    /// ground with it there. Asserting only that the point is white would pass
    /// on a sheet where nothing was drawn at all.
    #[test]
    fn a_chart_frame_covers_the_cells_underneath_it() {
        let mut wb = sheet_with_chart(ChartKind::Column, &[1.0, 2.0]);
        let red = wb.intern_style(casual_calc_model::Style {
            fill_color: Some("FF0000".to_owned()),
            ..casual_calc_model::Style::default()
        });
        let mut a1 = Cell::value(CellValue::Number(1.0));
        a1.style = Some(red);
        wb.sheets[0].cells.set(CellRef::new(0, 0), a1);

        // A1 is 64x20px, and this point is inside it.
        assert_pixel(&render(&wb), 30, 10, GROUND, "A1 is under the chart");

        wb.sheets[0].charts.clear();
        assert_pixel(
            &render(&wb),
            30,
            10,
            (255, 0, 0),
            "without the chart the cell's own fill is there",
        );
    }

    /// **`Align::Center` centres.**
    ///
    /// `draw_glyphs` places a run **twice** — once on the shaped path and once
    /// on the per-`char` fallback — and which one runs is decided by a Cargo
    /// feature (ADR-018), so one test can only ever exercise the build it was
    /// compiled for. Breaking the unshaped arm alone was invisible on a default
    /// build; this passes under both `--features shaping` and
    /// `--no-default-features --features all-fonts`, and goes red under either
    /// arm broken in its own configuration. Worth stating because the trap is
    /// silent: a new `Align` variant handled in one of the two arms renders
    /// correctly for everybody who tests natively.
    ///
    /// A chart title is centred over its frame; the cell path has never needed
    /// it, so nothing else in this crate exercises it. Asserted by where the
    /// ink starts: the same string in the same box, left-aligned, begins at the
    /// box's left edge, and centred begins far to the right of it. An
    /// implementation that fell through to `Left` would put ink in the same
    /// place both times, which is exactly the mistake a new match arm invites.
    #[test]
    fn centred_text_is_centred_and_not_left_aligned() {
        use casual_calc_layout::{Align, DisplayList, PaintItem, Rect};

        let geo = GridGeometry::default();
        let vp = Viewport {
            x: 0,
            y: 0,
            width: 300 * 15,
            height: 20 * 15,
        };
        let paint =
            |items: Vec<PaintItem>| render_pixmap(&DisplayList { items }, &geo, &vp, 96).unwrap();
        // The grid's own lines are painted whatever the display list says, so
        // "the first column with ink in it" would always be zero. The text's
        // ink is what this render has and an empty one does not.
        let bare = paint(Vec::new());
        let ink_starts_at = |align: Align| -> u32 {
            let pixmap = paint(vec![PaintItem::Text {
                rect: Rect {
                    x: 0,
                    y: 0,
                    w: 300 * 15,
                    h: 20 * 15,
                },
                content: "Hi".to_owned(),
                align,
                color: Some("000000".to_owned()),
                bold: false,
                italic: false,
                font_name: None,
                font_pt: Some(11.0),
            }]);
            (0..300)
                .find(|x| (0..20).any(|y| pixmap.pixel(*x, y) != bare.pixel(*x, y)))
                .expect("the string left no ink at all")
        };

        let left = ink_starts_at(Align::Left);
        let centre = ink_starts_at(Align::Center);
        assert!(left < 10, "left-aligned ink starts at {left}");
        assert!(
            centre > 130 && centre < 160,
            "centred ink starts at {centre}, not near the middle of a 300px box"
        );
    }
}
