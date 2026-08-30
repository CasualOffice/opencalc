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
    let handle = wb.store_formula_at(
        casual_calc_formula::parse("A1*2").unwrap(),
        casual_calc_formula::stored::Origin::at(1, 0),
    );
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
        let handle = wb.store_formula_at(
            casual_calc_formula::parse("A1*2").unwrap(),
            casual_calc_formula::stored::Origin::at(0, 1),
        );
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
        let handle = wb.store_formula_at(
            casual_calc_formula::parse("SHEET()").unwrap(),
            casual_calc_formula::stored::Origin::at(0, 0),
        );
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
        let handle = wb.store_formula_at(
            casual_calc_formula::parse("SUBTOTAL(109,A1:A3)").unwrap(),
            casual_calc_formula::stored::Origin::at(3, 0),
        );
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
        let handle = wb.store_formula_at(
            casual_calc_formula::parse("SUBTOTAL(109,A1:A3)").unwrap(),
            casual_calc_formula::stored::Origin::at(3, 0),
        );
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

/// A session opened from delimited text saves back as delimited text
/// (`WOPI-05`).
///
/// The defect these hold shut: the session built the workbook and then forgot
/// where it came from, so `save` reached for the only writer it knew and
/// returned an OOXML package. A host that handed us `books.csv` got a zip back
/// under that name — every tool downstream sees a corrupt CSV, and the original
/// is gone.
mod delimited_sessions {
    use casual_calc_io::{COMMA, PIPE, TAB, read_delimited};
    use casual_calc_model::{Cell, CellRange, CellRef, CellValue, Id, Sheet, SheetId, Style};

    use crate::{EditOperation, SessionFormat, WorkbookSession};

    const CSV: &[u8] = b"Item,Qty\r\nWidget,3\r\n";

    #[test]
    fn an_edited_csv_session_saves_csv_that_reads_back_with_the_edit() {
        let mut session =
            WorkbookSession::open_delimited(CSV.to_vec(), COMMA).expect("the csv opens");
        assert_eq!(session.format(), SessionFormat::Delimited(COMMA));

        session
            .edit(EditOperation::SetValue {
                sheet: 0,
                at: CellRef::new(1, 1),
                value: CellValue::Number(7.0),
            })
            .expect("edited");

        let saved = session.save().expect("saves");
        assert_ne!(
            &saved[..2],
            b"PK",
            "a csv session saved an OOXML package: {}",
            String::from_utf8_lossy(&saved[..saved.len().min(40)])
        );
        assert_eq!(
            String::from_utf8(saved.clone()).unwrap(),
            "Item,Qty\r\nWidget,7\r\n"
        );

        // And the bytes are readable as what they claim to be.
        let reread = read_delimited(&saved, COMMA).expect("the saved bytes are delimited text");
        assert_eq!(
            reread.sheets[0]
                .cells
                .get(CellRef::new(1, 1))
                .unwrap()
                .value,
            CellValue::Number(7.0),
        );
    }

    /// Opened and not edited saves as itself, exactly as a package does — a
    /// file that was only looked at is not rewritten.
    ///
    /// Written with bare newlines and a needlessly quoted field, both of which
    /// the writer normalises away: a source the writer would reproduce anyway
    /// could not tell the two paths apart, and this test would pass with the
    /// guarantee removed.
    #[test]
    fn an_untouched_csv_saves_byte_for_byte() {
        const AS_WRITTEN: &[u8] = b"Item,Qty\nWidget,\"3\"\n";
        let session = WorkbookSession::open_delimited(AS_WRITTEN.to_vec(), COMMA).expect("opens");
        assert_eq!(
            String::from_utf8(session.save().expect("saves")).unwrap(),
            String::from_utf8(AS_WRITTEN.to_vec()).unwrap(),
            "opening a csv and saving it rewrote the author's file"
        );
        assert!(session.is_unmodified());
    }

    /// The tab and pipe separators are remembered too, and each writes its own.
    ///
    /// The edited row holds one cell and is written as one field: a trailing
    /// separator would be padding out to the widest row, which is `IO-02` and
    /// which `read_delimited` discards on the way back in anyway.
    #[test]
    fn a_tsv_session_saves_tabs_and_a_psv_pipes() {
        for (delimiter, source) in [(TAB, "a\tb\r\n"), (PIPE, "a|b\r\n")] {
            let mut session =
                WorkbookSession::open_delimited(source.as_bytes().to_vec(), delimiter)
                    .expect("opens");
            session
                .edit(EditOperation::SetValue {
                    sheet: 0,
                    at: CellRef::new(1, 0),
                    value: CellValue::Number(1.0),
                })
                .expect("edited");
            let saved = String::from_utf8(session.save().expect("saves")).unwrap();
            assert_eq!(
                saved,
                format!("a{d}b\r\n1\r\n", d = delimiter as char),
                "delimiter {delimiter} was not the one written back"
            );
        }
    }

    /// The regression guard: nothing about an `.xlsx` session changed.
    #[test]
    fn an_edited_xlsx_session_still_saves_a_package() {
        let mut session = WorkbookSession::blank();
        session
            .workbook_mut()
            .sheets
            .push(Sheet::new(SheetId(Id::from_parts(9, 1)), "Sheet1"));
        assert_eq!(session.format(), SessionFormat::Xlsx);
        session
            .edit(EditOperation::SetValue {
                sheet: 0,
                at: CellRef::new(0, 0),
                value: CellValue::Number(1.0),
            })
            .expect("edited");

        let saved = session.save().expect("saves");
        assert_eq!(
            &saved[..2],
            b"PK",
            "an xlsx session stopped writing a package"
        );
        let reopened = WorkbookSession::open(saved).expect("the package reopens");
        assert_eq!(
            reopened.workbook().sheets[0]
                .cells
                .get(CellRef::new(0, 0))
                .unwrap()
                .value,
            CellValue::Number(1.0),
        );
        assert!(
            reopened.format_loss().is_empty(),
            "an xlsx session claimed its format loses something"
        );
    }

    /// **What the format costs is counted, never dropped quietly.**
    ///
    /// The three losses everyone names — one sheet, formulas as values, no
    /// formatting — plus the ones nobody does, which disappear just as
    /// completely.
    #[test]
    fn a_delimited_save_names_everything_it_cannot_carry() {
        let mut session = WorkbookSession::open_delimited(CSV.to_vec(), COMMA).expect("opens");
        let wb = session.workbook_mut();

        // A formula on the sheet that will be written.
        let handle = wb.store_formula_at(
            casual_calc_formula::parse("1+1").unwrap(),
            casual_calc_formula::stored::Origin::at(2, 1),
        );
        let mut b3 = Cell::value(CellValue::Number(2.0));
        b3.formula = Some(handle);
        // Bold: formatting a text field has nowhere to go.
        let bold = wb.intern_style(Style {
            bold: true,
            ..Style::default()
        });
        // A date format, which *is* carried — `write_delimited` renders it as
        // written and `read_delimited` types it back.
        let dated = wb.intern_style(Style {
            number_format: Some("yyyy-mm-dd".to_owned()),
            ..Style::default()
        });
        // A *non*-date number format, which is not: the writer puts `1234.5`
        // where the sheet showed `1,234.50`.
        let money = wb.intern_style(Style {
            number_format: Some("#,##0.00".to_owned()),
            ..Style::default()
        });
        let text = wb.intern_string("note");
        let sheet = &mut wb.sheets[0];
        sheet.cells.set(CellRef::new(2, 1), b3);
        let mut a3 = Cell::value(CellValue::SharedString(text));
        a3.style = Some(bold);
        sheet.cells.set(CellRef::new(2, 0), a3);
        let mut c1 = Cell::value(CellValue::Number(45356.0));
        c1.style = Some(dated);
        sheet.cells.set(CellRef::new(0, 2), c1);
        let mut c2 = Cell::value(CellValue::Number(1234.5));
        c2.style = Some(money);
        sheet.cells.set(CellRef::new(1, 2), c2);
        sheet
            .merges
            .push(CellRange::new(CellRef::new(4, 0), CellRef::new(4, 1)));
        sheet.view.frozen_rows = 1;
        // A second sheet: not written at all.
        wb.sheets
            .push(Sheet::new(SheetId(Id::from_parts(9, 2)), "Notes"));

        let loss = session.format_loss();
        let named: std::collections::BTreeMap<String, (crate::ModelOutcome, u64)> = loss
            .entries()
            .into_iter()
            .map(|e| (e.feature, (e.model, e.count)))
            .collect();

        assert_eq!(
            named.get("other sheets").map(|e| e.1),
            Some(1),
            "the second sheet is not written and was not reported: {named:?}"
        );
        assert_eq!(
            named.get("formulas"),
            Some(&(crate::ModelOutcome::Degraded, 1)),
            "a formula written as its value is a degradation, and must be said: {named:?}"
        );
        assert_eq!(
            named.get("cell formatting").map(|e| e.1),
            Some(2),
            "bold and a currency format are lost, a date format is not — exactly \
             two cells are reportable: {named:?}"
        );
        assert_eq!(named.get("merged cells").map(|e| e.1), Some(1), "{named:?}");
        assert_eq!(named.get("frozen panes").map(|e| e.1), Some(1), "{named:?}");
    }

    /// A clean single-sheet CSV loses nothing, and says so. A report that
    /// warned about every file would be ignored on the one that mattered.
    #[test]
    fn a_plain_csv_reports_no_loss_at_all() {
        let session = WorkbookSession::open_delimited(CSV.to_vec(), COMMA).expect("opens");
        assert!(
            session.format_loss().is_empty(),
            "a plain csv was reported as lossy: {:?}",
            session.format_loss().entries()
        );
    }

    /// The extension a host must put on the file it writes.
    #[test]
    fn a_format_names_its_own_extension_and_type() {
        assert_eq!(
            SessionFormat::for_extension("CSV"),
            Some(SessionFormat::Delimited(COMMA))
        );
        assert_eq!(
            SessionFormat::for_extension("tab"),
            Some(SessionFormat::Delimited(TAB))
        );
        assert_eq!(
            SessionFormat::for_extension("xlsx"),
            Some(SessionFormat::Xlsx)
        );
        assert_eq!(
            SessionFormat::for_extension("ods"),
            Some(SessionFormat::Ods)
        );
        // A format this engine neither reads nor writes must **not** name one:
        // falling back to `Xlsx` is how a `.xls` is opened, edited and written
        // back as a package under its original name.
        assert_eq!(SessionFormat::for_extension("xls"), None);
        assert_eq!(SessionFormat::for_extension("numbers"), None);
        assert_eq!(SessionFormat::Delimited(PIPE).extension(), "psv");
        assert!(
            SessionFormat::Delimited(COMMA)
                .content_type()
                .starts_with("text/csv")
        );
    }

    /// **Asking for a format that is not the session's own gets that format,
    /// not the file that was opened.**
    ///
    /// The trap in `save_as`: the byte-for-byte guarantee returns the source
    /// bytes when nothing has been edited, and returning them for *any*
    /// requested format hands a caller a `.csv` when it asked for a package.
    /// This is the WOPI adapter's whole fetch leg — it converts a file it has
    /// not touched — so the untouched case is exactly the one that matters.
    #[test]
    fn converting_an_untouched_session_does_not_return_the_original_bytes() {
        let session = WorkbookSession::open_delimited(CSV.to_vec(), COMMA).expect("opens");
        assert!(session.is_unmodified(), "nothing was edited");

        let package = session
            .save_as(crate::SessionFormat::Xlsx)
            .expect("converts to a package");
        assert_eq!(
            &package[..2],
            b"PK",
            "asked for a package and got the csv back: {}",
            String::from_utf8_lossy(&package[..package.len().min(40)])
        );
        let reopened = WorkbookSession::open(package).expect("the package opens");
        assert_eq!(
            reopened.workbook().sheets[0]
                .cells
                .get(CellRef::new(1, 0))
                .unwrap()
                .value,
            reread_first_column(),
        );

        // And the session is unchanged: `save` still gives its own format.
        assert_eq!(session.save().expect("saves"), CSV.to_vec());
    }

    /// The value `Widget` as it comes back out of a workbook, interned.
    fn reread_first_column() -> CellValue {
        // Compared through the round trip rather than by handle: the interned
        // id belongs to whichever workbook interned it.
        let wb = read_delimited(CSV, COMMA).expect("reads");
        wb.sheets[0]
            .cells
            .get(CellRef::new(1, 0))
            .unwrap()
            .value
            .clone()
    }

    /// **The loss of a format is asked of the document, not of the session's
    /// own format.**
    ///
    /// A converter holds an `.xlsx` in memory and is about to write a `.csv`;
    /// asking `format_loss` would say "nothing", because the session it built
    /// to do the reading is an xlsx one.
    #[test]
    fn a_conversion_can_ask_what_another_format_would_cost() {
        let mut session = WorkbookSession::blank();
        session
            .workbook_mut()
            .sheets
            .push(Sheet::new(SheetId(Id::from_parts(9, 1)), "Sheet1"));
        session
            .workbook_mut()
            .sheets
            .push(Sheet::new(SheetId(Id::from_parts(9, 2)), "Notes"));

        assert!(
            session.format_loss().is_empty(),
            "an xlsx session loses nothing writing xlsx"
        );
        let loss = session.loss_writing(SessionFormat::Delimited(COMMA));
        assert_eq!(
            loss.entries()
                .into_iter()
                .find(|e| e.feature == "other sheets")
                .map(|e| e.count),
            Some(1),
            "converting to csv would drop a sheet and said nothing: {:?}",
            loss.entries()
        );
    }

    /// Text that is not UTF-8 is refused rather than mangled.
    #[test]
    fn a_csv_that_is_not_utf8_is_refused() {
        let err = WorkbookSession::open_delimited(vec![0xff, 0xfe, b'a'], COMMA)
            .expect_err("invalid utf-8 must not open");
        assert!(matches!(err, crate::SdkError::Io(_)), "{err:?}");
    }
}

use std::collections::BTreeSet;

// ---------------------------------------------------------------------------
// COL-32 — personal views. docs/71.
//
// The whole feature is a set of things that must *not* happen, so these are
// mostly assertions of absence. That makes them easy to write and easy to write
// uselessly, so each one is paired with a positive control: the shared filter
// doing the same thing, visibly.
// ---------------------------------------------------------------------------

/// A sheet of 1..=3 in column A with `SUBTOTAL(109, A1:A3)` beneath it.
fn sheet_with_subtotal() -> WorkbookSession {
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
        let handle = wb.store_formula_at(
            casual_calc_formula::parse("SUBTOTAL(109,A1:A3)").unwrap(),
            casual_calc_formula::stored::Origin::at(3, 0),
        );
        let mut total = casual_calc_model::Cell::value(CellValue::Empty);
        total.formula = Some(handle);
        sheet.cells.set(CellRef::new(3, 0), total);
        wb.sheets.push(sheet);
    }
    session.recalculate();
    session
}

fn subtotal(session: &WorkbookSession) -> Option<CellValue> {
    session.workbook().sheets[0]
        .cells
        .get(CellRef::new(3, 0))
        .map(|c| c.value.clone())
}

/// **A personal view does not move a subtotal; a shared filter does.**
///
/// This is the constraint the whole design turns on. If a personal view could
/// change a value, two participants would hold different numbers for the same
/// cell and convergence — which every other part of the collaboration design
/// assumes — would simply be false.
///
/// The shared filter in the second half is the control: without it this test
/// would pass just as well against a build where filtering does nothing at all.
#[test]
fn a_personal_view_does_not_move_a_subtotal() {
    let mut session = sheet_with_subtotal();
    assert_eq!(subtotal(&session), Some(CellValue::Number(6.0)));

    session.set_personal_filter(0, BTreeSet::from([1]));
    session.recalculate();
    assert_eq!(
        subtotal(&session),
        Some(CellValue::Number(6.0)),
        "a personal view changed a cell value, so two participants now disagree"
    );
    // ...and the row really is hidden, for this participant, on screen.
    assert!(
        !session.is_row_visible(0, 1),
        "the view did not hide the row"
    );
    assert!(session.is_row_visible(0, 0));

    // The control: the shared filter moves it, so hiding row 1 is observable.
    let mut data = crate::SheetMetadata::capture(&session.workbook().sheets[0]);
    data.filter_hidden.insert(1);
    session
        .edit(EditOperation::SetSheetMetadata {
            sheet: 0,
            data: Box::new(data),
            changed: casual_calc_transaction::SheetFields::FILTER_HIDDEN,
            restore: Default::default(),
        })
        .expect("apply the shared filter");
    assert_eq!(
        subtotal(&session),
        Some(CellValue::Number(4.0)),
        "the shared filter did not move the subtotal, so this test proves nothing"
    );
}

/// **A personal view puts nothing on the wire.**
///
/// The test that fails if somebody later routes this through `edit` for
/// convenience — which is the obvious refactor, and would silently make every
/// participant's rows disappear.
#[test]
fn a_personal_view_emits_nothing_for_peers() {
    let mut session = sheet_with_subtotal();
    session.record_applied();
    assert!(!session.has_applied());

    session.set_personal_filter(0, BTreeSet::from([1]));
    session.clear_personal_view(0);
    session.set_personal_filter(0, BTreeSet::from([0, 2]));
    session.clear_all_personal_views();

    assert!(
        !session.has_applied(),
        "a personal view reached the outgoing log: every peer would see these rows vanish"
    );
    assert!(session.take_applied().is_empty());

    // Control: an ordinary edit does reach it, so the assertion above is not
    // just observing that recording was never on.
    session
        .edit(EditOperation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 1),
            value: CellValue::Number(1.0),
        })
        .expect("an ordinary edit");
    assert!(
        session.has_applied(),
        "recording was not on, so nothing was proven"
    );
}

/// **A personal view is not undoable, and does not disturb the undo stack.**
///
/// Undo after applying one undoes the last thing done to the *document*. That
/// will surprise somebody, which is why `clear_all_personal_views` exists as a
/// deliberate action rather than something reached for with ctrl-Z.
#[test]
fn a_personal_view_is_not_in_the_history() {
    let mut session = sheet_with_subtotal();
    session
        .edit(EditOperation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 1),
            value: CellValue::Number(42.0),
        })
        .expect("an ordinary edit");

    session.set_personal_filter(0, BTreeSet::from([1]));
    session.undo().expect("undo");

    // Undo reversed the *edit*, not the view.
    assert_eq!(
        session.workbook().sheets[0]
            .cells
            .get(CellRef::new(0, 1))
            .map(|c| c.value.clone())
            .unwrap_or(CellValue::Empty),
        CellValue::Empty,
        "undo did not reverse the document edit"
    );
    assert!(
        session.views().has_view(0),
        "undo cleared a personal view, so it had entered the history"
    );
}

/// **A personal view is not saved.**
///
/// "Not part of the document" has to be true of the bytes, not just of the
/// wire. The shared filter in the same test is the control: it *is* saved, so a
/// build that dropped both would not pass.
#[test]
fn a_personal_view_does_not_reach_the_saved_file() {
    let mut session = sheet_with_subtotal();

    let mut data = crate::SheetMetadata::capture(&session.workbook().sheets[0]);
    data.filter_hidden.insert(2);
    session
        .edit(EditOperation::SetSheetMetadata {
            sheet: 0,
            data: Box::new(data),
            changed: casual_calc_transaction::SheetFields::FILTER_HIDDEN,
            restore: Default::default(),
        })
        .expect("apply the shared filter");
    session.set_personal_filter(0, BTreeSet::from([0]));

    let bytes = session.save().expect("save");
    let reopened = WorkbookSession::open(bytes).expect("reopen");

    // `is_row_hidden`, not `filter_hidden`: `.xlsx` has no separate notion of a
    // filter-hidden row — ECMA-376 stores one as `<row hidden="1">` like any
    // other — so a shared filter comes back in `hidden_rows`. Asserting on
    // `filter_hidden` here fails, and the first version of this test did.
    assert!(
        reopened.workbook().sheets[0].is_row_hidden(2),
        "the shared filter was lost, so this test cannot see the personal one either"
    );
    assert!(
        !reopened.workbook().sheets[0].is_row_hidden(0),
        "a personal view was written into the file"
    );
    assert!(
        !reopened.views().has_view(0),
        "a personal view survived a round trip; it is meant to survive nothing"
    );
}

/// **One participant's view leaves the other's rows exactly as they were.**
///
/// The tracker's acceptance, at the session level: two sessions over the same
/// document, one applies a personal filter, the other sees nothing — not the
/// rows, not the subtotal.
#[test]
fn one_participants_view_does_not_reach_another() {
    let mut mine = sheet_with_subtotal();
    let bytes = mine.save().expect("save");
    let theirs = WorkbookSession::open(bytes).expect("open a second session");

    mine.set_personal_filter(0, BTreeSet::from([1]));

    assert!(
        !mine.is_row_visible(0, 1),
        "my own view did not take effect"
    );
    assert!(
        theirs.is_row_visible(0, 1),
        "my personal view hid a row for another participant"
    );
    assert_eq!(
        subtotal(&theirs),
        subtotal(&mine),
        "the two disagree about a cell"
    );
}

/// **A view follows its sheet through a real structural edit.**
///
/// Not the unit test in `views::tests` — this one goes through `edit`, which is
/// the path that actually renumbers sheets, and would catch the resequencing
/// being wired to the wrong operations or not wired at all.
#[test]
fn a_view_follows_its_sheet_through_an_insert() {
    let mut session = sheet_with_subtotal();
    session.set_personal_filter(0, BTreeSet::from([1]));

    session
        .edit(EditOperation::InsertSheet {
            index: 0,
            sheet: Box::new(Sheet::new(SheetId(Id::from_parts(9, 2)), "Ahead")),
        })
        .expect("insert a sheet in front");

    assert!(
        !session.views().has_view(0),
        "the view stayed on index 0, which is now a sheet nobody filtered"
    );
    assert!(
        session.views().hides(1, 1),
        "the view did not follow its sheet"
    );
    assert!(!session.is_row_visible(1, 1));
    assert!(
        session.is_row_visible(0, 1),
        "the new sheet inherited a filter"
    );
}

/// **And back again through undo** (`FID-38`).
///
/// `edit` resequences and `undo` did not, so the renumbering was one-way: the
/// insert moved the view from 0 to 1 and undoing the insert left it at 1, on a
/// sheet that no longer exists. Nothing errors — `hides` simply answers for a
/// key nobody will ask about again, and the rows the user hid come back
/// visible with no way to say why.
#[test]
fn a_view_follows_its_sheet_back_through_an_undo() {
    let mut session = sheet_with_subtotal();
    session.set_personal_filter(0, BTreeSet::from([1]));

    session
        .edit(EditOperation::InsertSheet {
            index: 0,
            sheet: Box::new(Sheet::new(SheetId(Id::from_parts(9, 2)), "Ahead")),
        })
        .expect("insert a sheet in front");
    assert!(session.views().hides(1, 1), "the insert did not resequence");

    session.undo().expect("undo the insert");

    assert!(
        session.views().hides(0, 1),
        "undoing the insert left the view on index 1: it hides rows on a sheet \
         that is gone, and the sheet it belongs to is unfiltered"
    );
    assert!(!session.is_row_visible(0, 1));
}

/// Redo puts it back where the edit did.
#[test]
fn a_view_follows_its_sheet_forward_through_a_redo() {
    let mut session = sheet_with_subtotal();
    session.set_personal_filter(0, BTreeSet::from([1]));

    session
        .edit(EditOperation::InsertSheet {
            index: 0,
            sheet: Box::new(Sheet::new(SheetId(Id::from_parts(9, 2)), "Ahead")),
        })
        .expect("insert a sheet in front");
    session.undo().expect("undo the insert");
    session.redo().expect("redo the insert");

    assert!(
        session.views().hides(1, 1),
        "redo re-inserted the sheet without renumbering the view"
    );
    assert!(
        session.is_row_visible(0, 1),
        "the re-inserted sheet inherited a filter"
    );
}

/// **A sheet operation inside a `Batch` renumbers too** (`FID-38`).
///
/// The fall-through arm did not look inside a batch, and a batch is not an
/// exotic shape here: `RemoveSheet`'s own inverse is one whenever a chart named
/// the removed sheet, so this is the shape *undo* hands back. A batch is
/// applied in order, so its members are resequenced in order — each sees the
/// index space the one before it produced.
#[test]
fn a_view_follows_its_sheet_through_a_batched_insert() {
    let mut session = sheet_with_subtotal();
    session.set_personal_filter(0, BTreeSet::from([1]));

    session
        .edit(EditOperation::Batch(vec![
            EditOperation::InsertSheet {
                index: 0,
                sheet: Box::new(Sheet::new(SheetId(Id::from_parts(9, 2)), "Ahead")),
            },
            EditOperation::InsertSheet {
                index: 0,
                sheet: Box::new(Sheet::new(SheetId(Id::from_parts(9, 3)), "Also ahead")),
            },
        ]))
        .expect("insert two sheets in front, atomically");

    assert!(
        session.views().hides(2, 1),
        "a batch of two inserts moved the sheet two places and the view none"
    );
    assert!(session.is_row_visible(0, 1) && session.is_row_visible(1, 1));
}

/// **`apply_raw` renumbers too** (`FID-38`).
///
/// It bypasses the *history*, which is what it is for, and used to bypass the
/// renumbering with it — a different question with the same answer as
/// [`WorkbookSession::edit`]'s, because the index a personal view is keyed by
/// moves whether or not the move is undoable.
#[test]
fn a_view_follows_its_sheet_through_apply_raw() {
    let mut session = sheet_with_subtotal();
    session.set_personal_filter(0, BTreeSet::from([1]));

    session
        .apply_raw(EditOperation::InsertSheet {
            index: 0,
            sheet: Box::new(Sheet::new(SheetId(Id::from_parts(9, 2)), "Ahead")),
        })
        .expect("insert a sheet without recording history");

    assert!(
        session.views().hides(1, 1),
        "apply_raw moved the sheet and left the view behind"
    );
}

/// A session opened from a `.ods` saves back as a `.ods` (`WOPI-07`).
///
/// The same rule the delimited sessions above hold, for the format a
/// LibreOffice-first shop actually has. The engine could read one and write one
/// before this; what it could not do was *remember* which, so a `.ods` handed to
/// the SDK came back as an OOXML package under its original name — the failure
/// `WOPI-05` was opened for, one format later.
mod ods_sessions {
    use casual_calc_model::{CellRange, CellRef, CellValue, Id, Sheet, SheetId};

    use crate::{EditOperation, SessionFormat, WorkbookSession};

    /// Written by LibreOffice. A fixture this engine wrote itself would prove
    /// the reader and the writer agree and nothing about whether either is
    /// right, and the files this has to open come from somebody else.
    const ODS: &[u8] = include_bytes!("../../casual-calc-ods/tests/fixtures/libreoffice-basic.ods");

    fn ods() -> SessionFormat {
        SessionFormat::for_extension("ods")
            .expect("`.ods` must name a format, or nothing downstream can save one back")
    }

    /// The ODF package marker: `mimetype`, stored uncompressed and first, so
    /// the media type sits in the first bytes of the zip.
    fn is_odf_package(bytes: &[u8]) -> bool {
        const MARKER: &[u8] = b"mimetypeapplication/vnd.oasis.opendocument.spreadsheet";
        bytes.starts_with(b"PK")
            && bytes
                .windows(MARKER.len())
                .take(128)
                .any(|window| window == MARKER)
    }

    /// **A `.ods` opened, edited and saved is still a `.ods`, and carries the
    /// edit.**
    #[test]
    fn an_edited_ods_session_saves_an_ods_that_reads_back_with_the_edit() {
        let mut session = WorkbookSession::open_as(ODS.to_vec(), ods()).expect("the ods opens");
        assert_eq!(
            session.format(),
            ods(),
            "the session forgot what it was opened from"
        );

        session
            .edit(EditOperation::SetValue {
                sheet: 0,
                at: CellRef::new(1, 1),
                value: CellValue::Number(9.0),
            })
            .expect("edited");

        let saved = session.save().expect("saves");
        assert!(
            is_odf_package(&saved),
            "a .ods session saved something that is not an ODF package: {:?}",
            String::from_utf8_lossy(&saved[..saved.len().min(64)])
        );

        let again = WorkbookSession::open_as(saved, ods()).expect("what we wrote must open");
        assert_eq!(
            again.workbook().sheets[0]
                .cells
                .get(CellRef::new(1, 1))
                .map(|c| c.value.clone()),
            Some(CellValue::Number(9.0)),
            "the edit did not survive the save"
        );
        assert_eq!(again.workbook().sheets[0].name, "seed");
    }

    /// **Opening a `.ods` and saving it without editing returns the file
    /// itself.**
    ///
    /// Merely looking at a workbook must not rewrite it — and here that matters
    /// more than for `.xlsx`, because this writer keeps far less than the reader
    /// takes in.
    #[test]
    fn an_untouched_ods_saves_as_itself() {
        let session = WorkbookSession::open_as(ODS.to_vec(), ods()).expect("opens");
        assert!(session.is_unmodified());
        assert_eq!(
            session.save().expect("saves"),
            ODS.to_vec(),
            "an untouched .ods was rewritten"
        );
    }

    /// **The extension and content type are ODF's, not a package's.**
    ///
    /// A host names the file and sets a header from these; bytes that are ODF
    /// under an `.xlsx` name are the same lie as the wrong bytes would be.
    #[test]
    fn the_format_names_its_own_extension_and_type() {
        assert_eq!(ods().extension(), "ods");
        assert_eq!(
            ods().content_type(),
            "application/vnd.oasis.opendocument.spreadsheet"
        );
        assert_eq!(SessionFormat::for_extension("ODS"), Some(ods()));
    }

    /// **What a `.ods` save cannot carry is counted before it is written.**
    ///
    /// The condition on advertising the format at all: this writer carries
    /// values, formulas and sheets, so a merge or a bold cell is gone from the
    /// file even though it is still in the session. The save is allowed — the
    /// user is editing a `.ods` — but nothing leaves without being said out
    /// loud (`AGENTS.md`: no silent data loss).
    #[test]
    fn saving_as_ods_names_what_that_format_costs() {
        let mut session = WorkbookSession::open_as(ODS.to_vec(), ods()).expect("opens");
        assert!(
            session.format_loss().is_empty(),
            "a document of nothing but values and formulas was reported as lossy: {:?}",
            session.format_loss().entries()
        );

        let workbook = session.workbook_mut();
        workbook.sheets[0]
            .merges
            .push(CellRange::new(CellRef::new(0, 0), CellRef::new(0, 1)));
        workbook
            .sheets
            .push(Sheet::new(SheetId(Id::from_parts(9, 7)), "Notes"));

        let loss = session.format_loss();
        let named: Vec<(String, u64)> = loss
            .entries()
            .into_iter()
            .map(|e| (e.feature.to_string(), e.count))
            .collect();
        assert!(
            named.contains(&("merged cells".to_owned(), 1)),
            "a merge is not written and was not named: {named:?}"
        );
        // …and the second sheet *is* carried, so it must not be reported as
        // lost. A report that warns about everything is read on nothing.
        assert!(
            !named.iter().any(|(feature, _)| feature == "other sheets"),
            "a .ods holds every sheet, and this claimed one was dropped: {named:?}"
        );
    }

    /// **A `.ods` is not read as a package by mistake.**
    ///
    /// `open_as` dispatches on the format the *caller* named, which for a WOPI
    /// adapter comes from the host's filename. ODF bytes handed to the OOXML
    /// importer have to fail rather than half-succeed.
    #[test]
    fn ods_bytes_are_not_read_as_a_package() {
        assert!(WorkbookSession::open(ODS.to_vec()).is_err());
    }
}

/// **`.xlsm` — the macro half of `IO-04`.**
///
/// Two things were wrong and only one of them looked like a bug. `.xlsm` was
/// *refused*, by a name check and nothing else: the package is the same OOXML
/// the `.xlsx` reader has always read, with one extra part in it. That was the
/// visible half. The dangerous half was what happened when somebody worked
/// around the refusal by renaming the file — the session opened as `.xlsx`,
/// saved as `.xlsx`, and the macros were gone with an empty compatibility
/// report to say so. A retained part that nothing reports is exactly the shape
/// `no silent data loss` exists to forbid.
///
/// What these hold shut is the *fate* of the macro project, both ways: carried
/// byte for byte into a macro-enabled package, and named in the report when the
/// target format has nowhere to put it. Either is an acceptable answer; a
/// silent drop is not.
mod macro_enabled_sessions {
    use casual_calc_model::{
        Cell, CellRef, CellValue, Id, RetainedPart, RetainedRel, Sheet, SheetId, Workbook,
    };

    use crate::{EditOperation, ModelOutcome, SessionFormat, WorkbookSession};

    /// The relationship type Office hangs a VBA project off the workbook part
    /// with.
    const VBA_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";
    /// What `[Content_Types].xml` declares that part as.
    const VBA_CT: &str = "application/vnd.ms-office.vbaProject";
    /// The workbook part's content type in a macro-enabled package — the one
    /// difference at the package level, and the one Excel reads.
    const CT_MACRO_MAIN: &str = "application/vnd.ms-excel.sheet.macroEnabled.main+xml";
    const CT_PLAIN_MAIN: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
    const VBA_PART: &str = "xl/vbaProject.bin";

    /// Stand-in bytes with a compound-file header on the front. Deliberately
    /// **not** a real project, and it does not need to be: nothing in this
    /// engine parses, interprets or executes them, so the only property under
    /// test is that they come out the other side unchanged.
    const VBA_BYTES: &[u8] =
        b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1 stand-in macro project, never parsed and never run";

    fn xlsm() -> SessionFormat {
        SessionFormat::for_extension("xlsm")
            .expect("`.xlsm` must name a format, or nothing below can open one")
    }

    /// A macro-enabled package: one sheet, one cell, one VBA project hanging
    /// off `xl/workbook.xml` exactly as Excel hangs it.
    fn xlsm_bytes() -> Vec<u8> {
        let mut workbook = Workbook::new(Id::from_parts(9, 0));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(9, 1)), "Sheet1");
        sheet
            .cells
            .set(CellRef::new(0, 0), Cell::value(CellValue::Number(41.0)));
        workbook.sheets.push(sheet);
        workbook.retained_parts.push(RetainedPart {
            path: VBA_PART.to_owned(),
            bytes: VBA_BYTES.to_vec(),
            content_type: Some(VBA_CT.to_owned()),
        });
        workbook.retained_rels.push(RetainedRel {
            source: "xl/workbook.xml".to_owned(),
            id: "rId99".to_owned(),
            rel_type: VBA_REL.to_owned(),
            target: "vbaProject.bin".to_owned(),
            external: false,
        });
        casual_calc_export::write_workbook(&workbook).expect("a macro workbook writes")
    }

    /// One part's bytes out of a package, or `None` when it holds no such part.
    fn part_of(package: &[u8], path: &str) -> Option<Vec<u8>> {
        let mut opened = casual_calc_ooxml::SpreadsheetPackage::open(
            package.to_vec(),
            casual_calc_ooxml::OoxmlLimits::default(),
        )
        .expect("the bytes are an admissible package");
        if !opened.contains(path) {
            return None;
        }
        Some(opened.read_part(path).expect("a part that is there reads"))
    }

    /// What `[Content_Types].xml` says `xl/workbook.xml` is.
    fn workbook_content_type(package: &[u8]) -> String {
        let mut opened = casual_calc_ooxml::SpreadsheetPackage::open(
            package.to_vec(),
            casual_calc_ooxml::OoxmlLimits::default(),
        )
        .expect("the bytes are an admissible package");
        opened
            .content_types()
            .expect("a package declares its parts")
            .resolve("xl/workbook.xml")
            .expect("the workbook part is always declared")
            .to_owned()
    }

    /// **The other half of the row, recorded rather than built.**
    ///
    /// `.xls` is legacy BIFF8 — a compound-file binary and a whole reader — and
    /// it stays refused. What is worth pinning is that the refusal is
    /// *consistent*: `for_extension` says no by name, and `for_bytes` says no
    /// by content, so a `.xls` renamed `.csv` and dropped on the engine is not
    /// read as a sheet of mojibake. The two halves of `IO-04` share a row and
    /// not a fate, and this is the line between them.
    #[test]
    fn xls_is_refused_by_name_and_by_content_alike() {
        assert_eq!(SessionFormat::for_extension("xls"), None);
        // The OLE2 compound-file header every BIFF8 workbook begins with,
        // followed by the NUL padding such a file is full of.
        let mut biff8 = b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1".to_vec();
        biff8.extend(std::iter::repeat_n(0u8, 512));
        assert_eq!(
            SessionFormat::for_bytes(&biff8),
            None,
            "a legacy workbook must not be guessed into some format this engine \
             does read"
        );
    }

    /// The name check that refused the format, and the two answers a host needs
    /// once it does not.
    #[test]
    fn xlsm_names_a_format_with_its_own_extension_and_type() {
        assert_eq!(SessionFormat::for_extension("xlsm"), Some(xlsm()));
        assert_eq!(SessionFormat::for_extension("XLSM"), Some(xlsm()));
        // Its own extension, not `xlsx`: a session that saved an `.xlsm` back
        // under the other name is the whole defect.
        assert_eq!(xlsm().extension(), "xlsm");
        assert_eq!(
            xlsm().content_type(),
            "application/vnd.ms-excel.sheet.macroEnabled.12"
        );
    }

    /// The reader was never the problem. Proven rather than asserted, because
    /// "the engine can already read it" was the premise the whole row rested on.
    #[test]
    fn an_xlsm_opens_and_its_sheets_are_read() {
        let session = WorkbookSession::open_as(xlsm_bytes(), xlsm()).expect("an `.xlsm` opens");
        assert_eq!(session.format(), xlsm());
        assert_eq!(
            session.workbook().sheets[0]
                .cells
                .get(CellRef::new(0, 0))
                .map(|c| c.value.clone()),
            Some(CellValue::Number(41.0)),
            "the sheets of a macro-enabled package read like any other"
        );
    }

    /// The round-trip floor holds for this format too: opened and not edited,
    /// it saves as itself.
    #[test]
    fn an_untouched_xlsm_saves_as_itself_byte_for_byte() {
        let bytes = xlsm_bytes();
        let session = WorkbookSession::open_as(bytes.clone(), xlsm()).expect("opens");
        assert_eq!(session.save().expect("saves"), bytes);
    }

    /// **The fate of the macro part, decided: retained byte for byte.**
    ///
    /// After an edit the semantic writer runs, so this is the path where a
    /// retained part either survives or quietly does not. It survives, *and*
    /// the package that comes out declares itself macro-enabled — which is the
    /// half that is easy to miss, because a package carrying a VBA project
    /// while declaring the plain workbook type is one Excel opens as damaged
    /// and repairs by deleting the project. Keeping the bytes and mis-declaring
    /// the package would lose the macros just as thoroughly, one step later.
    #[test]
    fn an_edited_xlsm_keeps_its_macro_project_byte_for_byte() {
        let mut session = WorkbookSession::open_as(xlsm_bytes(), xlsm()).expect("opens");
        session
            .edit(EditOperation::SetValue {
                sheet: 0,
                at: CellRef::new(0, 0),
                value: CellValue::Number(42.0),
            })
            .expect("edits");
        assert!(
            !session.is_unmodified(),
            "the semantic writer has to be the one that runs, or this proves nothing"
        );

        let saved = session.save().expect("an edited `.xlsm` saves");
        assert_eq!(
            part_of(&saved, VBA_PART).as_deref(),
            Some(VBA_BYTES),
            "the macro project was not carried through the write unchanged"
        );
        assert_eq!(
            workbook_content_type(&saved),
            CT_MACRO_MAIN,
            "a package holding a VBA project must declare itself macro-enabled"
        );
        assert!(
            session.format_loss().is_empty(),
            "an `.xlsm` written as an `.xlsm` loses nothing, and must not claim to: {:?}",
            session.format_loss().entries()
        );
    }

    /// **The fate of the macro part, the other way: named, never silent.**
    ///
    /// This is the conversion the row was raised for. `.xlsx` has nowhere to
    /// put a VBA project, so the project is removed rather than smuggled into a
    /// package that denies holding it — and the removal is counted and named
    /// before the bytes exist, so a host can say so before the download.
    #[test]
    fn saving_a_macro_workbook_as_xlsx_drops_the_macros_and_says_so() {
        let mut session = WorkbookSession::open_as(xlsm_bytes(), xlsm()).expect("opens");
        session
            .edit(EditOperation::SetValue {
                sheet: 0,
                at: CellRef::new(0, 0),
                value: CellValue::Number(42.0),
            })
            .expect("edits");

        let loss = session.loss_writing(SessionFormat::Xlsx);
        let entry = loss
            .entries()
            .into_iter()
            .find(|e| e.feature == "macros (VBA project)")
            .expect("converting a macro workbook to `.xlsx` must name what it costs");
        assert_eq!(entry.model, ModelOutcome::Omitted);

        let saved = session
            .save_as(SessionFormat::Xlsx)
            .expect("converts to a plain package");
        assert_eq!(
            part_of(&saved, VBA_PART),
            None,
            "the macro project was carried into a package that declares itself macro-free"
        );
        assert_eq!(
            workbook_content_type(&saved),
            CT_PLAIN_MAIN,
            "asking for an `.xlsx` and getting a macro-enabled declaration is the \
             same lie as a `.csv` inside an `.xlsx` name"
        );
    }

    /// The report and the bytes have to agree in **both** directions.
    ///
    /// An untouched session hands back the file it opened, whatever format is
    /// asked for by name — so for the session's own format there is no loss to
    /// report, and claiming one would send a host to warn about macros that are
    /// still in the file it is about to write.
    #[test]
    fn an_untouched_xlsm_is_not_reported_as_losing_its_own_macros() {
        let session = WorkbookSession::open_as(xlsm_bytes(), xlsm()).expect("opens");
        assert!(session.is_unmodified());
        assert!(
            session.format_loss().is_empty(),
            "{:?}",
            session.format_loss().entries()
        );
    }

    /// **A loss is only a loss if the write actually takes it.**
    ///
    /// The case is real and reachable: `SessionFormat::for_bytes` reads the
    /// zip's first entry name, and a macro-enabled package and a plain one both
    /// begin `[Content_Types].xml` — so `.xlsm` bytes detected by content are
    /// opened *as* `Xlsx`. Untouched, that session saves by handing the opened
    /// file straight back, macros included, and a report claiming they were
    /// dropped would send a host to warn about a loss that did not happen. The
    /// same session after one edit runs the writer, and then the loss is real
    /// and must be named.
    #[test]
    fn a_save_that_returns_the_opened_bytes_reports_no_macro_loss() {
        let bytes = xlsm_bytes();

        let session = WorkbookSession::open(bytes.clone()).expect("a package reads either way");
        assert_eq!(
            session.format(),
            SessionFormat::Xlsx,
            "the premise of this test is a macro package opened under the plain format"
        );
        assert_eq!(
            session.save().expect("saves"),
            bytes,
            "untouched, these are the bytes that were opened"
        );
        assert!(
            session.format_loss().is_empty(),
            "a save that returns the opened file has lost nothing: {:?}",
            session.format_loss().entries()
        );

        let mut edited = WorkbookSession::open(bytes).expect("opens");
        edited
            .edit(EditOperation::SetValue {
                sheet: 0,
                at: CellRef::new(0, 0),
                value: CellValue::Number(42.0),
            })
            .expect("edits");
        assert!(
            edited
                .format_loss()
                .entries()
                .iter()
                .any(|e| e.feature == "macros (VBA project)"),
            "once the writer runs the macros really do go, and that has to be said: {:?}",
            edited.format_loss().entries()
        );
    }
}

/// **The pictures a workbook holds reach the renderer** (`RND-14`).
///
/// `casual-calc-render` grew a picture backend, an `ImageSource` to feed it and
/// an [`ImageReport`](crate::ImageReport) to say what it could not draw — and
/// nothing in this crate asked for any of it, so the entry point every host
/// actually calls went on producing pictureless PNGs and saying nothing about
/// it. These tests are about the wiring, not the drawing: the render crate
/// proves the blit lands where the anchor says, and what is proven here is that
/// a session hands over its own media and hands back what that cost.
mod images {
    use casual_calc_model::{
        Cell, CellRange, CellRef, CellValue, Emu, Id, ImageView, RetainedPart, Sheet, SheetId,
        Workbook,
    };

    use crate::{GridViewport, UndrawnReason, WorkbookSession, render_sheet_png_with_report};

    const PART: &str = "xl/media/image1.png";
    const RED: (u8, u8, u8) = (220, 20, 20);

    /// A 40x40 PNG of one solid colour.
    ///
    /// Solid rather than the render crate's four quadrants, because the
    /// question here is whether the bytes travelled at all — where they landed
    /// is that crate's test, and duplicating it would mean two places to fix an
    /// orientation bug.
    fn red_png() -> Vec<u8> {
        let mut src = tiny_skia::Pixmap::new(40, 40).unwrap();
        src.fill(tiny_skia::Color::from_rgba8(RED.0, RED.1, RED.2, 255));
        src.encode_png().unwrap()
    }

    /// A sheet whose picture covers `A1:C3` — 192x60 device pixels at 96 dpi
    /// with the default geometry — with the media held where the importer puts
    /// it: in `retained_parts`, under the path the anchor names.
    fn workbook_with_picture(media: Option<Vec<u8>>) -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        sheet
            .cells
            .set(CellRef::new(0, 0), Cell::value(CellValue::Number(8.0)));
        sheet.images.push(ImageView {
            anchor: CellRange::new(CellRef::new(0, 0), CellRef::new(2, 2)),
            from_offset: Emu::default(),
            to_offset: Emu::default(),
            part: PART.to_owned(),
            extent: None,
        });
        wb.sheets.push(sheet);
        if let Some(bytes) = media {
            wb.retained_parts.push(RetainedPart {
                path: PART.to_owned(),
                bytes,
                content_type: Some("image/png".to_owned()),
            });
        }
        wb
    }

    fn viewport() -> GridViewport {
        GridViewport {
            x: 0,
            y: 0,
            width: 4 * 960,
            height: 4 * 300,
        }
    }

    #[track_caller]
    fn pixel(png: &[u8], x: u32, y: u32) -> (u8, u8, u8) {
        let map = tiny_skia::Pixmap::decode_png(png).expect("the session returned a readable PNG");
        let p = map.pixel(x, y).expect("pixel out of surface");
        (p.red(), p.green(), p.blue())
    }

    /// **A session draws its own pictures.**
    ///
    /// The media is already in the workbook, keyed by the same package path the
    /// display list carries, so nothing had to be handed in for this to work —
    /// which is exactly why the gap was invisible: the capability was complete
    /// and the one call that mattered went to the form without it.
    #[test]
    fn a_session_render_draws_the_workbooks_own_media() {
        let session = WorkbookSession::from_workbook(workbook_with_picture(Some(red_png())));
        let png = session.render_png(0, &viewport(), 96).unwrap();

        let close = |a: u8, b: u8| a.abs_diff(b) <= 3;
        let got = pixel(&png, 96, 30);
        assert!(
            close(got.0, RED.0) && close(got.1, RED.1) && close(got.2, RED.2),
            "the middle of the picture's frame is {got:?}, not the picture: \
             the session rendered without its media"
        );
        // …and only inside the frame, or a red pixel proves nothing except that
        // something painted the whole surface.
        assert_eq!(
            pixel(&png, 230, 70),
            (255, 255, 255),
            "the picture painted outside its own frame"
        );
    }

    /// **A picture that could not be drawn is named, not left as a blank.**
    ///
    /// A frame-shaped hole and a sheet that never had a logo produce the same
    /// pixels, and only one of them is a file somebody should be told about.
    #[test]
    fn a_picture_with_no_media_is_named_in_the_report() {
        let wb = workbook_with_picture(None);
        let geometry = casual_calc_layout::GridGeometry::for_sheet(&wb.sheets[0]);
        let (_, images) = render_sheet_png_with_report(&wb, 0, &geometry, &viewport(), 96).unwrap();

        assert_eq!(images.drawn, 0);
        assert!(!images.is_complete(), "a missing picture reported complete");
        assert_eq!(images.undrawn.len(), 1);
        assert_eq!(images.undrawn[0].part, PART, "the part is named");
        assert_eq!(images.undrawn[0].reason, UndrawnReason::NotSupplied);
    }

    /// **One report, not two.**
    ///
    /// A host already shows a `CompatibilityReport`; handing it a second,
    /// differently-shaped answer about the same document is how the second one
    /// stops being shown.
    #[test]
    fn an_undrawn_picture_folds_into_the_compatibility_report() {
        let session = WorkbookSession::from_workbook(workbook_with_picture(None));
        let (_, loss) = session.render_png_with_report(0, &viewport(), 96).unwrap();

        let named: Vec<(String, u64)> = loss
            .entries()
            .into_iter()
            .map(|e| (e.feature.to_string(), e.count))
            .collect();
        assert!(
            named.contains(&("image (media not supplied)".to_owned(), 1)),
            "the render's loss did not reach the report a host reads: {named:?}"
        );
    }

    /// A picture that *was* drawn leaves the report clean.
    ///
    /// The other half of the one above: a report that names something on every
    /// render is a report nobody reads by the third document.
    #[test]
    fn a_drawn_picture_reports_no_loss() {
        let session = WorkbookSession::from_workbook(workbook_with_picture(Some(red_png())));
        let (_, loss) = session.render_png_with_report(0, &viewport(), 96).unwrap();
        assert!(
            loss.is_empty(),
            "a picture that was drawn was reported as lost: {:?}",
            loss.entries()
        );
    }
}

// --- Format detection (ODS-01) -----------------------------------------

/// **What this engine writes, this engine recognises.**
///
/// The `casual-calc-io` tests prove the offsets against packages written by
/// Python's `zipfile`; this proves the same detector against packages written
/// by *our* writers, which is the pairing that matters. Either alone is a
/// half-test: the first says the parser reads real zips, the second says the
/// zips we produce are the shape the parser expects.
#[test]
fn a_saved_workbook_is_recognised_from_its_bytes_in_every_format_we_write() {
    use crate::SessionFormat;

    for format in [SessionFormat::Xlsx, SessionFormat::Ods] {
        let session = session_with_formula();
        let bytes = session
            .save_as(format)
            .unwrap_or_else(|e| panic!("save as {format:?}: {e}"));
        assert_eq!(
            SessionFormat::for_bytes(&bytes),
            Some(format),
            "a {format:?} this engine wrote was not recognised from its own bytes"
        );
    }
}

/// And the round trip closes: detected, then opened by what was detected.
///
/// The point of detecting at all. A host with an upload and no filename can
/// open it, which it could not do before — `for_extension` was the only way in.
#[test]
fn bytes_with_no_filename_can_be_opened_by_what_they_are() {
    use crate::SessionFormat;

    let session = session_with_formula();
    let bytes = session.save().unwrap();
    let format = SessionFormat::for_bytes(&bytes).expect("a saved workbook is recognisable");
    let reopened = WorkbookSession::open_as(bytes, format).expect("opened by detected format");
    assert_eq!(
        value(&reopened, CellRef::new(1, 0)),
        CellValue::Number(20.0),
        "the formula did not survive an open by detected format"
    );
}

/// Refusing is part of the contract: a caller must be able to tell "I know what
/// this is" from "I do not", or it will open nonsense as a spreadsheet.
#[test]
fn bytes_that_are_not_a_spreadsheet_are_refused() {
    use crate::SessionFormat;

    assert_eq!(SessionFormat::for_bytes(&[0u8, 1, 2, 3, 0xff]), None);
    assert_eq!(
        SessionFormat::for_bytes(b"a sentence with no separators"),
        None
    );
}

/// **A host should not have to know that a new workbook needs a sheet.**
///
/// `blank` returns a workbook of no sheets — correct for building one up
/// programmatically, and wrong for every interactive host: a window whose
/// workbook has no sheets has nothing to draw, no tab strip and no cell to put
/// a caret in. Both hosts had worked that out separately and pushed a `Sheet1`
/// of their own, which is a rule written down nowhere kept in two places
/// (`SDK-011`).
///
/// The two are asserted together on purpose. `with_sheet` is only worth having
/// if `blank` still does the other thing, and a test that checked one of them
/// would pass if the constructors were merged.
#[test]
fn a_new_session_can_be_asked_for_a_sheet_to_open_on() {
    let bare = WorkbookSession::blank();
    assert_eq!(
        bare.workbook().sheets.len(),
        0,
        "`blank` stays empty — thirty-six callers add their own sheets"
    );

    let ready = WorkbookSession::with_sheet();
    assert_eq!(
        ready.workbook().sheets.len(),
        1,
        "`with_sheet` gives a host something to draw"
    );
    assert_eq!(
        ready.workbook().sheets[0].name,
        "Sheet1",
        "named what both hosts named it, and what every spreadsheet does"
    );
}

/// The sheet a host is handed has to be usable, not merely present.
///
/// A sheet with an id that collides with the workbook's own would be a subtler
/// failure than an absent one: it exists, it draws, and something further down
/// resolves the wrong thing.
#[test]
fn the_opening_sheet_is_a_sheet_you_can_type_into() {
    let mut session = WorkbookSession::with_sheet();
    let at = CellRef::new(0, 0);
    let op = session.input_edit(0, at, "=1+1");
    session.edit(op).expect("typing into the opening sheet");

    assert_eq!(session.cell_input(0, at), "=1+1", "it reads back as typed");
    // A second sheet must get a distinct id — the opening one taking an id the
    // workbook would mint again is the subtle version of this failure.
    let before = session.workbook().sheets[0].id;
    session
        .workbook_mut()
        .sheets
        .push(casual_calc_model::Sheet::new(
            casual_calc_model::SheetId(casual_calc_model::Id::from_parts(0x5344, 9)),
            "Sheet2",
        ));
    assert_ne!(
        before,
        session.workbook().sheets[1].id,
        "the opening sheet's id is its own"
    );
}

/// **A formula conditional format reaches the display list, and therefore the
/// PNG.**
///
/// The unit tests for this live in `casual-calc-eval` and call `effect_for`
/// directly; this one runs the wiring — session → `layout_viewport_with` →
/// `effect_for` → the evaluator — because the seam it crosses is exactly the
/// one that used to be missing. A renderer that resolves every other rule and
/// silently skips this one is what `RND-05` was, and reading the call chain is
/// how it stayed unnoticed the first time.
///
/// The range starts at row 2, so an `A1`-anchored formula would paint the wrong
/// row and this would still be green if it started at `A1`.
#[test]
fn a_formula_conditional_format_paints_through_the_sdk_layout() {
    use casual_calc_model::{CellRange, CfRule, ConditionalFormat};

    let mut session = WorkbookSession::blank();
    let mut sheet = Sheet::new(SheetId(Id::from_parts(0x5346, 1)), "Sheet1");
    for (row, amount) in [(1u32, 150.0), (2, 50.0), (3, 900.0)] {
        sheet.cells.set(
            CellRef::new(row, 3), // column D
            casual_calc_model::Cell::value(CellValue::Number(amount)),
        );
        sheet.cells.set(
            CellRef::new(row, 0), // column A, the cell that gets painted
            casual_calc_model::Cell::value(CellValue::Number(f64::from(row))),
        );
    }
    let mut rule = ConditionalFormat::new(
        CellRange::new(CellRef::new(1, 0), CellRef::new(9, 7)), // A2:H10
        CfRule::Expression("$D2>100".to_owned()),
        "FFC7CE",
    );
    rule.priority = 1;
    sheet.conditional_formats = vec![rule];
    session.workbook_mut().sheets.push(sheet);

    let viewport = GridViewport {
        x: 0,
        y: 0,
        width: 8 * 960,
        height: 12 * 300,
    };
    let list = session.layout(0, &viewport);
    let geometry = session.geometry(0);

    // The rectangle a given cell occupies, so a fill can be attributed to a
    // cell rather than to "somewhere in the list".
    let painted = |row: u32, col: u32| -> Option<String> {
        let x = geometry.columns.offset(col);
        let y = geometry.rows.offset(row);
        list.items.iter().find_map(|item| match item {
            casual_calc_layout::PaintItem::CellBackground { rect, fill }
                if rect.x == x && rect.y == y =>
            {
                fill.clone()
            }
            _ => None,
        })
    };

    assert_eq!(
        painted(1, 0).as_deref(),
        Some("FFC7CE"),
        "A2, because D2 is 150"
    );
    assert_eq!(painted(2, 0), None, "A3 is not, because D3 is 50");
    assert_eq!(
        painted(3, 0).as_deref(),
        Some("FFC7CE"),
        "A4, because D4 is 900"
    );

    // And it is visible: the same sheet without the rule renders different
    // bytes. A fill in the display list that no backend paints would pass every
    // assertion above.
    let with_rule = session.render_png(0, &viewport, 96).unwrap();
    session.workbook_mut().sheets[0].conditional_formats.clear();
    let without = session.render_png(0, &viewport, 96).unwrap();
    assert_ne!(
        with_rule, without,
        "the highlight has to reach the pixels, not only the display list"
    );
}

/// The documented-bypass paths still move `edits_applied` (`FID-39`).
///
/// `apply_raw` and `workbook_mut` bypass *history* on purpose — that is what
/// they are for. Bypassing the **dirty signal** was never part of it. This
/// method's doc comment promises a host that comparing the number against its
/// save point answers "is there unsaved work?", and both of these change the
/// document without going near `History`.
///
/// Both already clear `self.source` for exactly this reason — they cannot see
/// what the caller does next, so they give up the untouched-original guarantee
/// rather than risk handing back a stale package. The counter is conservative
/// on the same grounds: a needless warning costs a click, and the other mistake
/// costs the document.
#[test]
fn the_history_bypasses_still_move_the_dirty_counter() {
    // A session with a real sheet: `blank()` has none, and `apply_raw` would
    // then fail with `SheetNotFound` — a red test proving nothing about the
    // counter.
    let mut session = session_with_formula();

    let before_raw = session.edits_applied();
    session
        .apply_raw(casual_calc_transaction::Operation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(1.0),
        })
        .unwrap();
    assert_ne!(
        session.edits_applied(),
        before_raw,
        "apply_raw changed the document without moving edits_applied, so a host \
         comparing it against its save point reports no unsaved work"
    );

    // **And `workbook_mut` deliberately does not** (`FID-44`). It reads as the
    // same case and is not: the wasm layer calls it as an ordinary accessor
    // *inside* an edit — to intern a style, to store a formula — so counting
    // here made every such edit count twice and the editor's draft bar told the
    // user a number that had roughly doubled. `source` can be conservative
    // because a needless re-serialize costs only time; a count is shown to a
    // person, so an over-count is a wrong statement rather than a safe one.
    let before_mut = session.edits_applied();
    session.workbook_mut();
    assert_eq!(
        session.edits_applied(),
        before_mut,
        "workbook_mut moved the dirty counter — it is used as a plain accessor \
         inside edits, so counting here double-counts every one of them"
    );
}

/// PDF export end to end (`IO-03`): a session in, a file real viewers open out.
///
/// The unit tests either side of this one check the paginator and the writer in
/// isolation. What only shows up here is the **composition** — that the pages
/// the paginator named are the bands that were laid out, moved to the right
/// place, and that the repeated header is on every page rather than only the
/// first. A writer and a paginator can both be right while the thing between
/// them puts page four's rows on page one.
mod pdf_export {
    use casual_calc_formula::Expr;
    use casual_calc_model::{CellRef, DefinedName, Id, Sheet, SheetId};

    use crate::WorkbookSession;

    fn write(session: &mut WorkbookSession, row: u32, col: u32, text: &str) {
        let op = session.input_edit(0, CellRef::new(row, col), text);
        session.edit(op).unwrap();
    }

    fn session(rows: u32, cols: u32) -> WorkbookSession {
        let mut session = WorkbookSession::blank();
        session
            .workbook_mut()
            .sheets
            .push(Sheet::new(SheetId(Id::from_parts(9, 1)), "Report"));
        for row in 0..rows {
            for col in 0..cols {
                write(&mut session, row, col, &format!("r{row}c{col}"));
            }
        }
        session
    }

    /// The characters the PDF draws on each page, recovered through the
    /// document's own `ToUnicode` map — the same route a reader or a search box
    /// takes, and the only one that proves the page says what it looks like it
    /// says.
    fn text_per_page(pdf: &[u8]) -> Vec<String> {
        let body = String::from_utf8_lossy(pdf);
        let mut map = std::collections::BTreeMap::new();
        let mut rest = &body[..];
        while let Some(at) = rest.find("beginbfchar") {
            rest = &rest[at + "beginbfchar".len()..];
            let end = rest.find("endbfchar").unwrap();
            for line in rest[..end].lines().map(str::trim).filter(|l| !l.is_empty()) {
                let mut parts = line.split_whitespace();
                let gid = u32::from_str_radix(parts.next().unwrap().trim_matches(['<', '>']), 16)
                    .unwrap();
                let uni = parts.next().unwrap().trim_matches(['<', '>']);
                let units: Vec<u16> = uni
                    .as_bytes()
                    .chunks(4)
                    .map(|c| u16::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
                    .collect();
                map.insert(gid, String::from_utf16(&units).unwrap());
            }
            rest = &rest[end..];
        }

        // A content stream per page, in the order the page tree lists them.
        //
        // Split on the *closing* keyword and take what follows the last opener
        // in each chunk. Splitting on `stream\n` instead does not work and the
        // way it fails is quiet: `endstream\n` ends with `stream\n` too, so
        // every stream gets cut four characters before its end and the search
        // for a terminator then finds nothing at all.
        let mut pages = Vec::new();
        for chunk in body.split("endstream") {
            let Some(at) = chunk.rfind("stream\n") else {
                continue;
            };
            let stream = &chunk[at + "stream\n".len()..];
            if !stream.contains(" cm") {
                continue;
            }
            let mut page = String::new();
            for run in stream.split("Tm <").skip(1) {
                let hex = run.split('>').next().unwrap();
                for chunk in hex.as_bytes().chunks(4) {
                    let gid = u32::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16).unwrap();
                    page.push_str(map.get(&gid).map(String::as_str).unwrap_or("\u{fffd}"));
                }
                page.push(' ');
            }
            pages.push(page);
        }
        pages
    }

    fn page_count(pdf: &[u8]) -> usize {
        let body = String::from_utf8_lossy(pdf);
        let at = body.find("/Type /Pages").expect("a page tree");
        let count = body[at..].find("/Count ").expect("a count") + at + 7;
        body[count..]
            .split_whitespace()
            .next()
            .unwrap()
            .trim_end_matches('>')
            .parse()
            .unwrap()
    }

    #[test]
    fn a_small_sheet_exports_one_page_carrying_its_cells() {
        let session = session(4, 3);
        let (pdf, report) = session.export_pdf_with_report(0).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        assert_eq!(page_count(&pdf), 1);
        assert!(report.entries().is_empty(), "nothing was lost: {report:?}");

        let pages = text_per_page(&pdf);
        assert_eq!(pages.len(), 1);
        for row in 0..4 {
            for col in 0..3 {
                assert!(
                    pages[0].contains(&format!("r{row}c{col}")),
                    "cell r{row}c{col} is missing from the page"
                );
            }
        }
    }

    /// The composition test: a sheet too tall for one page must put each row on
    /// exactly one page, and in order. A band placed at the wrong origin or
    /// laid out over the wrong range shows up here and nowhere else.
    #[test]
    fn a_tall_sheet_splits_and_every_row_lands_on_exactly_one_page() {
        let session = session(120, 2);
        let (pdf, _) = session.export_pdf_with_report(0).unwrap();
        let pages = text_per_page(&pdf);
        assert!(pages.len() > 1, "120 rows do not fit on one page");
        assert_eq!(page_count(&pdf), pages.len());

        for row in 0..120 {
            let needle = format!("r{row}c0");
            let on: Vec<usize> = pages
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    // `r1c0` is a prefix of `r10c0`, so match the whole run.
                    p.split_whitespace().any(|w| w == needle)
                })
                .map(|(i, _)| i)
                .collect();
            assert_eq!(on.len(), 1, "row {row} appears on pages {on:?}");
        }
    }

    /// `Print_Titles` earns its place only if the header is on page two.
    #[test]
    fn a_repeated_header_row_is_on_every_page() {
        let mut session = session(120, 2);
        let sheet_id = session.workbook().sheets[0].id;
        write(&mut session, 0, 0, "HEADER");
        session.workbook_mut().defined_names.push(DefinedName {
            name: "Print_Titles".to_owned(),
            sheet: Some(sheet_id),
            formula: Expr::Raw("'Report'!$1:$1".to_owned()),
        });

        let (pdf, _) = session.export_pdf_with_report(0).unwrap();
        let pages = text_per_page(&pdf);
        assert!(pages.len() > 1);
        for (index, page) in pages.iter().enumerate() {
            assert!(
                page.contains("HEADER"),
                "page {index} lost the repeated header row"
            );
        }
        // And the header is not *also* body content on page one, which would
        // print it twice.
        assert_eq!(
            pages[0]
                .split_whitespace()
                .filter(|w| *w == "HEADER")
                .count(),
            1,
            "the header was printed twice on page one"
        );
    }

    #[test]
    fn a_sheet_with_nothing_on_it_exports_a_document_with_no_pages() {
        let mut session = WorkbookSession::blank();
        session
            .workbook_mut()
            .sheets
            .push(Sheet::new(SheetId(Id::from_parts(9, 2)), "Empty"));
        let (pdf, _) = session.export_pdf_with_report(0).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        assert_eq!(page_count(&pdf), 0);
    }

    #[test]
    fn the_same_workbook_exports_the_same_bytes() {
        let session = session(30, 3);
        assert_eq!(
            session.export_pdf(0).unwrap(),
            session.export_pdf(0).unwrap()
        );
    }
}
