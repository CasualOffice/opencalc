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

/// The charts on a sheet, with their data already resolved, as JSON.
///
/// `[{r0,c0,r1,c1,kind,title,cats:[…],series:[{name,values:[…]}]}]`. The host
/// draws pictures; resolving `Sheet1!$B$2:$B$4` into numbers is the engine's
/// job, and doing it here means the canvas never parses a formula.
///
/// A series whose reference does not resolve is dropped rather than drawn as
/// zeroes — a chart of flat zeroes looks like data, which is worse than a
/// chart with one series missing.
#[wasm_bindgen]
pub fn session_charts(sheet: usize) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let items: Vec<String> = sh
            .charts
            .iter()
            .map(|ch| {
                let cats = ch
                    .series
                    .first()
                    .and_then(|s| s.categories.as_deref())
                    .map(|r| casual_calc_layout::chart_data::ref_text(wb, sheet, r))
                    .unwrap_or_default();
                let series: Vec<String> = ch
                    .series
                    .iter()
                    .map(|se| {
                        let values =
                            casual_calc_layout::chart_data::ref_numbers(wb, sheet, &se.values);
                        format!(
                            "{{\"name\":{},\"values\":[{}]}}",
                            json_string(&se.name),
                            values
                                .iter()
                                .map(|v| match v {
                                    Some(n) => format_json_number(*n),
                                    None => "null".to_owned(),
                                })
                                .collect::<Vec<_>>()
                                .join(",")
                        )
                    })
                    .collect();
                format!(
                    "{{\"r0\":{},\"c0\":{},\"r1\":{},\"c1\":{},\"kind\":{},\
                     \"title\":{},\"legend\":{},\"xTitle\":{},\"yTitle\":{},\
                     \"fx\":{},\"fy\":{},\"tx\":{},\"ty\":{},\
                     \"cats\":[{}],\"series\":[{}]}}",
                    ch.anchor.start.row,
                    ch.anchor.start.col,
                    ch.anchor.end.row,
                    ch.anchor.end.col,
                    json_string(chart_kind_name(ch.kind)),
                    json_string(&ch.title),
                    match &ch.legend {
                        Some(pos) => json_string(pos),
                        None => "null".to_owned(),
                    },
                    json_string(&ch.x_title),
                    json_string(&ch.y_title),
                    ch.from_offset.x,
                    ch.from_offset.y,
                    ch.to_offset.x,
                    ch.to_offset.y,
                    cats.iter()
                        .map(|t| json_string(t))
                        .collect::<Vec<_>>()
                        .join(","),
                    series.join(",")
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}
