//! TP1 as a property, not as examples.
//!
//! The convergence law is
//!
//! ```text
//! apply(apply(S, a), transform(b, a)) == apply(apply(S, b), transform(a, b))
//! ```
//!
//! and it is checked over the cross product of a generated operation set rather
//! than over cases someone thought of. Transform functions fail on the pair
//! nobody considered — that is how OT ships broken — and the op set being
//! closed with a pure `apply` is exactly what makes exhaustive generation
//! possible.

use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, StyleId, Workbook};

use crate::{
    Operation, apply,
    transform::{Side, transform},
};

/// A small workbook with enough populated cells, sizing and structure that a
/// divergence shows up in the comparison rather than in an empty grid.
fn seed() -> Workbook {
    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    for row in 0..12u32 {
        for col in 0..6u32 {
            sheet.cells.set(
                CellRef::new(row, col),
                Cell::value(CellValue::Number(f64::from(row * 10 + col))),
            );
        }
    }
    sheet.columns.sizes.insert(1, 120);
    sheet.columns.sizes.insert(4, 90);
    sheet.rows.sizes.insert(2, 40);
    sheet.rows.sizes.insert(9, 60);
    workbook.sheets.push(sheet);
    // A second sheet, so that renumbering has somewhere to renumber *to* and a
    // wrong index lands on real content instead of on nothing.
    let mut other = Sheet::new(SheetId(Id::from_parts(2, 2)), "T");
    for row in 0..4u32 {
        other.cells.set(
            CellRef::new(row, 0),
            Cell::value(CellValue::Number(f64::from(1000 + row))),
        );
    }
    workbook.sheets.push(other);
    workbook
}

/// The generated operations. Deliberately clustered around the same indices so
/// that overlaps, ties and containments are all hit rather than left to luck.
fn candidates() -> Vec<Operation> {
    let mut ops = Vec::new();
    for at in [0u32, 2, 3, 4, 7] {
        for count in [1u32, 2, 4] {
            ops.push(Operation::InsertRows {
                sheet: 0,
                at,
                count,
            });
            ops.push(Operation::DeleteRows {
                sheet: 0,
                at,
                count,
            });
        }
    }
    for at in [0u32, 1, 2, 4] {
        for count in [1u32, 2] {
            ops.push(Operation::InsertColumns {
                sheet: 0,
                at,
                count,
            });
            ops.push(Operation::DeleteColumns {
                sheet: 0,
                at,
                count,
            });
        }
    }
    for (row, col) in [(0u32, 0u32), (3, 1), (4, 2), (7, 4), (11, 5)] {
        ops.push(Operation::SetCell {
            sheet: 0,
            at: CellRef::new(row, col),
            cell: Some(Cell::value(CellValue::Number(f64::from(row + col) + 0.5))),
        });
        ops.push(Operation::ClearCell {
            sheet: 0,
            at: CellRef::new(row, col),
        });
        ops.push(Operation::SetStyle {
            sheet: 0,
            at: CellRef::new(row, col),
            style: Some(StyleId::at(1)),
        });
    }
    for line in [0u32, 2, 4, 9] {
        ops.push(Operation::SetColumnWidth {
            sheet: 0,
            col: line,
            width: Some(111),
        });
        ops.push(Operation::SetRowHeight {
            sheet: 0,
            row: line,
            height: Some(33),
        });
    }
    // Metadata bundles carrying positional state, so the rebase across a
    // structural band is exercised rather than assumed. Each changes exactly
    // one field, which is what the mask then reports.
    let base_meta = crate::SheetMetadata::capture(&seed().sheets[0]);
    let mut merged = base_meta.clone();
    merged.merges.push(casual_calc_model::CellRange::new(
        CellRef::new(4, 0),
        CellRef::new(5, 1),
    ));
    ops.push(Operation::set_sheet_metadata(0, merged));

    let mut hidden = base_meta.clone();
    hidden.hidden_rows.insert(5);
    ops.push(Operation::set_sheet_metadata(0, hidden));

    let mut widened = base_meta.clone();
    widened.columns.sizes.insert(3, 175);
    ops.push(Operation::set_sheet_metadata(0, widened));

    let mut frozen = base_meta.clone();
    frozen.view.frozen_rows = 2;
    ops.push(Operation::set_sheet_metadata(0, frozen));

    let mut outlined = base_meta.clone();
    outlined.row_outline_levels.insert(6, 1);
    outlined.collapsed_rows.insert(6);
    ops.push(Operation::set_sheet_metadata(0, outlined));

    // Sheet-level operations, and edits on the second sheet for them to move.
    //
    // **Two distinct sheets per index**, which is what makes a *tie* possible.
    // With one insertion per index no pair in this set ever contests the same
    // position, so the one case where sheet insertion needs a tie-break — two
    // clients adding a sheet at the same tab — could not be generated, and the
    // property test reported TP1 holding over a set that excluded the only pair
    // that broke it.
    for index in [0usize, 1, 2] {
        for (tag, id) in [("added", 9u64), ("other", 10)] {
            ops.push(Operation::InsertSheet {
                index,
                sheet: Box::new(Sheet::new(
                    SheetId(Id::from_parts(2, id)),
                    format!("{tag}{index}"),
                )),
            });
        }
    }
    for index in [0usize, 1] {
        ops.push(Operation::RemoveSheet { index });
        ops.push(Operation::RenameSheet {
            index,
            name: format!("renamed{index}"),
        });
        ops.push(Operation::SetTabColor {
            sheet: index,
            color: Some("FF00FF".to_owned()),
        });
    }
    for (from, to) in [(0usize, 1usize), (1, 0)] {
        ops.push(Operation::MoveSheet { from, to });
    }
    for (row, col) in [(0u32, 0u32), (2, 0)] {
        ops.push(Operation::SetCell {
            sheet: 1,
            at: CellRef::new(row, col),
            cell: Some(Cell::value(CellValue::Number(f64::from(row) - 7.0))),
        });
    }
    ops.push(Operation::InsertRows {
        sheet: 1,
        at: 1,
        count: 2,
    });

    ops.push(Operation::Batch(vec![
        Operation::InsertRows {
            sheet: 0,
            at: 3,
            count: 2,
        },
        Operation::SetCell {
            sheet: 0,
            at: CellRef::new(3, 0),
            cell: Some(Cell::value(CellValue::Number(-1.0))),
        },
    ]));
    ops
}

/// The observable state, as the comparison sees it. Every cell that could have
/// moved, plus the axis sizing the structural ops rewrite.
fn observe(workbook: &Workbook) -> String {
    // Every sheet, by name rather than by index: renumbering is exactly what is
    // being tested, so comparing position by position would call two different
    // orderings equal.
    let sheets: Vec<String> = workbook
        .sheets
        .iter()
        .map(|sheet| {
            let mut cells: Vec<String> = sheet
                .cells
                .iter()
                .map(|(at, cell)| format!("{}:{}={:?}", at.row, at.col, cell.value))
                .collect();
            cells.sort();
            format!(
                "{}[{}] tab{:?} cols{:?} rows{:?} merges{:?} hidden{:?}/{:?} frozen{:?} outline{:?}/{:?}",
                sheet.name,
                cells.join(","),
                sheet.tab_color,
                sheet.columns.sizes,
                sheet.rows.sizes,
                sheet.merges,
                sheet.hidden_rows,
                sheet.hidden_cols,
                (sheet.view.frozen_rows, sheet.view.frozen_cols),
                sheet.row_outline_levels,
                sheet.collapsed_rows,
            )
        })
        .collect();
    // **Order preserved, deliberately.** This used to `sheets.sort()`, on the
    // reasoning that comparing position by position would call two different
    // orderings equal — which is backwards: sorting is what calls two different
    // orderings *equal*, and sheet order is observable state. Every operation
    // on the wire addresses a sheet by index, so two replicas holding the same
    // sheets in different orders is not a cosmetic difference, it is the two of
    // them meaning different things by `sheet: 1` from then on.
    //
    // The name is already in each entry, so a genuine renumbering still shows
    // up as the same names in a different order rather than as a false match.
    sheets.join(" | ")
}

#[test]
fn tp1_holds_for_every_supported_pair() {
    let base = seed();
    // Ops enter the protocol narrowed — see `Operation::narrowed`. A metadata
    // op still declaring ALL would read as contending with every concurrent
    // metadata edit, and the transform has no workbook to work that out from.
    let ops: Vec<Operation> = candidates()
        .into_iter()
        .map(|op| op.narrowed(&base))
        .collect();
    let mut checked = 0usize;
    let mut skipped = 0usize;

    for a in &ops {
        for b in &ops {
            // The two replicas must agree on *one* order before they can
            // agree on a result, so fix it here: a is ordered before b. Each
            // side then transforms with the role that order gives it.
            let Ok(b_after_a) = transform(b, a, Side::Later, &[]) else {
                skipped += 1;
                continue;
            };
            let Ok(a_after_b) = transform(a, b, Side::Earlier, &[]) else {
                skipped += 1;
                continue;
            };

            let mut left = base.clone();
            apply(&mut left, a.clone()).expect("a applies to the seed");
            apply(&mut left, b_after_a.clone()).expect("b' applies after a");

            let mut right = base.clone();
            apply(&mut right, b.clone()).expect("b applies to the seed");
            apply(&mut right, a_after_b.clone()).expect("a' applies after b");

            assert_eq!(
                observe(&left),
                observe(&right),
                "TP1 violated.\n  a  = {a:?}\n  b  = {b:?}\n  b' = {b_after_a:?}\n  a' = {a_after_b:?}"
            );
            checked += 1;
        }
    }

    // A guard against the test passing because everything was skipped. The
    // seed deliberately contains no formulas and no sheet-renumbering ops, so
    // every generated pair must be answerable.
    // Concurrent sheet reordering is the one refused corner, so the skips are
    // expected here — but they are counted, not waved through, and the pairs
    // that *are* answered still have to outnumber them heavily.
    assert!(
        skipped * 8 < checked,
        "{skipped} refused against {checked} answered — too much is being skipped"
    );
    assert!(checked > 3_000, "only {checked} pairs checked");
    println!("TP1: {checked} pairs converged, {skipped} refused");
}

#[test]
fn operations_on_different_sheets_are_left_alone() {
    let a = Operation::DeleteRows {
        sheet: 0,
        at: 0,
        count: 5,
    };
    let b = Operation::SetCell {
        sheet: 1,
        at: CellRef::new(3, 0),
        cell: None,
    };
    assert_eq!(
        transform(&b, &a, Side::Later, &[]).unwrap(),
        b,
        "a different sheet cannot move"
    );
}

#[test]
fn a_cell_inside_a_deleted_band_becomes_a_no_op() {
    let deleted = Operation::DeleteRows {
        sheet: 0,
        at: 2,
        count: 3,
    };
    let edit = Operation::SetCell {
        sheet: 0,
        at: CellRef::new(3, 1),
        cell: Some(Cell::value(CellValue::Number(1.0))),
    };
    assert!(
        crate::transform::is_noop(&transform(&edit, &deleted, Side::Later, &[]).unwrap()),
        "the cell it addressed no longer exists"
    );
}

#[test]
fn a_cell_below_a_deleted_band_moves_up() {
    let deleted = Operation::DeleteRows {
        sheet: 0,
        at: 2,
        count: 3,
    };
    let edit = Operation::SetCell {
        sheet: 0,
        at: CellRef::new(8, 1),
        cell: None,
    };
    let Operation::SetCell { at, .. } = transform(&edit, &deleted, Side::Later, &[]).unwrap()
    else {
        panic!("still a cell edit");
    };
    assert_eq!(at, CellRef::new(5, 1));
}

#[test]
fn an_insert_strictly_inside_a_delete_is_swallowed_and_the_delete_widens() {
    // The pair is only convergent together: if the insert survived, the widened
    // delete would remove one row too many.
    let del = Operation::DeleteRows {
        sheet: 0,
        at: 5,
        count: 5,
    };
    let ins = Operation::InsertRows {
        sheet: 0,
        at: 7,
        count: 1,
    };
    assert!(crate::transform::is_noop(
        &transform(&ins, &del, Side::Later, &[]).unwrap()
    ));
    assert_eq!(
        transform(&del, &ins, Side::Later, &[]).unwrap(),
        Operation::DeleteRows {
            sheet: 0,
            at: 5,
            count: 6
        }
    );
}

#[test]
fn an_insert_at_a_deletes_first_line_survives_and_pushes_it() {
    // The boundary the "strictly inside" rule exists for: inserting at the
    // band's first line puts the new rows *before* it.
    let del = Operation::DeleteRows {
        sheet: 0,
        at: 5,
        count: 3,
    };
    let ins = Operation::InsertRows {
        sheet: 0,
        at: 5,
        count: 1,
    };
    assert_eq!(transform(&ins, &del, Side::Later, &[]).unwrap(), ins);
    assert_eq!(
        transform(&del, &ins, Side::Later, &[]).unwrap(),
        Operation::DeleteRows {
            sheet: 0,
            at: 6,
            count: 3
        }
    );
}

#[test]
fn two_deletes_do_not_remove_the_overlap_twice() {
    let a = Operation::DeleteRows {
        sheet: 0,
        at: 2,
        count: 4,
    };
    let b = Operation::DeleteRows {
        sheet: 0,
        at: 4,
        count: 4,
    };
    assert_eq!(
        transform(&a, &b, Side::Later, &[]).unwrap(),
        Operation::DeleteRows {
            sheet: 0,
            at: 2,
            count: 2
        }
    );
    assert_eq!(
        transform(&b, &a, Side::Later, &[]).unwrap(),
        Operation::DeleteRows {
            sheet: 0,
            at: 2,
            count: 2
        }
    );
}

#[test]
fn a_fully_covered_delete_becomes_a_no_op() {
    let inner = Operation::DeleteRows {
        sheet: 0,
        at: 3,
        count: 2,
    };
    let outer = Operation::DeleteRows {
        sheet: 0,
        at: 2,
        count: 6,
    };
    assert!(crate::transform::is_noop(
        &transform(&inner, &outer, Side::Later, &[]).unwrap()
    ));
}

#[test]
fn a_row_band_does_not_move_a_column_width() {
    let rows = Operation::InsertRows {
        sheet: 0,
        at: 0,
        count: 3,
    };
    let width = Operation::SetColumnWidth {
        sheet: 0,
        col: 2,
        width: Some(100),
    };
    assert_eq!(transform(&width, &rows, Side::Later, &[]).unwrap(), width);
}

#[test]
fn an_edit_follows_its_sheet_when_another_is_inserted_before_it() {
    let insert_sheet = Operation::InsertSheet {
        index: 0,
        sheet: Box::new(Sheet::new(SheetId(Id::from_parts(9, 1)), "new")),
    };
    let edit = Operation::SetCell {
        sheet: 1,
        at: CellRef::new(0, 0),
        cell: None,
    };
    let Operation::SetCell { sheet, .. } =
        transform(&edit, &insert_sheet, Side::Later, &[]).unwrap()
    else {
        panic!("still a cell edit");
    };
    assert_eq!(
        sheet, 2,
        "sheet 1 is sheet 2 once one is added at the front"
    );
}

#[test]
fn an_edit_on_a_removed_sheet_becomes_a_no_op() {
    let removed = Operation::RemoveSheet { index: 1 };
    let edit = Operation::SetCell {
        sheet: 1,
        at: CellRef::new(0, 0),
        cell: None,
    };
    assert!(crate::transform::is_noop(
        &transform(&edit, &removed, Side::Later, &[]).unwrap()
    ));

    // And a sheet after it moves down rather than vanishing.
    let elsewhere = Operation::SetTabColor {
        sheet: 2,
        color: None,
    };
    assert_eq!(
        transform(&elsewhere, &removed, Side::Later, &[]).unwrap(),
        Operation::SetTabColor {
            sheet: 1,
            color: None
        }
    );
}

#[test]
fn an_edit_follows_a_moved_sheet() {
    // [A, B, C, D] with from=0 to=2 becomes [B, C, A, D].
    let moved = Operation::MoveSheet { from: 0, to: 2 };
    for (before, after) in [(0usize, 2usize), (1, 0), (2, 1), (3, 3)] {
        let edit = Operation::SetTabColor {
            sheet: before,
            color: None,
        };
        assert_eq!(
            transform(&edit, &moved, Side::Later, &[]).unwrap(),
            Operation::SetTabColor {
                sheet: after,
                color: None
            },
            "sheet {before} should land at {after}"
        );
    }
}

#[test]
fn concurrent_sheet_reordering_is_refused_rather_than_guessed() {
    // The subtle corner, and a rare one. Returning either op untransformed
    // would diverge the replicas silently.
    let a = Operation::MoveSheet { from: 0, to: 2 };
    let b = Operation::MoveSheet { from: 1, to: 0 };
    assert!(transform(&a, &b, Side::Later, &[]).is_err());

    // An insertion *position* under a move is refused for the same reason: a
    // bare index does not record which sheets it meant to sit between.
    let insert = Operation::InsertSheet {
        index: 1,
        sheet: Box::new(Sheet::new(SheetId(Id::from_parts(9, 1)), "new")),
    };
    assert!(transform(&insert, &a, Side::Later, &[]).is_err());
}

#[test]
fn a_pending_metadata_edit_is_rebased_across_a_structural_op() {
    // A resize of column 1 must still mean column 1 after someone inserts a
    // column before it — the bundle's positional state moves with the sheet.
    let inserted = Operation::InsertColumns {
        sheet: 0,
        at: 0,
        count: 2,
    };
    let mut data = crate::SheetMetadata::default();
    data.columns.sizes.insert(1, 175);
    data.hidden_cols.insert(1);
    let resize = Operation::set_sheet_metadata(0, data);

    let Operation::SetSheetMetadata { data, .. } =
        transform(&resize, &inserted, Side::Later, &[]).unwrap()
    else {
        panic!("still a metadata change");
    };
    assert_eq!(data.columns.sizes.get(&3), Some(&175), "moved 1 -> 3");
    assert!(data.hidden_cols.contains(&3));
}

#[test]
fn a_metadata_edit_loses_what_a_concurrent_delete_removed() {
    let deleted = Operation::DeleteRows {
        sheet: 0,
        at: 4,
        count: 3,
    };
    let mut data = crate::SheetMetadata::default();
    data.hidden_rows.insert(5); // inside the deleted band
    data.hidden_rows.insert(9); // past it
    let hide = Operation::set_sheet_metadata(0, data);

    let Operation::SetSheetMetadata { data, .. } =
        transform(&hide, &deleted, Side::Later, &[]).unwrap()
    else {
        panic!("still a metadata change");
    };
    assert!(!data.hidden_rows.contains(&5), "the row it hid is gone");
    assert!(data.hidden_rows.contains(&6), "the one past it moved up");
}

#[test]
fn the_earlier_write_yields_only_the_aspect_the_later_one_takes() {
    // Bolding a cell while someone types into it must not discard the typing.
    // `SetStyle` keeps the value, so the value edit survives as a `SetValue`.
    let typed = Operation::SetCell {
        sheet: 0,
        at: CellRef::new(0, 0),
        cell: Some(Cell::value(CellValue::Number(9.0))),
    };
    let bolded = Operation::SetStyle {
        sheet: 0,
        at: CellRef::new(0, 0),
        style: Some(StyleId::at(1)),
    };

    assert_eq!(
        transform(&typed, &bolded, Side::Earlier, &[]).unwrap(),
        Operation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(9.0),
        },
        "the value survives; only the style is conceded"
    );
    assert_eq!(
        transform(&bolded, &typed, Side::Later, &[]).unwrap(),
        bolded,
        "the later operation is untouched"
    );
}

#[test]
fn two_writes_of_the_same_aspect_leave_only_the_later_one() {
    let a = Operation::SetCell {
        sheet: 0,
        at: CellRef::new(1, 1),
        cell: Some(Cell::value(CellValue::Number(1.0))),
    };
    let b = Operation::ClearCell {
        sheet: 0,
        at: CellRef::new(1, 1),
    };
    assert!(crate::transform::is_noop(
        &transform(&a, &b, Side::Earlier, &[]).unwrap()
    ));
    assert_eq!(transform(&b, &a, Side::Later, &[]).unwrap(), b);
}

#[test]
fn a_formula_that_cannot_be_rebased_is_refused_not_degraded() {
    // `SetValue` is the only content-without-style operation and it carries a
    // value, so a formula has nowhere to go. Refusing beats silently turning
    // the formula into its last result.
    let mut workbook = seed();
    let handle = workbook.store_formula(casual_calc_formula::parse("1+2").unwrap());
    let mut cell = Cell::value(CellValue::Number(3.0));
    cell.formula = Some(handle);
    let typed = Operation::SetCell {
        sheet: 0,
        at: CellRef::new(0, 0),
        cell: Some(cell),
    };
    let bolded = Operation::SetStyle {
        sheet: 0,
        at: CellRef::new(0, 0),
        style: Some(StyleId::at(1)),
    };
    assert!(transform(&typed, &bolded, Side::Earlier, &[]).is_err());
    // The other direction is fine: the style write needs no rebasing.
    assert!(transform(&bolded, &typed, Side::Later, &[]).is_ok());
}

#[test]
fn metadata_edits_to_different_fields_both_survive() {
    // The payoff of the change mask. Before it, these were two whole-sheet
    // bundles and the later one silently discarded the earlier.
    let mut base = crate::SheetMetadata::default();
    base.view.hide_gridlines = false;

    let mut resize = base.clone();
    resize.columns.sizes.insert(2, 140);
    let mut view = base.clone();
    view.view.hide_gridlines = true;

    let a = Operation::SetSheetMetadata {
        sheet: 0,
        data: Box::new(resize),
        changed: crate::SheetFields::COLUMNS,
        restore: Default::default(),
    };
    let b = Operation::SetSheetMetadata {
        sheet: 0,
        data: Box::new(view),
        changed: crate::SheetFields::VIEW,
        restore: Default::default(),
    };

    assert_eq!(
        transform(&a, &b, Side::Earlier, &[])
            .unwrap()
            .sheet_fields(),
        crate::SheetFields::COLUMNS,
        "the resize keeps its field"
    );
    assert_eq!(
        transform(&b, &a, Side::Later, &[]).unwrap().sheet_fields(),
        crate::SheetFields::VIEW,
        "and the view change keeps its own"
    );
}

#[test]
fn metadata_edits_to_the_same_field_are_ordered_not_merged() {
    let mut a_data = crate::SheetMetadata::default();
    a_data.columns.sizes.insert(2, 140);
    let mut b_data = crate::SheetMetadata::default();
    b_data.columns.sizes.insert(5, 200);

    let a = Operation::SetSheetMetadata {
        sheet: 0,
        data: Box::new(a_data),
        changed: crate::SheetFields::COLUMNS,
        restore: Default::default(),
    };
    let b = Operation::SetSheetMetadata {
        sheet: 0,
        data: Box::new(b_data),
        changed: crate::SheetFields::COLUMNS,
        restore: Default::default(),
    };

    assert!(
        transform(&a, &b, Side::Earlier, &[])
            .unwrap()
            .sheet_fields()
            .is_empty(),
        "the earlier one yields the contested field"
    );
    assert_eq!(
        transform(&b, &a, Side::Later, &[]).unwrap().sheet_fields(),
        crate::SheetFields::COLUMNS,
    );
}

/// **A pending chart edit must not reinstate a pre-insert series (FID-28).**
///
/// One client edits a chart while another inserts a row above the data it
/// plots. `shift_metadata_*` moves the chart's *frame*, because a frame is a
/// position — but a series is a reference **string**, and deciding whether
/// `S!$D$2:$D$11` names the sheet being shifted needs that sheet's *name*.
/// The transform is a pure function over operations and an operation carries
/// only an index, so this went undone by `FID-26`.
///
/// It does not need the wire to carry identity, though: the transform's callers
/// hold the workbook, so they pass what the indices mean. The empty slice every
/// other test uses is exactly the "identity unknown" case, which is why this one
/// supplies it.
#[test]
fn a_pending_chart_series_follows_a_concurrent_insert() {
    use casual_calc_model::{ChartKind, ChartSeries, ChartView};

    let mut data = crate::SheetMetadata::default();
    let mut chart = ChartView::new(
        casual_calc_model::CellRange::new(CellRef::new(0, 5), CellRef::new(9, 5)),
        ChartKind::Column,
    );
    chart.series.push(ChartSeries {
        name: "Amount".into(),
        categories: None,
        values: "S!$D$2:$D$11".into(),
    });
    data.charts.push(chart);

    let bundle = Operation::SetSheetMetadata {
        sheet: 0,
        data: Box::new(data),
        changed: crate::SheetFields::CHARTS,
        restore: Default::default(),
    };
    let insert = Operation::InsertRows {
        sheet: 0,
        at: 1,
        count: 2,
    };

    // What sheet 0's index actually names — supplied by the caller, which has
    // the workbook.
    let sheets = [("S".to_owned(), SheetId(Id::from_parts(2, 1)))];

    let Operation::SetSheetMetadata { data, .. } =
        transform(&bundle, &insert, Side::Later, &sheets).unwrap()
    else {
        panic!("a metadata bundle transforms into a metadata bundle");
    };

    assert_eq!(
        data.charts[0].series[0].values, "S!$D$4:$D$13",
        "the pending series must land on the rows the insert moved its data to"
    );
    assert_eq!(
        data.charts[0].anchor,
        casual_calc_model::CellRange::new(CellRef::new(0, 5), CellRef::new(11, 5)),
        "and the frame still moves, as it already did — this one *grows*, \
         because the insert lands inside it, exactly as a straddling merge does"
    );
}
