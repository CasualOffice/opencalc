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

use casual_calc_formula::stored::Origin;
use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, StyleId, Workbook};

use crate::{
    Operation, apply,
    transform::{Side, transform, transform_with_formulas},
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
    // **Formulas, and the seed used to have none.** `COL-46` — a `$`-anchored
    // reference rebased across a concurrent insert, landing as `$D$1` on one
    // replica and `$E$1` on the other — lived in this crate for as long as it
    // did because the property below could not see a formula: the seed said in
    // as many words that it "deliberately contains no formulas", and `observe`
    // printed `cell.value` and never the tree. A gate blind to the field the
    // defect lives in reports green for ever, which is `CI-018` and `CI-021`
    // over again.
    //
    // Four shapes, because they fail differently. Anchored and relative, since
    // a relative reference is stored against its own cell (`PERF-11`) and so
    // survives a band *outside* the span between it and that cell and diverges
    // for one inside it. A range, whose two ends can be shifted inconsistently.
    // And a cross-sheet pair, which is the only way to catch a rewrite that
    // moves a reference belonging to a sheet the operation is not on.
    for (row, col, text) in [
        (2u32, 2u32, "A1+$B$2"),
        (5, 3, "SUM($A$1:$C$4)"),
        (8, 5, "T!A1*2"),
        (9, 1, "SUM(A1:A8)"),
    ] {
        let handle = workbook.store_formula_at(
            casual_calc_formula::parse(text).expect("a seed formula parses"),
            Origin::at(row, col),
        );
        let mut cell = Cell::value(CellValue::Number(f64::from(row * 10 + col)));
        cell.formula = Some(handle);
        sheet.cells.set(CellRef::new(row, col), cell);
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
    // Pointing *into* the sheet the structural operations run on, from a sheet
    // they do not touch. This is the one shape that tells a rewrite keyed on
    // the wrong sheet from a correct one.
    let handle = workbook.store_formula_at(
        casual_calc_formula::parse("S!$B$2+S!A1").expect("a seed formula parses"),
        Origin::at(3, 0),
    );
    let mut cell = Cell::value(CellValue::Number(2000.0));
    cell.formula = Some(handle);
    other.cells.set(CellRef::new(3, 0), cell);
    workbook.sheets.push(other);
    workbook.defined_names.push(casual_calc_model::DefinedName {
        name: "Anchor".to_owned(),
        sheet: None,
        formula: casual_calc_formula::parse("S!$C$3").expect("a defined name parses"),
    });
    workbook
}

/// The generated operations. Deliberately clustered around the same indices so
/// that overlaps, ties and containments are all hit rather than left to luck.
fn candidates(base: &mut Workbook) -> Vec<Operation> {
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
    // Cell writes that **carry a formula**, interned into `base` so the handle
    // resolves in every clone of it. Placed either side of the indices the
    // bands above cluster on, so a reference and the cell holding it end up on
    // opposite sides of a band as often as on the same side — which is the
    // arrangement that separates a correct rewrite from one that only moves
    // the address.
    for (row, col, text) in [
        (1u32, 1u32, "$D$1"),
        (1, 1, "A1"),
        (4, 2, "SUM($A$1:$C$4)"),
        (7, 4, "B8*T!A1"),
        (2, 0, "$A$1+D9"),
    ] {
        let handle = base.store_formula_at(
            casual_calc_formula::parse(text).expect("a candidate formula parses"),
            Origin::at(row, col),
        );
        let mut cell = Cell::value(CellValue::Number(f64::from(row + col) + 0.25));
        cell.formula = Some(handle);
        ops.push(Operation::SetCell {
            sheet: 0,
            at: CellRef::new(row, col),
            cell: Some(cell),
        });
    }
    // The defined-name table, which the transform treats as positionally inert
    // and `apply` does not: `rewrite_defined_names` moves every name that
    // targets the sheet a band runs on.
    for text in ["S!$C$3", "S!$A$1:$B$9", "T!$A$1"] {
        ops.push(Operation::SetDefinedNames(vec![
            casual_calc_model::DefinedName {
                name: "Anchor".to_owned(),
                sheet: None,
                formula: casual_calc_formula::parse(text).expect("a defined name parses"),
            },
        ]));
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
    let base_meta = crate::SheetMetadata::capture(&base.sheets[0]);
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

/// What each sheet index is called, as [`session`](crate::session) supplies it.
fn sheet_names(workbook: &Workbook) -> Vec<(String, SheetId)> {
    workbook
        .sheets
        .iter()
        .map(|sheet| (sheet.name.clone(), sheet.id))
        .collect()
}

/// Every formula in the workbook, printed as it would be *written* in the cell
/// that holds it.
///
/// Printed rather than compared as a handle: two workbooks intern
/// independently, so equal handles prove nothing and unequal ones mean nothing.
/// And printed **at the holding cell's origin**, because a stored tree is
/// relative to that cell (`PERF-11`) — the same tree in two different cells is
/// two different formulas, which is the whole of what `COL-46` got wrong.
fn formulas_of(workbook: &Workbook) -> String {
    let mut out: Vec<String> = Vec::new();
    for sheet in &workbook.sheets {
        for (at, cell) in sheet.cells.iter() {
            if let Some(handle) = cell.formula
                && let Some(expr) = workbook.formula(handle)
            {
                out.push(format!(
                    "{}!{}:{} => {}",
                    sheet.name,
                    at.row,
                    at.col,
                    casual_calc_formula::print_at(expr, Origin::at(at.row, at.col))
                ));
            }
        }
    }
    out.sort();
    out.join(" | ")
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
                .map(|(at, cell)| {
                    // **The formula, not only the value.** A cell whose tree
                    // says `$D$1` on one replica and `$E$1` on the other has
                    // the same cached `value` on both until something
                    // recalculates, so a comparison of values alone calls two
                    // divergent documents equal — which is exactly how
                    // `COL-46` survived this property test.
                    let formula = cell
                        .formula
                        .and_then(|handle| workbook.formula(handle))
                        .map(|expr| {
                            casual_calc_formula::print_at(expr, Origin::at(at.row, at.col))
                        })
                        .unwrap_or_default();
                    format!("{}:{}={:?}/{formula}", at.row, at.col, cell.value)
                })
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
    // The defined-name table is workbook-level, so it hangs off the end rather
    // than off any one sheet. It is here for the same reason the formulas are:
    // `apply` rewrites every name that targets a sheet a band runs on, and
    // anything the transform does differently is a divergence this test could
    // not previously see.
    let names: Vec<String> = workbook
        .defined_names
        .iter()
        .map(|name| {
            format!(
                "{}={}",
                name.name,
                casual_calc_formula::print_at(&name.formula, Origin::at(0, 0))
            )
        })
        .collect();
    format!("{} || names[{}]", sheets.join(" | "), names.join(","))
}

/// Whether this pair is the one shape `COL-50` accounts for: an insert meeting
/// a delete on the same axis, so that a range endpoint on the band boundary is
/// grown by one order and clamped by the other.
///
/// Deliberately narrow. Anything else that diverges — two deletes, a cell edit,
/// a metadata bundle, an insert against an insert — is not this defect and must
/// fail the property rather than be absorbed by it.
fn one_grows_where_the_other_shrinks(a: &Operation, b: &Operation) -> bool {
    match (sole_band(a), sole_band(b)) {
        (Some((axis_a, insert_a)), Some((axis_b, insert_b))) => {
            axis_a == axis_b && insert_a != insert_b
        }
        _ => false,
    }
}

/// The one structural band an operation performs, as `(is_column, is_insert)`,
/// or `None` when it performs none or more than one.
///
/// Looks inside a batch, because the seed's batch is an insert with a cell
/// write attached and it is the insert that decides the shape.
fn sole_band(op: &Operation) -> Option<(bool, bool)> {
    match op {
        Operation::InsertRows { .. } => Some((false, true)),
        Operation::DeleteRows { .. } => Some((false, false)),
        Operation::InsertColumns { .. } => Some((true, true)),
        Operation::DeleteColumns { .. } => Some((true, false)),
        Operation::Batch(members) => {
            let mut found = None;
            for member in members {
                if let Some(band) = sole_band(member) {
                    if found.is_some() {
                        return None;
                    }
                    found = Some(band);
                }
            }
            found
        }
        _ => None,
    }
}

#[test]
fn tp1_holds_for_every_supported_pair() {
    let mut base = seed();
    // Ops enter the protocol narrowed — see `Operation::narrowed`. A metadata
    // op still declaring ALL would read as contending with every concurrent
    // metadata edit, and the transform has no workbook to work that out from.
    //
    // `candidates` interns its formulas **into `base`**, before the clones are
    // taken, so that every handle an operation carries resolves in the replica
    // it is applied to.
    let ops: Vec<Operation> = candidates(&mut base)
        .into_iter()
        .map(|op| op.narrowed(&base))
        .collect();
    // What the sheet indices name, from the state both replicas share. Not
    // from either replica's own workbook: the two must reach the same answer
    // about a qualified reference, and the shared fact is the base.
    let sheets = sheet_names(&base);
    let mut checked = 0usize;
    let mut skipped = 0usize;
    let mut diverged = 0usize;

    for a in &ops {
        for b in &ops {
            // The two replicas must agree on *one* order before they can
            // agree on a result, so fix it here: a is ordered before b. Each
            // side then transforms with the role that order gives it.
            //
            // Each side rebases against **its own** workbook, which is what
            // `session` does: rewriting a carried formula produces a new tree,
            // and the only place a handle for it can exist is the workbook the
            // rebased operation is about to be applied to.
            let mut left = base.clone();
            apply(&mut left, a.clone()).expect("a applies to the seed");
            let Ok(b_after_a) = transform_with_formulas(b, a, Side::Later, &sheets, &mut left)
            else {
                skipped += 1;
                continue;
            };

            let mut right = base.clone();
            apply(&mut right, b.clone()).expect("b applies to the seed");
            let Ok(a_after_b) = transform_with_formulas(a, b, Side::Earlier, &sheets, &mut right)
            else {
                skipped += 1;
                continue;
            };

            apply(&mut left, b_after_a.clone()).expect("b' applies after a");
            apply(&mut right, a_after_b.clone()).expect("a' applies after b");

            if observe(&left) != observe(&right) {
                assert!(
                    one_grows_where_the_other_shrinks(a, b),
                    "TP1 violated, and outside the one shape `COL-50` accounts \
                     for.\n  a  = {a:?}\n  b  = {b:?}\n  b' = {b_after_a:?}\n  \
                     a' = {a_after_b:?}\n  left  = {}\n  right = {}",
                    observe(&left),
                    observe(&right)
                );
                diverged += 1;
            }
            checked += 1;
        }
    }

    // A guard against the test passing because everything was skipped.
    // Concurrent sheet reordering is the one refused corner, so the skips are
    // expected here — but they are counted, not waved through, and the pairs
    // that *are* answered still have to outnumber them heavily.
    assert!(
        skipped * 8 < checked,
        "{skipped} refused against {checked} answered — too much is being skipped"
    );
    assert!(checked > 3_000, "only {checked} pairs checked");
    // **`COL-50`: the hole this property cannot close, pinned rather than
    // hidden.** With formulas in the seed, sixty-eight pairs still diverge, and
    // every one of them is an insert meeting a delete on the same axis with a
    // *range* endpoint on the band boundary. Removing the ranges from the seed
    // — and nothing else — takes the count to zero, which is how the shape was
    // established rather than guessed.
    //
    // It is not a missing transform arm and no choice of `a'`/`b'` fixes it.
    // The cells converge; the ranges do not, because `apply` grows a range an
    // insert lands inside and clamps one a delete overlaps, and those two rules
    // do not commute. `=SUM(A1:A8)` with a concurrent insert and delete at row
    // 8 settles as `A1:A8` in one order and `A1:A7` in the other, and both
    // answers are the one Excel gives for the sequence that produced them.
    // Closing it means deciding a range semantics that commutes, which is a
    // design note and not an edit here.
    //
    // The count is asserted **exactly** and the shape is asserted per pair, so
    // this records the hole without widening it: a divergence of any other
    // shape fails in the loop above, and one more of this shape fails here.
    assert_eq!(
        diverged, 68,
        "the known-divergent set changed; a pair either started or stopped \
         diverging and neither is a thing to accept quietly"
    );
    println!("TP1: {checked} pairs checked, {skipped} refused, {diverged} diverged (COL-50)");
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
        ..ChartSeries::default()
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

/// **`COL-46`.** A `SetCell` carrying a formula, rebased across a concurrent
/// insert or delete, must carry a formula that has been rewritten by the same
/// band — not the one it was written with.
///
/// Before the fix `rebase_onto_band` shifted the cell *address* and carried the
/// tree verbatim, so the two replicas ended up holding different formulas and
/// nothing anywhere said so. Divergence that looks like success.
///
/// The `A1` rows matter as much as the `$D$1` ones. The defect was reported as
/// an anchored-reference problem — relative references "survive because
/// `PERF-11` stores them relative to their own cell" — and that is only true
/// while the band lies **outside** the span between the reference and the cell
/// holding it. `A1` in `B2` across an `InsertColumns{at:1}` is inside it, and
/// diverges exactly as loudly.
#[test]
fn a_carried_formula_is_rebased_by_the_band_it_crosses() {
    for (formula, band) in [
        (
            "$D$1",
            Operation::InsertColumns {
                sheet: 0,
                at: 0,
                count: 1,
            },
        ),
        (
            "A1",
            Operation::InsertColumns {
                sheet: 0,
                at: 1,
                count: 1,
            },
        ),
        (
            "A1",
            Operation::InsertColumns {
                sheet: 0,
                at: 0,
                count: 1,
            },
        ),
        (
            "$D$1:$E$2",
            Operation::InsertColumns {
                sheet: 0,
                at: 0,
                count: 2,
            },
        ),
        (
            "SUM(A1:C1)",
            Operation::InsertColumns {
                sheet: 0,
                at: 1,
                count: 1,
            },
        ),
        (
            "$D$1",
            Operation::DeleteColumns {
                sheet: 0,
                at: 0,
                count: 1,
            },
        ),
        (
            "$A$1",
            Operation::InsertRows {
                sheet: 0,
                at: 0,
                count: 3,
            },
        ),
        (
            "A1",
            Operation::DeleteRows {
                sheet: 0,
                at: 0,
                count: 1,
            },
        ),
        (
            "S!$D$1",
            Operation::InsertColumns {
                sheet: 0,
                at: 0,
                count: 1,
            },
        ),
        (
            "T!$D$1",
            Operation::InsertColumns {
                sheet: 0,
                at: 0,
                count: 1,
            },
        ),
    ] {
        let (left, right) = diamond(formula, &band);
        assert_eq!(
            left, right,
            "`={formula}` diverged across a concurrent {band:?}"
        );
    }
}

/// The one case a `$`-anchored formula makes obvious, spelled out on its own so
/// the failure names the numbers in the `COL-46` row rather than a loop index.
#[test]
fn col46_the_reported_case() {
    let (left, right) = diamond(
        "$D$1",
        &Operation::InsertColumns {
            sheet: 0,
            at: 0,
            count: 1,
        },
    );
    // The cell the row reports on, pulled out of the whole picture so the
    // failure reads as the two formulas rather than as two paragraphs.
    let at_b2 = |replica: &str| {
        replica
            .split(" | ")
            .find(|entry| entry.starts_with("S!1:2 "))
            .expect("the edited cell is there")
            .to_owned()
    };
    // `$E$1` on both sides. Before the fix the right-hand replica held `$D$1`
    // — the formula as written, against a sheet that had gained a column.
    assert_eq!(at_b2(&left), "S!1:2 => $E$1", "a then b'");
    assert_eq!(at_b2(&right), "S!1:2 => $E$1", "b then a'");
    assert_eq!(left, right, "and the rest of the sheet agrees too");
}

/// **Different sheets do not interact — except through a formula.**
///
/// `S!$D$1` sitting on sheet `T` names a column of `S`, and an insert on `S`
/// moves it: `apply` rewrites every formula in the *workbook* that targets the
/// banded sheet, not only the ones living on it. `transform` returned such an
/// edit untouched, on the rule that operations on different sheets never
/// interact — true of every position and false of this one reference. Its
/// address genuinely does not move; its formula does.
#[test]
fn a_formula_naming_another_sheet_follows_that_sheets_insert() {
    use crate::transform::transform_with_formulas;

    let mut base = seed();
    let handle = base.store_formula_at(
        casual_calc_formula::parse("S!$D$1").unwrap(),
        Origin::at(1, 0),
    );
    let mut cell = Cell::value(CellValue::Number(0.0));
    cell.formula = Some(handle);
    // On sheet 1 — `T` — where the insert below does not reach.
    let edit = Operation::SetCell {
        sheet: 1,
        at: CellRef::new(1, 0),
        cell: Some(cell),
    };
    let band = Operation::InsertColumns {
        sheet: 0,
        at: 0,
        count: 1,
    };
    let sheets = sheet_names(&base);

    let mut left = base.clone();
    apply(&mut left, edit.clone()).expect("the edit applies to the seed");
    let rebased_band = transform_with_formulas(&band, &edit, Side::Later, &sheets, &mut left)
        .expect("band rebases");
    apply(&mut left, rebased_band).expect("the band applies after the edit");

    let mut right = base;
    apply(&mut right, band.clone()).expect("the band applies to the seed");
    let rebased_edit = transform_with_formulas(&edit, &band, Side::Earlier, &sheets, &mut right)
        .expect("edit rebases");
    apply(&mut right, rebased_edit).expect("the edit applies after the band");

    let at_t = |replica: &str| {
        replica
            .split(" | ")
            .find(|entry| entry.starts_with("T!1:0 "))
            .expect("the edited cell is there")
            .to_owned()
    };
    assert_eq!(formulas_of(&left), formulas_of(&right), "the two replicas");
    assert_eq!(
        at_t(&formulas_of(&left)),
        "T!1:0 => S!$E$1",
        "the reference into `S` follows `S`'s insert, and the cell stays on `T`"
    );
}

/// Run one convergence diamond for a formula written into `B2`, and report what
/// each replica ended up holding.
///
/// The formula table is the replica's **own** workbook, which is what
/// [`session`](crate::session) passes: a rewritten tree has to be interned
/// somewhere before a handle for it exists, and the only workbook that can
/// resolve the handle is the one the operation is about to be applied to.
fn diamond(formula: &str, band: &Operation) -> (String, String) {
    use crate::transform::transform_with_formulas;

    let mut base = seed();
    let handle = base.store_formula_at(
        casual_calc_formula::parse(formula).unwrap(),
        Origin::at(1, 1),
    );
    let mut cell = Cell::value(CellValue::Number(0.0));
    cell.formula = Some(handle);
    let edit = Operation::SetCell {
        sheet: 0,
        at: CellRef::new(1, 1),
        cell: Some(cell),
    };
    let sheets = sheet_names(&base);

    let mut left = base.clone();
    apply(&mut left, edit.clone()).expect("the edit applies to the seed");
    let rebased_band = transform_with_formulas(band, &edit, Side::Later, &sheets, &mut left)
        .expect("band rebases");
    apply(&mut left, rebased_band).expect("the band applies after the edit");

    let mut right = base;
    apply(&mut right, band.clone()).expect("the band applies to the seed");
    let rebased_edit = transform_with_formulas(&edit, band, Side::Earlier, &sheets, &mut right)
        .expect("edit rebases");
    apply(&mut right, rebased_edit).expect("the edit applies after the band");

    (formulas_of(&left), formulas_of(&right))
}
