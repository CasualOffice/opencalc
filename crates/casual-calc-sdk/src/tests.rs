//! SDK facade tests: the full open/edit/recalc/render/save lifecycle.

use casual_calc_model::{CellRef, CellValue, Id, Sheet, SheetId};

use crate::{EditOperation, GridViewport, WorkbookSession};

/// Build a session with A1 = 10 and A2 = A1*2 (a formula cell).
fn session_with_formula() -> WorkbookSession {
    let mut session = WorkbookSession::blank();
    let wb = session.workbook_mut();
    let mut sheet = Sheet::new(SheetId(Id::from_parts(9, 1)), "Sheet1");
    sheet.cells.set(
        CellRef::new(0, 0),
        casual_calc_model::Cell::value(CellValue::Number(10.0)),
    );
    let handle = wb.store_formula(casual_calc_formula::parse("A1*2").unwrap());
    let mut a2 = casual_calc_model::Cell::value(CellValue::Empty);
    a2.formula = Some(handle);
    sheet.cells.set(CellRef::new(1, 0), a2);
    wb.sheets.push(sheet);
    session.recalculate();
    session
}

fn value(session: &WorkbookSession, at: CellRef) -> CellValue {
    session.workbook().sheets[0]
        .cells
        .get(at)
        .map(|c| c.value.clone())
        .unwrap_or(CellValue::Empty)
}

#[test]
fn edit_recalculates_and_undo_redo_works() {
    let mut session = session_with_formula();
    assert_eq!(value(&session, CellRef::new(1, 0)), CellValue::Number(20.0));

    // Edit A1 to 30 → A2 recomputes to 60.
    session
        .edit(EditOperation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(30.0),
        })
        .unwrap();
    assert_eq!(value(&session, CellRef::new(1, 0)), CellValue::Number(60.0));
    assert!(session.can_undo());

    session.undo().unwrap();
    assert_eq!(value(&session, CellRef::new(0, 0)), CellValue::Number(10.0));
    assert_eq!(value(&session, CellRef::new(1, 0)), CellValue::Number(20.0));
    assert!(session.can_redo());

    session.redo().unwrap();
    assert_eq!(value(&session, CellRef::new(1, 0)), CellValue::Number(60.0));
}

#[test]
fn save_then_open_round_trips_and_recalculates() {
    let session = session_with_formula();
    let bytes = session.save().unwrap();

    let reopened = WorkbookSession::open(bytes).unwrap();
    // The formula recomputes on open.
    assert_eq!(
        value(&reopened, CellRef::new(1, 0)),
        CellValue::Number(20.0)
    );
    assert_eq!(reopened.workbook().sheets[0].name, "Sheet1");
}

#[test]
fn render_produces_a_png() {
    let session = session_with_formula();
    let viewport = GridViewport {
        x: 0,
        y: 0,
        width: 4 * 960,
        height: 4 * 300,
    };
    let png = session.render_png(0, &viewport, 96).unwrap();
    assert_eq!(&png[0..4], &[0x89, b'P', b'N', b'G']);
}

#[test]
fn render_honours_the_sheets_frozen_panes() {
    // The wiring, not the arithmetic — the split and the composition are gated
    // in their own crates. What this asserts is that the sheet's own freeze is
    // consulted at all, which for a long time it was not: `render_png` laid out
    // one unbroken window, so a pinned header scrolled off an exported image
    // while holding still in the editor.
    use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

    let scrolled = GridViewport {
        x: 4 * 960,
        y: 8 * 300,
        width: 6 * 960,
        height: 12 * 300,
    };

    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    for row in 0..30u32 {
        for col in 0..10u32 {
            sheet.cells.set(
                CellRef::new(row, col),
                Cell::value(CellValue::Number(f64::from(row * 100 + col))),
            );
        }
    }
    workbook.sheets.push(sheet);

    let mut session = WorkbookSession::from_workbook(workbook);
    let unfrozen = session.render_png(0, &scrolled, 96).unwrap();

    session.workbook_mut().sheets[0].view.frozen_rows = 2;
    session.workbook_mut().sheets[0].view.frozen_cols = 1;
    let frozen = session.render_png(0, &scrolled, 96).unwrap();

    assert_ne!(
        unfrozen, frozen,
        "freezing rows and columns changes what a scrolled render shows"
    );

    // Which count went to which axis, pinned against the composition done by
    // hand. An asymmetric freeze is essential here: with two rows and one
    // column, reading the two fields the wrong way round still produces a
    // frozen-looking render, and only a comparison that names the axes catches
    // it.
    let geometry = session.geometry(0);
    let regions = casual_calc_layout::panes(
        &geometry,
        &scrolled,
        casual_calc_layout::Freeze { rows: 2, cols: 1 },
    );
    let lists: Vec<_> = regions
        .iter()
        .map(|pane| {
            casual_calc_layout::layout_viewport(session.workbook(), 0, &geometry, &pane.viewport)
        })
        .collect();
    let paints: Vec<_> = regions
        .iter()
        .zip(&lists)
        .map(|(pane, display_list)| casual_calc_render::PanePaint {
            pane: *pane,
            display_list,
        })
        .collect();
    assert_eq!(
        frozen,
        casual_calc_render::render_panes_png(&paints, &geometry, &scrolled, 96).unwrap(),
        "two frozen rows and one frozen column, not the other way about"
    );

    // And with nothing frozen the bytes are what they always were, so the new
    // path is not a new rendering for every existing sheet.
    session.workbook_mut().sheets[0].view.frozen_rows = 0;
    session.workbook_mut().sheets[0].view.frozen_cols = 0;
    assert_eq!(
        unfrozen,
        session.render_png(0, &scrolled, 96).unwrap(),
        "unfreezing restores the original render exactly"
    );
}

#[test]
fn blank_session_saves_and_reopens_empty() {
    let session = WorkbookSession::blank();
    let bytes = session.save().unwrap();
    let reopened = WorkbookSession::open(bytes).unwrap();
    assert!(reopened.workbook().sheets.is_empty());
}

// --- Configuration ---------------------------------------------------------

use crate::{CalculationMode, Environment, SessionConfig};

/// A session in manual mode over the same A1 / A2=A1*2 sheet.
fn manual_session() -> WorkbookSession {
    let mut session = session_with_formula();
    session.set_calculation_mode(CalculationMode::Manual);
    session
}

#[test]
fn manual_mode_defers_the_calculation_but_not_the_edit() {
    let mut session = manual_session();
    assert!(!session.needs_recalculation());

    session
        .edit(EditOperation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(50.0),
        })
        .unwrap();

    // The edit landed — it is calculation that is deferred, not editing.
    assert_eq!(value(&session, CellRef::new(0, 0)), CellValue::Number(50.0));
    // ...and the formula still shows the answer its author last saw.
    assert_eq!(value(&session, CellRef::new(1, 0)), CellValue::Number(20.0));
    assert!(
        session.needs_recalculation(),
        "the host has to be able to say so, the way Excel says Calculate"
    );

    session.recalculate();
    assert_eq!(
        value(&session, CellRef::new(1, 0)),
        CellValue::Number(100.0)
    );
    assert!(!session.needs_recalculation());
}

#[test]
fn a_pure_formatting_edit_does_not_make_a_manual_workbook_stale() {
    // Nothing to recalculate means nothing outstanding: telling the user to
    // press Calculate after changing a fill would train them to ignore it.
    let mut session = manual_session();
    session
        .edit(EditOperation::SetColumnWidth {
            sheet: 0,
            col: 0,
            width: Some(3000),
        })
        .unwrap();
    assert!(!session.needs_recalculation());
}

#[test]
fn switching_back_to_automatic_catches_up_at_once() {
    let mut session = manual_session();
    session
        .edit(EditOperation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(7.0),
        })
        .unwrap();
    assert_eq!(value(&session, CellRef::new(1, 0)), CellValue::Number(20.0));

    session.set_calculation_mode(CalculationMode::Automatic);
    // The point of the mode is that values are current, so switching to it
    // while something is outstanding has to settle the arrears.
    assert_eq!(value(&session, CellRef::new(1, 0)), CellValue::Number(14.0));
    assert!(!session.needs_recalculation());
}

#[test]
fn the_mode_is_written_back_so_it_survives_a_save() {
    // Turning calculation off and saving must not produce a file that turns
    // itself back on: the reason it was turned off does not go away when the
    // file closes.
    let mut session = session_with_formula();
    assert!(!session.workbook().settings.calc.contains_key("calcMode"));

    session.set_calculation_mode(CalculationMode::Manual);
    assert_eq!(
        session
            .workbook()
            .settings
            .calc
            .get("calcMode")
            .map(String::as_str),
        Some("manual")
    );

    // `auto` is the schema default, so going back writes it by omission rather
    // than leaving a difference in a file nobody changed.
    session.set_calculation_mode(CalculationMode::Automatic);
    assert!(!session.workbook().settings.calc.contains_key("calcMode"));
}

#[test]
fn a_file_saved_in_manual_mode_opens_in_manual_mode_and_is_not_recalculated() {
    let mut session = session_with_formula();
    session.set_calculation_mode(CalculationMode::Manual);
    // A stale cached value, as a manual-mode file legitimately carries: this is
    // what its author last saw, and opening must not silently replace it.
    let mut stale = session.workbook().sheets[0]
        .cells
        .get(CellRef::new(1, 0))
        .unwrap()
        .clone();
    stale.value = CellValue::Number(999.0);
    session.workbook_mut().sheets[0]
        .cells
        .set(CellRef::new(1, 0), stale);
    let bytes = session.save().unwrap();

    let reopened = WorkbookSession::open(bytes).unwrap();
    assert_eq!(reopened.calculation_mode(), CalculationMode::Manual);
    assert_eq!(
        value(&reopened, CellRef::new(1, 0)),
        CellValue::Number(999.0),
        "opening a manual workbook must not recalculate it"
    );

    // ...unless the host insists, which is what a headless renderer wants.
    let bytes = reopened.save().unwrap();
    let forced = WorkbookSession::open_with(
        bytes,
        SessionConfig::new().with_calculation(CalculationMode::Automatic),
    )
    .unwrap();
    assert_eq!(value(&forced, CellRef::new(1, 0)), CellValue::Number(20.0));
}

#[test]
fn undo_depth_bounds_what_the_history_keeps() {
    let mut session = WorkbookSession::blank_with(SessionConfig::new().with_undo_depth(2));
    session
        .workbook_mut()
        .sheets
        .push(Sheet::new(SheetId(Id::from_parts(9, 1)), "Sheet1"));
    for n in 0..5 {
        session
            .edit(EditOperation::SetValue {
                sheet: 0,
                at: CellRef::new(n, 0),
                value: CellValue::Number(f64::from(n)),
            })
            .unwrap();
    }
    // Two steps back, then nothing: the oldest entries were dropped, which is
    // the point of the bound.
    assert!(session.undo().is_ok());
    assert!(session.undo().is_ok());
    assert!(!session.can_undo());
    // ...and the edits the history forgot are still on the sheet. A bounded
    // history forgets how to reverse an edit; it never reverses one by itself.
    assert_eq!(value(&session, CellRef::new(0, 0)), CellValue::Number(0.0));
    assert_eq!(value(&session, CellRef::new(2, 0)), CellValue::Number(2.0));
}

#[test]
fn the_environment_is_supplied_rather_than_sampled() {
    let mut session =
        WorkbookSession::blank_with(SessionConfig::new().with_environment(Environment {
            now: 45_000.0,
            seed: 7,
        }));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(9, 1)), "Sheet1");
    let handle = session
        .workbook_mut()
        .store_formula(casual_calc_formula::parse("TODAY()").unwrap());
    let mut cell = casual_calc_model::Cell::value(CellValue::Empty);
    cell.formula = Some(handle);
    sheet.cells.set(CellRef::new(0, 0), cell);
    session.workbook_mut().sheets.push(sheet);
    session.recalculate();
    assert_eq!(
        value(&session, CellRef::new(0, 0)),
        CellValue::Number(45_000.0)
    );

    // Moving the clock moves the answer, and does so at once in automatic mode
    // — a stale NOW() beside a clock that has visibly advanced is worse than
    // the cost of the pass.
    session.set_environment(Environment {
        now: 45_100.0,
        seed: 7,
    });
    assert_eq!(
        value(&session, CellRef::new(0, 0)),
        CellValue::Number(45_100.0)
    );
}

#[test]
fn a_read_only_session_refuses_every_write_path() {
    // Not "hides the toolbar": a read-only mode enforced only in the UI is
    // read-only until someone calls the API.
    let mut session = WorkbookSession::blank_with(SessionConfig::new().read_only());
    session
        .workbook_mut()
        .sheets
        .push(Sheet::new(SheetId(Id::from_parts(9, 1)), "Sheet1"));

    let write = || EditOperation::SetValue {
        sheet: 0,
        at: CellRef::new(0, 0),
        value: CellValue::Number(1.0),
    };
    assert!(matches!(
        session.edit(write()),
        Err(crate::SdkError::ReadOnly)
    ));
    // ...including the path that bypasses undo, which is exactly where a
    // documented bypass would undo the whole mode.
    assert!(matches!(
        session.apply_raw(write()),
        Err(crate::SdkError::ReadOnly)
    ));
    assert!(session.is_read_only());
    // Nothing was written, and nothing was recorded to undo.
    assert_eq!(value(&session, CellRef::new(0, 0)), CellValue::Empty);
    assert!(!session.can_undo());
}

#[test]
fn a_read_only_session_still_reads_recalculates_and_saves() {
    // A viewer that cannot compute or export is not a viewer, it is a picture.
    let mut session = session_with_formula();
    session.config_mut().read_only = true;
    assert_eq!(value(&session, CellRef::new(1, 0)), CellValue::Number(20.0));
    session.recalculate();
    assert_eq!(value(&session, CellRef::new(1, 0)), CellValue::Number(20.0));
    assert!(session.save().is_ok());
}

/// Byte-identical repackaging: an unedited file saves as itself (P1B-002).
mod untouched_saves {
    use super::*;

    /// A package with something the engine does not model, so the test is about
    /// the whole file rather than the part of it we happen to rebuild well.
    fn source_package() -> Vec<u8> {
        let mut wb = casual_calc_model::Workbook::new(casual_calc_model::Id::from_parts(1, 1));
        let mut sheet = casual_calc_model::Sheet::new(
            casual_calc_model::SheetId(casual_calc_model::Id::from_parts(2, 1)),
            "S",
        );
        sheet.cells.set(
            casual_calc_model::CellRef::new(0, 0),
            casual_calc_model::Cell::value(casual_calc_model::CellValue::Number(1.0)),
        );
        wb.sheets.push(sheet);
        WorkbookSession::from_workbook(wb).save().unwrap()
    }

    #[test]
    fn opening_and_saving_without_editing_returns_the_same_bytes() {
        // The guarantee itself. Reconstructing canonical OOXML instead would
        // rewrite a file the user only looked at.
        let original = source_package();
        let session = WorkbookSession::open(original.clone()).unwrap();
        assert!(session.is_unmodified());
        assert_eq!(
            session.save().unwrap(),
            original,
            "an untouched workbook saves as the file it was opened from"
        );
    }

    #[test]
    fn an_edit_ends_the_guarantee_and_the_semantic_writer_takes_over() {
        let original = source_package();
        let mut session = WorkbookSession::open(original.clone()).unwrap();
        session
            .edit(EditOperation::SetCell {
                sheet: 0,
                at: CellRef::new(5, 5),
                cell: Some(casual_calc_model::Cell::value(CellValue::Number(9.0))),
            })
            .unwrap();

        assert!(!session.is_unmodified());
        let saved = session.save().unwrap();
        assert_ne!(saved, original, "the edit is in the file");
        // And it is still a real package.
        let reopened = WorkbookSession::open(saved).unwrap();
        assert_eq!(
            reopened.workbook().sheets[0]
                .cells
                .get(CellRef::new(5, 5))
                .map(|c| c.value.clone()),
            Some(CellValue::Number(9.0))
        );
    }

    #[test]
    fn undo_back_to_the_start_does_not_restore_the_guarantee() {
        // Deliberate. Undoing to the opening state leaves a workbook that is
        // *equal* to the one opened, but this cannot prove the package would be
        // too — and the failure mode of guessing wrong is handing back a file
        // that silently is not what the session holds.
        let original = source_package();
        let mut session = WorkbookSession::open(original.clone()).unwrap();
        session
            .edit(EditOperation::SetCell {
                sheet: 0,
                at: CellRef::new(5, 5),
                cell: Some(casual_calc_model::Cell::value(CellValue::Number(9.0))),
            })
            .unwrap();
        session.undo().unwrap();
        assert!(!session.is_unmodified());
    }

    #[test]
    fn reaching_for_the_workbook_directly_ends_it_too() {
        // `workbook_mut` hands out the right to change anything and the session
        // cannot see what happens next, so the guarantee ends at the call
        // whether or not the caller writes a byte.
        let original = source_package();
        let mut session = WorkbookSession::open(original).unwrap();
        assert!(session.is_unmodified());
        let _ = session.workbook_mut();
        assert!(!session.is_unmodified());
    }

    #[test]
    fn a_package_this_engine_did_not_write_comes_back_exactly() {
        // The one that matters. A file we wrote ourselves proves little — the
        // guarantee is for the file somebody else made, carrying parts this
        // engine does not model and would rebuild differently if it tried.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/generated/minimal.xlsx");
        let original = std::fs::read(&path).expect("the committed fixture");
        let session = WorkbookSession::open(original.clone()).unwrap();
        assert!(session.is_unmodified());
        assert_eq!(
            session.save().unwrap(),
            original,
            "a foreign package saves as itself"
        );
    }

    #[test]
    fn a_session_that_was_never_opened_from_a_package_makes_no_such_claim() {
        let session = WorkbookSession::blank();
        assert!(!session.is_unmodified());
        // And it still saves.
        assert_eq!(&session.save().unwrap()[0..2], b"PK");
    }

    #[test]
    fn a_read_only_session_keeps_the_guarantee_through_a_refused_edit() {
        // A refused edit must leave no trace — the same rule the history keeps.
        let original = source_package();
        let mut session =
            WorkbookSession::open_with(original.clone(), SessionConfig::new().read_only()).unwrap();
        assert!(
            session
                .edit(EditOperation::SetCell {
                    sheet: 0,
                    at: CellRef::new(0, 0),
                    cell: None,
                })
                .is_err()
        );
        assert!(
            session.is_unmodified(),
            "nothing happened, so nothing changed"
        );
        assert_eq!(session.save().unwrap(), original);
    }
}

/// **The invalidation discipline that makes a kept precedent graph safe.**
///
/// Step three of `docs/66` keeps the graph across edits, which is only correct
/// because the reference-shifting operations drop it. Nothing in the eval crate
/// can test that: the obligation lives here, in the session that decides which
/// edits are structural, so this is where a future change that forgets it has to
/// be caught.
///
/// Inserting a row moves `A1` to `A2` and rewrites the formula that reads it. A
/// graph that survived the insertion still believes the old row numbers, so the
/// next value edit dirties nothing — no error, no panic, just a formula sitting
/// at its previous answer. That is the whole failure mode, and it is the reason
/// this asserts a recomputed *number* rather than that some method was called.
#[test]
fn a_structural_edit_does_not_leave_a_stale_precedent_graph() {
    let mut session = session_with_formula();
    assert_eq!(value(&session, CellRef::new(1, 0)), CellValue::Number(20.0));

    // A value edit first, so a graph exists to go stale.
    session
        .edit(EditOperation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(5.0),
        })
        .unwrap();
    assert_eq!(value(&session, CellRef::new(1, 0)), CellValue::Number(10.0));

    // Everything shifts down one: A1 -> A2, and the formula moves to A3 with its
    // reference rewritten to A2.
    session
        .edit(EditOperation::InsertRows {
            sheet: 0,
            at: 0,
            count: 1,
        })
        .unwrap();
    assert_eq!(value(&session, CellRef::new(2, 0)), CellValue::Number(10.0));

    // The edit that exposes a graph describing the document as it was.
    session
        .edit(EditOperation::SetValue {
            sheet: 0,
            at: CellRef::new(1, 0),
            value: CellValue::Number(7.0),
        })
        .unwrap();
    assert_eq!(
        value(&session, CellRef::new(2, 0)),
        CellValue::Number(14.0),
        "the formula must follow its precedent to the row it moved to"
    );
}

/// Undo replays whichever kind of edit it reverses and does not say which, so
/// it drops the graph too — asserted by undoing a structural edit and then
/// editing a cell whose address the undo moved.
#[test]
fn undoing_a_structural_edit_does_not_leave_a_stale_precedent_graph() {
    let mut session = session_with_formula();
    session
        .edit(EditOperation::InsertRows {
            sheet: 0,
            at: 0,
            count: 1,
        })
        .unwrap();
    session
        .edit(EditOperation::SetValue {
            sheet: 0,
            at: CellRef::new(1, 0),
            value: CellValue::Number(3.0),
        })
        .unwrap();
    assert_eq!(value(&session, CellRef::new(2, 0)), CellValue::Number(6.0));

    // Undo the value edit, then the insertion: A1 is A1 again.
    session.undo().unwrap();
    session.undo().unwrap();

    session
        .edit(EditOperation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(9.0),
        })
        .unwrap();
    assert_eq!(
        value(&session, CellRef::new(1, 0)),
        CellValue::Number(18.0),
        "after undo the graph describes the document undo produced"
    );
}

/// A document in a script with no registered face must be able to *say so*.
///
/// The renderer's behaviour is unchanged and correct — it draws what it has.
/// What was missing is the sentence: fonts are supplied by the host, so a sheet
/// of Arabic renders as boxes, and a box is indistinguishable from a rendering
/// bug unless something names the cause.
#[test]
fn a_document_reports_the_scripts_it_cannot_be_drawn_in() {
    let mut session = WorkbookSession::blank();
    let wb = session.workbook_mut();
    let mut sheet = Sheet::new(SheetId(Id::from_parts(9, 2)), "Sheet1");
    let latin = wb.intern_string("Total");
    sheet.cells.set(
        CellRef::new(0, 0),
        casual_calc_model::Cell::value(CellValue::InlineString(latin)),
    );
    wb.sheets.push(sheet);

    assert!(
        session.missing_font_coverage().is_empty(),
        "Latin is bundled, so an ordinary document reports nothing"
    );

    // U+0E44 THAI CHARACTER SARA AI MAIMALAI — no bundled family covers Thai.
    let thai = session.workbook_mut().intern_string("ไทย");
    session.workbook_mut().sheets[0].cells.set(
        CellRef::new(1, 0),
        casual_calc_model::Cell::value(CellValue::InlineString(thai)),
    );

    let missing = session.missing_font_coverage();
    assert_eq!(
        missing.len(),
        1,
        "one script, named once, not one entry per character: {missing:?}"
    );
    assert_eq!(missing[0].script, "Thai");
}

/// **A refused edit must leave no trace — including in what peers are told.**
///
/// `edit` narrows an operation against the pre-edit workbook, which it has to:
/// afterwards the state it was written against is gone. It then used to *record*
/// it in the same breath, before `History::apply` had said whether the edit was
/// possible. A refused operation was therefore already in the outgoing log, and
/// the next flush sent the server — and through it every peer — an edit this
/// client had rejected.
///
/// Nothing downstream can catch that. The operation is well formed and applies
/// cleanly everywhere else; it simply is not what happened here. So the
/// assertion is local: after a refusal, every observable is untouched.
#[test]
fn a_refused_edit_is_not_queued_for_collaborators() {
    // One operation per class that reaches a different arm of `apply`, each
    // naming a sheet that does not exist so it fails for the same honest reason.
    let refusals: Vec<(&str, EditOperation)> = vec![
        (
            "a value",
            EditOperation::SetValue {
                sheet: 99,
                at: CellRef::new(0, 0),
                value: CellValue::Number(1.0),
            },
        ),
        (
            "a style",
            EditOperation::SetStyle {
                sheet: 99,
                at: CellRef::new(0, 0),
                style: None,
            },
        ),
        (
            "a column width",
            EditOperation::SetColumnWidth {
                sheet: 99,
                col: 0,
                width: Some(120),
            },
        ),
        (
            "a structural edit",
            EditOperation::InsertRows {
                sheet: 99,
                at: 0,
                count: 1,
            },
        ),
        (
            "a batch",
            EditOperation::Batch(vec![EditOperation::ClearCell {
                sheet: 99,
                at: CellRef::new(0, 0),
            }]),
        ),
    ];

    for (what, op) in refusals {
        let mut session = session_with_formula();
        session.record_applied();
        // A real edit first, so the log and the history are non-empty and a
        // leaked entry has to be distinguished from an empty one.
        session
            .edit(EditOperation::SetValue {
                sheet: 0,
                at: CellRef::new(0, 0),
                value: CellValue::Number(4.0),
            })
            .expect("the honest edit lands");
        let before_save = session.save().expect("saves");
        let before_value = value(&session, CellRef::new(1, 0));

        assert!(session.edit(op).is_err(), "{what}: should be refused");

        let queued = session.take_applied();
        assert_eq!(
            queued.len(),
            1,
            "{what}: only the edit that actually applied may be sent, got {queued:?}"
        );
        assert_eq!(
            value(&session, CellRef::new(1, 0)),
            before_value,
            "{what}: the workbook is unchanged"
        );
        assert!(
            session.can_undo(),
            "{what}: the successful edit is still undoable"
        );
        session.undo().expect("undo");
        assert!(
            !session.can_undo(),
            "{what}: the refusal did not add a history step"
        );
        session.redo().expect("redo");
        assert_eq!(
            session.save().expect("saves"),
            before_save,
            "{what}: the saved bytes are unchanged"
        );
    }
}

/// A read-only session refuses `apply_raw` **before** spending the untouched
/// source, rather than after.
///
/// Asserted against a **LibreOffice-written** file, not one this crate produced.
/// Re-serialising our own output can come out byte-identical, so a session that
/// had thrown the original away would still have passed — the first version of
/// this test did exactly that, and only failed to fail.
#[test]
fn a_read_only_raw_apply_keeps_the_untouched_original() {
    let original = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/corpus/libreoffice-formulas.xlsx"),
    )
    .expect("the LibreOffice fixture is present");
    let mut session =
        WorkbookSession::open_with(original.clone(), SessionConfig::new().read_only()).unwrap();

    assert!(
        session
            .apply_raw(EditOperation::SetValue {
                sheet: 0,
                at: CellRef::new(0, 0),
                value: CellValue::Number(1.0),
            })
            .is_err(),
        "a read-only session refuses"
    );
    assert_eq!(
        session.save().expect("saves"),
        original,
        "and the file it refused to change is still byte-identical"
    );
}

/// **An undo is an edit, and collaborators have to be told.**
///
/// Undo mutated history and the workbook and stopped there: nothing entered the
/// outgoing log, so the author reverted while the server and every peer kept the
/// change. That divergence never heals — no later operation contradicts it, so
/// nothing ever notices, and the two documents are simply different from then
/// on.
///
/// Asserted as convergence rather than as "a message was sent": the second
/// session replays what the first emitted and must end up holding the same
/// values, which is the property that actually matters.
#[test]
fn an_undo_reaches_the_other_participant_and_both_converge() {
    let mut author = session_with_formula();
    author.record_applied();

    // A peer opening the same document.
    let mut peer = WorkbookSession::from_workbook(author.workbook().clone());

    let replay = |peer: &mut WorkbookSession, ops: Vec<EditOperation>| {
        for op in ops {
            peer.edit(op).expect("a peer applies what it is told");
        }
    };

    author
        .edit(EditOperation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(41.0),
        })
        .expect("edits");
    replay(&mut peer, author.take_applied());
    assert_eq!(value(&peer, CellRef::new(0, 0)), CellValue::Number(41.0));
    assert_eq!(
        value(&peer, CellRef::new(1, 0)),
        CellValue::Number(82.0),
        "the peer recalculated the dependent formula too"
    );

    author.undo().expect("undo");
    let after_undo = author.take_applied();
    assert!(
        !after_undo.is_empty(),
        "the undo must be sent; peers cannot infer it"
    );
    replay(&mut peer, after_undo);

    assert_eq!(
        value(&peer, CellRef::new(0, 0)),
        value(&author, CellRef::new(0, 0)),
        "author and peer agree about the undone cell"
    );
    assert_eq!(
        value(&peer, CellRef::new(1, 0)),
        value(&author, CellRef::new(1, 0)),
        "and about what depended on it"
    );

    // Redo is a fresh intention and travels the same way.
    author.redo().expect("redo");
    let after_redo = author.take_applied();
    assert!(!after_redo.is_empty(), "the redo must be sent too");
    replay(&mut peer, after_redo);
    assert_eq!(value(&peer, CellRef::new(0, 0)), CellValue::Number(41.0));
    assert_eq!(value(&author, CellRef::new(0, 0)), CellValue::Number(41.0));
}

/// Undoing a *structural* edit has to travel as well — it is the case where a
/// silent divergence is worst, because every later address on one side means
/// something different from the same address on the other.
#[test]
fn undoing_a_structural_edit_reaches_the_other_participant() {
    let mut author = session_with_formula();
    author.record_applied();
    let mut peer = WorkbookSession::from_workbook(author.workbook().clone());

    author
        .edit(EditOperation::InsertRows {
            sheet: 0,
            at: 0,
            count: 1,
        })
        .expect("inserts");
    for op in author.take_applied() {
        peer.edit(op).expect("peer inserts");
    }
    assert_eq!(value(&peer, CellRef::new(1, 0)), CellValue::Number(10.0));

    author.undo().expect("undo");
    let sent = author.take_applied();
    assert!(!sent.is_empty(), "undoing an insertion must be sent");
    for op in sent {
        peer.edit(op).expect("peer removes the row again");
    }

    assert_eq!(
        value(&peer, CellRef::new(0, 0)),
        value(&author, CellRef::new(0, 0)),
        "the row came back out on both sides"
    );
    assert_eq!(value(&peer, CellRef::new(0, 0)), CellValue::Number(10.0));
}

/// Pressing undo with nothing to undo is not an event, and must not be sent.
#[test]
fn an_undo_that_does_nothing_is_not_broadcast() {
    let mut session = session_with_formula();
    session.record_applied();
    session
        .undo()
        .expect("undo with an empty stack is not an error");
    assert!(
        session.take_applied().is_empty(),
        "nothing happened, so there is nothing to tell anybody"
    );
}

/// **A tab drag renumbers every sheet, and the kept graph is keyed by number.**
///
/// `MoveSheet` was classified `RecalcPlan::Skip` on the grounds that reordering
/// tabs changes no value and no name resolution. Both readings were too narrow:
/// the precedent graph is keyed by sheet *index*, and `MoveSheet` removes and
/// re-inserts, renumbering the lot. The graph then described the old numbering,
/// so every later edit to the moved sheet found no dependents and propagated to
/// nothing — silently, and into the saved file.
///
/// Asserted against a full recalculation rather than a literal, because the
/// property that matters is that keeping a graph never changes the answer.
#[test]
fn moving_a_sheet_does_not_leave_a_graph_keyed_to_the_old_order() {
    let mut session = WorkbookSession::blank();
    {
        let wb = session.workbook_mut();
        let mut first = Sheet::new(SheetId(Id::from_parts(9, 1)), "First");
        first.cells.set(
            CellRef::new(0, 0),
            casual_calc_model::Cell::value(CellValue::Number(1.0)),
        );
        let handle = wb.store_formula(casual_calc_formula::parse("A1*2").unwrap());
        let mut b1 = casual_calc_model::Cell::value(CellValue::Empty);
        b1.formula = Some(handle);
        first.cells.set(CellRef::new(0, 1), b1);
        wb.sheets.push(first);
        wb.sheets
            .push(Sheet::new(SheetId(Id::from_parts(9, 2)), "Second"));
    }
    session.recalculate();

    // An ordinary edit, which is what builds and keeps the graph.
    session
        .edit(EditOperation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(2.0),
        })
        .expect("edit");

    session
        .edit(EditOperation::MoveSheet { from: 0, to: 1 })
        .expect("drag the tab");
    assert_eq!(
        session.workbook().sheets[1].name,
        "First",
        "the move happened"
    );

    // Edit the same cell at the sheet's new index.
    session
        .edit(EditOperation::SetValue {
            sheet: 1,
            at: CellRef::new(0, 0),
            value: CellValue::Number(100.0),
        })
        .expect("edit after the move");

    let kept = session.workbook().sheets[1]
        .cells
        .get(CellRef::new(0, 1))
        .map(|c| c.value.clone())
        .unwrap_or(CellValue::Empty);

    let mut truth = session.workbook().clone();
    casual_calc_eval::recalculate(&mut truth);
    let full = truth.sheets[1]
        .cells
        .get(CellRef::new(0, 1))
        .map(|c| c.value.clone())
        .unwrap_or(CellValue::Empty);

    assert_eq!(
        kept, full,
        "the kept graph disagrees with a full recalculation after a tab drag"
    );
    assert_eq!(
        kept,
        CellValue::Number(200.0),
        "and the answer is the right one"
    );
}

/// **`SHEET()` reports a sheet's position, so a tab drag changes its value.**
///
/// The other half of why `MoveSheet` cannot be `Skip`, and the one that holds
/// even for a session that never built a graph at all.
#[test]
fn moving_a_sheet_recomputes_a_formula_that_reads_its_position() {
    let mut session = WorkbookSession::blank();
    {
        let wb = session.workbook_mut();
        let mut first = Sheet::new(SheetId(Id::from_parts(9, 1)), "First");
        let handle = wb.store_formula(casual_calc_formula::parse("SHEET()").unwrap());
        let mut a1 = casual_calc_model::Cell::value(CellValue::Empty);
        a1.formula = Some(handle);
        first.cells.set(CellRef::new(0, 0), a1);
        wb.sheets.push(first);
        wb.sheets
            .push(Sheet::new(SheetId(Id::from_parts(9, 2)), "Second"));
    }
    session.recalculate();
    assert_eq!(
        session.workbook().sheets[0]
            .cells
            .get(CellRef::new(0, 0))
            .map(|c| c.value.clone()),
        Some(CellValue::Number(1.0)),
        "First is the first sheet"
    );

    session
        .edit(EditOperation::MoveSheet { from: 0, to: 1 })
        .expect("drag the tab");

    assert_eq!(
        session.workbook().sheets[1]
            .cells
            .get(CellRef::new(0, 0))
            .map(|c| c.value.clone()),
        Some(CellValue::Number(2.0)),
        "SHEET() must report the new position, not the old one"
    );
}

/// **Hiding a row changes what a subtotal is.**
///
/// `SetSheetMetadata` was classified `Skip` as a presentation-only bundle, but
/// two of its twenty-three fields are read by the evaluator: `SUBTOTAL`'s
/// 101–111 codes and `AGGREGATE` skip hidden rows, and `Sheet::is_row_hidden`
/// is the union of the hand-hidden set and the set an autofilter hides. So
/// applying a filter changed the sheet without recomputing the one function
/// whose answer depends on it — and since no *cell* was written, nothing in the
/// dependency graph could have noticed either.
///
/// This is the engine half of a filter that co-editors see differently: the
/// operation relays, and the subtotal underneath it did not move.
#[test]
fn hiding_a_row_recomputes_a_subtotal_that_ignores_hidden_rows() {
    let mut session = WorkbookSession::blank();
    {
        let wb = session.workbook_mut();
        let mut sheet = Sheet::new(SheetId(Id::from_parts(9, 1)), "Sheet1");
        for (row, n) in [(0u32, 1.0), (1, 2.0), (2, 3.0)] {
            sheet.cells.set(
                CellRef::new(row, 0),
                casual_calc_model::Cell::value(CellValue::Number(n)),
            );
        }
        // 109 is SUM ignoring hidden rows.
        let handle = wb.store_formula(casual_calc_formula::parse("SUBTOTAL(109,A1:A3)").unwrap());
        let mut total = casual_calc_model::Cell::value(CellValue::Empty);
        total.formula = Some(handle);
        sheet.cells.set(CellRef::new(3, 0), total);
        wb.sheets.push(sheet);
    }
    session.recalculate();
    assert_eq!(
        session.workbook().sheets[0]
            .cells
            .get(CellRef::new(3, 0))
            .map(|c| c.value.clone()),
        Some(CellValue::Number(6.0)),
        "nothing hidden yet"
    );

    // Hide the middle row the way a filter does.
    let mut data = crate::SheetMetadata::capture(&session.workbook().sheets[0]);
    data.filter_hidden.insert(1);
    session
        .edit(EditOperation::SetSheetMetadata {
            sheet: 0,
            data: Box::new(data),
            changed: casual_calc_transaction::SheetFields::FILTER_HIDDEN,
            restore: Default::default(),
        })
        .expect("apply the filter");

    assert_eq!(
        session.workbook().sheets[0]
            .cells
            .get(CellRef::new(3, 0))
            .map(|c| c.value.clone()),
        Some(CellValue::Number(4.0)),
        "SUBTOTAL(109) must drop the row the filter hid"
    );
}

/// **Does a filter reach the other participant at all?**
///
/// Reported as "the filter does not relay". At the engine level it does: the
/// editor's filter commands go through `commit_filter`, which builds a
/// `SetSheetMetadata` and applies it with `session.edit`, so it enters the
/// outgoing log like any edit and transforms like one. This pins that, so a
/// later change cannot quietly move filtering off the operation path — and so
/// the remaining half of the report is known to be the editor's redraw rather
/// than the engine's transport.
#[test]
fn applying_a_filter_reaches_the_other_participant() {
    let mut author = WorkbookSession::blank();
    {
        let wb = author.workbook_mut();
        let mut sheet = Sheet::new(SheetId(Id::from_parts(9, 1)), "Sheet1");
        for (row, n) in [(0u32, 1.0), (1, 2.0), (2, 3.0)] {
            sheet.cells.set(
                CellRef::new(row, 0),
                casual_calc_model::Cell::value(CellValue::Number(n)),
            );
        }
        let handle = wb.store_formula(casual_calc_formula::parse("SUBTOTAL(109,A1:A3)").unwrap());
        let mut total = casual_calc_model::Cell::value(CellValue::Empty);
        total.formula = Some(handle);
        sheet.cells.set(CellRef::new(3, 0), total);
        wb.sheets.push(sheet);
    }
    author.recalculate();
    author.record_applied();

    let mut peer = WorkbookSession::from_workbook(author.workbook().clone());

    let mut data = crate::SheetMetadata::capture(&author.workbook().sheets[0]);
    data.filter_hidden.insert(1);
    author
        .edit(EditOperation::SetSheetMetadata {
            sheet: 0,
            data: Box::new(data),
            changed: casual_calc_transaction::SheetFields::FILTER_HIDDEN,
            restore: Default::default(),
        })
        .expect("apply the filter");

    let sent = author.take_applied();
    assert!(
        !sent.is_empty(),
        "a filter is a document change and has to be sent"
    );
    for op in sent {
        peer.edit(op).expect("the peer applies it");
    }

    assert!(
        peer.workbook().sheets[0].is_row_hidden(1),
        "the peer hides the row the filter hid"
    );
    // And the value underneath agrees on both sides, which is the half that
    // was broken independently of transport.
    let total = |s: &WorkbookSession| {
        s.workbook().sheets[0]
            .cells
            .get(CellRef::new(3, 0))
            .map(|c| c.value.clone())
    };
    assert_eq!(total(&peer), total(&author));
    assert_eq!(total(&peer), Some(CellValue::Number(4.0)));
}

// --- Collaborative undo (COL-28, docs/69) ------------------------------------
//
// The policy: cell edits clobber, structural edits refuse. These cover the
// structural half — the one where the loss is unbounded and no undo stack
// anywhere can bring it back.

mod collaborative_undo {
    use super::*;
    use crate::SdkError;

    /// A session with one sheet and nothing in it.
    fn blank_sheet() -> WorkbookSession {
        let mut session = WorkbookSession::blank();
        session
            .workbook_mut()
            .sheets
            .push(Sheet::new(SheetId(Id::from_parts(9, 1)), "Sheet1"));
        session
    }

    /// Write to the workbook **without** touching this session's history —
    /// which is what an operation arriving from a peer does.
    fn a_peer_writes(session: &mut WorkbookSession, at: CellRef, mark: f64) {
        session.workbook_mut().sheets[0]
            .cells
            .set(at, casual_calc_model::Cell::value(CellValue::Number(mark)));
    }

    fn number_at(session: &WorkbookSession, at: CellRef) -> Option<f64> {
        match session.workbook().sheets[0].cells.get(at).map(|c| &c.value) {
            Some(CellValue::Number(n)) => Some(*n),
            _ => None,
        }
    }

    /// **Undoing an insert somebody has filled is refused, and says why.**
    ///
    /// Ada inserts row 10; Grace types in it; Ada presses undo. The stored
    /// inverse deletes row 10 and Grace's data goes with it — work that was
    /// never in that row when the undo was recorded, and which Grace's own
    /// history cannot restore, because it holds "typed into row 10" and not
    /// "here is row 10's content".
    #[test]
    fn undoing_an_insert_a_peer_has_filled_is_refused() {
        let mut session = blank_sheet();
        session
            .edit(EditOperation::InsertRows {
                sheet: 0,
                at: 9,
                count: 1,
            })
            .expect("inserted");

        a_peer_writes(&mut session, CellRef::new(9, 1), 250.0);

        let refused = session.undo().expect_err("undo must not run");
        let SdkError::UndoWouldDiscard(what) = &refused else {
            panic!("expected a refusal naming the band, got {refused:?}");
        };
        assert_eq!(what.at, 9);
        assert_eq!(what.count, 1);
        assert_eq!(what.cells, 1);
        assert_eq!(what.occupied, CellRef::new(9, 1));

        // Refused *loudly*: the message names the line and what is in it, so the
        // user can act. A button that appears to do nothing is the failure this
        // policy exists to avoid.
        let said = refused.to_string();
        assert!(said.contains("row"), "{said}");
        assert!(
            said.contains("10"),
            "the message does not name the line: {said}"
        );

        // And nothing moved.
        assert_eq!(
            number_at(&session, CellRef::new(9, 1)),
            Some(250.0),
            "the refusal did not leave the document alone"
        );
    }

    /// **An insert nobody has touched still undoes.**
    ///
    /// The conservative check must not cost the ordinary case: this is the same
    /// gesture with no peer involved, and it has to work.
    #[test]
    fn undoing_an_untouched_insert_still_works() {
        let mut session = blank_sheet();
        a_peer_writes(&mut session, CellRef::new(20, 0), 7.0);
        session
            .edit(EditOperation::InsertRows {
                sheet: 0,
                at: 9,
                count: 1,
            })
            .expect("inserted");

        session.undo().expect("an empty band undoes");
        // The row below shifted down on insert and back up on undo.
        assert_eq!(number_at(&session, CellRef::new(20, 0)), Some(7.0));
    }

    /// **This session's own writes do not block its undo.**
    ///
    /// The stack is last-in-first-out, which is the whole reason this needs no
    /// per-cell authorship: by the time the insert is the operation being
    /// undone, everything Ada did after it has already been undone, so the band
    /// is empty again. If that stopped being true the check would refuse Ada's
    /// own work and undo would appear broken.
    #[test]
    fn a_sessions_own_writes_do_not_block_its_undo() {
        let mut session = blank_sheet();
        session
            .edit(EditOperation::InsertRows {
                sheet: 0,
                at: 9,
                count: 1,
            })
            .expect("inserted");
        session
            .edit(EditOperation::SetValue {
                sheet: 0,
                at: CellRef::new(9, 0),
                value: CellValue::Number(1.0),
            })
            .expect("typed");

        session
            .undo()
            .expect("her own typing comes back off the stack first");
        session
            .undo()
            .expect("and then the insert, against an empty band");
    }

    /// **Undoing a delete is never refused.**
    ///
    /// It re-inserts a band, which is additive: it destroys nothing, and a
    /// peer's concurrent edit simply keeps its shifted address. Refusing here
    /// would be a policy that costs the user a working undo for no gain.
    #[test]
    fn undoing_a_delete_is_not_refused() {
        let mut session = blank_sheet();
        a_peer_writes(&mut session, CellRef::new(9, 0), 42.0);
        session
            .edit(EditOperation::DeleteRows {
                sheet: 0,
                at: 9,
                count: 1,
            })
            .expect("deleted");

        // **Into the band the undo will re-insert at.** Writing somewhere else
        // would leave that band empty, and the test would pass whether or not
        // the policy distinguishes a delete's inverse from an insert's — which
        // is exactly what it is here to check.
        a_peer_writes(&mut session, CellRef::new(9, 0), 99.0);

        session
            .undo()
            .expect("undoing a delete restores, it does not destroy");
        assert_eq!(
            number_at(&session, CellRef::new(9, 0)),
            Some(42.0),
            "the deleted row did not come back"
        );
        assert_eq!(
            number_at(&session, CellRef::new(10, 0)),
            Some(99.0),
            "the peer's edit was not carried down by the re-inserted row"
        );
    }

    /// **A cell edit still clobbers.**
    ///
    /// Deliberate, and the other half of the policy: last-writer-wins is what
    /// concurrent cell writes already do, an undo is a write, and the value is
    /// one cell the peer's own stack still holds. Excel and Sheets both make
    /// this trade. A refusal here would be the worse failure.
    #[test]
    fn a_cell_undo_still_overwrites_a_peers_value() {
        let mut session = blank_sheet();
        session
            .edit(EditOperation::SetValue {
                sheet: 0,
                at: CellRef::new(0, 0),
                value: CellValue::Number(100.0),
            })
            .expect("typed");

        a_peer_writes(&mut session, CellRef::new(0, 0), 250.0);

        session.undo().expect("a cell undo is never refused");
        assert_ne!(
            number_at(&session, CellRef::new(0, 0)),
            Some(250.0),
            "the undo did not run"
        );
    }
}

// --- The session's invariants (SDK-008) --------------------------------------

mod escapes {
    use super::*;
    use crate::SdkError;

    /// **A workbook made invalid through `workbook_mut` does not become a
    /// file.**
    ///
    /// The session exists to hold invariants, and `workbook_mut` hands a host
    /// the right to change anything — which it needs, for programmatic setup,
    /// and which the session cannot supervise: it does not see what happens
    /// through that reference, and checking on every call is not affordable,
    /// because `validate` walks every cell and hosts reach for it per
    /// keystroke.
    ///
    /// So the check is at the boundary where being wrong stops being
    /// recoverable. A corrupt workbook in memory is a bug somebody will notice.
    /// One that became a `.xlsx` is a file the author opens tomorrow.
    #[test]
    fn an_invalid_workbook_is_refused_rather_than_written() {
        let mut session = WorkbookSession::blank();
        let id = SheetId(Id::from_parts(9, 1));
        {
            let wb = session.workbook_mut();
            // Two sheets with one identity: every lookup by id now resolves to
            // whichever comes first, and a saved file names the same sheet
            // twice.
            wb.sheets.push(Sheet::new(id, "First"));
            wb.sheets.push(Sheet::new(id, "Second"));
        }

        match session.save() {
            Err(SdkError::Model(e)) => {
                assert_eq!(e.code(), "OC-MDL-0001");
                assert!(e.to_string().contains("duplicate sheet id"), "{e}");
            }
            Err(other) => panic!("expected the invariant to be named, got {other}"),
            Ok(bytes) => panic!("wrote {} bytes of invalid workbook", bytes.len()),
        }
    }

    /// **A valid workbook still saves.**
    ///
    /// The check must cost nothing anybody notices except the host that broke
    /// something.
    #[test]
    fn an_ordinary_workbook_still_saves() {
        let mut session = WorkbookSession::blank();
        session
            .workbook_mut()
            .sheets
            .push(Sheet::new(SheetId(Id::from_parts(9, 1)), "Sheet1"));
        session
            .edit(EditOperation::SetValue {
                sheet: 0,
                at: CellRef::new(0, 0),
                value: CellValue::Number(1.0),
            })
            .expect("edited");

        let bytes = session.save().expect("a valid workbook saves");
        assert!(bytes.len() > 100, "the package is suspiciously small");
    }

    /// **You cannot hold both an untouched original and a workbook you have
    /// changed.**
    ///
    /// This is what makes the ordering in `save` safe, and it is the part that
    /// can actually be tested. Validating the untouched path would be wrong —
    /// this engine does not model every construct, so refusing to hand back a
    /// file it merely does not understand turns "open and close" into data loss
    /// — but no test can currently distinguish that, because import always
    /// produces a valid model, so the check would simply pass.
    ///
    /// What holds the property up instead is this: taking a mutable reference
    /// drops the original bytes. There is no reachable state where `source` is
    /// still set and the workbook has been changed behind the session's back,
    /// which is why the untouched path has nothing to validate.
    #[test]
    fn taking_a_mutable_reference_gives_up_the_untouched_original() {
        let mut source = WorkbookSession::blank();
        source
            .workbook_mut()
            .sheets
            .push(Sheet::new(SheetId(Id::from_parts(9, 1)), "Sheet1"));
        let bytes = source.save().expect("saved");

        let mut opened = WorkbookSession::open(bytes.clone()).expect("opened");
        assert!(opened.is_unmodified(), "a freshly opened file is untouched");
        assert_eq!(
            opened.save().expect("saves"),
            bytes,
            "the original bytes did not come back unchanged"
        );

        // One reference, no writes through it.
        let _ = opened.workbook_mut();
        assert!(
            !opened.is_unmodified(),
            "the session still believes it holds the file's own bytes, while a \
             host has had the right to change anything in it"
        );
    }
}
