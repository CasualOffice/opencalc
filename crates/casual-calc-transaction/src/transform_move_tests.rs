//! TP1 over a candidate set that contains **line moves** — `COL-44`.
//!
//! ```text
//! apply(apply(S, a), transform_with_formulas(b, a)) == apply(apply(S, b), transform_with_formulas(a, b))
//! ```
//!
//! Separate from [`crate::transform_tests`] rather than folded into it because
//! a move needs a wider grid to be interesting — a drag has to have somewhere
//! to land — and because the interesting pairs are `move × everything` rather
//! than the full cross product of everything.
//!
//! Checked against the **real** `apply`, so the transform cannot converge on a
//! model of the move that the engine does not perform. That is the failure mode
//! this whole file is aimed at: a move transform is easy to make
//! self-consistent and wrong.

use casual_calc_formula::stored::Origin;
use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, StyleId, Workbook};

use crate::{
    Operation, apply,
    transform::{Side, TransformError, transform_with_formulas},
};

fn seed() -> Workbook {
    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    for row in 0..12u32 {
        for col in 0..8u32 {
            sheet.cells.set(
                CellRef::new(row, col),
                Cell::value(CellValue::Number(f64::from(row * 100 + col))),
            );
        }
    }
    sheet.columns.sizes.insert(1, 120);
    sheet.columns.sizes.insert(4, 90);
    sheet.rows.sizes.insert(2, 40);
    sheet.rows.sizes.insert(9, 60);
    workbook.sheets.push(sheet);
    let mut other = Sheet::new(SheetId(Id::from_parts(2, 2)), "T");
    for row in 0..4u32 {
        other.cells.set(
            CellRef::new(row, 0),
            Cell::value(CellValue::Number(f64::from(1000 + row))),
        );
    }
    workbook.sheets.push(other);
    // Formulas, for the same reason [`crate::transform_tests::seed`] has them:
    // a move rewrites every reference that names the sheet it reorders, and a
    // transform that shifted the address alone would diverge exactly as
    // `COL-46` did for a band. Single references only — a *range* under two
    // concurrent structural operations is `COL-50`, which is not about moves
    // and is pinned there.
    for (at, text) in [
        (CellRef::new(1, 1), "$D$1"),
        (CellRef::new(5, 0), "C6*2"),
        (CellRef::new(8, 6), "$G$3"),
    ] {
        let handle = workbook.store_formula_at(
            casual_calc_formula::parse(text).expect("parses"),
            Origin::at(at.row, at.col),
        );
        let mut cell = workbook.sheets[0]
            .cells
            .get(at)
            .cloned()
            .expect("populated");
        cell.formula = Some(handle);
        workbook.sheets[0].cells.set(at, cell);
    }
    workbook
}

/// Every line move worth generating over an 8-column, 12-row sheet: `(at,
/// count, before)` clustered so that abutting bands, containments, ties at the
/// landing gap and drops on either edge are all hit rather than left to luck.
fn moves() -> Vec<Operation> {
    let mut ops = Vec::new();
    for at in 0..6u32 {
        for count in [1u32, 2, 3] {
            for before in 0..8u32 {
                ops.push(Operation::MoveColumns {
                    sheet: 0,
                    at,
                    count,
                    before,
                });
            }
        }
    }
    for at in [0u32, 1, 3, 6, 9] {
        for count in [1u32, 2, 4] {
            for before in [0u32, 1, 2, 5, 8, 11, 12] {
                ops.push(Operation::MoveRows {
                    sheet: 0,
                    at,
                    count,
                    before,
                });
            }
        }
    }
    ops
}

/// Everything a move has to be transformed against.
fn others(workbook: &mut Workbook) -> Vec<Operation> {
    let mut ops = Vec::new();
    for at in [0u32, 1, 2, 3, 5, 7] {
        for count in [1u32, 2, 3] {
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
    for at in [0u32, 1, 3, 6, 10] {
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
    for (row, col) in [(0u32, 0u32), (3, 1), (4, 2), (7, 4), (11, 7), (2, 5)] {
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
        ops.push(Operation::SetValue {
            sheet: 0,
            at: CellRef::new(row, col),
            value: CellValue::Number(f64::from(row * col)),
        });
    }
    // A pending edit carrying a formula, which is what a move has to rewrite
    // rather than merely relocate.
    for (at, text) in [(CellRef::new(2, 3), "$B$1"), (CellRef::new(6, 2), "E7")] {
        let handle = workbook.store_formula_at(
            casual_calc_formula::parse(text).expect("parses"),
            Origin::at(at.row, at.col),
        );
        let mut cell = Cell::value(CellValue::Number(-9.0));
        cell.formula = Some(handle);
        ops.push(Operation::SetCell {
            sheet: 0,
            at,
            cell: Some(cell),
        });
    }
    for line in [0u32, 2, 4, 7] {
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
    ops.push(Operation::SetTabColor {
        sheet: 0,
        color: Some("FF00FF".to_owned()),
    });
    // The metadata bundle: the arm the out-of-tree prototype could not write,
    // because the permutation it needs is `structural::move_metadata` and that
    // is `pub(crate)`.
    let mut sized = crate::SheetMetadata::capture(&seed().sheets[0]);
    sized.columns.sizes.insert(6, 175);
    sized.hidden_cols.insert(2);
    ops.push(Operation::set_sheet_metadata(0, sized));
    // Sheet-level renumbering, so the `sheet` field of a move is exercised.
    for index in [0usize, 1] {
        ops.push(Operation::InsertSheet {
            index,
            sheet: Box::new(Sheet::new(
                SheetId(Id::from_parts(2, 9)),
                format!("n{index}"),
            )),
        });
        ops.push(Operation::RemoveSheet { index });
        ops.push(Operation::RenameSheet {
            index,
            name: format!("r{index}"),
        });
    }
    ops.push(Operation::MoveSheet { from: 0, to: 1 });
    // An edit on the other sheet: a move on sheet 0 must leave it alone.
    ops.push(Operation::SetCell {
        sheet: 1,
        at: CellRef::new(1, 0),
        cell: Some(Cell::value(CellValue::Number(-1.0))),
    });
    // A batch, so the threading is exercised on both sides of a move.
    ops.push(Operation::Batch(vec![
        Operation::InsertColumns {
            sheet: 0,
            at: 2,
            count: 1,
        },
        Operation::SetCell {
            sheet: 0,
            at: CellRef::new(0, 2),
            cell: Some(Cell::value(CellValue::Number(-2.0))),
        },
    ]));
    ops
}

/// The observable state: every cell with its value **and its formula**, the
/// axis sizing, and the positional metadata a move permutes.
fn observe(workbook: &Workbook) -> String {
    workbook
        .sheets
        .iter()
        .map(|sheet| {
            let mut cells: Vec<String> = sheet
                .cells
                .iter()
                .map(|(at, cell)| {
                    let formula = cell
                        .formula
                        .and_then(|handle| workbook.formula(handle))
                        .map_or_else(String::new, |expr| format!("={expr:?}"));
                    format!("{}:{}={:?}{formula}", at.row, at.col, cell.value)
                })
                .collect();
            cells.sort();
            format!(
                "{}[{}] tab{:?} cols{:?} rows{:?} merges{:?} hidden{:?}/{:?} frozen{:?}",
                sheet.name,
                cells.join(","),
                sheet.tab_color,
                sheet.columns.sizes,
                sheet.rows.sizes,
                sheet.merges,
                sheet.hidden_rows,
                sheet.hidden_cols,
                (sheet.view.frozen_rows, sheet.view.frozen_cols),
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

struct Report {
    checked: usize,
    refused: usize,
    failures: Vec<String>,
}

/// Every ordered pair from `lefts × rights`, transformed both ways and applied
/// both ways.
fn sweep(base: &mut Workbook, lefts: &[Operation], rights: &[Operation]) -> Report {
    let names: Vec<(String, SheetId)> = base
        .sheets
        .iter()
        .map(|sheet| (sheet.name.clone(), sheet.id))
        .collect();
    let mut report = Report {
        checked: 0,
        refused: 0,
        failures: Vec::new(),
    };
    for a in lefts {
        for b in rights {
            let (Ok(b_after_a), Ok(a_after_b)) = (
                transform_with_formulas(b, a, Side::Later, &names, &mut *base),
                transform_with_formulas(a, b, Side::Earlier, &names, &mut *base),
            ) else {
                report.refused += 1;
                continue;
            };
            let mut left = base.clone();
            apply(&mut left, a.clone()).expect("a applies");
            apply(&mut left, b_after_a.clone()).expect("b' applies");
            let mut right = base.clone();
            apply(&mut right, b.clone()).expect("b applies");
            apply(&mut right, a_after_b.clone()).expect("a' applies");
            if observe(&left) == observe(&right) {
                report.checked += 1;
            } else {
                report.failures.push(format!(
                    "TP1 violated.\n  a  = {a:?}\n  b  = {b:?}\n  b' = {b_after_a:?}\n  a' = {a_after_b:?}\n  left  = {}\n  right = {}",
                    observe(&left),
                    observe(&right)
                ));
            }
        }
    }
    report
}

#[test]
fn tp1_holds_for_every_pair_involving_a_line_move() {
    let mut base = seed();
    let mut all = moves();
    all.extend(others(&mut base));
    let report = sweep(&mut base, &moves(), &all);
    println!(
        "move TP1: {} converged, {} refused, {} diverged",
        report.checked,
        report.refused,
        report.failures.len()
    );
    for failure in report.failures.iter().take(4) {
        println!("{failure}\n");
    }
    assert!(
        report.failures.is_empty(),
        "{} of {} pairs diverged",
        report.failures.len(),
        report.failures.len() + report.checked
    );
    assert!(
        report.checked > 80_000,
        "only {} pairs checked",
        report.checked
    );
    assert!(
        report.refused * 8 < report.checked,
        "{} refused against {} answered — too much is being skipped",
        report.refused,
        report.checked
    );
}

/// What a move rebased across each kind of operation costs, named rather than
/// left as one number.
///
/// A refusal is survivable and a divergence is not, so the residual matters —
/// but it matters as a *shape*: two people reordering the same columns is
/// contention and is meant to be refused for the earlier one, while two people
/// reordering different columns must work.
#[test]
fn what_a_move_still_refuses_is_confined_to_the_two_named_shapes() {
    let mut base = seed();
    let names: Vec<(String, SheetId)> = base
        .sheets
        .iter()
        .map(|sheet| (sheet.name.clone(), sheet.id))
        .collect();
    let all_moves = moves();
    let mut refused_move_pairs = 0usize;
    let mut answered_move_pairs = 0usize;
    for a in &all_moves {
        for b in &all_moves {
            let ok = transform_with_formulas(b, a, Side::Later, &names, &mut base).is_ok()
                && transform_with_formulas(a, b, Side::Earlier, &names, &mut base).is_ok();
            if ok {
                answered_move_pairs += 1;
            } else {
                refused_move_pairs += 1;
            }
        }
    }
    println!("move vs move: {answered_move_pairs} answered, {refused_move_pairs} refused");

    // Everything that is *not* another move is answered outright.
    let others = others(&mut base);
    let mut refused_other = Vec::new();
    for a in &all_moves {
        for b in &others {
            if transform_with_formulas(b, a, Side::Later, &names, &mut base).is_err()
                || transform_with_formulas(a, b, Side::Earlier, &names, &mut base).is_err()
            {
                refused_other.push(format!("{a:?} vs {b:?}"));
            }
        }
    }
    assert!(
        refused_other.is_empty(),
        "a move should answer everything that is not another move, but refused {}: {:?}",
        refused_other.len(),
        &refused_other[..refused_other.len().min(4)]
    );
    assert!(
        answered_move_pairs * 3 > refused_move_pairs,
        "only {answered_move_pairs} of {} move-vs-move pairs answered",
        answered_move_pairs + refused_move_pairs
    );
}

/// The arm the out-of-tree prototype had to refuse: a pending **column resize**
/// meeting a concurrent column drag.
///
/// The bundle carries the whole sizing map, so a move has to permute it — with
/// `move_metadata`, the pass `apply` runs on the sheet itself, rather than a
/// second implementation of the same permutation. Without this the resize lands
/// on whichever column now happens to sit at that index, which is a silent
/// wrong answer rather than a refusal.
#[test]
fn a_pending_column_resize_follows_a_concurrent_column_drag() {
    let mut base = seed();
    let names: Vec<(String, SheetId)> = base
        .sheets
        .iter()
        .map(|sheet| (sheet.name.clone(), sheet.id))
        .collect();

    let mut data = crate::SheetMetadata::default();
    data.columns.sizes.insert(1, 175);
    data.hidden_cols.insert(1);
    let resize = Operation::set_sheet_metadata(0, data);
    // Column 1 is dragged to the end: it becomes column 7.
    let drag = Operation::MoveColumns {
        sheet: 0,
        at: 1,
        count: 1,
        before: 8,
    };

    let Operation::SetSheetMetadata { data, .. } =
        transform_with_formulas(&resize, &drag, Side::Later, &names, &mut base)
            .expect("answered, not refused")
    else {
        panic!("a metadata bundle transforms into a metadata bundle");
    };
    assert_eq!(
        data.columns.sizes.get(&7),
        Some(&175),
        "the resize must follow the column that was dragged, not stay at index 1"
    );
    assert!(
        data.hidden_cols.contains(&7),
        "and so must everything else the bundle indexes by column"
    );
}

/// A pending chart series follows a concurrent drag too — the `FID-28` half of
/// the metadata arm, which only worked for a band until the move had a
/// transform at all.
#[test]
fn a_pending_chart_series_follows_a_concurrent_column_drag() {
    use casual_calc_model::{ChartKind, ChartSeries, ChartView};

    let mut base = seed();
    let names: Vec<(String, SheetId)> = base
        .sheets
        .iter()
        .map(|sheet| (sheet.name.clone(), sheet.id))
        .collect();

    let mut data = crate::SheetMetadata::default();
    let mut chart = ChartView::new(
        casual_calc_model::CellRange::new(CellRef::new(0, 0), CellRef::new(9, 0)),
        ChartKind::Column,
    );
    chart.series.push(ChartSeries {
        name: "Amount".into(),
        categories: None,
        values: "S!$B$1:$B$10".into(),
        ..ChartSeries::default()
    });
    data.charts.push(chart);
    // A pivot whose *source* is on the sheet being dragged. Its report block is
    // a position and moves with `move_metadata`; its source rectangle is on
    // `source_sheet` and needs the same identity a chart series does, so it
    // goes through the same pass.
    data.pivots.push(casual_calc_model::PivotTable::new(
        1,
        "P".to_owned(),
        SheetId(Id::from_parts(2, 1)),
        casual_calc_model::CellRange::new(CellRef::new(0, 1), CellRef::new(9, 1)),
        CellRef::new(0, 6),
    ));
    let bundle = Operation::SetSheetMetadata {
        sheet: 0,
        data: Box::new(data),
        changed: crate::SheetFields::CHARTS.union(crate::SheetFields::PIVOTS),
        restore: crate::RetainedBytes::default(),
    };
    let drag = Operation::MoveColumns {
        sheet: 0,
        at: 1,
        count: 1,
        before: 8,
    };

    let Operation::SetSheetMetadata { data, .. } =
        transform_with_formulas(&bundle, &drag, Side::Later, &names, &mut base).expect("answered")
    else {
        panic!("a metadata bundle transforms into a metadata bundle");
    };
    assert_eq!(
        data.charts[0].series[0].values, "S!$H$1:$H$10",
        "column B was dragged to the end, so the series has to follow it"
    );
    assert_eq!(
        data.pivots[0].source,
        casual_calc_model::CellRange::new(CellRef::new(0, 7), CellRef::new(9, 7)),
        "and so does the pivot's source rectangle, which lives on that sheet"
    );
}

/// A move rebased across a *delete* answers one move, because the survivors of
/// a contiguous band minus a contiguous band are contiguous. The other
/// direction cannot promise that, and does not: a delete rebased across a move
/// answers a batch of up to three, highest first.
#[test]
fn a_delete_scattered_by_a_move_answers_several_deletes_highest_first() {
    let mut base = seed();
    let names: Vec<(String, SheetId)> = base
        .sheets
        .iter()
        .map(|sheet| (sheet.name.clone(), sheet.id))
        .collect();
    // Columns 0–3 are deleted; concurrently column 1 is dragged to the far end,
    // so the deleted set is no longer contiguous: {0, 1, 2} and {7}.
    let del = Operation::DeleteColumns {
        sheet: 0,
        at: 0,
        count: 4,
    };
    let drag = Operation::MoveColumns {
        sheet: 0,
        at: 1,
        count: 1,
        before: 8,
    };
    let answer =
        transform_with_formulas(&del, &drag, Side::Later, &names, &mut base).expect("answered");
    assert_eq!(
        answer,
        Operation::Batch(vec![
            Operation::DeleteColumns {
                sheet: 0,
                at: 7,
                count: 1
            },
            Operation::DeleteColumns {
                sheet: 0,
                at: 0,
                count: 3
            },
        ]),
        "highest first, or the first delete renumbers the second"
    );
}

/// Two people dragging the **same** columns somewhere different is contention,
/// and is resolved as two writes of one cell are.
#[test]
fn two_moves_of_the_same_band_are_ordered_rather_than_both_applied() {
    let mut base = seed();
    let names: Vec<(String, SheetId)> = base
        .sheets
        .iter()
        .map(|sheet| (sheet.name.clone(), sheet.id))
        .collect();
    let mine = Operation::MoveColumns {
        sheet: 0,
        at: 1,
        count: 2,
        before: 6,
    };
    let theirs = Operation::MoveColumns {
        sheet: 0,
        at: 1,
        count: 2,
        before: 0,
    };
    assert!(
        crate::transform::is_noop(
            &transform_with_formulas(&mine, &theirs, Side::Earlier, &names, &mut base).unwrap()
        ),
        "the earlier drag yields entirely — relocating both leaves the band \
         wherever the second application happened to put it, which is a \
         different place on each replica"
    );
    assert!(
        !crate::transform::is_noop(
            &transform_with_formulas(&theirs, &mine, Side::Later, &names, &mut base).unwrap()
        ),
        "and the later one still moves the band it meant to move"
    );
}

/// Bands that overlap without being equal have no answer a single `Move*` can
/// express, and are refused rather than approximated.
#[test]
fn overlapping_but_unequal_move_bands_are_refused() {
    let mut base = seed();
    let names: Vec<(String, SheetId)> = base
        .sheets
        .iter()
        .map(|sheet| (sheet.name.clone(), sheet.id))
        .collect();
    let wide = Operation::MoveColumns {
        sheet: 0,
        at: 1,
        count: 3,
        before: 7,
    };
    let narrow = Operation::MoveColumns {
        sheet: 0,
        at: 2,
        count: 1,
        before: 0,
    };
    assert!(matches!(
        transform_with_formulas(&wide, &narrow, Side::Later, &names, &mut base),
        Err(TransformError::Unsupported { .. })
    ));
}

/// The **complete** set of pairs this transform refuses, pinned (`COL-61`).
///
/// Every other test here asks whether one pair behaves. None of them asks the
/// question this one does: *what is the whole refusal surface?* Nothing did,
/// and the module docs drifted because of it — [`crate::transform`]'s "What is
/// not handled yet" names two cases, and there are three. The missing one is
/// two line moves whose source bands overlap, which is the only one of the
/// three a user reaches by ordinary dragging, and it is what latched the
/// session behind `COL-55` and `COL-56`.
///
/// A refusal is not a bug — `TransformError::Unsupported` is the honest answer
/// where a wrong answer would diverge the replicas silently. What is a bug is a
/// refusal **nobody wrote down**, because the module docs are where a caller
/// looks to find out what it must handle. So this asserts the set exactly:
/// widening it fails here, and the fix is to document the new case as well as
/// to intend it.
///
/// Geometries, not just variant pairs. Move-against-move transforms cleanly
/// when the bands are disjoint, identical, crossing or adjacent, and refuses
/// only on overlap — so a matrix over variant pairs alone would have found
/// nothing and reported the surface as one case wide.
#[test]
fn the_refusal_surface_is_exactly_these_cases() {
    let mut base = seed();
    let names: Vec<(String, SheetId)> = base
        .sheets
        .iter()
        .map(|sheet| (sheet.name.clone(), sheet.id))
        .collect();

    let mc = |at, count, before| Operation::MoveColumns {
        sheet: 0,
        at,
        count,
        before,
    };
    let cases: Vec<(&str, Operation, Operation)> = vec![
        ("move/move identical", mc(2, 1, 6), mc(2, 1, 6)),
        ("move/move disjoint", mc(2, 1, 6), mc(10, 1, 14)),
        ("move/move same source", mc(2, 1, 6), mc(2, 1, 9)),
        ("move/move same target", mc(2, 1, 6), mc(3, 1, 6)),
        ("move/move overlapping bands", mc(2, 3, 9), mc(3, 3, 12)),
        ("move/move nested bands", mc(2, 4, 10), mc(3, 1, 11)),
        ("move/move crossing", mc(2, 2, 8), mc(7, 2, 1)),
        ("move/move adjacent", mc(2, 1, 3), mc(3, 1, 4)),
        (
            "sheet move/sheet move",
            Operation::MoveSheet { from: 0, to: 1 },
            Operation::MoveSheet { from: 1, to: 0 },
        ),
        (
            "move/insert columns",
            mc(2, 1, 6),
            Operation::InsertColumns {
                sheet: 0,
                at: 3,
                count: 1,
            },
        ),
        (
            "move/delete columns",
            mc(2, 1, 6),
            Operation::DeleteColumns {
                sheet: 0,
                at: 3,
                count: 1,
            },
        ),
    ];

    let mut refused: Vec<String> = Vec::new();
    for (name, subject, against) in &cases {
        for side in [Side::Earlier, Side::Later] {
            if let Err(TransformError::Unsupported { .. }) =
                transform_with_formulas(subject, against, side, &names, &mut base)
            {
                refused.push(format!("{name} ({side:?})"));
            }
        }
    }
    refused.sort();

    // Each entry is a case the module docs must name. Adding one here without
    // adding it there is the drift this test exists to stop.
    let expected = vec![
        "move/move nested bands (Earlier)",
        "move/move nested bands (Later)",
        "move/move overlapping bands (Earlier)",
        "move/move overlapping bands (Later)",
        "sheet move/sheet move (Earlier)",
        "sheet move/sheet move (Later)",
    ];
    assert_eq!(
        refused, expected,
        "the set of refused pairs changed. A refusal is the honest answer where \
         a wrong one would diverge the replicas silently — but an *undocumented* \
         refusal is a caller discovering it in production. Update \
         `transform`'s \"What is not handled yet\" as well as this list."
    );
}
