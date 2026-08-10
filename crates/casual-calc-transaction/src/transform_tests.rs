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
            style: Some(StyleId(Id::from_parts(3, 1))),
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
    let sheet = &workbook.sheets[0];
    let mut cells: Vec<String> = sheet
        .cells
        .iter()
        .map(|(at, cell)| format!("{}:{}={:?}", at.row, at.col, cell.value))
        .collect();
    cells.sort();
    format!(
        "cells[{}] cols{:?} rows{:?}",
        cells.join(","),
        sheet.columns.sizes,
        sheet.rows.sizes
    )
}

#[test]
fn tp1_holds_for_every_supported_pair() {
    let base = seed();
    let ops = candidates();
    let mut checked = 0usize;
    let mut skipped = 0usize;

    for a in &ops {
        for b in &ops {
            // The two replicas must agree on *one* order before they can
            // agree on a result, so fix it here: a is ordered before b. Each
            // side then transforms with the role that order gives it.
            let Ok(b_after_a) = transform(b, a, Side::Later) else {
                skipped += 1;
                continue;
            };
            let Ok(a_after_b) = transform(a, b, Side::Earlier) else {
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
    assert_eq!(skipped, 0, "no generated pair should have been skipped");
    assert!(checked > 3_000, "only {checked} pairs checked");
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
        transform(&b, &a, Side::Later).unwrap(),
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
        crate::transform::is_noop(&transform(&edit, &deleted, Side::Later).unwrap()),
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
    let Operation::SetCell { at, .. } = transform(&edit, &deleted, Side::Later).unwrap() else {
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
        &transform(&ins, &del, Side::Later).unwrap()
    ));
    assert_eq!(
        transform(&del, &ins, Side::Later).unwrap(),
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
    assert_eq!(transform(&ins, &del, Side::Later).unwrap(), ins);
    assert_eq!(
        transform(&del, &ins, Side::Later).unwrap(),
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
        transform(&a, &b, Side::Later).unwrap(),
        Operation::DeleteRows {
            sheet: 0,
            at: 2,
            count: 2
        }
    );
    assert_eq!(
        transform(&b, &a, Side::Later).unwrap(),
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
        &transform(&inner, &outer, Side::Later).unwrap()
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
    assert_eq!(transform(&width, &rows, Side::Later).unwrap(), width);
}

#[test]
fn sheet_renumbering_is_refused_rather_than_guessed() {
    // Returning the op untransformed would diverge the replicas silently, which
    // is the one outcome this layer must not produce.
    let insert_sheet = Operation::InsertSheet {
        index: 0,
        sheet: Box::new(Sheet::new(SheetId(Id::from_parts(9, 1)), "new")),
    };
    let edit = Operation::SetCell {
        sheet: 1,
        at: CellRef::new(0, 0),
        cell: None,
    };
    assert!(transform(&edit, &insert_sheet, Side::Later).is_err());
}

#[test]
fn a_metadata_bundle_meeting_a_structural_op_is_refused() {
    let structural = Operation::InsertRows {
        sheet: 0,
        at: 0,
        count: 1,
    };
    let metadata = Operation::set_sheet_metadata(0, crate::SheetMetadata::default());
    assert!(transform(&metadata, &structural, Side::Later).is_err());
    // ...but on another sheet there is nothing to shift.
    let elsewhere = Operation::set_sheet_metadata(1, crate::SheetMetadata::default());
    assert!(transform(&elsewhere, &structural, Side::Later).is_ok());
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
        style: Some(StyleId(Id::from_parts(3, 1))),
    };

    assert_eq!(
        transform(&typed, &bolded, Side::Earlier).unwrap(),
        Operation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(9.0),
        },
        "the value survives; only the style is conceded"
    );
    assert_eq!(
        transform(&bolded, &typed, Side::Later).unwrap(),
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
        &transform(&a, &b, Side::Earlier).unwrap()
    ));
    assert_eq!(transform(&b, &a, Side::Later).unwrap(), b);
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
        style: Some(StyleId(Id::from_parts(3, 1))),
    };
    assert!(transform(&typed, &bolded, Side::Earlier).is_err());
    // The other direction is fine: the style write needs no rebasing.
    assert!(transform(&bolded, &typed, Side::Later).is_ok());
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
    };
    let b = Operation::SetSheetMetadata {
        sheet: 0,
        data: Box::new(view),
        changed: crate::SheetFields::VIEW,
    };

    assert_eq!(
        transform(&a, &b, Side::Earlier).unwrap().sheet_fields(),
        crate::SheetFields::COLUMNS,
        "the resize keeps its field"
    );
    assert_eq!(
        transform(&b, &a, Side::Later).unwrap().sheet_fields(),
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
    };
    let b = Operation::SetSheetMetadata {
        sheet: 0,
        data: Box::new(b_data),
        changed: crate::SheetFields::COLUMNS,
    };

    assert!(
        transform(&a, &b, Side::Earlier)
            .unwrap()
            .sheet_fields()
            .is_empty(),
        "the earlier one yields the contested field"
    );
    assert_eq!(
        transform(&b, &a, Side::Later).unwrap().sheet_fields(),
        crate::SheetFields::COLUMNS,
    );
}
