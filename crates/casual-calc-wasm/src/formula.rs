//! Formula services the editor asks for without editing anything: parsing,
//! validation, the function catalogue and previews.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

/// Open an `.xlsx` and render a viewport of the first sheet to PNG bytes.
///
/// Stoppable, like every other admission on this thread
/// ([`session_set_time_budget_ms`]): the landing page hands this a file its
/// visitor picked, so "enormous but admissible" is exactly as reachable here as
/// it is in the editor.
///
/// # Errors
///
/// If the bytes are not an admissible package, if the render fails, or if the
/// import was stopped.
#[wasm_bindgen]
pub fn render_xlsx(
    bytes: &[u8],
    width_px: u32,
    height_px: u32,
    dpi: u32,
) -> Result<Vec<u8>, JsError> {
    let outcome = admit(bytes)?;
    let mut workbook = outcome.workbook;
    recalculate(&mut workbook);
    let geometry = workbook
        .sheets
        .first()
        .map(GridGeometry::for_sheet)
        .unwrap_or_default();
    let viewport = viewport_px(width_px, height_px, dpi);
    render_sheet_png(&workbook, 0, &geometry, &viewport, dpi).map_err(js)
}

/// The catalog of supported worksheet functions as JSON `[{n,sig}, …]`, sorted
/// by name — drives the editor's formula autocomplete. Kept in step with the
/// dispatch table in `casual-calc-eval::functions`.
#[wasm_bindgen]
pub fn function_catalog() -> String {
    // Single source of truth: the eval crate's dispatch catalog.
    let items: Vec<String> = casual_calc_eval::FUNCTIONS
        .iter()
        .map(|(n, sig)| format!("{{\"n\":{},\"sig\":{}}}", json_string(n), json_string(sig)))
        .collect();
    format!("[{}]", items.join(","))
}

/// The CSS font stack for a requested family (the deterministic bundled
/// substitute first, then the requested name, then the generic), from the shared
/// substitution table. The editor caches this per unique font name to build the
/// canvas font so families render as their metric-compatible bundled faces.
#[wasm_bindgen]
pub fn font_css_stack(name: &str) -> String {
    casual_calc_layout::css_stack(name)
}

/// Format `value` with `code`, for a live preview while a format is being
/// typed. Pure — it touches no session — so the dialog can call it per keystroke
/// and show the engine's real answer rather than an approximation of it.
#[wasm_bindgen]
pub fn format_preview(value: f64, code: &str) -> String {
    // Honour the open workbook's epoch, or a 1904 file's format dialog would
    // preview dates four years off from what the grid shows.
    let date1904 = with_session(|s| s.workbook().date1904).unwrap_or(false);
    if date1904 {
        casual_calc_layout::format_number_1904(value, code)
    } else {
        casual_calc_layout::format_number(value, code)
    }
}

/// Likewise for a text value, so a preview shows what the `@` section does.
#[wasm_bindgen]
pub fn format_preview_text(text: &str, code: &str) -> String {
    casual_calc_layout::format_text(text, code).unwrap_or_else(|| text.to_owned())
}

/// The sheet's conditional-format rules, in evaluation order, as JSON
/// `[{i,range,desc,fill,priority,stop}]` — for a Manage Rules list.
///
/// `i` is the index in document order, which is what the mutators take; the
/// array itself is sorted the way the rules are actually evaluated, so the list
/// shows what wins.
#[wasm_bindgen]
pub fn session_cf_rules(sheet: usize) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let mut order: Vec<usize> = (0..sh.conditional_formats.len()).collect();
        order.sort_by_key(|&i| {
            let p = sh.conditional_formats[i].priority;
            (if p == 0 { u32::MAX } else { p }, i)
        });
        let items: Vec<String> = order
            .iter()
            .map(|&i| {
                let cf = &sh.conditional_formats[i];
                format!(
                    "{{\"i\":{i},\"range\":{},\"desc\":{},\"fill\":{},\"priority\":{},\"stop\":{}}}",
                    json_string(&range_a1(&cf.range)),
                    json_string(&describe_cf_rule(&cf.rule)),
                    json_string(&cf.fill),
                    cf.priority,
                    u8::from(cf.stop_if_true),
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// The editable input for a cell (formula text with `=`, or the raw value).
#[wasm_bindgen]
pub fn session_cell_input(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let Some(sheet) = wb.sheets.get(sheet) else {
            return String::new();
        };
        let Some(cell) = sheet.cells.get(CellRef::new(row, col)) else {
            return String::new();
        };
        if let Some(handle) = cell.formula
            && let Some(expr) = wb.formula(handle)
            // `<protection hidden="1">` on a protected sheet keeps the formula
            // out of the formula bar. Carrying the flag through every save
            // while still showing the formula defeats the only thing it does.
            && !formula_is_hidden(wb, sheet, CellRef::new(row, col))
        {
            // Printed **at this cell**: a stored tree's references are
            // offsets from the cell holding it (`PERF-11`), so `Display` would
            // show the absolute form rather than what the person typed.
            return format!(
                "={}",
                casual_calc_formula::print_at(expr, Origin::at(row, col))
            );
        }
        // A date cell edits as its date, not as serial 45356 — the serial is an
        // implementation detail, and showing it means editing a date is a
        // lookup exercise. Only date/time formats get this: Excel shows the
        // plain number in the formula bar for currency and percentages.
        // A quote-prefixed cell edits with its apostrophe: without it, opening
        // the cell and pressing Enter would commit the bare text and drop the
        // marker, turning the value numeric again.
        let quoted = cell
            .style
            .and_then(|id| wb.styles.get(id))
            .is_some_and(|st| st.quote_prefix);
        if quoted {
            return format!("'{}", value_text(wb, &cell.value));
        }
        if let CellValue::Number(n) = cell.value
            && let Some(code) = cell
                .style
                .and_then(|id| wb.styles.get(id))
                .and_then(|st| st.number_format.as_deref())
            && casual_calc_io::is_date_format(code)
        {
            return casual_calc_layout::format_number(n, code);
        }
        value_text(wb, &cell.value)
    })
    .unwrap_or_default()
}
