//! Pivot tests: build a source table, define a pivot, refresh, read the grid.
//!
//! The reports are asserted as whole rendered blocks rather than cell by cell.
//! A pivot's bugs are nearly all *layout* bugs — a subtotal one row low, a
//! label repeated where it should be blank, a grand total under the wrong
//! column — and none of those show up in a spot check of three cells.

use casual_calc_model::{
    Cell, CellRange, CellRef, CellValue, Id, PivotAggregate, PivotAxisField, PivotFilterField,
    PivotSort, PivotTable, PivotValueField, Sheet, SheetId, Workbook,
};

use crate::pivot::{self, PivotError};

/// Four fields, eight records, deliberately unsorted and with a repeat so
/// grouping has something to do.
const SOURCE: &[[&str; 4]] = &[
    ["Region", "Product", "Rep", "Amount"],
    ["West", "Widget", "Ann", "100"],
    ["East", "Widget", "Bob", "50"],
    ["West", "Gadget", "Ann", "25"],
    ["East", "Gadget", "Bob", "70"],
    ["West", "Widget", "Cal", "5"],
    ["East", "Widget", "Ann", "30"],
    ["West", "Gadget", "Bob", "40"],
    ["East", "Gadget", "Cal", "10"],
];

fn workbook() -> Workbook {
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "Data");
    for (r, row) in SOURCE.iter().enumerate() {
        for (c, text) in row.iter().enumerate() {
            let at = CellRef::new(r as u32, c as u32);
            let value = match text.parse::<f64>() {
                Ok(n) if r > 0 => CellValue::Number(n),
                _ => CellValue::SharedString(wb.intern_string(text)),
            };
            sheet.cells.set(at, Cell::value(value));
        }
    }
    wb.sheets.push(sheet);
    wb.sheets
        .push(Sheet::new(SheetId(Id::from_parts(2, 2)), "Report"));
    wb
}

fn axis(source_column: u32) -> PivotAxisField {
    PivotAxisField {
        source_column,
        sort: PivotSort::Ascending,
        subtotal: true,
    }
}

fn sum(source_column: u32) -> PivotValueField {
    PivotValueField {
        source_column,
        aggregate: PivotAggregate::Sum,
        name: String::new(),
        number_format: None,
    }
}

/// A pivot on the second sheet over the whole of the first.
fn pivot(wb: &Workbook) -> PivotTable {
    PivotTable::new(
        1,
        "PivotTable1".to_owned(),
        wb.sheets[0].id,
        CellRange::new(CellRef::new(0, 0), CellRef::new(8, 3)),
        CellRef::new(0, 0),
    )
}

/// Render the pivot's output as rows of trimmed display text, so a test reads
/// like the block on screen.
fn render(wb: &Workbook, range: CellRange) -> Vec<Vec<String>> {
    (range.start.row..=range.end.row)
        .map(|row| {
            (range.start.col..=range.end.col)
                .map(|col| match wb.sheets[1].cells.get(CellRef::new(row, col)) {
                    None => String::new(),
                    Some(cell) => match &cell.value {
                        CellValue::Empty => String::new(),
                        CellValue::Number(n) => casual_calc_layout::format_general(*n),
                        CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
                        CellValue::Error(e) => e.to_string(),
                        CellValue::SharedString(id) | CellValue::InlineString(id) => {
                            wb.strings.get(*id).unwrap_or_default().to_owned()
                        }
                    },
                })
                .collect()
        })
        .collect()
}

fn install(wb: &mut Workbook, p: PivotTable) -> Vec<Vec<String>> {
    wb.sheets[1].pivots.push(p);
    let range = pivot::refresh(wb, 1, 0).expect("refresh");
    render(wb, range)
}

#[test]
fn one_row_field_and_one_measure_is_a_list_and_a_total() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.values.push(sum(3));
    assert_eq!(
        install(&mut wb, p),
        vec![
            vec!["Region", "Sum of Amount"],
            vec!["East", "160"],
            vec!["West", "170"],
            vec!["Grand Total", "330"],
        ]
    );
}

#[test]
fn a_column_field_spreads_the_measure_across_and_totals_both_ways() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.cols.push(axis(1));
    p.values.push(sum(3));
    // The corner names the measure, the row under it names the column field's
    // items, and the grand totals close both axes. 25+40=65 West Gadget.
    assert_eq!(
        install(&mut wb, p),
        vec![
            vec!["Sum of Amount", "Product", "", ""],
            vec!["Region", "Gadget", "Widget", "Grand Total"],
            vec!["East", "80", "80", "160"],
            vec!["West", "65", "105", "170"],
            vec!["Grand Total", "145", "185", "330"],
        ]
    );
}

#[test]
fn an_outer_row_field_gets_a_subtotal_and_the_inner_one_does_not() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.rows.push(axis(1));
    p.values.push(sum(3));
    // Region repeats only where it changes; each region closes with its own
    // total; the innermost field has none, because it would restate the line
    // directly above it.
    assert_eq!(
        install(&mut wb, p),
        vec![
            vec!["Region", "Product", "Sum of Amount"],
            vec!["East", "Gadget", "80"],
            vec!["", "Widget", "80"],
            vec!["East Total", "", "160"],
            vec!["West", "Gadget", "65"],
            vec!["", "Widget", "105"],
            vec!["West Total", "", "170"],
            vec!["Grand Total", "", "330"],
        ]
    );
}

#[test]
fn subtotals_can_be_turned_off_per_field() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(PivotAxisField {
        subtotal: false,
        ..axis(0)
    });
    p.rows.push(axis(1));
    p.values.push(sum(3));
    let grid = install(&mut wb, p);
    assert!(
        !grid.iter().any(|r| r[0] == "East Total"),
        "subtotal suppressed: {grid:?}"
    );
    assert_eq!(grid.last().unwrap()[0], "Grand Total");
}

#[test]
fn two_measures_get_a_caption_row_of_their_own() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.cols.push(axis(1));
    p.values.push(sum(3));
    p.values.push(PivotValueField {
        source_column: 3,
        aggregate: PivotAggregate::Count,
        name: String::new(),
        number_format: None,
    });
    assert_eq!(
        install(&mut wb, p),
        vec![
            vec!["Values", "Product", "", "", "", "", ""],
            vec!["", "Gadget", "", "Widget", "", "Grand Total", ""],
            vec![
                "Region",
                "Sum of Amount",
                "Count of Amount",
                "Sum of Amount",
                "Count of Amount",
                "Sum of Amount",
                "Count of Amount"
            ],
            vec!["East", "80", "2", "80", "2", "160", "4"],
            vec!["West", "65", "2", "105", "2", "170", "4"],
            vec!["Grand Total", "145", "4", "185", "4", "330", "8"],
        ]
    );
}

#[test]
fn every_aggregate_answers_from_the_same_pass() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    for aggregate in [
        PivotAggregate::Sum,
        PivotAggregate::Count,
        PivotAggregate::Average,
        PivotAggregate::Max,
        PivotAggregate::Min,
    ] {
        p.values.push(PivotValueField {
            source_column: 3,
            aggregate,
            name: String::new(),
            number_format: None,
        });
    }
    let grid = install(&mut wb, p);
    // East is 50, 70, 30, 10.
    assert_eq!(grid[1], vec!["East", "160", "4", "40", "70", "10"]);
}

#[test]
fn a_page_filter_narrows_every_figure_including_the_totals() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.filters.push(PivotFilterField {
        source_column: 1,
        selected: vec!["Widget".to_owned()],
    });
    p.values.push(sum(3));
    assert_eq!(
        install(&mut wb, p),
        vec![
            vec!["Product", "Widget"],
            vec!["", ""],
            vec!["Region", "Sum of Amount"],
            vec!["East", "80"],
            vec!["West", "105"],
            vec!["Grand Total", "185"],
        ]
    );
}

#[test]
fn an_empty_selection_is_all_items_not_none() {
    // The distinction matters because "I have not chosen" and "I chose
    // nothing" both arrive as an empty list, and only one of them should blank
    // the report.
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.filters.push(PivotFilterField {
        source_column: 1,
        selected: Vec::new(),
    });
    p.values.push(sum(3));
    let grid = install(&mut wb, p);
    assert_eq!(grid[0], vec!["Product", "(All)"]);
    assert_eq!(grid.last().unwrap(), &vec!["Grand Total", "330"]);
}

#[test]
fn descending_and_source_order_change_the_item_order() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(PivotAxisField {
        sort: PivotSort::Descending,
        ..axis(0)
    });
    p.values.push(sum(3));
    let grid = install(&mut wb, p);
    assert_eq!(grid[1][0], "West");
    assert_eq!(grid[2][0], "East");

    // `DataSource` keeps the order the records arrived in: West is the first
    // record, so it leads even though East sorts first.
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(PivotAxisField {
        sort: PivotSort::DataSource,
        ..axis(0)
    });
    p.values.push(sum(3));
    let grid = install(&mut wb, p);
    assert_eq!(grid[1][0], "West");
}

#[test]
fn a_refresh_gives_back_the_cells_the_report_shrank_out_of() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.rows.push(axis(1));
    p.values.push(sum(3));
    let wide = install(&mut wb, p);
    assert_eq!(wide.len(), 8);

    // Drop the inner field: the report loses its subtotal rows and a column.
    wb.sheets[1].pivots[0].rows.pop();
    let range = pivot::refresh(&mut wb, 1, 0).expect("refresh");
    assert_eq!(render(&wb, range).len(), 4);
    // Nothing of the old block is left behind. Without clearing the previous
    // extent the stale "West Total" row would sit under the new report looking
    // like part of it.
    assert!(
        wb.sheets[1]
            .cells
            .iter()
            .all(|(at, _)| at.row <= range.end.row && at.col <= range.end.col),
        "stale cells outside {range:?}"
    );
}

#[test]
fn a_refresh_refuses_rather_than_overwriting_and_changes_nothing() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.values.push(sum(3));
    p.anchor = CellRef::new(0, 0);
    install(&mut wb, p);

    // Something typed just past the report, then a change that would grow into
    // it.
    let note = wb.intern_string("do not lose me");
    wb.sheets[1].cells.set(
        CellRef::new(4, 0),
        Cell::value(CellValue::SharedString(note)),
    );
    wb.sheets[1].pivots[0].rows.push(axis(1));
    let before = wb.sheets[1].cells.len();

    let error = pivot::refresh(&mut wb, 1, 0).expect_err("must refuse");
    assert_eq!(error, PivotError::Collision(CellRef::new(4, 0)));
    // The refusal is total: the old report is still readable and the note is
    // still there. A partial write would be worse than either outcome.
    assert_eq!(wb.sheets[1].cells.len(), before);
    assert_eq!(
        wb.sheets[1]
            .cells
            .get(CellRef::new(4, 0))
            .map(|c| c.value.clone()),
        Some(CellValue::SharedString(note))
    );
}

#[test]
fn a_pivot_with_no_measure_says_so_instead_of_writing_an_empty_block() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    wb.sheets[1].pivots.push(p);
    assert_eq!(pivot::refresh(&mut wb, 1, 0), Err(PivotError::NoValues));
}

#[test]
fn a_header_only_source_has_nothing_to_report() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.source = CellRange::new(CellRef::new(0, 0), CellRef::new(0, 3));
    p.rows.push(axis(0));
    p.values.push(sum(3));
    wb.sheets[1].pivots.push(p);
    assert_eq!(pivot::refresh(&mut wb, 1, 0), Err(PivotError::EmptySource));
}

#[test]
fn blanks_group_under_one_item_and_sort_last() {
    let mut wb = workbook();
    // Clear one Region cell: the record still counts, under `(blank)`.
    wb.sheets[0].cells.clear(CellRef::new(1, 0));
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.values.push(sum(3));
    let grid = install(&mut wb, p);
    assert_eq!(
        grid,
        vec![
            vec!["Region", "Sum of Amount"],
            vec!["East", "160"],
            vec!["West", "70"],
            vec!["(blank)", "100"],
            vec!["Grand Total", "330"],
        ],
        "blanks are their own group, ordered after the text"
    );
}

#[test]
fn an_average_over_no_numbers_is_an_error_and_a_sum_is_blank() {
    // Aggregating a text column: `Sum` has nothing to add, which is not zero —
    // writing 0 would claim a measurement nobody took.
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.values.push(sum(2));
    p.values.push(PivotValueField {
        source_column: 2,
        aggregate: PivotAggregate::Average,
        name: String::new(),
        number_format: None,
    });
    p.values.push(PivotValueField {
        source_column: 2,
        aggregate: PivotAggregate::Count,
        name: String::new(),
        number_format: None,
    });
    let grid = install(&mut wb, p);
    assert_eq!(grid[1], vec!["East", "", "#DIV/0!", "4"]);
}

#[test]
fn a_renamed_measure_keeps_its_caption() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.values.push(PivotValueField {
        name: "Revenue".to_owned(),
        ..sum(3)
    });
    assert_eq!(install(&mut wb, p)[0], vec!["Region", "Revenue"]);
}

#[test]
fn a_measure_carries_its_number_format_onto_every_figure() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.values.push(PivotValueField {
        number_format: Some("#,##0.00".to_owned()),
        ..sum(3)
    });
    wb.sheets[1].pivots.push(p);
    let range = pivot::refresh(&mut wb, 1, 0).expect("refresh");
    let at = CellRef::new(range.start.row + 1, range.start.col + 1);
    let style = wb.sheets[1].cells.get(at).and_then(|c| c.style).unwrap();
    assert_eq!(
        wb.styles.get(style).unwrap().number_format.as_deref(),
        Some("#,##0.00")
    );
}

#[test]
fn column_subtotals_appear_for_an_outer_column_field() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.cols.push(axis(1));
    p.cols.push(axis(2));
    p.values.push(sum(3));
    // A label sits on the header row for the level at which its line stops:
    // `Grand Total` spans no field so it goes on the outer row, `Gadget Total`
    // stops after one field so it goes on the row below — the same rule the
    // row axis uses when it indents a subtotal into column `c0 + depth`.
    // An intersection with no records stays blank rather than showing 0.
    assert_eq!(
        install(&mut wb, p),
        vec![
            vec!["Sum of Amount", "Product", "", "", "", "", "", "", "", ""],
            vec![
                "",
                "Gadget",
                "",
                "",
                "",
                "Widget",
                "",
                "",
                "",
                "Grand Total"
            ],
            vec![
                "Region",
                "Ann",
                "Bob",
                "Cal",
                "Gadget Total",
                "Ann",
                "Bob",
                "Cal",
                "Widget Total",
                ""
            ],
            vec!["East", "", "70", "10", "80", "30", "50", "", "80", "160"],
            vec!["West", "25", "40", "", "65", "100", "", "5", "105", "170"],
            vec![
                "Grand Total",
                "25",
                "110",
                "10",
                "145",
                "130",
                "50",
                "5",
                "185",
                "330"
            ],
        ]
    );
}

#[test]
fn the_grand_totals_can_be_turned_off() {
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.cols.push(axis(1));
    p.values.push(sum(3));
    p.row_grand_totals = false;
    p.col_grand_totals = false;
    let grid = install(&mut wb, p);
    assert_eq!(grid[1], vec!["Region", "Gadget", "Widget"]);
    assert_eq!(grid.last().unwrap()[0], "West");
}

#[test]
fn a_pivot_summarizes_formula_results_once_the_recalculation_has_run() {
    // The order matters: a pivot over a column of formulas reads their cached
    // values, so refreshing before the recalculation would report the previous
    // pass's numbers — or nothing at all on a freshly loaded file.
    let mut wb = workbook();
    for row in 1..=8u32 {
        let expr = casual_calc_formula::parse(&format!("D{}*2", row + 1)).unwrap();
        let handle = wb.store_formula(expr);
        let mut cell = Cell::value(CellValue::Empty);
        cell.formula = Some(handle);
        wb.sheets[0].cells.set(CellRef::new(row, 4), cell);
    }
    let mut p = pivot(&wb);
    p.source = CellRange::new(CellRef::new(0, 0), CellRef::new(8, 4));
    p.rows.push(axis(0));
    p.values.push(sum(4));
    wb.sheets[1].pivots.push(p);

    crate::recalculate(&mut wb);
    let failures = pivot::refresh_all(&mut wb);
    assert!(failures.is_empty(), "{failures:?}");
    let range = wb.sheets[1].pivots[0].output.unwrap();
    assert_eq!(render(&wb, range).last().unwrap()[1], "660");
}

#[test]
fn the_field_list_and_a_filters_items_come_from_the_source() {
    let wb = workbook();
    let p = pivot(&wb);
    assert_eq!(
        pivot::field_names(&wb, &p),
        vec!["Region", "Product", "Rep", "Amount"]
    );
    assert_eq!(pivot::field_items(&wb, &p, 1), vec!["Gadget", "Widget"]);
    assert_eq!(
        pivot::field_items(&wb, &p, 3),
        vec!["5", "10", "25", "30", "40", "50", "70", "100"],
        "numbers order as numbers, not as the text they render to"
    );
}

/// The same shape Excel writes: a pivot part reached from the report sheet, a
/// cache reached from both the workbook and the pivot part, and the
/// `<pivotCache>` element in workbook.xml that declares the pair.
fn imported() -> Workbook {
    use casual_calc_model::{RetainedPart, RetainedRel};
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.values.push(sum(3));
    p.part = Some("xl/pivotTables/pivotTable1.xml".to_owned());
    p.output = Some(CellRange::new(CellRef::new(0, 0), CellRef::new(3, 1)));
    wb.sheets[1].pivots.push(p);

    for path in [
        "xl/pivotTables/pivotTable1.xml",
        "xl/pivotCache/pivotCacheDefinition1.xml",
        "xl/pivotCache/pivotCacheRecords1.xml",
    ] {
        wb.retained_parts.push(RetainedPart {
            path: path.to_owned(),
            bytes: b"<x/>".to_vec(),
            content_type: None,
        });
    }
    let rel = |source: &str, id: &str, kind: &str, target: &str| RetainedRel {
        source: source.to_owned(),
        id: id.to_owned(),
        rel_type: format!(
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}"
        ),
        target: target.to_owned(),
    };
    wb.retained_rels.push(rel(
        "xl/worksheets/sheet2.xml",
        "rId1",
        "pivotTable",
        "../pivotTables/pivotTable1.xml",
    ));
    wb.retained_rels.push(rel(
        "xl/pivotTables/pivotTable1.xml",
        "rId1",
        "pivotCacheDefinition",
        "../pivotCache/pivotCacheDefinition1.xml",
    ));
    wb.retained_rels.push(rel(
        "xl/workbook.xml",
        "rId9",
        "pivotCacheDefinition",
        "pivotCache/pivotCacheDefinition1.xml",
    ));
    wb.retained_rels.push(rel(
        "xl/pivotCache/pivotCacheDefinition1.xml",
        "rId1",
        "pivotCacheRecords",
        "pivotCacheRecords1.xml",
    ));
    wb.retained_refs.push((
        "pivotCache".to_owned(),
        [
            ("cacheId".to_owned(), "7".to_owned()),
            ("id".to_owned(), "rId9".to_owned()),
        ]
        .into_iter()
        .collect(),
    ));
    wb
}

#[test]
fn an_imported_pivot_is_left_alone_until_it_is_asked_to_refresh() {
    // Automatic refresh would rewrite every imported pivot in our tabular
    // layout the moment a file was opened, so opening and saving would be an
    // edit nobody made.
    let mut wb = imported();
    let before = wb.sheets[1].cells.len();
    let failures = pivot::refresh_all(&mut wb);
    assert!(failures.is_empty());
    assert_eq!(wb.sheets[1].cells.len(), before, "nothing written");
    assert_eq!(wb.retained_parts.len(), 3, "and nothing dropped");
}

#[test]
fn refreshing_an_imported_pivot_drops_the_parts_that_would_disagree_with_it() {
    let mut wb = imported();
    pivot::refresh(&mut wb, 1, 0).expect("refresh");

    // The pivot part described Excel's compact block; the cells now hold ours.
    // Writing the old part back would leave a file whose definition and whose
    // figures disagree, and a reader believes the definition.
    assert_eq!(wb.sheets[1].pivots[0].part, None);
    assert!(
        wb.retained_parts.is_empty(),
        "the cache and its records go too, or they dangle: {:?}",
        wb.retained_parts
    );
    // Every relationship reaching any of them, in both directions.
    assert!(wb.retained_rels.is_empty(), "{:?}", wb.retained_rels);
    // And the element in workbook.xml that declared the cache. Left behind, it
    // names a relationship that no longer exists, which Excel reports as a file
    // needing repair rather than as a missing pivot.
    assert!(wb.retained_refs.is_empty(), "{:?}", wb.retained_refs);
}

#[test]
fn a_shared_cache_survives_until_the_last_pivot_using_it_is_refreshed() {
    let mut wb = imported();
    // Excel writes one cache per source range, not one per pivot: a second
    // pivot over the same data shares it.
    let mut second = pivot(&wb);
    second.anchor = CellRef::new(0, 6);
    second.rows.push(axis(1));
    second.values.push(sum(3));
    second.part = Some("xl/pivotTables/pivotTable2.xml".to_owned());
    wb.sheets[1].pivots.push(second);
    wb.retained_parts.push(casual_calc_model::RetainedPart {
        path: "xl/pivotTables/pivotTable2.xml".to_owned(),
        bytes: b"<x/>".to_vec(),
        content_type: None,
    });
    wb.retained_rels.push(casual_calc_model::RetainedRel {
        source: "xl/pivotTables/pivotTable2.xml".to_owned(),
        id: "rId1".to_owned(),
        rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition".to_owned(),
        target: "../pivotCache/pivotCacheDefinition1.xml".to_owned(),
    });

    pivot::refresh(&mut wb, 1, 0).expect("refresh the first");
    assert!(
        wb.retained_parts
            .iter()
            .any(|p| p.path == "xl/pivotCache/pivotCacheDefinition1.xml"),
        "the second pivot still reads this cache"
    );

    pivot::refresh(&mut wb, 1, 1).expect("refresh the second");
    assert!(wb.retained_parts.is_empty(), "{:?}", wb.retained_parts);
    assert!(wb.retained_refs.is_empty());
}

#[test]
fn a_refused_refresh_leaves_an_imported_pivot_exactly_as_it_arrived() {
    let mut wb = imported();
    // Something typed where the report would grow to.
    let note = wb.intern_string("mine");
    wb.sheets[1].cells.set(
        CellRef::new(9, 9),
        Cell::value(CellValue::SharedString(note)),
    );
    wb.sheets[1].pivots[0].anchor = CellRef::new(8, 8);
    wb.sheets[1].pivots[0].output = None;

    assert!(pivot::refresh(&mut wb, 1, 0).is_err());
    assert_eq!(
        wb.sheets[1].pivots[0].part.as_deref(),
        Some("xl/pivotTables/pivotTable1.xml"),
        "a refusal must not half-detach it"
    );
    assert_eq!(wb.retained_parts.len(), 3);
}

#[test]
fn the_report_widens_its_columns_but_never_narrows_one() {
    // `Sum of Amount` in a default-width column reads as `Sum of Am`: a header
    // is clipped only because the cell beside it is occupied, which in a report
    // is always. Excel widens on every update and so does this.
    let mut wb = workbook();
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.values.push(sum(3));
    // One column already far wider than the report needs, by the user's hand.
    wb.sheets[1].columns.sizes.insert(0, 6000);
    wb.sheets[1].pivots.push(p);
    let plan = pivot::plan(&mut wb, 1, 0).expect("plan");

    let widths: std::collections::BTreeMap<u32, i64> = plan.widths.into_iter().collect();
    assert!(
        !widths.contains_key(&0),
        "a column the user widened keeps its width: {widths:?}"
    );
    let measure = widths[&1];
    assert!(
        measure > casual_calc_layout::DEFAULT_COL_WIDTH,
        "`Sum of Amount` needs more than the default {} twips, got {measure}",
        casual_calc_layout::DEFAULT_COL_WIDTH
    );
    // 13 characters plus two of room, in OOXML's own character unit:
    // (15 * 7 + 5) * 15.
    assert_eq!(measure, 1650);
}

#[test]
fn a_runaway_label_does_not_push_the_report_off_the_screen() {
    let mut wb = workbook();
    let long = "x".repeat(400);
    let id = wb.intern_string(&long);
    wb.sheets[0]
        .cells
        .set(CellRef::new(1, 0), Cell::value(CellValue::SharedString(id)));
    let mut p = pivot(&wb);
    p.rows.push(axis(0));
    p.values.push(sum(3));
    wb.sheets[1].pivots.push(p);
    let plan = pivot::plan(&mut wb, 1, 0).expect("plan");
    let widths: std::collections::BTreeMap<u32, i64> = plan.widths.into_iter().collect();
    // Capped at 40 characters: (40 * 7 + 5) * 15.
    assert_eq!(widths[&0], 4275);
}
