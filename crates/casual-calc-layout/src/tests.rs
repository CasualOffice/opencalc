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
        ..Style::default()
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
fn style_populates_fill_font_and_border_paint_items() {
    use crate::{BorderLine, PaintItem};
    use casual_calc_model::{BorderEdge, Borders, Style};

    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let styled = wb.intern_style(Style {
        fill_color: Some("FFEE00".to_owned()),
        font_color: Some("112233".to_owned()),
        bold: true,
        italic: true,
        border: Some(Borders {
            top: Some(BorderEdge {
                style: "thin".to_owned(),
                color: Some("FF0000".to_owned()),
            }),
            bottom: Some(BorderEdge {
                style: "thick".to_owned(),
                color: None,
            }),
            ..Borders::default()
        }),
        ..Style::default()
    });
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    let mut cell = Cell::value(CellValue::Number(7.0));
    cell.style = Some(styled);
    sheet.cells.set(CellRef::new(0, 0), cell);
    wb.sheets.push(sheet);

    let list = layout_full(&wb, 0, &GridGeometry::default());

    // Painter's order for the one cell: fill, then text, then border.
    assert_eq!(list.items.len(), 3);
    assert!(matches!(
        &list.items[0],
        PaintItem::CellBackground { fill: Some(c), .. } if c == "FFEE00"
    ));
    assert!(matches!(
        &list.items[1],
        PaintItem::Text { color: Some(c), bold: true, italic: true, .. } if c == "112233"
    ));
    assert!(matches!(
        &list.items[2],
        PaintItem::CellBorder {
            top: Some(BorderLine { width: 1, color: Some(tc) }),
            bottom: Some(BorderLine { width: 3, color: None }),
            left: None,
            right: None,
            ..
        } if tc == "FF0000"
    ));
}

#[test]
fn unstyled_cell_emits_a_plain_text_item() {
    use crate::PaintItem;
    let wb = sample();
    let list = layout_full(&wb, 0, &GridGeometry::default());
    assert!(list.items.iter().all(|i| matches!(
        i,
        PaintItem::Text {
            color: None,
            bold: false,
            italic: false,
            ..
        }
    )));
}

#[test]
fn display_list_json_round_trips_with_style() {
    use casual_calc_model::{BorderEdge, Borders, Style};

    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let styled = wb.intern_style(Style {
        fill_color: Some("00FF00".to_owned()),
        border: Some(Borders {
            left: Some(BorderEdge {
                style: "medium".to_owned(),
                color: None,
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

    let list = layout_full(&wb, 0, &GridGeometry::default());
    let json = serde_json::to_string(&list).unwrap();
    let back: crate::DisplayList = serde_json::from_str(&json).unwrap();
    assert_eq!(list, back);
}

#[test]
fn layout_is_deterministic() {
    let wb = sample();
    let geo = GridGeometry::default();
    assert_eq!(layout_full(&wb, 0, &geo), layout_full(&wb, 0, &geo));
}

#[test]
fn elapsed_time_counts_past_a_day() {
    use crate::format_number;

    // 1.25 days = 30 hours. `[h]` must show the *total*, not the clock hour —
    // the brackets used to be swallowed, so this rendered without its hours and
    // a 30-hour timesheet entry read as 6:00.
    assert_eq!(format_number(1.25, "[h]:mm"), "30:00");
    assert_eq!(format_number(1.25, "[m]"), "1800");
    assert_eq!(format_number(1.25, "[s]"), "108000");
    // Padding follows the bracket's own width.
    assert_eq!(format_number(0.25, "[hh]:mm"), "06:00");
    // A plain `h` still wraps at 24, which is the difference being tested.
    assert_eq!(format_number(1.25, "h:mm"), "6:00");
}

#[test]
fn elapsed_hours_make_the_following_m_a_minute() {
    use crate::format_number;

    // Without the neighbour rule the `mm` after `[h]` would read as a month.
    assert_eq!(format_number(1.5, "[h]:mm:ss"), "36:00:00");
}

#[test]
fn negative_elapsed_time_keeps_its_sign() {
    use crate::format_number;

    assert_eq!(format_number(-0.5, "[h]:mm"), "-12:00");
}

#[test]
fn locale_and_colour_brackets_still_emit_nothing() {
    use crate::format_number;

    // The bracket arm now recognises elapsed units, so the tokens that are *not*
    // elapsed must still be dropped rather than leaking into the output.
    assert_eq!(format_number(0.5, "[Red]h:mm"), "12:00");
    // `[$-409]` is a locale marker with an empty currency symbol, and one of the
    // commonest prefixes on real date/time formats. The `0` inside the locale id
    // used to be mistaken for a digit placeholder, routing the whole format down
    // the numeric path — it rendered as the literal text `h:mm0`.
    assert_eq!(format_number(0.5, "[$-409]h:mm"), "12:00");
    assert_eq!(format_number(1.25, "[$-409][h]:mm"), "30:00");
}

#[test]
fn the_1904_epoch_shifts_dates_but_not_plain_numbers() {
    use crate::numfmt::{format_number, format_number_1904};

    // Serial 0 is 1904-01-01 under the Mac epoch and 1900-01-00 under the
    // default one. Reading a 1904 workbook as 1900 puts every date out by 1462
    // days — over four years — with nothing to show that it happened.
    assert_eq!(format_number_1904(0.0, "yyyy-mm-dd"), "1904-01-01");
    assert_eq!(format_number_1904(1.0, "yyyy-mm-dd"), "1904-01-02");
    // The same serial under the default epoch. Excel's serial 1 is 1900-01-01,
    // which only works because it also believes 1900 was a leap year.
    assert_eq!(format_number(1.0, "yyyy-mm-dd"), "1900-01-01");
    assert_eq!(format_number(59.0, "yyyy-mm-dd"), "1900-02-28");
    // Serial 60 is Excel's phantom 1900-02-29. Reproduced rather than corrected:
    // disagreeing here means disagreeing with the file's author about a date.
    assert_eq!(format_number(60.0, "yyyy-mm-dd"), "1900-02-29");
    assert_eq!(format_number(61.0, "yyyy-mm-dd"), "1900-03-01");
    // The anchor everything else is measured from.
    assert_eq!(format_number(25569.0, "yyyy-mm-dd"), "1970-01-01");
    // A plain number means the same under either epoch, so it must not shift.
    assert_eq!(
        format_number_1904(1234.5, "0.00"),
        format_number(1234.5, "0.00")
    );
    // Time-of-day is epoch-independent too.
    assert_eq!(format_number_1904(0.5, "h:mm"), "12:00");
}

#[test]
fn month_and_day_names_follow_the_format_locale() {
    use crate::format_number;

    // 45000 = 2023-03-15, a Wednesday.
    assert_eq!(
        format_number(45000.0, "dddd d mmmm yyyy"),
        "Wednesday 15 March 2023"
    );
    // Excel takes the language from the code, not the reader's machine, so the
    // same file reads the same everywhere.
    assert_eq!(
        format_number(45000.0, "[$-40C]dddd d mmmm yyyy"),
        "mercredi 15 mars 2023"
    );
    assert_eq!(
        format_number(45000.0, "[$-407]dddd d mmmm yyyy"),
        "Mittwoch 15 März 2023"
    );
    // Sub-language is ignored: Swiss German (0x807) is still German.
    assert_eq!(format_number(45000.0, "[$-807]mmmm"), "März");
    // An unknown language falls back to English rather than failing.
    assert_eq!(format_number(45000.0, "[$-41F]mmmm"), "March");
}

#[test]
fn abbreviating_a_localized_name_does_not_split_a_code_point() {
    use crate::format_number;

    // `&"décembre"[..3]` would panic mid-character; truncation is by chars.
    assert_eq!(format_number(45261.0, "[$-40C]mmm"), "déc");
    assert_eq!(format_number(45261.0, "[$-40C]mmmmm"), "d");
}

#[test]
fn a_currency_symbol_before_the_locale_still_selects_the_language() {
    use crate::format_number;

    // `[$€-40C]` carries both; the id is after the dash.
    assert_eq!(format_number(45000.0, "[$€-40C]mmmm"), "mars");
}

/// The pane split: what a freeze does to a viewport.
///
/// These are the arithmetic the headless renderer needs in order to draw a
/// frozen sheet the way the editor canvas does — for a long time it did not,
/// and a frozen header scrolled away in a PNG while holding still on screen.
mod panes {
    use crate::{DEFAULT_COL_WIDTH, DEFAULT_ROW_HEIGHT, Freeze, GridGeometry, Viewport, panes};

    fn geometry() -> GridGeometry {
        GridGeometry::default()
    }

    fn viewport() -> Viewport {
        Viewport {
            x: 4_000,
            y: 3_000,
            width: DEFAULT_COL_WIDTH * 10,
            height: DEFAULT_ROW_HEIGHT * 20,
        }
    }

    #[test]
    fn no_freeze_is_one_pane_that_is_the_viewport_itself() {
        // The property that lets every existing caller move to the split path
        // without changing a pixel: an unfrozen sheet is the same window it
        // always was, at the origin.
        let vp = viewport();
        let out = panes(&geometry(), &vp, Freeze::default());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].viewport, vp);
        assert_eq!(out[0].origin, (0, 0));
    }

    #[test]
    fn a_freeze_on_both_axes_makes_four_panes_that_tile_the_image() {
        let vp = viewport();
        let freeze = Freeze { rows: 2, cols: 1 };
        let out = panes(&geometry(), &vp, freeze);
        assert_eq!(out.len(), 4);

        let fw = DEFAULT_COL_WIDTH;
        let fh = DEFAULT_ROW_HEIGHT * 2;
        let origins: Vec<_> = out.iter().map(|p| p.origin).collect();
        assert_eq!(origins, vec![(0, 0), (fw, 0), (0, fh), (fw, fh)]);

        // Together they cover the image exactly: the right edge of the frozen
        // column plus the body's width is the whole width, and likewise down.
        assert_eq!(out[0].viewport.width + out[1].viewport.width, vp.width);
        assert_eq!(out[0].viewport.height + out[2].viewport.height, vp.height);
        assert_eq!(out[3].viewport.width, vp.width - fw);
        assert_eq!(out[3].viewport.height, vp.height - fh);
    }

    #[test]
    fn the_frozen_bands_look_at_the_top_left_however_far_the_body_has_scrolled() {
        // This is the entire point of a freeze. The corner never moves, the top
        // band scrolls only sideways, the left band only downwards.
        let freeze = Freeze { rows: 2, cols: 1 };
        let far = Viewport {
            x: 500_000,
            y: 900_000,
            ..viewport()
        };
        let out = panes(&geometry(), &far, freeze);

        assert_eq!((out[0].viewport.x, out[0].viewport.y), (0, 0), "corner");
        assert_eq!(out[1].viewport.y, 0, "top band does not scroll down");
        assert_eq!(out[1].viewport.x, far.x, "but it does scroll across");
        assert_eq!(out[2].viewport.x, 0, "left band does not scroll across");
        assert_eq!(out[2].viewport.y, far.y, "but it does scroll down");
        assert_eq!((out[3].viewport.x, out[3].viewport.y), (far.x, far.y));
    }

    #[test]
    fn the_body_cannot_scroll_back_into_the_frozen_band() {
        // Scrolled to the very top-left, the body still starts at the first
        // unfrozen line — otherwise the pinned rows appear twice, once held and
        // once beside themselves.
        let freeze = Freeze { rows: 3, cols: 2 };
        let home = Viewport {
            x: 0,
            y: 0,
            ..viewport()
        };
        let out = panes(&geometry(), &home, freeze);
        let body = out.last().unwrap();
        assert_eq!(body.viewport.x, DEFAULT_COL_WIDTH * 2);
        assert_eq!(body.viewport.y, DEFAULT_ROW_HEIGHT * 3);
    }

    #[test]
    fn freezing_one_axis_splits_only_that_axis() {
        let vp = viewport();

        let rows_only = panes(&geometry(), &vp, Freeze { rows: 1, cols: 0 });
        assert_eq!(rows_only.len(), 2, "a top band and a body");
        assert_eq!(rows_only[0].origin, (0, 0));
        assert_eq!(rows_only[1].origin, (0, DEFAULT_ROW_HEIGHT));
        assert_eq!(rows_only[0].viewport.width, vp.width, "full width each");
        assert_eq!(rows_only[1].viewport.x, vp.x, "and still scrolls across");

        let cols_only = panes(&geometry(), &vp, Freeze { rows: 0, cols: 1 });
        assert_eq!(cols_only.len(), 2, "a left band and a body");
        assert_eq!(cols_only[1].origin, (DEFAULT_COL_WIDTH, 0));
        assert_eq!(cols_only[0].viewport.height, vp.height);
    }

    #[test]
    fn a_freeze_too_wide_for_the_image_keeps_the_frozen_part_and_drops_the_rest() {
        // Not an error: the author asked for those lines to always be visible,
        // so when there is only room for them, they are what is shown.
        let vp = Viewport {
            x: 0,
            y: 0,
            width: DEFAULT_COL_WIDTH * 2,
            height: DEFAULT_ROW_HEIGHT * 2,
        };
        let out = panes(&geometry(), &vp, Freeze { rows: 50, cols: 50 });
        assert_eq!(out.len(), 1, "the corner alone; nothing scrolls");
        assert_eq!(out[0].origin, (0, 0));
        assert_eq!(out[0].viewport.width, vp.width);
        assert_eq!(out[0].viewport.height, vp.height);
    }

    #[test]
    fn a_freeze_exactly_filling_the_image_leaves_no_body() {
        let vp = Viewport {
            x: 0,
            y: 0,
            width: DEFAULT_COL_WIDTH * 3,
            height: DEFAULT_ROW_HEIGHT * 4,
        };
        let out = panes(&geometry(), &vp, Freeze { rows: 4, cols: 3 });
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].viewport.width, vp.width);
    }

    #[test]
    fn a_zero_sized_viewport_yields_nothing_rather_than_a_negative_pane() {
        let vp = Viewport {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
        assert!(panes(&geometry(), &vp, Freeze { rows: 1, cols: 1 }).is_empty());
        assert!(panes(&geometry(), &vp, Freeze::default()).is_empty());
    }

    #[test]
    fn is_none_is_true_only_when_nothing_is_pinned() {
        assert!(Freeze::default().is_none());
        assert!(!Freeze { rows: 1, cols: 0 }.is_none());
        assert!(!Freeze { rows: 0, cols: 1 }.is_none());
    }
}

/// Merged ranges: one cell that happens to be large.
///
/// RND-03. `casual-calc-layout` had no notion of a merge at all, so the
/// headless backend drew a merged range as separate cells — each with its own
/// gridlines and the anchor's text in the wrong box. It survived because every
/// surface a person looks at was right: the editor canvas handles merges and
/// they round-trip through the model. Only the PNG was wrong, and nothing
/// looked at the PNG.
mod merges {
    use casual_calc_model::{CellRange, Style};

    use super::*;
    use crate::{DEFAULT_COL_WIDTH, DEFAULT_ROW_HEIGHT, PaintItem, Rect};

    /// A2:C3 merged, with the anchor carrying text and a fill, and every cell
    /// the merge covers holding a value of its own — which Excel keeps in the
    /// file and does not show, and so must this.
    fn merged_sheet() -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let fill = wb.intern_style(Style {
            fill_color: Some("FFCC00".to_owned()),
            ..Style::default()
        });
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        for row in 1..=2u32 {
            for col in 0..=2u32 {
                sheet.cells.set(
                    CellRef::new(row, col),
                    Cell::value(CellValue::Number(f64::from(row * 10 + col))),
                );
            }
        }
        let mut anchor = Cell::value(CellValue::Number(10.0));
        anchor.style = Some(fill);
        sheet.cells.set(CellRef::new(1, 0), anchor);
        sheet
            .merges
            .push(CellRange::new(CellRef::new(1, 0), CellRef::new(2, 2)));
        wb.sheets.push(sheet);
        wb
    }

    fn rects(list: &crate::DisplayList) -> Vec<Rect> {
        list.items
            .iter()
            .filter_map(|item| match item {
                PaintItem::CellBackground { rect, .. }
                | PaintItem::GridLine { rect }
                | PaintItem::MergedRegion { rect, .. }
                | PaintItem::DataBar { rect, .. }
                | PaintItem::Text { rect, .. }
                | PaintItem::Image { rect, .. }
                | PaintItem::CellBorder { rect, .. } => Some(*rect),
                // The geometry variants are not addressed by a rectangle at
                // all — that is what makes them a different kind of item
                // (ADR-021) — so there is nothing for this helper to collect.
                PaintItem::Polyline { .. }
                | PaintItem::Polygon { .. }
                | PaintItem::Wedge { .. } => None,
            })
            .collect()
    }

    #[test]
    fn a_merged_range_is_drawn_once_across_its_whole_rectangle() {
        let wb = merged_sheet();
        let list = layout_full(&wb, 0, &GridGeometry::default());

        let union = Rect {
            x: 0,
            y: DEFAULT_ROW_HEIGHT,
            w: DEFAULT_COL_WIDTH * 3,
            h: DEFAULT_ROW_HEIGHT * 2,
        };
        assert!(
            rects(&list).contains(&union),
            "the merge paints as its union rectangle, not as six cells"
        );

        // And exactly one text item, the anchor's — six values are stored, one
        // is shown.
        let texts: Vec<_> = list
            .items
            .iter()
            .filter(|i| matches!(i, PaintItem::Text { .. }))
            .collect();
        assert_eq!(texts.len(), 1, "one visible cell, so one string");
        let PaintItem::Text { rect, content, .. } = texts[0] else {
            unreachable!()
        };
        assert_eq!(content, "10", "the anchor's value");
        assert_eq!(*rect, union, "laid out across the merge, not its own cell");
    }

    #[test]
    fn the_covered_cells_keep_their_values_and_are_not_drawn() {
        // The distinction that matters: a merge hides cells, it does not empty
        // them. Losing them on save would be data loss.
        let wb = merged_sheet();
        assert_eq!(
            wb.sheets[0].cells.get(CellRef::new(2, 2)).map(|c| &c.value),
            Some(&CellValue::Number(22.0)),
            "still in the model"
        );

        let list = layout_full(&wb, 0, &GridGeometry::default());
        let own_cell = Rect {
            x: DEFAULT_COL_WIDTH * 2,
            y: DEFAULT_ROW_HEIGHT * 2,
            w: DEFAULT_COL_WIDTH,
            h: DEFAULT_ROW_HEIGHT,
        };
        assert!(
            !rects(&list).contains(&own_cell),
            "and not painted in its own right"
        );
    }

    #[test]
    fn the_anchors_style_covers_the_whole_merge() {
        let wb = merged_sheet();
        let list = layout_full(&wb, 0, &GridGeometry::default());
        // The fill rides on the region item, not on a separate background:
        // the two must not be orderable, or the background paints away the
        // region's outline and two adjacent merges read as one.
        let fills: Vec<_> = list
            .items
            .iter()
            .filter_map(|i| match i {
                PaintItem::MergedRegion { rect, fill } => Some((*rect, fill.clone())),
                _ => None,
            })
            .collect();
        assert!(
            !list
                .items
                .iter()
                .any(|i| matches!(i, PaintItem::CellBackground { .. })),
            "a merged anchor emits no separate background"
        );
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].1.as_deref(), Some("FFCC00"));
        assert_eq!(
            fills[0].0.w,
            DEFAULT_COL_WIDTH * 3,
            "the fill spans the merge — a merged header half-coloured is the \
             most visible way to get this wrong"
        );
    }

    #[test]
    fn a_merge_anchored_off_screen_still_shows_in_the_window() {
        // The virtualization trap: the anchor is off screen *because* the block
        // is wide, so dropping merges whose anchor is outside the window makes
        // a merged band vanish exactly when it is widest.
        let wb = merged_sheet();
        let geo = GridGeometry::default();
        // A window over column C only — the merge is anchored at column A.
        let vp = Viewport {
            x: DEFAULT_COL_WIDTH * 2 + 10,
            y: DEFAULT_ROW_HEIGHT + 10,
            width: DEFAULT_COL_WIDTH / 2,
            height: DEFAULT_ROW_HEIGHT / 2,
        };
        let list = layout_viewport(&wb, 0, &geo, &vp);
        assert!(
            list.items
                .iter()
                .any(|i| matches!(i, PaintItem::Text { content, .. } if content == "10")),
            "the merged block is visible here, so it is laid out here"
        );
    }

    #[test]
    fn a_viewport_covering_everything_still_equals_the_full_layout() {
        // The crate's founding invariant, re-checked with merges in play: the
        // virtualized path and the reference path must not diverge.
        let wb = merged_sheet();
        let geo = GridGeometry::default();
        let vp = Viewport {
            x: 0,
            y: 0,
            width: 1_000_000,
            height: 1_000_000,
        };
        assert_eq!(
            layout_full(&wb, 0, &geo),
            layout_viewport(&wb, 0, &geo, &vp)
        );
    }

    #[test]
    fn the_display_list_does_not_depend_on_the_order_merges_are_stored_in() {
        // `Sheet::merges` is a Vec with no promised order, and a re-saved file
        // can reorder it. A display list that inherited that order would be a
        // golden test failing for no reason anyone could see.
        let mut a = merged_sheet();
        a.sheets[0]
            .merges
            .push(CellRange::new(CellRef::new(5, 0), CellRef::new(5, 1)));
        let mut b = a.clone();
        b.sheets[0].merges.reverse();

        let geo = GridGeometry::default();
        assert_eq!(layout_full(&a, 0, &geo), layout_full(&b, 0, &geo));
    }

    #[test]
    fn a_hidden_column_inside_a_merge_narrows_it_rather_than_breaking_it() {
        // Width is measured from the offset index, not by multiplying a default,
        // so a zero-width column inside the span simply contributes nothing.
        let wb = merged_sheet();
        let mut geo = GridGeometry::default();
        geo.columns.set_size(1, 0);

        let list = layout_full(&wb, 0, &geo);
        let widths: Vec<i64> = list
            .items
            .iter()
            .filter_map(|i| match i {
                PaintItem::MergedRegion { rect, .. } => Some(rect.w),
                _ => None,
            })
            .collect();
        assert_eq!(
            widths,
            vec![DEFAULT_COL_WIDTH * 2],
            "three columns, one hidden"
        );
    }

    #[test]
    fn a_sheet_with_no_merges_lays_out_exactly_as_before() {
        // The compatibility property: the merge pass must be invisible to every
        // sheet that has none, which is nearly all of them.
        let wb = sample();
        let geo = GridGeometry::default();
        let list = layout_full(&wb, 0, &geo);
        assert_eq!(list.items.len(), 4, "the four cells of the sample sheet");
    }
}

// --- Conditional formatting reaches the display list (RND-05) ----------------

mod conditional_formatting {
    use super::*;
    use crate::display::{DisplayList, PaintItem};
    use casual_calc_model::{CellRange, CfRule, ConditionalFormat, Style};

    fn sheet_with_rule(rule: CfRule, fill: &str) -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        for (row, value) in [(0u32, 1.0), (1, 50.0), (2, 100.0)] {
            sheet
                .cells
                .set(CellRef::new(row, 0), Cell::value(CellValue::Number(value)));
        }
        sheet.conditional_formats.push(ConditionalFormat {
            range: CellRange::new(CellRef::new(0, 0), CellRef::new(2, 0)),
            rule,
            fill: fill.to_owned(),
            font_color: None,
            bold: false,
            priority: 0,
            stop_if_true: false,
        });
        wb.sheets.push(sheet);
        wb
    }

    fn fills(list: &DisplayList) -> Vec<Option<String>> {
        list.items
            .iter()
            .filter_map(|item| match item {
                PaintItem::CellBackground { fill, .. } => Some(fill.clone()),
                _ => None,
            })
            .collect()
    }

    fn whole_sheet(workbook: &Workbook) -> DisplayList {
        let geometry = GridGeometry::for_sheet(&workbook.sheets[0]);
        layout_viewport(
            workbook,
            0,
            &geometry,
            &Viewport {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            },
        )
    }

    /// **A highlight rule reaches the display list.**
    ///
    /// This is `RND-05`: the rules were resolved inside `casual-calc-wasm`, a
    /// host crate the render path cannot depend on, so the canvas showed them
    /// and every headless PNG — thumbnail, preview, server-side export — showed
    /// a sheet of plain cells. Not because the logic was missing, but because
    /// it was in the wrong crate.
    #[test]
    fn a_highlight_rule_paints_the_headless_render_too() {
        let wb = sheet_with_rule(CfRule::GreaterThan(40.0), "FF0000");
        let painted = fills(&whole_sheet(&wb));

        // Two of the three cells are over forty.
        assert_eq!(
            painted
                .iter()
                .filter(|f| f.as_deref() == Some("FF0000"))
                .count(),
            2,
            "got {painted:?}"
        );
    }

    /// **A colour scale is interpolated across the range, not applied flat.**
    ///
    /// The scale needs the range's own extremes, which is why the statistics
    /// are computed per rule rather than per cell — and why a rule that reached
    /// the display list with a single colour would still look wrong.
    #[test]
    fn a_colour_scale_varies_across_the_range() {
        let wb = sheet_with_rule(
            CfRule::ColorScale(vec!["FFFFFF".to_owned(), "000000".to_owned()]),
            "",
        );
        let painted: Vec<String> = fills(&whole_sheet(&wb)).into_iter().flatten().collect();

        assert_eq!(painted.len(), 3, "every cell in the range is filled");
        let distinct: std::collections::BTreeSet<&String> = painted.iter().collect();
        assert!(
            distinct.len() >= 2,
            "the scale produced one colour for the whole range: {painted:?}"
        );
        // Low end is the first stop, high end the last.
        assert_eq!(painted[0], "FFFFFF");
        assert_eq!(painted[2], "000000");
    }

    /// **A rule's fill beats the cell's own.**
    ///
    /// That is what conditional formatting means, and the order is what makes
    /// it true: taking the style's fill first would paint the rule away.
    #[test]
    fn a_rule_overrides_the_cells_own_fill() {
        let mut wb = sheet_with_rule(CfRule::GreaterThan(40.0), "FF0000");
        let style = wb.styles.intern(Style {
            fill_color: Some("00FF00".to_owned()),
            ..Style::default()
        });
        for row in 0..3 {
            if let Some(cell) = wb.sheets[0].cells.get(CellRef::new(row, 0)).cloned() {
                wb.sheets[0].cells.set(
                    CellRef::new(row, 0),
                    Cell {
                        style: Some(style),
                        ..cell
                    },
                );
            }
        }

        let painted: Vec<String> = fills(&whole_sheet(&wb)).into_iter().flatten().collect();
        assert_eq!(
            painted,
            vec!["00FF00", "FF0000", "FF0000"],
            "the first cell is under the threshold and keeps its own fill"
        );
    }

    /// **A sheet with no rules is untouched, and pays nothing.**
    #[test]
    fn a_sheet_without_rules_is_unchanged() {
        let mut wb = sheet_with_rule(CfRule::GreaterThan(40.0), "FF0000");
        wb.sheets[0].conditional_formats.clear();
        assert!(
            fills(&whole_sheet(&wb)).iter().all(Option::is_none),
            "a sheet with no rules gained a fill"
        );
    }

    // --- Data bars (RND-07) --------------------------------------------------

    /// Every data bar in the list, in painter's order.
    fn bars(list: &DisplayList) -> Vec<(crate::Rect, f64, String)> {
        list.items
            .iter()
            .filter_map(|item| match item {
                PaintItem::DataBar {
                    rect,
                    fraction,
                    color,
                } => Some((*rect, *fraction, color.clone())),
                _ => None,
            })
            .collect()
    }

    /// **A data bar reaches the display list, at the width the value earns.**
    ///
    /// This is `RND-07`: `conditional::effect_for` has always resolved the
    /// fraction and the colour, and nothing consumed it — the display list had
    /// no primitive for a partial-width rectangle inside a cell, so the browser
    /// canvas drew bars from its own payload and every headless PNG did not.
    /// The fraction is the whole point of the item, so it is what is asserted:
    /// a bar that reached the list at a constant width would still be wrong.
    #[test]
    fn a_data_bar_reaches_the_display_list_at_the_right_fraction() {
        let wb = sheet_with_rule(CfRule::DataBar("638EC6".to_owned()), "");
        let list = whole_sheet(&wb);
        let bars = bars(&list);

        assert_eq!(bars.len(), 3, "one bar per cell in the range: {bars:?}");
        // 1, 50 and 100 across a range whose extremes are 1 and 100.
        //
        // The position in the range is interpolated between `minLength` and
        // `maxLength` -- ECMA-376's `dataBar` defaults, 10% and 90% of the cell
        // -- rather than emitted raw, so the range minimum draws a short bar
        // instead of nothing at all (`RND-09`).
        let fractions: Vec<f64> = bars.iter().map(|b| b.1).collect();
        let (lo, hi) = (0.10, 0.90);
        let expected = [lo, lo + (49.0 / 99.0) * (hi - lo), hi];
        for (got, want) in fractions.iter().zip(expected) {
            assert!(
                (got - want).abs() < 1e-9,
                "fractions {fractions:?} should be {expected:?}"
            );
        }
        assert!(
            bars.iter().all(|b| b.2 == "638EC6"),
            "the rule's colour travels with the bar: {bars:?}"
        );
        // The cell's rectangle, not the bar's: the backend applies the inset
        // and multiplies by the fraction.
        for (i, (rect, _, _)) in bars.iter().enumerate() {
            assert_eq!(rect.w, DEFAULT_COL_WIDTH, "bar {i} carries the cell width");
            assert_eq!(
                rect.y,
                i as i64 * DEFAULT_ROW_HEIGHT,
                "bar {i} sits on its own row"
            );
        }
    }

    /// **A cell with no data bar emits no data bar.**
    ///
    /// Every other rule kind resolves through the same `CellEffect`, so an
    /// emit that keyed off the effect being non-empty rather than off
    /// `data_bar` would put a zero-width bar under every highlighted cell.
    #[test]
    fn a_cell_without_a_data_bar_emits_none() {
        let wb = sheet_with_rule(CfRule::GreaterThan(40.0), "FF0000");
        assert!(
            bars(&whole_sheet(&wb)).is_empty(),
            "a highlight rule produced a data bar"
        );

        let mut plain = wb;
        plain.sheets[0].conditional_formats.clear();
        assert!(
            bars(&whole_sheet(&plain)).is_empty(),
            "a sheet with no rules at all produced a data bar"
        );
    }

    /// **The smallest value in the range still draws a bar.**
    ///
    /// A raw `(n - lo) / (hi - lo)` gives the range minimum a fraction of
    /// zero, so it rendered nothing — indistinguishable from a cell the rule
    /// does not cover, or from an empty one. That is the one value a reader
    /// most wants to pick out of the range, and it was the only one with no
    /// mark on it (`RND-09`).
    ///
    /// The bound is not a guess at what Excel appears to do: ECMA-376 gives
    /// `dataBar` a `minLength` defaulting to 10% of the cell width, and the
    /// fraction is interpolated from there.
    #[test]
    fn the_lowest_value_in_a_data_bar_range_still_draws_a_bar() {
        let wb = sheet_with_rule(CfRule::DataBar("638EC6".to_owned()), "");
        let bars = bars(&whole_sheet(&wb));

        // **By position, not by sorting the fractions.** The range holds 1, 50
        // and 100 down column A, so the first bar belongs to the smallest
        // value. Taking `min` of the fractions instead would pass just as
        // happily on a bar that runs backwards -- the smallest value drawing
        // the longest bar -- because reversing the scale leaves the *set* of
        // fractions untouched and only moves which cell gets which.
        let (smallest, largest) = (bars[0].1, bars[2].1);
        assert!(
            smallest > 0.0,
            "the range minimum drew no bar at all: {bars:?}"
        );
        assert!(
            (smallest - 0.10).abs() < 1e-9,
            "the smallest value should draw ECMA-376's default minLength of 10%: {bars:?}"
        );

        // And the largest stops short of the full cell, per the matching
        // `maxLength` default -- a bar with no edge is not readable as a bar.
        assert!(
            (largest - 0.90).abs() < 1e-9,
            "the largest value should draw maxLength, 90%: {bars:?}"
        );
    }

    /// **The bar is painted after the cell's background and before its text.**
    ///
    /// Both halves matter and each fails differently: behind the background the
    /// fill paints the bar away, and in front of the text the bar covers the
    /// number it exists to annotate.
    #[test]
    fn a_data_bar_is_ordered_between_the_fill_and_the_text() {
        let mut wb = sheet_with_rule(CfRule::DataBar("638EC6".to_owned()), "");
        let style = wb.styles.intern(Style {
            fill_color: Some("00FF00".to_owned()),
            ..Style::default()
        });
        for row in 0..3 {
            if let Some(cell) = wb.sheets[0].cells.get(CellRef::new(row, 0)).cloned() {
                wb.sheets[0].cells.set(
                    CellRef::new(row, 0),
                    Cell {
                        style: Some(style),
                        ..cell
                    },
                );
            }
        }

        let list = whole_sheet(&wb);
        let kinds: Vec<&'static str> = list
            .items
            .iter()
            .map(|item| match item {
                PaintItem::CellBackground { .. } => "fill",
                PaintItem::DataBar { .. } => "bar",
                PaintItem::Text { .. } => "text",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                "fill", "bar", "text", // A1
                "fill", "bar", "text", // A2
                "fill", "bar", "text", // A3
            ],
            "fill, then bar, then text, for each of the three cells"
        );
    }
}

/// Images: an anchored picture reaching the display list at the rectangle its
/// anchor and EMU offsets put it at (`RND-06`).
mod images {
    use super::*;
    use crate::{DisplayList, PaintItem, Rect};
    use casual_calc_model::{CellRange, Emu, ImageView};

    /// A workbook whose sheet carries one picture over `B2:C3`, offset into
    /// both corners so the frame does not land on a gridline — which is the
    /// case that a cell-only anchor gets visibly wrong.
    fn with_image() -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        sheet
            .cells
            .set(CellRef::new(0, 0), Cell::value(CellValue::Number(1.0)));
        sheet.images.push(ImageView {
            anchor: CellRange::new(CellRef::new(1, 1), CellRef::new(2, 2)),
            // 635 EMU to the twip exactly (914_400 per inch / 1_440 per inch).
            from_offset: Emu {
                x: 10 * 635,
                y: 5 * 635,
            },
            to_offset: Emu {
                x: 20 * 635,
                y: 7 * 635,
            },
            part: "xl/media/image1.png".to_owned(),
            extent: None,
        });
        wb.sheets.push(sheet);
        wb
    }

    fn images_of(list: &DisplayList) -> Vec<(Rect, String)> {
        list.items
            .iter()
            .filter_map(|item| match item {
                PaintItem::Image { rect, part } => Some((*rect, part.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_image_reaches_the_display_list_at_its_anchor_rect() {
        let wb = with_image();
        let geo = GridGeometry::default();
        let list = layout_full(&wb, 0, &geo);

        let found = images_of(&list);
        assert_eq!(found.len(), 1, "expected one image item, got {found:?}");
        let (rect, part) = &found[0];
        assert_eq!(part, "xl/media/image1.png");
        // Left/top: the anchor cell's own edge plus the `from` offset.
        assert_eq!(rect.x, DEFAULT_COL_WIDTH + 10, "left edge");
        assert_eq!(rect.y, DEFAULT_ROW_HEIGHT + 5, "top edge");
        // Right/bottom: the far edge of the *last covered* line plus the `to`
        // offset. `to` is measured past that edge, so the width is
        // `offset(end + 1) + to - x`.
        assert_eq!(
            rect.w,
            3 * DEFAULT_COL_WIDTH + 20 - (DEFAULT_COL_WIDTH + 10),
            "width"
        );
        assert_eq!(
            rect.h,
            3 * DEFAULT_ROW_HEIGHT + 7 - (DEFAULT_ROW_HEIGHT + 5),
            "height"
        );
    }

    /// A picture floats over the grid: it is drawn after every cell item, not
    /// interleaved with the cells it happens to overlap. Emitted before them it
    /// would be painted away by the backgrounds of the cells underneath.
    #[test]
    fn an_image_is_painted_on_top_of_the_cells() {
        let wb = with_image();
        let geo = GridGeometry::default();
        let list = layout_full(&wb, 0, &geo);

        let image_at = list
            .items
            .iter()
            .position(|i| matches!(i, PaintItem::Image { .. }))
            .expect("no image item");
        let last_cell_item = list
            .items
            .iter()
            .rposition(|i| !matches!(i, PaintItem::Image { .. }))
            .expect("no cell items");
        assert!(
            image_at > last_cell_item,
            "image at {image_at} should follow every cell item (last at {last_cell_item})"
        );
    }

    /// The display list is serialisable so it can be golden-tested (ADR-008),
    /// which a variant carrying a `String` can break on its own: the picture's
    /// part path has to survive the round trip or a golden compares two lists
    /// that only look alike.
    #[test]
    fn a_picture_survives_a_display_list_round_trip() {
        let list = layout_full(&with_image(), 0, &GridGeometry::default());
        assert!(!images_of(&list).is_empty(), "nothing to round trip");
        let json = serde_json::to_string(&list).unwrap();
        assert!(
            json.contains("xl/media/image1.png"),
            "the part path did not reach the JSON: {json}"
        );
        let back: DisplayList = serde_json::from_str(&json).unwrap();
        assert_eq!(list, back);
    }

    /// Virtualization: a picture anchored outside the window is not laid out,
    /// and one straddling the window's edge still is — the anchor is often off
    /// screen precisely because the frame is large.
    #[test]
    fn an_image_is_virtualized_by_its_frame() {
        let mut wb = with_image();
        wb.sheets[0].images.push(ImageView {
            anchor: CellRange::new(CellRef::new(40, 40), CellRef::new(41, 41)),
            from_offset: Emu::default(),
            to_offset: Emu::default(),
            part: "xl/media/faraway.png".to_owned(),
            extent: None,
        });
        let geo = GridGeometry::default();
        // A window over A1:E5 — the first picture is inside it, the second is
        // nowhere near.
        let vp = Viewport {
            x: 0,
            y: 0,
            width: 5 * DEFAULT_COL_WIDTH,
            height: 5 * DEFAULT_ROW_HEIGHT,
        };
        let parts: Vec<String> = images_of(&layout_viewport(&wb, 0, &geo, &vp))
            .into_iter()
            .map(|(_, p)| p)
            .collect();
        assert_eq!(parts, vec!["xl/media/image1.png".to_owned()]);
    }

    /// A picture whose anchor starts left of and above the window but whose
    /// frame reaches into it is still laid out. Testing containment rather than
    /// intersection is how a large picture flickers out of existence as it is
    /// scrolled into.
    #[test]
    fn an_image_straddling_the_window_edge_is_kept() {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        sheet.images.push(ImageView {
            anchor: CellRange::new(CellRef::new(0, 0), CellRef::new(30, 30)),
            from_offset: Emu::default(),
            to_offset: Emu::default(),
            part: "xl/media/big.png".to_owned(),
            extent: None,
        });
        wb.sheets.push(sheet);
        let geo = GridGeometry::default();
        // A window well inside the picture, touching neither of its corners.
        let vp = Viewport {
            x: 10 * DEFAULT_COL_WIDTH,
            y: 10 * DEFAULT_ROW_HEIGHT,
            width: 2 * DEFAULT_COL_WIDTH,
            height: 2 * DEFAULT_ROW_HEIGHT,
        };
        assert_eq!(images_of(&layout_viewport(&wb, 0, &geo, &vp)).len(), 1);
    }
}

/// Charts reaching the display list as geometry (`RND-11`).
///
/// The series resolution these lean on used to live in `casual-calc-wasm`, so
/// none of it could be tested from here at all — the render path could not
/// reach it, which is exactly why the headless PNG had no charts in it.
mod charts {
    use casual_calc_model::{
        Cell, CellRange, CellRef, CellValue, ChartKind, ChartSeries, ChartView, Id, Sheet, SheetId,
        Workbook,
    };

    use crate::chart::{
        MAX_LABEL_POINTS, MAX_LINE_POINTS, MAX_PIE_WEDGES, MAX_SCATTER_MARKERS, PX, resolve,
        series_colors, value_extent,
    };
    use crate::chart_data::{MAX_SERIES_POINTS, ref_cells, ref_numbers, ref_text};
    use crate::{GridGeometry, PaintItem, Point, layout_full};

    /// `A1:A3` holding 1, "two", 3 on `S`, and a second sheet `T` holding 9 in
    /// `A1`, so a cross-sheet reference has somewhere to point.
    fn wb() -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let two = wb.intern_string("two");
        let mut s = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        s.cells
            .set(CellRef::new(0, 0), Cell::value(CellValue::Number(1.0)));
        s.cells.set(
            CellRef::new(1, 0),
            Cell::value(CellValue::SharedString(two)),
        );
        s.cells
            .set(CellRef::new(2, 0), Cell::value(CellValue::Number(3.0)));
        wb.sheets.push(s);
        let mut t = Sheet::new(SheetId(Id::from_parts(2, 2)), "T");
        t.cells
            .set(CellRef::new(0, 0), Cell::value(CellValue::Number(9.0)));
        wb.sheets.push(t);
        wb
    }

    #[test]
    fn a_range_resolves_to_its_cells_in_reading_order() {
        let wb = wb();
        let cells = ref_cells(&wb, 0, "Nope!$A$1:$B$2");
        // An unknown sheet name resolves to nothing rather than to the default.
        assert!(cells.is_empty(), "unknown sheet resolved: {cells:?}");

        let cells = ref_cells(&wb, 0, "$A$1:$B$2");
        assert_eq!(
            cells,
            vec![
                (0, CellRef::new(0, 0)),
                (0, CellRef::new(0, 1)),
                (0, CellRef::new(1, 0)),
                (0, CellRef::new(1, 1)),
            ]
        );
    }

    /// **A non-numeric cell is a gap, not a zero.** The distinction is the
    /// whole reason `ref_numbers` returns options: a chart of flat zeroes looks
    /// like data.
    #[test]
    fn a_non_numeric_cell_is_a_gap_and_not_a_zero() {
        let wb = wb();
        assert_eq!(
            ref_numbers(&wb, 0, "$A$1:$A$3"),
            vec![Some(1.0), None, Some(3.0)]
        );
        // Cross-sheet, by name, case-insensitively.
        assert_eq!(ref_numbers(&wb, 0, "t!$A$1"), vec![Some(9.0)]);
        // Not a reference at all: no cells, no guess.
        assert!(ref_numbers(&wb, 0, "1+1").is_empty());
        assert_eq!(ref_text(&wb, 0, "$A$1:$A$2"), vec!["1", "two"]);
    }

    /// The extent always contains zero, so a bar's length is proportional to
    /// its value rather than to its distance from the smallest one.
    #[test]
    fn the_value_extent_always_includes_zero() {
        let wb = wb();
        let chart = column_chart(&["$A$1:$A$3"]);
        let (_, series) = resolve(&wb, 0, &chart);
        assert_eq!(value_extent(&series), (0.0, 3.0));
    }

    /// Series colours come from the workbook's own theme accents.
    ///
    /// **This test used to assert the defect** (`CHT-09`): it pinned
    /// `series_colors(&wb, 7)[6] == "4472C4"` with the comment *"the palette
    /// cycles"*. Two things were tangled in that one line. The part worth
    /// keeping is that the palette is *the file's theme* and not a list
    /// invented here — that is checked below and unchanged. The part that was
    /// wrong is the literal repeat: cycling six accents gives series 1 and
    /// series 7 the same fill and a legend that cannot tell them apart. So the
    /// assertion is not overridden, it is split: the theme is still the source,
    /// and the seventh colour is now a *variant* of the first rather than the
    /// first.
    #[test]
    fn series_colours_are_the_workbook_theme_accents() {
        let mut wb = wb();
        assert_eq!(series_colors(&wb, 2), vec!["4472C4", "ED7D31"]);
        // Accent 1 darkened 25%, Excel's own variant of the colour it shares a
        // slot with — recognisably the same hue, and not the same fill.
        assert_eq!(
            series_colors(&wb, 7)[6],
            "335693",
            "a variant, not a repeat"
        );
        wb.theme_colors = vec![
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            "AA0000".to_owned(),
        ];
        assert_eq!(series_colors(&wb, 1), vec!["AA0000"], "this file's accent");
    }

    /// The seventh series must not repeat the first's fill — the whole of
    /// `CHT-09`, asserted as a property rather than as one literal so it keeps
    /// holding if the theme or the variant table changes.
    #[test]
    fn no_two_of_the_first_twelve_series_share_a_colour() {
        let wb = wb();
        let colors = series_colors(&wb, 12);
        assert_eq!(colors[0], "4472C4");
        for (i, a) in colors.iter().enumerate() {
            for (j, b) in colors.iter().enumerate().skip(i + 1) {
                assert_ne!(a, b, "series {} and series {} share {a}", i + 1, j + 1);
            }
        }
    }

    /// A theme colour this file spells in a way the variant arithmetic cannot
    /// read is returned as it stands. Repeating an accent is a bad picture;
    /// inventing a colour for one is a wrong one.
    #[test]
    fn an_unreadable_theme_colour_is_not_guessed_at() {
        let mut wb = wb();
        wb.theme_colors = vec![
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            "not a colour".to_owned(),
        ];
        let colors = series_colors(&wb, 7);
        assert_eq!(colors[0], "not a colour");
        assert_eq!(colors[6], "not a colour");
    }

    /// A chart over `A1:F10` — 384x200 px at 96 dpi with the default geometry,
    /// which is room enough for a plot.
    fn column_chart(values: &[&str]) -> ChartView {
        let mut ch = ChartView::new(
            CellRange::new(CellRef::new(0, 0), CellRef::new(9, 5)),
            ChartKind::Column,
        );
        ch.series = values
            .iter()
            .map(|v| ChartSeries {
                name: String::new(),
                categories: None,
                values: (*v).to_owned(),
                ..ChartSeries::default()
            })
            .collect();
        ch
    }

    fn polygons(list: &crate::DisplayList) -> Vec<(Vec<Point>, String)> {
        list.items
            .iter()
            .filter_map(|i| match i {
                PaintItem::Polygon { points, fill } => Some((points.clone(), fill.clone())),
                _ => None,
            })
            .collect()
    }

    // --- The legend (RND-11) -------------------------------------------
    //
    // Layout had no text advances, so it could not size a legend box; and the
    // plot is *what is left over* from that box, so it could not place the plot
    // either. It left the legend out entirely and gave the plot the whole
    // frame, which made every chart with a legend render with a plot the width
    // of the legend too wide. `casual-calc-text` measures it now.

    /// Every text item's content, in the order they were emitted.
    fn labels(list: &crate::DisplayList) -> Vec<String> {
        list.items
            .iter()
            .filter_map(|i| match i {
                PaintItem::Text { content, .. } => Some(content.clone()),
                _ => None,
            })
            .collect()
    }

    /// The bars of a two-series column chart, with `legend` set or not.
    fn bars_with_legend(legend: Option<&str>) -> Vec<(Vec<Point>, String)> {
        let mut wb = wb();
        let mut chart = column_chart(&["$A$1:$A$3"]);
        chart.legend = legend.map(str::to_owned);
        wb.sheets[0].charts.push(chart);
        let list = layout_full(&wb, 0, &GridGeometry::default());
        polygons(&list)
    }

    /// **The plot is narrower with a legend than without one.**
    ///
    /// The defect stated exactly. Asserted against the *same chart* with the
    /// legend turned off, so it cannot pass by the bars being any particular
    /// width — only by their being narrower than they are with no legend.
    #[test]
    fn a_legend_takes_its_side_out_of_the_plot() {
        let without = bars_with_legend(None);
        let with = bars_with_legend(Some("r"));

        // The frame's ground is polygon 0 and is the full frame either way; the
        // bars follow. The last bar is the rightmost, so its right edge is how
        // far the plot reaches.
        let right_edge = |bars: &[(Vec<Point>, String)]| {
            bars.last()
                .expect("bars")
                .0
                .iter()
                .map(|p| p.x)
                .max()
                .expect("a bar has corners")
        };
        let plain = right_edge(&without);
        let legended = right_edge(&with);
        assert!(
            legended < plain,
            "the plot reached just as far with a legend ({legended}) as without ({plain}) — \
             the legend took nothing out of it"
        );
    }

    /// A swatch and a name for each series, and the names are the ones a series
    /// with no name of its own is given.
    #[test]
    fn a_legend_names_every_series() {
        let mut wb = wb();
        let mut chart = column_chart(&["$A$1:$A$3", "$A$1:$A$3"]);
        chart.legend = Some("r".to_owned());
        wb.sheets[0].charts.push(chart);

        let list = layout_full(&wb, 0, &GridGeometry::default());
        let found = labels(&list);
        assert!(
            found.contains(&"Series 1".to_owned()) && found.contains(&"Series 2".to_owned()),
            "a series with no name of its own must be labelled by its position: {found:?}"
        );

        // One swatch per series, on top of the frame's ground and the bars.
        let swatches = polygons(&list);
        assert!(
            swatches.len() >= 2,
            "a legend with no swatches is a list of words: {swatches:?}"
        );
    }

    /// A named series is named, rather than numbered.
    #[test]
    fn a_named_series_keeps_its_name() {
        let mut wb = wb();
        let mut chart = column_chart(&["$A$1:$A$3"]);
        chart.series[0].name = "Revenue".to_owned();
        chart.legend = Some("r".to_owned());
        wb.sheets[0].charts.push(chart);

        let list = layout_full(&wb, 0, &GridGeometry::default());
        assert!(labels(&list).contains(&"Revenue".to_owned()));
    }

    /// **Left and right are different places.** A legend that ignored its side
    /// would pass every test above.
    #[test]
    fn a_legend_sits_on_the_side_it_names() {
        let left_bars = bars_with_legend(Some("l"));
        let right_bars = bars_with_legend(Some("r"));

        let left_edge = |bars: &[(Vec<Point>, String)]| {
            bars.last()
                .expect("bars")
                .0
                .iter()
                .map(|p| p.x)
                .min()
                .expect("corners")
        };
        assert!(
            left_edge(&left_bars) > left_edge(&right_bars),
            "a legend on the left must push the plot right, not shrink it from the right"
        );
    }

    /// A legend along the foot takes *height*, not width — so the bars stay as
    /// wide as they were and get shorter instead.
    #[test]
    fn a_legend_below_takes_height_rather_than_width() {
        let plain = bars_with_legend(None);
        let below = bars_with_legend(Some("b"));

        let right_edge = |bars: &[(Vec<Point>, String)]| {
            bars.last()
                .expect("bars")
                .0
                .iter()
                .map(|p| p.x)
                .max()
                .expect("corners")
        };
        let bottom_edge = |bars: &[(Vec<Point>, String)]| {
            bars.last()
                .expect("bars")
                .0
                .iter()
                .map(|p| p.y)
                .max()
                .expect("corners")
        };
        assert_eq!(
            right_edge(&plain),
            right_edge(&below),
            "a legend along the foot narrowed the plot"
        );
        assert!(
            bottom_edge(&below) < bottom_edge(&plain),
            "a legend along the foot took no height out of the plot"
        );
    }

    /// A frame too small to hold both is a frame that keeps its plot. A legend
    /// that leaves no room for the chart has cost more than it explains.
    #[test]
    fn a_legend_that_would_leave_no_plot_is_refused() {
        let mut wb = wb();
        // Two columns by two rows, which is nowhere near enough.
        let mut chart = ChartView::new(
            CellRange::new(CellRef::new(0, 0), CellRef::new(1, 1)),
            ChartKind::Column,
        );
        chart.series = vec![ChartSeries {
            name: "A series with a very long name indeed".to_owned(),
            categories: None,
            values: "$A$1:$A$3".to_owned(),
            ..ChartSeries::default()
        }];
        chart.legend = Some("r".to_owned());
        wb.sheets[0].charts.push(chart);

        // The point is that this does not panic and does not emit a legend it
        // has no room for.
        let list = layout_full(&wb, 0, &GridGeometry::default());
        assert!(
            !labels(&list).contains(&"A series with a very long name indeed".to_owned()),
            "a legend was drawn into a frame with no room for one"
        );
    }

    /// A column chart's bars land at the twip rectangle the canvas puts them
    /// at, and **the taller value gets the taller bar measured from the same
    /// zero line**.
    ///
    /// Asserted as exact geometry rather than as "some polygons were emitted":
    /// a plot that scaled every bar to the same height, or that measured them
    /// from the top of the frame, would emit exactly the same number of
    /// polygons.
    #[test]
    fn a_column_charts_bars_are_scaled_from_the_zero_line() {
        let mut wb = wb();
        // 1, a gap, then 3 — so a gap is also shown to draw nothing rather
        // than a zero-height bar sitting on the axis.
        wb.sheets[0].charts.push(column_chart(&["$A$1:$A$3"]));

        let list = layout_full(&wb, 0, &GridGeometry::default());
        let bars = polygons(&list);
        // The frame's own ground is the first polygon; the bars follow.
        assert_eq!(bars.len(), 3, "ground plus two bars: {bars:?}");
        assert_eq!(bars[0].1, "FFFFFF", "the frame's ground");

        // Plot: x = 34px, y = 6px, w = 384-44 = 340px, h = 200-6-18 = 176px.
        // The extent is 0..3, so the zero line is at the plot's foot, y = 182px.
        let px = |v: f64| (v * PX).round() as i64;
        let group_w = 340.0 / 3.0;
        let bar_w = group_w * 0.7;

        // The first value, 1 of 3: a third of the plot's height.
        let x0 = 34.0 + group_w * 0.15;
        let top0 = 6.0 + 176.0 * (2.0 / 3.0);
        assert_eq!(bars[1].1, "4472C4");
        assert_eq!(
            bars[1].0,
            vec![
                Point {
                    x: px(x0),
                    y: px(top0)
                },
                Point {
                    x: px(x0 + bar_w - 1.0),
                    y: px(top0)
                },
                Point {
                    x: px(x0 + bar_w - 1.0),
                    y: px(182.0)
                },
                Point {
                    x: px(x0),
                    y: px(182.0)
                },
            ],
            "first bar"
        );

        // The third value, 3 of 3: the whole plot height, and in the third
        // group — the gap in between drew nothing at all.
        let x2 = 34.0 + 2.0 * group_w + group_w * 0.15;
        assert_eq!(
            bars[2].0,
            vec![
                Point {
                    x: px(x2),
                    y: px(6.0)
                },
                Point {
                    x: px(x2 + bar_w - 1.0),
                    y: px(6.0)
                },
                Point {
                    x: px(x2 + bar_w - 1.0),
                    y: px(182.0)
                },
                Point {
                    x: px(x2),
                    y: px(182.0)
                },
            ],
            "third bar"
        );
    }

    /// A pie's slices start at twelve o'clock and run clockwise, and their
    /// sweeps are proportional to the values and sum to a full turn.
    #[test]
    fn a_pie_starts_at_twelve_oclock_and_sweeps_clockwise() {
        let mut wb = wb();
        let mut chart = column_chart(&["$A$1:$A$3"]);
        chart.kind = ChartKind::Pie;
        wb.sheets[0].charts.push(chart);

        let list = layout_full(&wb, 0, &GridGeometry::default());
        let wedges: Vec<_> = list
            .items
            .iter()
            .filter_map(|i| match i {
                PaintItem::Wedge {
                    from,
                    sweep,
                    inner_radius,
                    ..
                } => Some((*from, *sweep, *inner_radius)),
                _ => None,
            })
            .collect();
        // 1 and 3 of a total of 4 — the gap is not a slice.
        assert_eq!(wedges.len(), 2, "{wedges:?}");
        assert_eq!(wedges[0].0, 0.0, "the first slice starts at twelve");
        assert_eq!(wedges[0].1, 90.0, "1 of 4 is a quarter turn");
        assert_eq!(wedges[1].0, 90.0, "the second starts where the first ends");
        assert_eq!(wedges[1].1, 270.0);
        assert_eq!(wedges[0].2, 0, "a pie has no hole");
    }

    /// A doughnut is the same slices with a hole cut out of them, and the hole
    /// is **in the geometry** rather than a disc painted over it.
    #[test]
    fn a_doughnut_carries_its_hole_in_the_wedge() {
        let mut wb = wb();
        let mut chart = column_chart(&["$A$1:$A$3"]);
        chart.kind = ChartKind::Doughnut;
        wb.sheets[0].charts.push(chart);

        let list = layout_full(&wb, 0, &GridGeometry::default());
        let holes: Vec<(i64, i64)> = list
            .items
            .iter()
            .filter_map(|i| match i {
                PaintItem::Wedge {
                    radius,
                    inner_radius,
                    ..
                } => Some((*radius, *inner_radius)),
                _ => None,
            })
            .collect();
        assert!(!holes.is_empty());
        for (r, inner) in holes {
            assert!(inner > 0, "a doughnut with no hole is a pie");
            assert_eq!(inner, ((r as f64) * 0.55).round() as i64);
        }
    }

    /// A chart whose series resolve to nothing says so in the picture rather
    /// than leaving a blank frame — the canvas's rule, kept.
    #[test]
    fn a_chart_whose_data_does_not_resolve_says_no_data() {
        let mut wb = wb();
        wb.sheets[0].charts.push(column_chart(&["$Z$1:$Z$9"]));
        let list = layout_full(&wb, 0, &GridGeometry::default());
        let texts: Vec<&str> = list
            .items
            .iter()
            .filter_map(|i| match i {
                PaintItem::Text { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&"no data"), "{texts:?}");
        assert!(
            !list
                .items
                .iter()
                .any(|i| matches!(i, PaintItem::Wedge { .. })),
            "nothing was plotted"
        );
    }

    /// A chart kind this does not draw is **named in the picture**, not
    /// silently absent — which is what `ChartKind::Unsupported` is documented
    /// to be, and what the display list previously had no way to express.
    #[test]
    fn an_unsupported_kind_is_named_rather_than_left_blank() {
        let mut wb = wb();
        let mut chart = column_chart(&["$A$1:$A$3"]);
        chart.kind = ChartKind::Unsupported;
        wb.sheets[0].charts.push(chart);
        let list = layout_full(&wb, 0, &GridGeometry::default());
        assert!(
            list.items.iter().any(|i| matches!(
                i,
                PaintItem::Text { content, .. } if content == "unsupported chart not drawn"
            )),
            "{:?}",
            list.items
        );
    }

    /// The geometry variants survive the display list's serialization, which is
    /// what keeps it golden-testable (ADR-008).
    #[test]
    fn the_geometry_variants_round_trip_through_json() {
        let mut wb = wb();
        let mut chart = column_chart(&["$A$1:$A$3"]);
        chart.kind = ChartKind::Line;
        wb.sheets[0].charts.push(chart);
        let list = layout_full(&wb, 0, &GridGeometry::default());
        assert!(
            list.items
                .iter()
                .any(|i| matches!(i, PaintItem::Polyline { .. })),
            "a line chart draws a polyline"
        );
        let json = serde_json::to_string(&list).expect("serializes");
        let back: crate::DisplayList = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, list);
    }

    // --- A series whose sheet went away (CHT-08) -------------------------

    /// A series whose reference names no cells at all is **kept and marked**,
    /// where it used to be filtered out of the resolved list and vanish from
    /// the picture with nothing said.
    #[test]
    fn a_series_naming_a_missing_sheet_is_kept_and_marked() {
        let wb = wb();
        let mut chart = column_chart(&["$A$1:$A$3", "Gone!$A$1:$A$3"]);
        chart.series[0].name = "Rev".to_owned();
        chart.series[1].name = "FromGone".to_owned();
        let (_, series) = resolve(&wb, 0, &chart);
        assert_eq!(
            series.len(),
            2,
            "the broken series was dropped in silence: {series:?}"
        );
        assert!(!series[0].broken, "the live series must not be marked");
        assert!(series[1].broken, "the dead one must be");
        assert!(
            series[1].values.is_empty(),
            "a broken series plots nothing: {:?}",
            series[1].values
        );
    }

    /// A range of blank cells is **not** broken. The distinction is the point:
    /// a chart waiting for numbers and a chart that lost them are different
    /// faults, and marking the first `#REF!` would be its own lie.
    #[test]
    fn an_empty_range_is_unfilled_and_not_broken() {
        let wb = wb();
        // `$D$1:$D$3` is inside the sheet and holds nothing.
        let chart = column_chart(&["$D$1:$D$3"]);
        let (_, series) = resolve(&wb, 0, &chart);
        assert!(
            series.is_empty(),
            "an empty range is still dropped rather than plotted as zeroes: {series:?}"
        );
    }

    /// And the legend says so, so the picture names the series it cannot draw.
    #[test]
    fn the_legend_marks_a_broken_series() {
        let mut wb = wb();
        let mut chart = column_chart(&["$A$1:$A$3", "Gone!$A$1:$A$3"]);
        chart.series[0].name = "Rev".to_owned();
        chart.series[1].name = "FromGone".to_owned();
        chart.legend = Some("r".to_owned());
        wb.sheets[0].charts.push(chart);

        let found = labels(&layout_full(&wb, 0, &GridGeometry::default()));
        assert!(
            found.contains(&"FromGone (#REF!)".to_owned()),
            "the legend must name the dead series and mark it: {found:?}"
        );
        assert!(
            found.contains(&"Rev".to_owned()),
            "and leave the live one alone: {found:?}"
        );
    }

    /// Every series broken is not "no data" — it is a chart that had data.
    #[test]
    fn a_chart_whose_every_series_is_broken_says_which_fault_it_has() {
        let mut wb = wb();
        wb.sheets[0].charts.push(column_chart(&["Gone!$A$1:$A$3"]));
        let found = labels(&layout_full(&wb, 0, &GridGeometry::default()));
        assert!(
            found.contains(&"series reference broken (#REF!)".to_owned()),
            "{found:?}"
        );
        assert!(!found.contains(&"no data".to_owned()), "{found:?}");
    }

    // --- The bar plot's bound (CHT-06) ----------------------------------

    /// A frame 400x300 CSS pixels, in twips, which is the size the measurement
    /// in `docs/84` §3.5 was taken at.
    fn frame_400x300() -> crate::Rect {
        crate::Rect {
            x: 0,
            y: 0,
            w: (400.0 * PX) as i64,
            h: (300.0 * PX) as i64,
        }
    }

    /// A workbook with `rows` rows of `series` columns starting at `B1`, each
    /// column a series, plus one outlier planted at `spike_row` in column B.
    fn wide_workbook(rows: u32, series: u32, spike_row: u32) -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut s = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        for r in 0..rows {
            for c in 0..series {
                let v = if c == 0 && r == spike_row {
                    1_000_000.0
                } else {
                    f64::from(r % 50) + 1.0
                };
                s.cells
                    .set(CellRef::new(r, c + 1), Cell::value(CellValue::Number(v)));
            }
        }
        wb.sheets.push(s);
        wb
    }

    fn wide_chart(rows: u32, series: u32) -> ChartView {
        let refs: Vec<String> = (0..series)
            .map(|c| {
                let col = char::from(b'B' + u8::try_from(c).expect("fits"));
                format!("${col}$1:${col}${rows}")
            })
            .collect();
        let mut ch = ChartView::new(
            CellRange::new(CellRef::new(0, 0), CellRef::new(9, 5)),
            ChartKind::Column,
        );
        ch.series = refs
            .iter()
            .map(|v| ChartSeries {
                name: String::new(),
                categories: None,
                values: v.clone(),
                ..ChartSeries::default()
            })
            .collect();
        ch
    }

    /// **The bound, structurally.** Six series over 1,000 rows emitted 6,007
    /// display-list items — one polygon per point per series, uncapped, and
    /// every one of the 6,000 bars zero pixels wide because `bar_w` had hit its
    /// clamp. The bound is stated in items, not in milliseconds, because a
    /// wall-clock assertion on a shared machine is a test that gets deleted.
    #[test]
    fn a_bar_plot_is_bounded_by_what_the_plot_can_resolve() {
        for rows in [1_000u32, 5_000] {
            let wb = wide_workbook(rows, 6, 7);
            let chart = wide_chart(rows, 6);
            let mut list = crate::DisplayList::new();
            crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());
            let bars = polygons(&list).len() - 1; // less the frame's own ground
            assert!(
                bars <= 128,
                "{rows} rows x 6 series emitted {bars} bar polygons; \
                 uncapped that is {}",
                rows * 6
            );
            assert!(
                bars > 0,
                "the plot drew nothing at all, which is not a bound but a blank"
            );
            assert!(
                list.items.len() < 200,
                "{rows} rows x 6 series is {} display-list items",
                list.items.len()
            );
        }
    }

    /// **And the bound is honest.** A bucket is drawn from its minimum to its
    /// maximum, so the one row in five thousand holding a spike still reaches
    /// the top of the plot. A cap that kept every nth point would drop it and
    /// the chart would say the spike never happened.
    #[test]
    fn bucketing_keeps_the_outlier_it_would_be_a_lie_to_drop() {
        let rows = 5_000u32;
        let wb = wide_workbook(rows, 6, 3_331);
        let chart = wide_chart(rows, 6);
        let mut list = crate::DisplayList::new();
        crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());

        let bars = polygons(&list);
        // The plot's top edge: the tallest bar must reach it, and only the
        // spike is anywhere near 1,000,000.
        let top = bars
            .iter()
            .skip(1)
            .flat_map(|(pts, _)| pts.iter().map(|p| p.y))
            .min()
            .expect("bars");
        let floor = bars
            .iter()
            .skip(1)
            .flat_map(|(pts, _)| pts.iter().map(|p| p.y))
            .max()
            .expect("bars");
        // The spike is 20,000x the next value, so if it were dropped every bar
        // would be within a whisker of the zero line — the axis still scales to
        // 1,000,000 because `value_extent` sees every point — instead of
        // spanning the plot.
        let spike_reaches = (floor - top) as f64;
        let plot_h = 300.0 * PX;
        assert!(
            spike_reaches > plot_h * 0.5,
            "the tallest bar spans {spike_reaches} twips of a {plot_h}-twip frame — \
             the outlier was dropped rather than kept"
        );
    }

    /// A series shorter than the longest keeps every point it has.
    ///
    /// Found by asking what the bucketing does at the ragged end: the bucket
    /// range is built from the *longest* series' point count, so a short series
    /// has buckets that run past its last value. Slicing that out of bounds
    /// returns nothing for the whole bucket, which would drop the points at its
    /// start that do exist.
    #[test]
    fn a_series_shorter_than_the_longest_keeps_its_points() {
        // Ragged **references**, not ragged data: `ref_numbers` returns one
        // entry per cell of the range, gaps included, so blanking cells gives
        // two series of equal length and would prove nothing here. Two ranges
        // of different sizes is what makes one `values` vector shorter.
        let wb = wide_workbook(600, 2, 999);
        let mut chart = wide_chart(600, 2);
        chart.series[1].values = "$C$1:$C$295".to_owned();

        let mut list = crate::DisplayList::new();
        crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());
        let fills: Vec<String> = polygons(&list).into_iter().map(|(_, f)| f).collect();
        let first = fills.iter().filter(|f| *f == "4472C4").count();
        let second = fills.iter().filter(|f| *f == "ED7D31").count();
        assert!(second > 0, "the short series drew nothing: {fills:?}");
        assert_eq!(
            second,
            first.div_ceil(2),
            "the short series covers just under half the rows, so it must fill \
             every bucket that starts inside them and no fewer: {first} vs {second}"
        );
    }

    /// Below the bound nothing changes: an uncapped plot is the plot it always
    /// was, point for point. This is the control — a cap that fired on every
    /// chart would pass the two tests above and ruin every real one.
    #[test]
    fn a_plot_inside_the_bound_is_untouched() {
        let wb = wide_workbook(3, 2, 99);
        let chart = wide_chart(3, 2);
        let mut list = crate::DisplayList::new();
        crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());
        // Ground, plus one bar per point per series, and none merged.
        assert_eq!(polygons(&list).len(), 1 + 3 * 2);
    }

    // --- Reading a range: the bound, and the band (CHT-10, CHT-11) ------

    /// **A chart series reference was unbounded, and it is a file's string.**
    ///
    /// `$A$1:$XFD$1048576` names 17,179,869,184 cells. `ref_cells` built one
    /// `(usize, CellRef)` per cell, so resolving that chart asked the allocator
    /// for about 206 GB — measured, and the process was killed rather than
    /// erring. That is a denial of service reachable from an untrusted `.xlsx`,
    /// which AGENTS.md ranks third, above fidelity and speed both.
    ///
    /// Asserted through [`ref_strip`], which computes the size **without**
    /// allocating it: a test that had to allocate the unbounded answer in order
    /// to complain about it would take the runner down with it instead of
    /// failing.
    #[test]
    fn a_series_reference_cannot_name_more_cells_than_the_bound() {
        let wb = wide_workbook(4, 1, 99);
        for reference in [
            "$A$1:$XFD$1048576",
            "$A$1:$A$1048576",
            "A:A",
            "$A:$XFD",
            "$A$1:$B$1048576",
        ] {
            let strip = crate::chart_data::ref_strip(&wb, 0, reference)
                .unwrap_or_else(|| panic!("{reference} must resolve"));
            assert!(
                strip.len() <= MAX_SERIES_POINTS,
                "{reference} resolved to {} points, past the {MAX_SERIES_POINTS} bound: \
                 at 9 bytes a point that is {:.1} GB the reader would try to allocate",
                strip.len(),
                strip.len() as f64 * 9.0 / 1e9
            );
            assert!(
                strip.named() > strip.len() && strip.truncated > 0,
                "{reference} names more than the bound keeps, so the shortfall must be \
                 counted: len {} truncated {}",
                strip.len(),
                strip.truncated
            );
        }
        // And the read itself honours it, not only the strip's arithmetic.
        assert_eq!(ref_numbers(&wb, 0, "A:A").len(), MAX_SERIES_POINTS);
        assert_eq!(ref_cells(&wb, 0, "A:A").len(), MAX_SERIES_POINTS);
        assert_eq!(ref_text(&wb, 0, "A:A").len(), MAX_SERIES_POINTS);
    }

    /// A reference inside the bound keeps every point, and counts none lost.
    ///
    /// The control the test above needs: a cap that fired on every chart would
    /// satisfy it and ruin every real one.
    #[test]
    fn a_series_reference_inside_the_bound_is_untouched() {
        let wb = wide_workbook(4, 1, 99);
        let strip = crate::chart_data::ref_strip(&wb, 0, "$A$1:$B$4").expect("resolves");
        assert_eq!((strip.len(), strip.truncated, strip.named()), (8, 0, 8));
        assert_eq!(ref_numbers(&wb, 0, "$B$1:$B$4").len(), 4);
    }

    /// **Truncation is said, not swallowed** (`CHT-11`).
    ///
    /// A capped series is a series drawing a prefix of itself. Layout has no
    /// compatibility report to write that into, so the legend carries it — the
    /// same place and the same reason `CHT-08` puts `#REF!`. Drawing the first
    /// 65,536 of a million and looking finished is the `CHT-05` silent lie.
    #[test]
    fn a_truncated_series_says_so_in_the_legend() {
        let wb = wide_workbook(4, 1, 99);
        let mut chart = wide_chart(4, 1);
        chart.series[0].values = "$B$1:$B$1048576".to_owned();
        chart.legend = Some("r".to_owned());
        chart.series[0].name = "Sales".to_owned();

        let (_, resolved) = resolve(&wb, 0, &chart);
        assert_eq!(resolved.len(), 1);
        assert!(
            resolved[0].truncated > 0,
            "the series lost points and did not count them"
        );

        let mut list = crate::DisplayList::new();
        crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());
        let found = labels(&list);
        assert!(
            found.iter().any(|l| l == "Sales (truncated)"),
            "the legend does not say the series is a prefix of itself: {found:?}"
        );
    }

    /// **The band read answers exactly what the point read answered**
    /// (`CHT-10`).
    ///
    /// The residual after `CHT-06` was cell *reading*: `ref_numbers` did one
    /// `BTreeMap` descent per cell, sixty thousand of them a frame for a
    /// 10,000-row six-series chart. It does one ordered `row_band` traversal
    /// per reference now, which is a different algorithm over the same data —
    /// so the thing that can be silently wrong is not the speed but the
    /// **answer**, and an index off by a column or a row would show up as a
    /// chart plotting its neighbour's numbers.
    ///
    /// The twin below is written against `CellStore::get` and shares no code
    /// with the thing it checks. The data is deliberately awkward: sparse,
    /// ragged, multi-column, off-origin, on two sheets, with text and booleans
    /// among the numbers.
    #[test]
    fn reading_a_strip_by_band_agrees_with_reading_it_cell_by_cell() {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let label = wb.intern_string("label");
        for (name, seed) in [("S", 3u32), ("T", 7)] {
            let mut sh = Sheet::new(SheetId(Id::from_parts(u64::from(seed), 1)), name);
            for r in 0..40u32 {
                for c in 0..7u32 {
                    // Holes, on purpose: a band traversal only visits populated
                    // cells, so a dense sheet would hide an index error that a
                    // sparse one exposes.
                    if (r * 7 + c + seed) % 3 == 0 {
                        continue;
                    }
                    let cell = match (r + c) % 5 {
                        0 => Cell::value(CellValue::SharedString(label)),
                        1 => Cell::value(CellValue::Bool(r % 2 == 0)),
                        _ => Cell::value(CellValue::Number(f64::from(r * 100 + c))),
                    };
                    sh.cells.set(CellRef::new(r, c), cell);
                }
            }
            wb.sheets.push(sh);
        }

        /// The obvious implementation: one point lookup per cell of the strip.
        fn by_cell(wb: &Workbook, sheet: usize, reference: &str) -> Vec<Option<f64>> {
            let strip = crate::chart_data::ref_strip(wb, sheet, reference).expect("resolves");
            let mut out = Vec::new();
            for r in strip.start.row..=strip.end.row {
                for c in strip.start.col..=strip.end.col {
                    out.push(
                        wb.sheets[strip.sheet]
                            .cells
                            .get(CellRef::new(r, c))
                            .and_then(|cell| match cell.value {
                                CellValue::Number(n) => Some(n),
                                CellValue::Bool(b) => Some(if b { 1.0 } else { 0.0 }),
                                _ => None,
                            }),
                    );
                }
            }
            out
        }

        for reference in [
            "$A$1:$A$40",
            "$C$5:$C$31",
            "$B$3:$E$29",
            "$A$1:$G$40",
            "$D$12",
            "T!$B$2:$D$18",
            "$G$40:$G$40",
        ] {
            for sheet in [0usize, 1] {
                assert_eq!(
                    ref_numbers(&wb, sheet, reference),
                    by_cell(&wb, sheet, reference),
                    "{reference} from sheet {sheet} reads differently by band than by cell"
                );
            }
        }
        // Text goes down the same traversal and must agree too.
        assert_eq!(
            ref_text(&wb, 0, "$B$3:$E$29").len(),
            by_cell(&wb, 0, "$B$3:$E$29").len()
        );
        assert!(
            ref_text(&wb, 0, "$A$1:$G$40").iter().any(|t| t == "label"),
            "the text read down the same traversal lost its strings"
        );
    }

    // --- Scatter, pie and the line's bytes (CHT-11) ---------------------

    /// A chart of `series` columns over `rows` rows, of a given kind.
    fn wide_chart_of(rows: u32, series: u32, kind: ChartKind) -> ChartView {
        let mut ch = wide_chart(rows, series);
        ch.kind = kind;
        ch
    }

    fn wedges(list: &crate::DisplayList) -> Vec<(f64, f64, String)> {
        list.items
            .iter()
            .filter_map(|i| match i {
                PaintItem::Wedge {
                    from, sweep, fill, ..
                } => Some((*from, *sweep, fill.clone())),
                _ => None,
            })
            .collect()
    }

    /// The polylines that are *series* rather than chrome.
    ///
    /// The frame outline and the two axes are polylines too, and the outline is
    /// five points — long enough to be mistaken for a short line run, which it
    /// was until this filtered on the stroke instead. A series is drawn at the
    /// canvas's 1.8 px `lineWidth` and every piece of chrome at one pixel.
    fn line_runs(list: &crate::DisplayList) -> Vec<Vec<Point>> {
        list.items
            .iter()
            .filter_map(|i| match i {
                PaintItem::Polyline {
                    points,
                    width,
                    color: _,
                } if *width > crate::chart::PX as i64 => Some(points.clone()),
                _ => None,
            })
            .collect()
    }

    fn series_points(list: &crate::DisplayList) -> usize {
        line_runs(list).iter().map(Vec::len).sum()
    }

    /// **A line plot was bounded in items and not in bytes** (`CHT-11`).
    ///
    /// One polyline per series is 12 display-list items for six series over
    /// 10,000 rows — and **1,181,094 bytes of JSON across the wasm boundary,
    /// every frame**, which is larger than the 722 KB the *uncapped* bar plot
    /// was fixed for under `CHT-06`. An item count is not a size, and the item
    /// count is what the existing gate looks at.
    #[test]
    fn a_line_plot_is_bounded_in_points_and_not_only_in_items() {
        for rows in [1_000u32, 10_000] {
            let wb = wide_workbook(rows, 6, 7);
            let chart = wide_chart_of(rows, 6, ChartKind::Line);
            let mut list = crate::DisplayList::new();
            crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());

            let points = series_points(&list);
            // Two per pixel column per series is the geometric bound; the plot
            // is under 400 px wide once the axis gutter is taken off.
            assert!(
                points <= 2 * 400 * 6,
                "{rows} rows x 6 series carried {points} polyline points; \
                 uncapped that is {}",
                rows * 6
            );
            assert!(
                points <= MAX_LINE_POINTS,
                "{points} points is past the plot's ceiling of {MAX_LINE_POINTS}"
            );
            assert!(points > 0, "the plot drew no line at all, which is a blank");
        }
    }

    /// **And the bound is honest**: the one row in ten thousand holding a spike
    /// still reaches the top of the plot.
    ///
    /// A cap that kept every nth point, or the first point of each column,
    /// would drop it — and the axis would still scale to the spike, so the
    /// picture would show a flat line under an axis that says a million.
    #[test]
    fn a_line_keeps_the_outlier_it_would_be_a_lie_to_drop() {
        let rows = 10_000u32;
        let wb = wide_workbook(rows, 6, 6_667);
        let chart = wide_chart_of(rows, 6, ChartKind::Line);
        let mut list = crate::DisplayList::new();
        crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());

        let top = line_runs(&list)
            .iter()
            .flat_map(|p| p.iter().map(|q| q.y))
            .min()
            .expect("a line");
        let floor = line_runs(&list)
            .iter()
            .flat_map(|p| p.iter().map(|q| q.y))
            .max()
            .expect("a line");
        let plot_h = 300.0 * PX;
        assert!(
            (floor - top) as f64 > plot_h * 0.5,
            "the line spans {} twips of a {plot_h}-twip frame — the spike was dropped",
            floor - top
        );
    }

    /// A gap still breaks the line rather than being bridged across.
    ///
    /// The column bucketing accumulates points and flushes them, and a gap has
    /// to close the open column *before* it closes the run — flush them the
    /// other way round and the points either side of the gap join up, which is
    /// a line drawn through data that is not there.
    #[test]
    fn a_gap_still_breaks_the_line() {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut s = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        for r in 0..9u32 {
            if r == 4 {
                continue;
            }
            s.cells.set(
                CellRef::new(r, 1),
                Cell::value(CellValue::Number(f64::from(r) + 1.0)),
            );
        }
        wb.sheets.push(s);
        let mut chart = wide_chart_of(9, 1, ChartKind::Line);
        chart.series[0].values = "$B$1:$B$9".to_owned();

        let mut list = crate::DisplayList::new();
        crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());
        let runs: Vec<usize> = line_runs(&list).iter().map(Vec::len).collect();
        assert_eq!(
            runs,
            vec![4, 4],
            "the gap at row 5 must leave two runs of four points, not one bridge"
        );
    }

    /// Below the bound a line is the line it always was, point for point.
    ///
    /// **Twelve points, not five.** A column holding two points emits both, in
    /// index order, which is the same two points the unbucketed plot drew — so
    /// a five-point series survives a *deliberately broken* pitch unchanged and
    /// proves nothing. Three in a column is the first case where min-and-max is
    /// fewer than all, so twelve points is what makes this control discriminate.
    #[test]
    fn a_line_inside_the_bound_is_untouched() {
        let wb = wide_workbook(12, 1, 99);
        let chart = wide_chart_of(12, 1, ChartKind::Line);
        let mut list = crate::DisplayList::new();
        crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());
        let runs = line_runs(&list);
        assert_eq!(
            runs.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![12],
            "twelve points must stay twelve points"
        );
        // And in place: evenly spaced along the plot, which is what a merged
        // column would break even where the count happened to survive.
        let xs: Vec<i64> = runs[0].iter().map(|p| p.x).collect();
        let gaps: Vec<i64> = xs.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(
            gaps.windows(2).all(|w| (w[0] - w[1]).abs() <= 1),
            "the points are no longer evenly spaced: {gaps:?}"
        );
    }

    /// **Scatter emitted one polygon per point, uncapped** (`CHT-11`).
    ///
    /// 10,000 rows x 6 series was 60,006 display-list items and **7,181,064
    /// bytes of JSON per frame**, ten times the uncapped bar plot `CHT-06` was
    /// opened for. `CHT-06`'s min-max bucket cannot be reused: a bar's ink is a
    /// rectangle from zero to its value and a marker's is four pixels around
    /// its own position, so the bound is on positions instead.
    #[test]
    fn a_scatter_plot_is_bounded_by_the_positions_a_plot_can_distinguish() {
        for rows in [1_000u32, 10_000] {
            let wb = wide_workbook(rows, 6, 7);
            let chart = wide_chart_of(rows, 6, ChartKind::Scatter);
            let mut list = crate::DisplayList::new();
            crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());
            let markers = polygons(&list).len() - 1; // less the frame's ground
            assert!(
                markers <= MAX_SCATTER_MARKERS,
                "{rows} rows x 6 series emitted {markers} markers against a \
                 {MAX_SCATTER_MARKERS} ceiling; uncapped that is {}",
                rows * 6
            );
            assert!(markers > 0, "the plot drew nothing at all");
        }
    }

    /// **And the ceiling holds for a frame that is not near square.**
    ///
    /// Found by asking what would have to be true for the bound to be wrong.
    /// The marker pitch is derived from the plot's *area*, and area bounds the
    /// cell count only for a plot near square: a frame 4,000 pixels wide and 40
    /// tall has the same area as 400x400 and forty times as many marker-wide
    /// columns. So the stated ceiling was not one, and a chart anchored across
    /// a very wide, very short band of cells is an ordinary thing to draw.
    #[test]
    fn a_scatter_plot_on_a_long_thin_frame_still_honours_the_ceiling() {
        let rows = 10_000u32;
        // No spike: an outlier pins the axis to itself and squashes every other
        // point into a single row of cells, which is fewer cells and so a
        // weaker test. The sawtooth alone fills the plot's height.
        let wb = wide_workbook(rows, 6, rows);
        let chart = wide_chart_of(rows, 6, ChartKind::Scatter);
        let mut list = crate::DisplayList::new();
        crate::chart::push_chart(
            &mut list,
            &wb,
            0,
            &chart,
            crate::Rect {
                x: 0,
                y: 0,
                w: (60_000.0 * PX) as i64,
                h: (200.0 * PX) as i64,
            },
        );
        let markers = polygons(&list).len() - 1;
        assert!(
            markers <= MAX_SCATTER_MARKERS,
            "a 60000x200 frame emitted {markers} markers against a \
             {MAX_SCATTER_MARKERS} ceiling"
        );
        assert!(markers > 0, "the plot drew nothing at all");
    }

    /// **And nothing vanishes from a place that would otherwise be blank.**
    ///
    /// The grid *merges* points that were already drawing the same square; it
    /// does not drop points. A lone outlier is alone in its cell, so it keeps
    /// its own marker — and the extremes of the cloud are exactly where they
    /// were, because the merge radius is the marker's own size.
    #[test]
    fn a_scatter_plot_keeps_a_lone_outlier() {
        let rows = 10_000u32;
        let wb = wide_workbook(rows, 1, 6_667);
        let chart = wide_chart_of(rows, 1, ChartKind::Scatter);
        let mut list = crate::DisplayList::new();
        crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());

        let top = polygons(&list)
            .iter()
            .skip(1)
            .flat_map(|(pts, _)| pts.iter().map(|p| p.y))
            .min()
            .expect("markers");
        let floor = polygons(&list)
            .iter()
            .skip(1)
            .flat_map(|(pts, _)| pts.iter().map(|p| p.y))
            .max()
            .expect("markers");
        let plot_h = 300.0 * PX;
        assert!(
            (floor - top) as f64 > plot_h * 0.5,
            "the cloud spans {} twips of a {plot_h}-twip frame — the spike was merged away",
            floor - top
        );
    }

    /// Below the bound every point still gets its own marker.
    ///
    /// **Twenty points, not four.** Four points spread across a 400-pixel plot
    /// land in four separate cells of *any* grid coarse enough to be wrong, so
    /// they survive a deliberately broken pitch and prove nothing. Twenty are
    /// 21 pixels apart, which a marker-fine grid separates and a coarse one
    /// does not.
    #[test]
    fn a_scatter_plot_inside_the_bound_is_untouched() {
        let wb = wide_workbook(20, 2, 99);
        let chart = wide_chart_of(20, 2, ChartKind::Scatter);
        let mut list = crate::DisplayList::new();
        crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());
        assert_eq!(
            polygons(&list).len(),
            1 + 20 * 2,
            "a point inside the bound lost its own marker"
        );
    }

    /// **A pie emitted one wedge per value, uncapped** (`CHT-11`).
    ///
    /// 10,000 values was 10,002 items and 1.4 MB of JSON for a picture with no
    /// visible divisions in it: at a 300-pixel frame each slice had a tenth of
    /// a pixel of arc. Adjacent wedges merge, which is sound where the marker
    /// grid is not — a wedge's angle is contiguous and ordered, so a merged
    /// wedge occupies exactly the angle its members occupied.
    #[test]
    fn a_pie_is_bounded_by_the_arc_a_rim_can_show() {
        for rows in [500u32, 10_000] {
            let wb = wide_workbook(rows, 1, 7);
            let chart = wide_chart_of(rows, 1, ChartKind::Pie);
            let mut list = crate::DisplayList::new();
            crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());
            let drawn = wedges(&list);
            assert!(
                drawn.len() <= MAX_PIE_WEDGES,
                "{rows} values drew {} wedges against a {MAX_PIE_WEDGES} ceiling",
                drawn.len()
            );
            // The geometric bound: the rim is under 1,000 px around, and no
            // wedge below two pixels of it is worth an item.
            assert!(
                drawn.len() <= 500,
                "{rows} values drew {} wedges, more than the rim can show",
                drawn.len()
            );
            assert!(!drawn.is_empty(), "the pie drew nothing");

            // **Merging must not lose a value.** The wedges tile the circle:
            // each starts where the last ended and together they close at 360.
            let mut expected = 0.0f64;
            for (from, sweep, _) in &drawn {
                assert!(
                    (from - expected).abs() < 1e-6,
                    "a wedge starts at {from} where the one before ended at {expected}: \
                     the merge left a gap or an overlap"
                );
                expected += sweep;
            }
            assert!(
                (expected - 360.0).abs() < 1e-6,
                "the wedges cover {expected} degrees, not 360 — a value was dropped"
            );
        }
    }

    /// **Merging groups *adjacent* values, and loses none of them.**
    ///
    /// The strongest available statement of that without reaching inside: give
    /// every value the same size, and every merged wedge must then cover the
    /// same angle, because each holds the same number of them. A merge that
    /// dropped its group and let the final wedge absorb the remainder still
    /// closes at 360 — the tail correction sees to that — so the tiling
    /// assertion above does **not** catch it and this does.
    #[test]
    fn merging_a_pie_groups_neighbours_rather_than_discarding_them() {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut sh = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        for r in 0..10_000u32 {
            sh.cells
                .set(CellRef::new(r, 1), Cell::value(CellValue::Number(1.0)));
        }
        wb.sheets.push(sh);
        let chart = wide_chart_of(10_000, 1, ChartKind::Pie);

        let mut list = crate::DisplayList::new();
        crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());
        let sweeps: Vec<f64> = wedges(&list).into_iter().map(|(_, s, _)| s).collect();
        assert!(
            sweeps.len() > 100,
            "the pie merged into {} wedges",
            sweeps.len()
        );
        let widest = sweeps.iter().cloned().fold(f64::MIN, f64::max);
        let narrowest = sweeps.iter().cloned().fold(f64::MAX, f64::min);
        assert!(
            widest < narrowest * 2.0,
            "ten thousand equal values gave wedges from {narrowest} to {widest} degrees —              the merge is not grouping neighbours, it is discarding them and letting \
             the last wedge cover what is left"
        );
    }

    /// Below the bound a pie is the pie it always was: one wedge per value, in
    /// order, each in its own series colour.
    #[test]
    fn a_pie_inside_the_bound_is_untouched() {
        // Ten values, not three: three are 60 degrees apart or more, so they
        // survive a threshold set wrong by two orders of magnitude and prove
        // nothing. The smallest of ten is 6.5 degrees, which is comfortably
        // above the real 0.8-degree threshold and below a broken one.
        let wb = wide_workbook(10, 1, 99);
        let chart = wide_chart_of(10, 1, ChartKind::Pie);
        let mut list = crate::DisplayList::new();
        crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());
        let drawn = wedges(&list);
        assert_eq!(drawn.len(), 10, "ten values must draw ten wedges");
        let palette = series_colors(&wb, 10);
        let fills: Vec<String> = drawn.into_iter().map(|(_, _, f)| f).collect();
        assert_eq!(fills, palette, "the wedges lost their own colours");
    }

    // --- CHT-05: the picture stops being a plausible lie --------------------
    //
    // The measured defect: a stacked column, a 100%-stacked bar, a combination
    // chart and a secondary-axis chart each produced a display list
    // **identical to a clustered control** — same items, same fills, same
    // geometry — and the compatibility report named a chart zero times. A user
    // was shown a picture that was not their chart, and told nothing.
    //
    // Item counts are not enough to pin this. Stacking is documented to change
    // the arithmetic and not the item count, so these assert **geometry**: how
    // wide a bar is, where its top and bottom sit, and what the axis says.

    /// A three-category workbook: `B` is 100/120/140, `C` is 60/70/80, `D` is a
    /// margin in the range 0.4..0.43 — three orders of magnitude below `B`,
    /// which is the disparity a secondary axis exists for.
    fn stacking_workbook() -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut s = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        for (row, (b, c, d)) in [
            (100.0, 60.0, 0.40),
            (120.0, 70.0, 0.42),
            (140.0, 80.0, 0.43),
        ]
        .into_iter()
        .enumerate()
        {
            let r = u32::try_from(row).expect("fits");
            s.cells
                .set(CellRef::new(r, 1), Cell::value(CellValue::Number(b)));
            s.cells
                .set(CellRef::new(r, 2), Cell::value(CellValue::Number(c)));
            s.cells
                .set(CellRef::new(r, 3), Cell::value(CellValue::Number(d)));
        }
        wb.sheets.push(s);
        wb
    }

    /// A column chart over `B` and `C`, in a 400x300 frame.
    fn two_series_chart() -> ChartView {
        let mut ch = ChartView::new(
            CellRange::new(CellRef::new(0, 0), CellRef::new(9, 5)),
            ChartKind::Column,
        );
        ch.series = ["$B$1:$B$3", "$C$1:$C$3"]
            .iter()
            .map(|v| ChartSeries {
                values: (*v).to_owned(),
                ..ChartSeries::default()
            })
            .collect();
        ch
    }

    fn drawn(chart: &ChartView) -> crate::DisplayList {
        let mut list = crate::DisplayList::new();
        crate::chart::push_chart(&mut list, &stacking_workbook(), 0, chart, frame_400x300());
        list
    }

    /// Every bar rectangle as `(x0, x1, y0, y1, fill)`, in emission order.
    fn bars(list: &crate::DisplayList) -> Vec<(i64, i64, i64, i64, String)> {
        polygons(list)
            .into_iter()
            // The frame's own ground is a polygon too, and it is the only
            // white one.
            .filter(|(_, fill)| fill != "FFFFFF")
            .map(|(points, fill)| {
                let xs: Vec<i64> = points.iter().map(|p| p.x).collect();
                let ys: Vec<i64> = points.iter().map(|p| p.y).collect();
                (
                    *xs.iter().min().expect("a rectangle"),
                    *xs.iter().max().expect("a rectangle"),
                    *ys.iter().min().expect("a rectangle"),
                    *ys.iter().max().expect("a rectangle"),
                    fill,
                )
            })
            .collect()
    }

    fn texts(list: &crate::DisplayList) -> Vec<String> {
        list.items
            .iter()
            .filter_map(|i| match i {
                PaintItem::Text { content, .. } => Some(content.clone()),
                _ => None,
            })
            .collect()
    }

    /// **The finding, refuted.** A stacked chart used to draw exactly the
    /// clustered control: two half-width bars side by side, both measured from
    /// the axis, on an extent of 140. Stacking is one full-width bar per
    /// category, the second band sitting on the first, on an extent of 220.
    #[test]
    fn a_stacked_chart_no_longer_draws_the_clustered_picture() {
        let control = two_series_chart();
        let mut stacked = two_series_chart();
        stacked.grouping = Some(casual_calc_model::ChartGrouping::Stacked);

        let control_bars = bars(&drawn(&control));
        let stacked_bars = bars(&drawn(&stacked));
        assert_eq!(
            control_bars.len(),
            stacked_bars.len(),
            "stacking changes the arithmetic, not the item count"
        );
        assert_ne!(
            control_bars, stacked_bars,
            "the stacked chart is still drawn as the clustered one"
        );

        // The axis covers the sum, not the tallest single value.
        assert!(
            texts(&drawn(&control)).contains(&"140".to_owned()),
            "{:?}",
            texts(&drawn(&control))
        );
        assert!(
            texts(&drawn(&stacked)).contains(&"220".to_owned()),
            "{:?}",
            texts(&drawn(&stacked))
        );

        // One lane, not two: a stacked bar is as wide as its whole group.
        let width = |b: &(i64, i64, i64, i64, String)| b.1 - b.0;
        assert!(
            width(&stacked_bars[0]) > width(&control_bars[0]) * 3 / 2,
            "stacked {} vs clustered {}",
            width(&stacked_bars[0]),
            width(&control_bars[0])
        );
        // And the two series share one x position rather than sitting apart.
        assert_eq!(stacked_bars[0].0, stacked_bars[1].0);
        assert_ne!(control_bars[0].0, control_bars[1].0);
        // The second band starts exactly where the first ends — that abutment
        // *is* stacking, and a chart drawing both from the axis has none of it.
        assert_eq!(
            stacked_bars[1].3, stacked_bars[0].2,
            "the second band sits on top of the first: {stacked_bars:?}"
        );
    }

    /// 100%-stacking normalises every category to the whole, so every column
    /// reaches the top of the plot whatever its total — which is the one thing
    /// that distinguishes it from plain stacking on the screen.
    #[test]
    fn a_percent_stacked_chart_fills_every_column() {
        let mut chart = two_series_chart();
        chart.grouping = Some(casual_calc_model::ChartGrouping::PercentStacked);
        let list = drawn(&chart);
        assert!(
            texts(&list).contains(&"100".to_owned()),
            "{:?}",
            texts(&list)
        );

        let drawn_bars = bars(&list);
        // Six bars: three categories, two bands each. The top of each category's
        // upper band is the same, because each category is a whole.
        assert_eq!(drawn_bars.len(), 6, "{drawn_bars:?}");
        let tops: Vec<i64> = drawn_bars.iter().skip(1).step_by(2).map(|b| b.2).collect();
        assert!(
            tops.windows(2).all(|w| w[0] == w[1]),
            "every column should reach the top: {tops:?}"
        );
        // The categories have different totals (160, 190, 220), so a *stacked*
        // chart of the same data does not do this — which is what makes the
        // assertion above discriminating rather than vacuous.
        let mut stacked = two_series_chart();
        stacked.grouping = Some(casual_calc_model::ChartGrouping::Stacked);
        let stacked_tops: Vec<i64> = bars(&drawn(&stacked))
            .iter()
            .skip(1)
            .step_by(2)
            .map(|b| b.2)
            .collect();
        assert!(
            stacked_tops.windows(2).any(|w| w[0] != w[1]),
            "{stacked_tops:?}"
        );
    }

    /// A combination chart used to draw its line series as a third column. The
    /// line is a polyline now, and there is one fewer bar for it.
    #[test]
    fn a_combination_chart_draws_its_line_as_a_line() {
        let control = two_series_chart();
        let mut combo = two_series_chart();
        combo.series[1].kind = Some(ChartKind::Line);

        let control_list = drawn(&control);
        let combo_list = drawn(&combo);
        assert_eq!(bars(&control_list).len(), 6);
        assert_eq!(
            bars(&combo_list).len(),
            3,
            "only the first series is still bars"
        );
        let polylines = |l: &crate::DisplayList| {
            l.items
                .iter()
                .filter(|i| matches!(i, PaintItem::Polyline { .. }))
                .count()
        };
        assert_eq!(
            polylines(&combo_list),
            polylines(&control_list) + 1,
            "the line series draws a line"
        );
        // The second series keeps its own colour: a combination chart draws in
        // two passes, and a colour taken from a position within one of them is
        // not the colour the legend drew.
        let palette = series_colors(&stacking_workbook(), 2);
        assert!(bars(&combo_list).iter().all(|b| b.4 == palette[0]));
    }

    /// **The scale disparity, which is the whole of the switching blocker.** A
    /// margin of 0.4 beside revenue of 140 on one shared extent is a bar under
    /// a pixel tall — drawn, and invisible. Its own axis makes it a chart.
    #[test]
    fn a_secondary_axis_series_is_measured_against_its_own_extent() {
        let mut shared = two_series_chart();
        shared.series[1].values = "$D$1:$D$3".to_owned();
        let mut split = shared.clone();
        split.series[1].secondary_axis = true;

        let shared_bars = bars(&drawn(&shared));
        let split_bars = bars(&drawn(&split));
        let height = |b: &(i64, i64, i64, i64, String)| b.3 - b.2;

        // On one axis the margin bars collapse to the minimum mark.
        let margin_shared: Vec<i64> = shared_bars.iter().skip(1).step_by(2).map(height).collect();
        assert!(
            margin_shared.iter().all(|h| *h <= (PX as i64)),
            "expected invisible bars on a shared axis: {margin_shared:?}"
        );
        let margin_split: Vec<i64> = split_bars.iter().skip(1).step_by(2).map(height).collect();
        assert!(
            margin_split.iter().all(|h| *h > (10.0 * PX) as i64),
            "the secondary axis should give the margin series a real height: {margin_split:?}"
        );

        // And the second axis is drawn: its extent is the margin's, so the
        // plot now carries a `0.43` label it did not have.
        let labels = texts(&drawn(&split));
        assert!(labels.contains(&"0.43".to_owned()), "{labels:?}");
        assert!(!texts(&drawn(&shared)).contains(&"0.43".to_owned()));
    }

    /// Data labels are a `Text` per plotted point, and the value they carry is
    /// the file's own — not the stacked position, which would be a second wrong
    /// picture on top of the first.
    #[test]
    fn data_labels_show_the_value_the_file_holds() {
        let mut chart = two_series_chart();
        chart.series[0].data_labels = true;
        let before = texts(&drawn(&two_series_chart()));
        let after = texts(&drawn(&chart));
        assert_eq!(after.len(), before.len() + 3, "one label per point");
        for v in ["100", "120", "140"] {
            assert!(after.contains(&v.to_owned()), "{after:?}");
        }

        // Stacked, the label still reads the value and not the running total.
        let mut stacked = chart.clone();
        stacked.series[1].data_labels = true;
        stacked.grouping = Some(casual_calc_model::ChartGrouping::Stacked);
        let labels = texts(&drawn(&stacked));
        assert!(labels.contains(&"60".to_owned()), "{labels:?}");
        assert!(
            !labels.contains(&"160".to_owned()),
            "a label showed the stacked position: {labels:?}"
        );
    }

    /// **The cap is part of the feature.** A label is a `Text` item per point on
    /// a path that had to be capped in the first place, so past
    /// `MAX_LABEL_POINTS` the plot draws none rather than labelling a prefix and
    /// stopping — which would read as data that ends.
    #[test]
    fn a_series_past_the_label_cap_is_not_labelled() {
        let rows = u32::try_from(MAX_LABEL_POINTS + 1).expect("fits");
        let wb = wide_workbook(rows, 1, 7);
        let mut chart = wide_chart(rows, 1);
        chart.series[0].data_labels = true;
        let mut list = crate::DisplayList::new();
        crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());
        let labelled = texts(&list).len();

        let rows = u32::try_from(MAX_LABEL_POINTS).expect("fits");
        let wb = wide_workbook(rows, 1, 7);
        let mut chart = wide_chart(rows, 1);
        chart.series[0].data_labels = true;
        let mut list = crate::DisplayList::new();
        crate::chart::push_chart(&mut list, &wb, 0, &chart, frame_400x300());
        assert!(
            texts(&list).len() > labelled,
            "at the cap the labels are drawn, past it none are"
        );
    }

    /// A grouping on a kind that has no groups does nothing, stated as a
    /// decision rather than found as a surprise.
    #[test]
    fn a_grouping_on_a_pie_changes_nothing() {
        let mut pie = two_series_chart();
        pie.kind = ChartKind::Pie;
        let plain = drawn(&pie);
        pie.grouping = Some(casual_calc_model::ChartGrouping::Stacked);
        assert_eq!(drawn(&pie).items.len(), plain.items.len());
    }
}

/// Pagination: which rows and columns land on which sheet of paper (`IO-03`).
///
/// The HTML print path hands this question to the browser; a PDF has nobody to
/// hand it to, so these are the answers the writer prints. Every one of them is
/// a property of the *set* of pages — that it tiles the sheet without a gap,
/// that a break lands where the file asked, that a fit-to-page setting produces
/// the page count it names — because a single band asserted in isolation can be
/// right while the run it belongs to is wrong.
mod pagination {
    use casual_calc_formula::Expr;
    use casual_calc_model::{Cell, CellRef, CellValue, DefinedName, Id, Sheet, SheetId, Workbook};

    use crate::GridGeometry;
    use crate::print::{MAX_PAGES, paginate, scope};

    /// A sheet of `rows` x `cols` populated cells, at the default line sizes.
    fn filled(rows: u32, cols: u32) -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        for row in 0..rows {
            for col in 0..cols {
                sheet.cells.set(
                    CellRef::new(row, col),
                    Cell::value(CellValue::Number(f64::from(row * cols + col))),
                );
            }
        }
        wb.sheets.push(sheet);
        wb
    }

    fn name(wb: &mut Workbook, name: &str, refers_to: &str) {
        let sheet = wb.sheets[0].id;
        wb.defined_names.push(DefinedName {
            name: name.to_owned(),
            sheet: Some(sheet),
            formula: Expr::Raw(refers_to.to_owned()),
        });
    }

    #[test]
    fn an_empty_sheet_has_nothing_to_paginate() {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        wb.sheets
            .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "S"));
        assert!(scope(&wb, 0).is_none());
        assert!(paginate(&wb, 0, &GridGeometry::default()).is_none());
        // And a sheet index nobody has.
        assert!(paginate(&wb, 7, &GridGeometry::default()).is_none());
    }

    /// The property that matters most: every printed row appears on exactly one
    /// page, in order. A paginator that drops a row or repeats one produces a
    /// printout nobody can reconcile against the screen, and neither failure is
    /// visible in a single band.
    #[test]
    fn the_pages_tile_the_sheet_without_a_gap_or_an_overlap() {
        let wb = filled(300, 12);
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        let plan = paginate(&wb, 0, &geometry).expect("a sheet with cells paginates");
        assert!(plan.pages.len() > 1, "300 rows do not fit on one page");
        assert!(!plan.truncated);

        // Distinct row bands, in order, covering 0..=299 exactly once.
        let mut rows: Vec<(u32, u32)> = plan.pages.iter().map(|p| p.rows).collect();
        rows.dedup();
        rows.sort_unstable();
        rows.dedup();
        assert_eq!(rows.first().unwrap().0, 0);
        assert_eq!(rows.last().unwrap().1, 299);
        for pair in rows.windows(2) {
            assert_eq!(
                pair[1].0,
                pair[0].1 + 1,
                "row bands must abut: {pair:?} leaves a gap or overlaps"
            );
        }

        let mut cols: Vec<(u32, u32)> = plan.pages.iter().map(|p| p.cols).collect();
        cols.sort_unstable();
        cols.dedup();
        assert_eq!(cols.first().unwrap().0, 0);
        assert_eq!(cols.last().unwrap().1, 11);
        for pair in cols.windows(2) {
            assert_eq!(pair[1].0, pair[0].1 + 1, "column bands must abut");
        }
        assert_eq!(plan.pages.len(), rows.len() * cols.len());
    }

    /// No band may be wider or taller than the paper it is drawn on — that is
    /// the whole job. Checked against the printable box at the plan's own
    /// scale, so it holds for a scaled printout too.
    #[test]
    fn no_band_overflows_the_printable_box() {
        let wb = filled(200, 20);
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        let plan = paginate(&wb, 0, &geometry).unwrap();
        for page in &plan.pages {
            let (w, h) = crate::print::content_extent(&geometry, page.rows, page.cols);
            let single_row = page.rows.0 == page.rows.1;
            let single_col = page.cols.0 == page.cols.1;
            if !single_col {
                assert!(
                    (w as f64) * plan.scale <= plan.page_box.width as f64,
                    "{page:?} is {w} twips wide at scale {}",
                    plan.scale
                );
            }
            if !single_row {
                assert!(
                    (h as f64) * plan.scale <= plan.page_box.height as f64,
                    "{page:?}"
                );
            }
        }
    }

    /// `downThenOver` is the default and puts the whole first column band on
    /// paper before moving right; `overThenDown` reverses it. A printout
    /// numbered the other way is collated wrong, and nothing about the pages
    /// themselves says which is which.
    #[test]
    fn page_order_follows_the_files_own_setting() {
        let wb = filled(200, 20);
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        let down = paginate(&wb, 0, &geometry).unwrap();
        assert_eq!((down.pages[0].across, down.pages[0].down), (0, 0));
        assert_eq!((down.pages[1].across, down.pages[1].down), (0, 1));

        let mut wb = wb;
        wb.sheets[0]
            .print
            .page
            .insert("pageOrder".to_owned(), "overThenDown".to_owned());
        let over = paginate(&wb, 0, &geometry).unwrap();
        assert_eq!((over.pages[0].across, over.pages[0].down), (0, 0));
        assert_eq!((over.pages[1].across, over.pages[1].down), (1, 0));
        assert_eq!(over.pages.len(), down.pages.len());
    }

    /// A manual break is the one input that is not arithmetic. It must win over
    /// "there was still room", or a user who put a break between two sections
    /// gets them on one page anyway.
    #[test]
    fn a_manual_row_break_starts_a_page_even_where_there_was_room() {
        let mut wb = filled(10, 2);
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        assert_eq!(
            paginate(&wb, 0, &geometry).unwrap().pages.len(),
            1,
            "ten rows fit on one page before the break"
        );

        wb.sheets[0].print.row_breaks.push(
            [
                ("id".to_owned(), "4".to_owned()),
                ("man".to_owned(), "1".to_owned()),
            ]
            .into_iter()
            .collect(),
        );
        let plan = paginate(&wb, 0, &geometry).unwrap();
        assert_eq!(plan.pages.len(), 2);
        // `brk@id` is the zero-based index of the first row on the next page.
        assert_eq!(plan.pages[0].rows, (0, 3));
        assert_eq!(plan.pages[1].rows, (4, 9));
    }

    #[test]
    fn a_manual_column_break_does_the_same_across() {
        let mut wb = filled(4, 6);
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        assert_eq!(paginate(&wb, 0, &geometry).unwrap().pages.len(), 1);

        wb.sheets[0]
            .print
            .col_breaks
            .push([("id".to_owned(), "2".to_owned())].into_iter().collect());
        let plan = paginate(&wb, 0, &geometry).unwrap();
        assert_eq!(plan.pages.len(), 2);
        assert_eq!(plan.pages[0].cols, (0, 1));
        assert_eq!(plan.pages[1].cols, (2, 5));
    }

    #[test]
    fn print_area_narrows_what_paginates() {
        let mut wb = filled(100, 10);
        name(&mut wb, "Print_Area", "'S'!$B$2:$D$5");
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        let plan = paginate(&wb, 0, &geometry).unwrap();
        assert_eq!(plan.scope.rows, (1, 4));
        assert_eq!(plan.scope.cols, (1, 3));
        assert_eq!(plan.pages.len(), 1);
        assert_eq!(plan.pages[0].rows, (1, 4));
        assert_eq!(plan.scope.extra_areas, 0);
    }

    /// A print area of several rectangles prints several groups of pages in
    /// Excel and one here. The count is what stops that being invisible.
    #[test]
    fn a_multi_rectangle_print_area_is_counted_not_swallowed() {
        let mut wb = filled(20, 20);
        name(&mut wb, "Print_Area", "'S'!$A$1:$C$3,'S'!$F$1:$H$3");
        let scope = scope(&wb, 0).unwrap();
        assert_eq!(scope.cols, (0, 2));
        assert_eq!(scope.extra_areas, 1);
    }

    /// Repeated header rows come off the body and are reserved on every page,
    /// so page one does not show them twice and page two shows them at all.
    #[test]
    fn print_title_rows_leave_the_body_and_are_reserved_on_every_page() {
        let mut wb = filled(200, 4);
        name(&mut wb, "Print_Titles", "'S'!$1:$2");
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        let plan = paginate(&wb, 0, &geometry).unwrap();
        assert_eq!(plan.scope.title_rows, Some((0, 1)));
        assert_eq!(plan.title_height, 2 * crate::DEFAULT_ROW_HEIGHT);
        assert_eq!(
            plan.pages[0].rows.0, 2,
            "the header is drawn as the repeated band, not as body"
        );
        // Every page's body is that much shorter, because the header is on it.
        let without = {
            let mut plain = wb.clone();
            plain.defined_names.clear();
            paginate(&plain, 0, &geometry).unwrap()
        };
        let rows_per_page =
            |p: &crate::print::Pagination| p.pages[0].rows.1 - p.pages[0].rows.0 + 1;
        assert_eq!(rows_per_page(&plan) + 2, rows_per_page(&without));
    }

    #[test]
    fn print_title_columns_are_read_from_the_same_name() {
        let mut wb = filled(20, 40);
        name(&mut wb, "Print_Titles", "'S'!$A:$B,'S'!$1:$1");
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        let plan = paginate(&wb, 0, &geometry).unwrap();
        assert_eq!(plan.scope.title_cols, Some((0, 1)));
        assert_eq!(plan.scope.title_rows, Some((0, 0)));
        assert_eq!(plan.title_width, 2 * crate::DEFAULT_COL_WIDTH);
        assert_eq!(plan.pages[0].cols.0, 2);
        assert_eq!(plan.pages[0].rows.0, 1);
    }

    /// The reason the scale is computed before the axis is cut: "fit to one
    /// page wide" has to *be* one page wide, not one page wide if it happens to
    /// work out.
    #[test]
    fn fit_to_one_page_wide_produces_exactly_one_column_band() {
        let mut wb = filled(60, 30);
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        assert!(
            paginate(&wb, 0, &geometry)
                .unwrap()
                .pages
                .iter()
                .any(|p| p.across > 0),
            "thirty columns need more than one page unscaled"
        );

        let print = &mut wb.sheets[0].print;
        print
            .setup_pr
            .insert("fitToPage".to_owned(), "1".to_owned());
        print.page.insert("fitToWidth".to_owned(), "1".to_owned());
        print.page.insert("fitToHeight".to_owned(), "0".to_owned());
        let plan = paginate(&wb, 0, &geometry).unwrap();
        assert!(plan.scale < 1.0, "it had to shrink to fit");
        assert!(
            plan.pages.iter().all(|p| p.across == 0),
            "fit-to-one-page-wide left {} column bands",
            plan.pages.iter().map(|p| p.across).max().unwrap() + 1
        );
        assert!(plan.pages.iter().all(|p| p.cols == (0, 29)));
    }

    /// A row taller than the paper cannot be split, so it gets a page of its
    /// own — and, crucially, the loop still terminates.
    #[test]
    fn a_line_larger_than_the_page_gets_a_page_to_itself() {
        let mut wb = filled(3, 1);
        for row in 0..3 {
            wb.sheets[0].rows.sizes.insert(row, 40_000);
        }
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        let plan = paginate(&wb, 0, &geometry).unwrap();
        assert_eq!(plan.pages.len(), 3);
        assert_eq!(plan.pages[0].rows, (0, 0));
        assert_eq!(plan.pages[2].rows, (2, 2));
    }

    /// The bound exists because a hostile file can make the printable box one
    /// twip tall, and every page costs a display list downstream.
    #[test]
    fn a_page_cap_stops_a_hostile_page_setup_rather_than_the_machine() {
        let mut wb = filled(MAX_PAGES as u32 + 50, 1);
        // Margins wider than the paper: `PageBox` floors the box at one twip
        // rather than refusing, so every row becomes its own page.
        for edge in ["top", "bottom", "left", "right"] {
            wb.sheets[0]
                .print
                .margins
                .insert(edge.to_owned(), "40".to_owned());
        }
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        let plan = paginate(&wb, 0, &geometry).unwrap();
        assert!(plan.truncated, "the cap must say it was reached");
        assert!(plan.pages.len() <= MAX_PAGES, "{}", plan.pages.len());
    }

    #[test]
    fn gridlines_are_off_unless_the_file_asks_for_them() {
        let mut wb = filled(2, 2);
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        assert!(!paginate(&wb, 0, &geometry).unwrap().gridlines);
        wb.sheets[0]
            .print
            .options
            .insert("gridLines".to_owned(), "1".to_owned());
        assert!(paginate(&wb, 0, &geometry).unwrap().gridlines);
    }

    #[test]
    fn orientation_and_paper_reach_the_plan() {
        let mut wb = filled(2, 2);
        let print = &mut wb.sheets[0].print;
        print.page.insert("paperSize".to_owned(), "9".to_owned());
        print
            .page
            .insert("orientation".to_owned(), "landscape".to_owned());
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        let plan = paginate(&wb, 0, &geometry).unwrap();
        assert_eq!(plan.paper.css, "A4");
        assert!(plan.landscape);
        // The box is cut from the turned paper.
        assert!(plan.page_box.width > plan.page_box.height);
    }
}

/// Print geometry: the paper table, the printable box, and the scale the three
/// page-setup controls work out to.
///
/// This arithmetic is the half of printing CSS cannot express — there is no
/// fit-to-page primitive — so it is the half that has to be right here.
mod print_geometry {
    use crate::print::{PageBox, Scaling, TWIPS_PER_INCH, effective_scale, paper};

    #[test]
    fn the_named_papers_have_their_real_extents() {
        assert_eq!(paper("1").css, "letter");
        assert_eq!(paper("1").width, 8 * TWIPS_PER_INCH + TWIPS_PER_INCH / 2);
        assert_eq!(paper("9").css, "A4");
        // 210 mm x 297 mm to the nearest twip.
        assert_eq!((paper("9").width, paper("9").height), (11906, 16838));
        // An unnamed stock size keeps an extent to compute a scale from while
        // letting the printer choose the sheet.
        assert_eq!(paper("42").css, "auto");
        assert_eq!(paper("42").width, paper("1").width);
    }

    #[test]
    fn landscape_swaps_the_axes_and_margins_come_off_both() {
        let portrait = PageBox::new(paper("1"), false, [0.75, 0.7, 0.75, 0.7]);
        let landscape = PageBox::new(paper("1"), true, [0.75, 0.7, 0.75, 0.7]);
        assert_eq!(portrait.width, 12240 - 2 * 1008);
        assert_eq!(landscape.width, 15840 - 2 * 1008);
        assert_eq!(landscape.height, 12240 - 2 * 1080);
        // Margins wider than the paper must not produce a zero or negative
        // printable area, or every scale computed from it is nonsense.
        let absurd = PageBox::new(paper("1"), false, [20.0, 20.0, 20.0, 20.0]);
        assert_eq!((absurd.width, absurd.height), (1, 1));
    }

    #[test]
    fn fit_to_page_shrinks_but_never_enlarges() {
        let page = PageBox::new(paper("1"), false, [0.75, 0.7, 0.75, 0.7]);
        let fit = Scaling::Fit {
            wide: Some(1),
            tall: None,
        };
        // Twice the printable width fits at half scale.
        assert!((effective_scale(fit, (page.width * 2, 100), page) - 0.5).abs() < 1e-9);
        // A sheet narrower than the page is left alone.
        assert!((effective_scale(fit, (page.width / 4, 100), page) - 1.0).abs() < 1e-9);
        // Both axes constrained: the tighter one wins.
        let both = Scaling::Fit {
            wide: Some(1),
            tall: Some(1),
        };
        let scale = effective_scale(both, (page.width * 2, page.height * 4), page);
        assert!((scale - 0.25).abs() < 1e-9, "{scale}");
    }

    #[test]
    fn a_percentage_is_clamped_to_the_range_the_dialog_offers() {
        let page = PageBox::new(paper("1"), false, [0.75, 0.7, 0.75, 0.7]);
        let at = |p| effective_scale(Scaling::Percent(p), (1000, 1000), page);
        assert!((at(70) - 0.7).abs() < 1e-9);
        assert!((at(0) - 0.1).abs() < 1e-9);
        assert!((at(9999) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn the_scaling_a_sheet_asks_for_comes_from_its_own_attributes() {
        let mut sheet = casual_calc_model::Sheet::new(
            casual_calc_model::SheetId(casual_calc_model::Id::from_parts(2, 1)),
            "S",
        );
        assert_eq!(Scaling::from_print(&sheet), Scaling::Percent(100));

        sheet.print.page.insert("scale".to_owned(), "70".to_owned());
        assert_eq!(Scaling::from_print(&sheet), Scaling::Percent(70));

        // `fitToPage` selects fit-to-page whatever `scale` also says, and an
        // explicit 0 means that axis is unconstrained where an absent one
        // means one page.
        sheet
            .print
            .setup_pr
            .insert("fitToPage".to_owned(), "1".to_owned());
        sheet
            .print
            .page
            .insert("fitToHeight".to_owned(), "0".to_owned());
        assert_eq!(
            Scaling::from_print(&sheet),
            Scaling::Fit {
                wide: Some(1),
                tall: None
            }
        );
    }
}

/// Headers and footers (`IO-10`): the field-code language, the variants, and
/// the room they take out of the paper.
mod header_footer {
    use std::collections::BTreeMap;

    use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

    use crate::GridGeometry;
    use crate::print::{
        HF_MAX_CHARS, HF_MAX_LINES, HeaderField, HeaderFooter, PrintContext, header_footers,
        paginate, paginate_with_context, parse_header_footer,
    };

    fn parse(raw: &str) -> (HeaderFooter, BTreeMap<&'static str, u64>) {
        let mut refused = BTreeMap::new();
        let parsed = parse_header_footer(raw, "Sheet1", &PrintContext::default(), &mut refused);
        (parsed, refused)
    }

    /// The text of one section's first line, with `&P` and `&N` filled in.
    fn line(raw: &str, section: usize, page: i64, pages: usize) -> String {
        let (parsed, _) = parse(raw);
        parsed.resolve(page, pages)[section]
            .first()
            .map(|runs| runs.iter().map(|r| r.text.as_str()).collect::<String>())
            .unwrap_or_default()
    }

    /// **The counter counts.** `IO-10`'s first named gap: `&P` had nothing to
    /// resolve against, so a printed sheet carried no page number at all.
    #[test]
    fn the_page_counter_is_the_page_it_is_printed_on() {
        assert_eq!(line("&CPage &P of &N", 1, 1, 4), "Page 1 of 4");
        assert_eq!(line("&CPage &P of &N", 1, 3, 4), "Page 3 of 4");
        // `&P+2` shifts the printed number, which is how a chapter that starts
        // at page three is numbered.
        assert_eq!(line("&C&P+2", 1, 1, 4), "3");
        assert_eq!(line("&C&P-1", 1, 4, 4), "3");
    }

    /// Text before any `&L`/`&C`/`&R` is centred, and the three sections are
    /// independent boxes rather than three paragraphs.
    #[test]
    fn an_unmarked_header_is_centred_and_the_sections_are_independent() {
        assert_eq!(line("Just a title", 1, 1, 1), "Just a title");
        assert_eq!(line("&Lleft&Ccentre&Rright", 0, 1, 1), "left");
        assert_eq!(line("&Lleft&Ccentre&Rright", 1, 1, 1), "centre");
        assert_eq!(line("&Lleft&Ccentre&Rright", 2, 1, 1), "right");
    }

    /// `&&` is the escape for a literal ampersand.
    #[test]
    fn a_doubled_ampersand_is_one_ampersand() {
        assert_eq!(line("&CSmith && Sons", 1, 1, 1), "Smith & Sons");
    }

    /// **Nothing is dropped without being named.** Every code this cannot draw
    /// is counted under the name a compatibility report shows — and its letters
    /// do not leak onto the paper, which is the other half of the bargain.
    #[test]
    fn a_code_that_cannot_be_drawn_is_named_and_its_letters_do_not_print() {
        let (parsed, refused) = parse("&L&GLogo&C&KFF0000Red&R&Uunder");
        assert_eq!(
            refused.keys().copied().collect::<Vec<_>>(),
            [
                "header/footer picture (&G)",
                "header/footer text colour (&K)",
                "header/footer underline (&U)",
            ]
        );
        let resolved = parsed.resolve(1, 1);
        let text = |section: usize| -> String {
            resolved[section]
                .first()
                .map(|runs| runs.iter().map(|r| r.text.as_str()).collect())
                .unwrap_or_default()
        };
        assert_eq!(text(0), "Logo", "&G is consumed, the text after it is not");
        assert_eq!(text(1), "Red", "the six colour digits are not text");
        assert_eq!(text(2), "under");
    }

    /// The engine reads no clock and knows no file name. Asking for either
    /// without supplying it is a **refusal by name**, not an empty string: a
    /// header reading "Printed on" and stopping is a defect a reader has to
    /// guess at.
    #[test]
    fn a_date_with_no_clock_is_refused_rather_than_printed_blank() {
        let (parsed, refused) = parse("&LPrinted on &D at &T&R&F");
        assert_eq!(
            refused.keys().copied().collect::<Vec<_>>(),
            [
                "header/footer date (&D)",
                "header/footer file name (&F)",
                "header/footer time (&T)",
            ]
        );
        assert!(parsed.resolve(1, 1)[2].is_empty(), "no file, no run");

        let mut refused = BTreeMap::new();
        let ctx = PrintContext {
            file: "book.xlsx",
            now: Some(45_000.0),
        };
        let dated = parse_header_footer("&L&D&R&F", "Sheet1", &ctx, &mut refused);
        assert!(refused.is_empty(), "everything was answered: {refused:?}");
        assert_eq!(dated.resolve(1, 1)[0][0][0].text, "2023-03-15");
        assert_eq!(dated.resolve(1, 1)[2][0][0].text, "book.xlsx");
    }

    /// `&B` and `&"Arial,Bold Italic"` dress a run rather than printing.
    #[test]
    fn the_formatting_codes_that_are_supported_dress_the_run() {
        let (parsed, refused) = parse("&C&BTotal&B: &\"Arial,Bold Italic\"&14x");
        assert!(refused.is_empty(), "{refused:?}");
        let runs = parsed.resolve(1, 1)[1][0].clone();
        assert_eq!(runs[0].text, "Total");
        assert!(runs[0].bold && !runs[0].italic);
        assert_eq!(runs[1].text, ": ");
        assert!(!runs[1].bold, "&B toggles off as well as on");
        assert_eq!(runs[2].text, "x");
        assert_eq!(runs[2].font.as_deref(), Some("Arial"));
        assert!(runs[2].bold && runs[2].italic);
        assert!((runs[2].size_pt - 14.0).abs() < f32::EPSILON);
    }

    /// A newline in the string is a new line on the paper.
    #[test]
    fn a_newline_starts_a_new_line() {
        let (parsed, _) = parse("&Cfirst\nsecond\nthird");
        let lines = parsed.resolve(1, 1)[1].clone();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[2][0].text, "third");
    }

    fn sheet_with(rows: u32) -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        for row in 0..rows {
            sheet
                .cells
                .set(CellRef::new(row, 0), Cell::value(CellValue::Number(1.0)));
        }
        wb.sheets.push(sheet);
        wb
    }

    /// **An ordinary header does not move the printout.** Excel's default 0.3"
    /// header margin and a 9-point line fit inside its default 0.75" top
    /// margin, so reserving room for them would print a page that does not
    /// match Excel's.
    #[test]
    fn a_header_that_fits_the_top_margin_leaves_the_body_where_it_was() {
        let plain = sheet_with(60);
        let geometry = GridGeometry::for_sheet(&plain.sheets[0]);
        let before = paginate(&plain, 0, &geometry).expect("pages");

        let mut with_header = sheet_with(60);
        with_header.sheets[0]
            .print
            .header_footer_text
            .insert("oddHeader".to_owned(), "&CQuarterly".to_owned());
        let after = paginate(&with_header, 0, &geometry).expect("pages");

        assert_eq!(after.header_twips(), 0, "nothing reserved");
        assert_eq!(after.margins, before.margins);
        assert_eq!(after.page_box, before.page_box);
        assert_eq!(after.pages.len(), before.pages.len());
    }

    /// **A header too tall for the margin pushes the body down.** The other
    /// half of the same rule: a three-line 24-point header cannot be drawn
    /// inside 0.75", so the text area starts below it rather than under it.
    #[test]
    fn a_header_taller_than_the_top_margin_moves_the_body_down() {
        let plain = sheet_with(200);
        let geometry = GridGeometry::for_sheet(&plain.sheets[0]);
        let before = paginate(&plain, 0, &geometry).expect("pages");

        let mut tall = sheet_with(200);
        tall.sheets[0].print.header_footer_text.insert(
            "oddHeader".to_owned(),
            "&C&24BIG\n&24BIGGER\n&24BIGGEST".to_owned(),
        );
        let after = paginate(&tall, 0, &geometry).expect("pages");

        assert!(
            after.header_twips() > 0,
            "a header 0.3in down and three 24pt lines tall does not fit 0.75in"
        );
        assert_eq!(
            after.margins[0],
            before.margins[0] + after.header_twips(),
            "the reservation is the top margin's, so the body starts below it"
        );
        assert_eq!(
            after.page_box.height,
            before.page_box.height - after.header_twips() - after.footer_twips()
        );
        assert!(
            after.pages.len() >= before.pages.len(),
            "a shorter text area cannot hold more rows per page"
        );
    }

    /// `differentFirst` selects the first-page variant **whether or not the
    /// file wrote one**. A sheet whose author cleared the title page's header
    /// must print nothing there, not the ordinary header.
    #[test]
    fn different_first_prints_nothing_when_the_file_cleared_it() {
        let mut wb = sheet_with(200);
        let print = &mut wb.sheets[0].print;
        print
            .header_footer
            .insert("differentFirst".to_owned(), "1".to_owned());
        print
            .header_footer_text
            .insert("oddHeader".to_owned(), "&Cordinary".to_owned());
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        let plan = paginate(&wb, 0, &geometry).expect("pages");
        assert!(plan.pages.len() > 1, "the test needs a second page");

        let (first, _) = plan.furniture(0);
        let (second, _) = plan.furniture(1);
        assert!(first.is_empty(), "page one carries the cleared variant");
        assert_eq!(second.resolve(2, 2)[1][0][0].text, "ordinary");
    }

    /// `differentOddEven` swaps the variant by the **printed** number, which is
    /// what a reader of a bound report sees.
    #[test]
    fn different_odd_even_swaps_by_the_printed_page_number() {
        let mut wb = sheet_with(200);
        let print = &mut wb.sheets[0].print;
        print
            .header_footer
            .insert("differentOddEven".to_owned(), "1".to_owned());
        print
            .header_footer_text
            .insert("oddHeader".to_owned(), "&Codd".to_owned());
        print
            .header_footer_text
            .insert("evenHeader".to_owned(), "&Ceven".to_owned());
        print
            .page
            .insert("useFirstPageNumber".to_owned(), "1".to_owned());
        print
            .page
            .insert("firstPageNumber".to_owned(), "2".to_owned());
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        let plan = paginate(&wb, 0, &geometry).expect("pages");
        assert!(plan.pages.len() > 1, "the test needs a second page");

        assert_eq!(plan.page_number(0), 2, "the file asked to start at two");
        let text = |index: usize| {
            let (header, _) = plan.furniture(index);
            header.resolve(plan.page_number(index), plan.pages.len())[1][0][0]
                .text
                .clone()
        };
        assert_eq!(text(0), "even", "page two is even, even though it is first");
        assert_eq!(text(1), "odd");
    }

    /// The two schema flags default to *on*, so only an explicit zero turns
    /// them off — reading an absent attribute as false would move every header
    /// to the paper's edge.
    #[test]
    fn align_with_margins_and_scale_with_doc_default_to_on() {
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        let read = header_footers(&sheet, &PrintContext::default());
        assert!(read.align_with_margins && read.scale_with_doc);

        sheet
            .print
            .header_footer
            .insert("alignWithMargins".to_owned(), "0".to_owned());
        let read = header_footers(&sheet, &PrintContext::default());
        assert!(!read.align_with_margins && read.scale_with_doc);
    }

    /// `&A` is the sheet's own tab name, which is why parsing needs it.
    #[test]
    fn the_sheet_name_code_is_the_sheet_it_prints() {
        let mut refused = BTreeMap::new();
        let parsed =
            parse_header_footer("&C&A", "Q3 Actuals", &PrintContext::default(), &mut refused);
        assert_eq!(parsed.resolve(1, 1)[1][0][0].text, "Q3 Actuals");
        assert!(refused.is_empty());
    }

    /// The host's values reach the plan, not only the parser.
    #[test]
    fn the_context_reaches_the_plan() {
        let mut wb = sheet_with(10);
        wb.sheets[0]
            .print
            .header_footer_text
            .insert("oddHeader".to_owned(), "&C&F".to_owned());
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        let ctx = PrintContext {
            file: "ledger.xlsx",
            now: None,
        };
        let plan = paginate_with_context(&wb, 0, &geometry, &ctx).expect("pages");
        assert!(plan.header_footers.refused.is_empty());
        let (header, _) = plan.furniture(0);
        assert_eq!(header.resolve(1, 1)[1][0][0].text, "ledger.xlsx");

        // And without it, the same file is a refusal rather than a blank.
        let plan = paginate(&wb, 0, &geometry).expect("pages");
        assert_eq!(
            plan.header_footers
                .refused
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            ["header/footer file name (&F)"]
        );
    }

    /// A header is a string out of an untrusted file. Neither its line count
    /// nor its length may decide how much paper or work it costs.
    ///
    /// The reservation is the half that bites: an unbounded height leaves a
    /// printable box of one twip, which is a page per row up to the page cap —
    /// a memory cost with a page count for a multiplier, from a header nobody
    /// can see.
    #[test]
    fn a_hostile_header_is_bounded() {
        let (parsed, _) = parse(&format!("&C{}", "x\n".repeat(500)));
        assert!(
            parsed.resolve(1, 1)[1].len() <= HF_MAX_LINES,
            "line count is capped"
        );
        let mut wb = sheet_with(40);
        wb.sheets[0].print.header_footer_text.insert(
            "oddHeader".to_owned(),
            format!("&C{}", "x\n".repeat(100_000)),
        );
        let geometry = GridGeometry::for_sheet(&wb.sheets[0]);
        let plan = paginate(&wb, 0, &geometry).expect("pages");
        assert!(
            plan.page_box.height > 1,
            "a header of a hundred thousand lines must not consume the page: \
             box is {} twips after reserving {}",
            plan.page_box.height,
            plan.header_twips()
        );
        assert!(
            plan.pages.len() < 40,
            "one page per row is the failure mode"
        );
        let (parsed, _) = parse(&format!("&C{}", "x".repeat(100_000)));
        let drawn: usize = parsed.resolve(1, 1)[1][0]
            .iter()
            .map(|r| r.text.chars().count())
            .sum();
        assert_eq!(drawn, HF_MAX_CHARS);
    }

    /// The parsed form must not depend on anything that moves between runs.
    #[test]
    fn parsing_is_deterministic() {
        let raw = "&L&BSales&B&C&P of &N&R&G&Kff0000x";
        let (first, first_refused) = parse(raw);
        let (second, second_refused) = parse(raw);
        assert_eq!(first, second);
        assert_eq!(first_refused, second_refused);
        assert_eq!(
            HeaderField::Text("x".to_owned()),
            second.sections[2][0].field
        );
    }
}
