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
