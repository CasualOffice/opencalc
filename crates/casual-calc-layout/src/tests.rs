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
            .map(|item| match item {
                PaintItem::CellBackground { rect, .. }
                | PaintItem::GridLine { rect }
                | PaintItem::MergedRegion { rect, .. }
                | PaintItem::DataBar { rect, .. }
                | PaintItem::Text { rect, .. }
                | PaintItem::Image { rect, .. }
                | PaintItem::CellBorder { rect, .. } => *rect,
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
