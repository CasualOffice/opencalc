//! Recalculation: the time budget, the calculation mode and the clock.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

/// Let long jobs run to completion again.
#[wasm_bindgen]
pub fn session_clear_time_budget() {
    TIME_BUDGET_MS.with(|b| b.set(None));
}

/// A token that stops the job it is given to once the budget is spent.
///
/// The deadline is fixed when the token is made, so one budget bounds one job
/// rather than every job restarting somebody else's clock. With no budget set
/// the closure never reads the clock at all, which is what keeps the imported
/// `performance.now()` off every native build of this crate.
pub(crate) fn budget_token() -> impl casual_calc_model::Cancel {
    let deadline = TIME_BUDGET_MS
        .with(std::cell::Cell::get)
        .map(|ms| performance_now() + ms);
    move || deadline.is_some_and(|at| performance_now() > at)
}

/// The engine version string.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Whether this session refuses edits.
#[wasm_bindgen]
pub fn session_read_only() -> bool {
    with_session(|s| s.is_read_only()).unwrap_or(false)
}

/// Open the workbook for reading only, or release it.
///
/// Enforced in the engine, not by hiding chrome: the host hides what it likes,
/// but an edit that reaches the session is refused whatever the UI is showing.
#[wasm_bindgen]
pub fn session_set_read_only(on: bool) {
    SESSION.with(|cell| {
        if let Some(session) = cell.borrow_mut().as_mut() {
            session.config_mut().read_only = on;
        }
    });
}

/// The calculation mode in force: `"auto"` or `"manual"`.
///
/// Resolved from the file's own `<calcPr calcMode>` on open, so a workbook
/// saved with calculation turned off opens that way.
#[wasm_bindgen]
pub fn session_calculation_mode() -> String {
    with_session(|s| s.calculation_mode().token().to_owned()).unwrap_or_else(|| "auto".to_owned())
}

/// Switch calculation mode — Excel's Formulas ▸ Calculation Options.
///
/// Switching to automatic settles anything outstanding at once, and either way
/// the choice is recorded so a save carries it.
#[wasm_bindgen]
pub fn session_set_calculation_mode(mode: &str) {
    SESSION.with(|cell| {
        if let Some(session) = cell.borrow_mut().as_mut() {
            session.set_calculation_mode(casual_calc_sdk::CalculationMode::from_token(mode));
        }
    });
}

/// Whether an edit has changed a value that has not been recalculated — what
/// Excel shows as "Calculate" in the status bar. Always false in automatic
/// mode.
#[wasm_bindgen]
pub fn session_needs_recalculation() -> bool {
    with_session(|s| s.needs_recalculation()).unwrap_or(false)
}

/// Recalculate every formula — Excel's F9.
///
/// Deliberately **not** an undoable edit and it does not dirty the document:
/// recalculating produces the values the formulas already imply, so there is
/// nothing to undo and nothing new to save. Putting it through `session.edit`
/// would fill the undo stack with steps that change nothing visible.
///
/// Stoppable: see [`session_set_time_budget_ms`]. Returns what happened —
/// `"full"`, `"cancelled"`, `"over-budget"`, or `"none"` when there is no
/// session. A cancelled recalculation **keeps what it computed** and leaves the
/// workbook stale, which is why the answer is returned rather than swallowed: a
/// host that presents a half-fresh sheet as final is showing numbers that do
/// not follow from the formulas above them.
#[wasm_bindgen]
pub fn session_recalculate() -> String {
    let cancel = budget_token();
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(session) = guard.as_mut() else {
            return "none".to_owned();
        };
        // The SDK's, not the engine's: it also clears the outstanding flag,
        // which is the whole point of pressing Calculate in manual mode.
        match session.recalculate_cancellable(&cancel) {
            Recalculated::Fully => "full",
            Recalculated::Cancelled => "cancelled",
            Recalculated::OverBudget => "over-budget",
        }
        .to_owned()
    })
}

/// Where each chart on a sheet is anchored, as JSON. **Anchors only.**
///
/// `[{r0,c0,r1,c1,fx,fy,tx,ty}]` — the anchor cells and the EMU offsets into
/// them, which is the whole of what the host needs to turn a chart into a
/// rectangle. Nothing here is read from a cell, so the payload is a fixed size
/// per chart however much data the chart names.
///
/// # Why this is not `session_charts` (`CHT-13`)
///
/// It used to be, and it also carried `cats` and `series` with **every point
/// resolved**: `Sheet1!$B$2:$B$4` came back as numbers so the canvas would
/// never parse a formula. That was the right shape while the canvas drew its
/// own charts. It stopped being right at `RND-10`, which moved the picture into
/// [`session_chart_items`](crate::session_chart_items) — the canvas has drawn
/// from the engine's display list since, and the resolved values here have been
/// parsed and dropped on the floor by every frame.
///
/// The cost was not small and it was not bounded by the data on the sheet.
/// Measured native, release:
///
/// | sheet | old payload | now |
/// | --- | --- | --- |
/// | 10,000 rows x 6 series | 592,530 bytes, 4 ms | 61 bytes |
/// | **empty** sheet, 6 series naming whole columns | 2,162,988 bytes, 14 ms | 61 bytes |
///
/// The second row is the one that matters: a series reference is a string out
/// of an untrusted `.xlsx`, so `$A$1:$A$1048576` costs
/// [`MAX_SERIES_POINTS`](casual_calc_layout::chart_data::MAX_SERIES_POINTS)
/// nulls per series per frame **on a sheet with nothing in it**.
/// [`SEC-024`'s bound](casual_calc_layout::chart_data::MAX_SERIES_POINTS)
/// stopped that being an out-of-memory kill; it did not stop it being two
/// megabytes a frame against a 16.7 ms budget.
#[wasm_bindgen]
pub fn session_chart_frames(sheet: usize) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let items: Vec<String> = sh
            .charts
            .iter()
            .map(|ch| {
                format!(
                    "{{\"r0\":{},\"c0\":{},\"r1\":{},\"c1\":{},\
                     \"fx\":{},\"fy\":{},\"tx\":{},\"ty\":{}}}",
                    ch.anchor.start.row,
                    ch.anchor.start.col,
                    ch.anchor.end.row,
                    ch.anchor.end.col,
                    ch.from_offset.x,
                    ch.from_offset.y,
                    ch.to_offset.x,
                    ch.to_offset.y,
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

#[cfg(test)]
mod chart_frame_payload {
    use super::*;
    use casual_calc_model::{
        Cell, CellRange, CellRef, CellValue, ChartKind, ChartSeries, ChartView, Emu, Id, Sheet,
        SheetId, Workbook,
    };

    fn book(rows: u32, series: u32) -> Workbook {
        let mut workbook = Workbook::new(Id::from_parts(0x5742, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(0x5348, 1)), "Sheet1");
        for r in 0..rows {
            let label = workbook.strings.intern(&format!("Label {r}"));
            sheet.cells.set(
                CellRef::new(r, 0),
                Cell::value(CellValue::SharedString(label)),
            );
            for c in 0..series {
                sheet.cells.set(
                    CellRef::new(r, c + 1),
                    Cell::value(CellValue::Number(f64::from(r) + f64::from(c) / 8.0)),
                );
            }
        }
        let col = |c: u32| char::from(b'B' + u8::try_from(c).unwrap());
        sheet.charts.push(ChartView {
            id: 1,
            anchor: CellRange::new(CellRef::new(0, 8), CellRef::new(20, 16)),
            from_offset: Emu { x: 0, y: 0 },
            to_offset: Emu { x: 0, y: 0 },
            grouping: None,
            kind: ChartKind::Line,
            title: "Revenue".to_owned(),
            series: (0..series)
                .map(|c| ChartSeries {
                    name: format!("S{c}"),
                    categories: Some(format!("Sheet1!$A$1:$A${rows}")),
                    values: format!("Sheet1!${0}$1:${0}${rows}", col(c)),
                    ..ChartSeries::default()
                })
                .collect(),
            legend: Some("r".to_owned()),
            x_title: String::new(),
            y_title: String::new(),
            part: None,
        });
        workbook.sheets.push(sheet);
        workbook
    }

    /// An **empty** sheet whose chart names whole columns — what a `.xlsx` can
    /// say, and what `SEC-024`'s bound leaves after capping.
    fn empty_book_naming_whole_columns(series: u32) -> Workbook {
        let mut workbook = Workbook::new(Id::from_parts(0x5742, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(0x5348, 1)), "Sheet1");
        let col = |c: u32| char::from(b'B' + u8::try_from(c).unwrap());
        sheet.charts.push(ChartView {
            id: 1,
            anchor: CellRange::new(CellRef::new(0, 8), CellRef::new(20, 16)),
            from_offset: Emu { x: 0, y: 0 },
            to_offset: Emu { x: 0, y: 0 },
            grouping: None,
            kind: ChartKind::Line,
            title: "Revenue".to_owned(),
            series: (0..series)
                .map(|c| ChartSeries {
                    name: format!("S{c}"),
                    categories: Some("Sheet1!$A$1:$A$1048576".to_owned()),
                    values: format!("Sheet1!${0}$1:${0}$1048576", col(c)),
                    ..ChartSeries::default()
                })
                .collect(),
            legend: Some("r".to_owned()),
            x_title: String::new(),
            y_title: String::new(),
            part: None,
        });
        workbook.sheets.push(sheet);
        workbook
    }

    /// **A chart's per-frame payload does not depend on what its series name.**
    ///
    /// This is the `CHT-13` invariant, and it is asserted as an equality
    /// between two payloads rather than as a size or a duration: a byte count
    /// would have to be re-tuned whenever a field is added, and a wall-clock
    /// assertion on a shared machine is a flaky test that gets deleted.
    ///
    /// The two workbooks are chosen so that *only* the referenced ranges
    /// differ — the anchors, the offsets and the chart count are identical. The
    /// first names ten thousand rows of real data; the second is an **empty**
    /// sheet whose series each name a whole column, which is what a `.xlsx` is
    /// free to say and what `SEC-024`'s bound leaves after capping. Under the
    /// payload this replaced they came to 592,530 and 2,162,988 bytes.
    #[test]
    fn a_frame_pays_for_the_anchor_not_for_the_data() {
        set_session(WorkbookSession::from_workbook(book(10_000, 6)));
        let with_data = session_chart_frames(0);

        set_session(WorkbookSession::from_workbook(
            empty_book_naming_whole_columns(6),
        ));
        let naming_the_grid = session_chart_frames(0);

        assert_eq!(
            with_data,
            naming_the_grid,
            "the frame payload changed with the data the series name, so the host is \
             paying per point for a rectangle: {} bytes against {} bytes",
            with_data.len(),
            naming_the_grid.len(),
        );
    }

    /// **And the anchor is all of it.** The equality above would still hold if
    /// both payloads resolved the *same* enormous range, so the shape is
    /// pinned too: eight integers, and no array.
    #[test]
    fn the_frame_payload_is_the_anchor_and_the_offsets() {
        set_session(WorkbookSession::from_workbook(book(10_000, 6)));
        assert_eq!(
            session_chart_frames(0),
            r#"[{"r0":0,"c0":8,"r1":20,"c1":16,"fx":0,"fy":0,"tx":0,"ty":0}]"#,
        );
    }

    /// A sheet with no charts, and a sheet index that is not a sheet.
    #[test]
    fn no_charts_is_an_empty_list() {
        set_session(WorkbookSession::from_workbook(book(4, 1)));
        assert_eq!(session_chart_frames(7), "[]");
    }
}

/// Supply the clock and the random seed the volatile functions read
/// (`CALC-VOL-01`).
///
/// `TODAY`, `NOW`, `RAND` and `RANDBETWEEN` read `Workbook::volatile_now` and
/// `volatile_seed`, which the engine never sets: a calc engine that reaches for
/// the wall clock cannot be tested or replayed, and `AGENTS.md` puts time and
/// I/O in the host. The engine has always been right about that — and **no host
/// ever supplied either value**, so `TODAY()` returned 0 and `RAND()` returned
/// the same sequence in every session, in every host, since the functions were
/// written.
///
/// `now` is an Excel serial: whole days since 1899-12-30, with the time of day
/// as the fraction. It is **local** time, because that is what `TODAY` means to
/// a person — a spreadsheet that rolls over at midnight UTC is wrong for most
/// of the world for part of every day.
///
/// The host calls this before recalculating rather than once at load, which is
/// what makes `NOW()` current and `RAND()` reroll the way Excel's does.
#[wasm_bindgen]
pub fn session_set_volatile(now: f64, seed: f64) {
    // Remembered before it is applied, so a session created *later* — `File ▸
    // New`, an open — starts with a clock rather than at 1899-12-30.
    if now.is_finite() && seed.is_finite() && seed >= 0.0 {
        crate::VOLATILE.with(|v| v.set((now, seed as u64)));
    }
    let _ = with_session_mut(|s| {
        let wb = s.workbook_mut();
        // A non-finite serial would poison every date formula in the sheet with
        // a value no format can render, so it is refused rather than stored.
        if now.is_finite() {
            wb.volatile_now = now;
        }
        // `f64` because a `u64` crosses into JavaScript as a `BigInt`; the seed
        // only has to differ between passes, and 2^53 distinct values is more
        // recalculations than a session performs.
        if seed.is_finite() && seed >= 0.0 {
            wb.volatile_seed = seed as u64;
        }
        Ok(())
    });
}

/// What the volatile clock currently reads, for a host that wants to check.
#[wasm_bindgen]
#[must_use]
pub fn session_volatile_now() -> f64 {
    with_session(|s| s.workbook().volatile_now).unwrap_or(0.0)
}
