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
