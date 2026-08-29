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

    use crate::chart::{PX, resolve, series_colors, value_extent};
    use crate::chart_data::{ref_cells, ref_numbers, ref_text};
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

    /// Series colours come from the workbook's own theme accents, and cycle.
    #[test]
    fn series_colours_are_the_workbook_theme_accents() {
        let mut wb = wb();
        assert_eq!(series_colors(&wb, 2), vec!["4472C4", "ED7D31"]);
        assert_eq!(series_colors(&wb, 7)[6], "4472C4", "the palette cycles");
        wb.theme_colors = vec![
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            "AA0000".to_owned(),
        ];
        assert_eq!(series_colors(&wb, 1), vec!["AA0000"], "this file's accent");
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
